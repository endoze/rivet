//! Keeping Rivet current: listing its releases on GitHub, downloading a
//! build beside the running program, and putting it in place.

use crate::{Cancels, Config, USER_AGENT, failure};
use futures::channel::mpsc::UnboundedSender;
use rivet_core::Msg;
use rivet_core::self_update::{Asset, Download, DownloadFailure, Found, Release, Settled};
use rivet_platform::executable::{self, backup_of, staged_of};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use thunderstore_engine::progress::CancelToken;
use tokio::io::AsyncWriteExt as _;

/// GitHub's API host.
pub const GITHUB: &str = "https://api.github.com";

/// The GitHub repository Rivet is released from.
const REPOSITORY: &str = "endoze/rivet";

/// How long asking GitHub for its releases may take.
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// How long connecting to GitHub may take before the request fails.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a connection to GitHub may go without sending anything before
/// the request fails, so a stalled download ends instead of hanging.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// How many bytes a download gets, at least, between progress reports.
const PROGRESS_STEP: u64 = 256 * 1024;

/// The file in the data folder recording an update, until the next start
/// settles it.
const RECORD: &str = "rivet-update.toml";

/// How long a new build is watched after it starts: one that fails within
/// this time did not start, and the old build goes back in place.
const STARTUP: Duration = Duration::from_secs(3);

/// How often a new build being watched is asked whether it has exited.
const STARTUP_POLL: Duration = Duration::from_millis(50);

/// How often, and how far apart, a start tries to delete the build an update
/// replaced. On Windows it can't be deleted until the old Rivet has exited,
/// which is at least [`STARTUP`] after the new one started.
const DELETE_TRIES: u32 = 40;
const DELETE_PAUSE: Duration = Duration::from_millis(250);

/// Where Rivet looks for a newer release of itself, and the program an
/// update replaces.
#[derive(Debug, Clone)]
pub struct Updater {
  /// The GitHub API host, such as [`GITHUB`].
  pub github: String,
  /// The running program. `None` when the platform can't say where it is,
  /// so no update can be put in place.
  pub executable: Option<PathBuf>,
  /// The cancel flags of downloads, by download id.
  pub cancels: Cancels,
  /// Starts the new build once it is in place, such as [`start_watched`].
  /// An error puts the old build back.
  pub start: fn(std::process::Command) -> io::Result<()>,
}

impl Default for Updater {
  /// Asks [`GITHUB`], with no program to replace, starting new builds with
  /// [`start_watched`].
  fn default() -> Self {
    Self {
      github: GITHUB.to_string(),
      executable: None,
      cancels: Cancels::default(),
      start: start_watched,
    }
  }
}

/// Starts `command`, a new build of Rivet, and watches it for a few seconds.
/// One still running after that is left to run, and reaped when it exits.
///
/// # Errors
///
/// Why the process could not be started, or the status it failed with
/// while watched.
pub fn start_watched(mut command: std::process::Command) -> io::Result<()> {
  let mut child = command.spawn()?;
  let started = Instant::now();

  while started.elapsed() < STARTUP {
    if let Some(status) = child.try_wait()? {
      if status.success() {
        return Ok(());
      }

      return Err(io::Error::other(format!("it stopped at once ({status})")));
    }

    std::thread::sleep(STARTUP_POLL);
  }

  std::thread::spawn(move || child.wait());

  Ok(())
}

/// An update put in place and not yet settled by a start.
#[derive(Debug, Serialize, Deserialize)]
struct Record {
  /// The version updated to.
  to: String,
  /// Where the build it replaced is kept.
  backup: PathBuf,
}

/// A release as GitHub's releases API lists it.
#[derive(Deserialize)]
struct Listed {
  tag_name: String,
  #[serde(default)]
  draft: bool,
  #[serde(default)]
  prerelease: bool,
  html_url: String,
  #[serde(default)]
  assets: Vec<ListedAsset>,
}

#[derive(Deserialize)]
struct ListedAsset {
  name: String,
  browser_download_url: String,
}

impl From<Listed> for Release {
  fn from(listed: Listed) -> Self {
    Release {
      tag: listed.tag_name,
      draft: listed.draft,
      prerelease: listed.prerelease,
      page: listed.html_url,
      assets: listed
        .assets
        .into_iter()
        .map(|asset| Asset {
          name: asset.name,
          url: asset.browser_download_url,
        })
        .collect(),
    }
  }
}

/// A client for GitHub whose stalled connections fail rather than hang.
fn client() -> reqwest::ClientBuilder {
  reqwest::Client::builder()
    .user_agent(USER_AGENT)
    .connect_timeout(CONNECT_TIMEOUT)
    .read_timeout(READ_TIMEOUT)
}

/// Rivet's releases on GitHub, newest first, drafts and prereleases
/// included.
pub(crate) async fn releases(config: &Config) -> Result<Vec<Release>, String> {
  let client = client()
    .timeout(CHECK_TIMEOUT)
    .build()
    .map_err(|error| error.to_string())?;
  let url = format!(
    "{}/repos/{REPOSITORY}/releases",
    config.updater.github.trim_end_matches('/')
  );
  let response = client
    .get(url)
    .header(reqwest::header::ACCEPT, "application/vnd.github+json")
    .send()
    .await
    .map_err(|error| error.to_string())?;
  let status = response.status();

  if !status.is_success() {
    return Err(format!("GitHub answered {status}"));
  }

  let body = response.bytes().await.map_err(|error| error.to_string())?;
  let listed: Vec<Listed> = serde_json::from_slice(&body)
    .map_err(|error| format!("GitHub's answer could not be read: {error}"))?;

  Ok(listed.into_iter().map(Release::from).collect())
}

/// Downloads `download`'s build beside the running program and checks it
/// against its checksum. Progress goes to `progress`. Nothing is downloaded
/// when Rivet can't write there, and whatever was downloaded is deleted when
/// it fails or is cancelled.
pub(crate) async fn download(
  config: &Config,
  download: Download,
  progress: &UnboundedSender<Msg>,
) -> Result<PathBuf, DownloadFailure> {
  let cancels = &config.updater.cancels;
  let token = cancels.token(download.id);
  let result = download_beside(config, &download.found, &token, progress).await;

  cancels.forget(download.id);

  result
}

async fn download_beside(
  config: &Config,
  found: &Found,
  token: &CancelToken,
  progress: &UnboundedSender<Msg>,
) -> Result<PathBuf, DownloadFailure> {
  let executable = config
    .updater
    .executable
    .as_ref()
    .ok_or(DownloadFailure::Unwritable)?;
  let staged = staged_of(executable);
  let mut file = tokio::fs::File::create(&staged)
    .await
    .map_err(|_| DownloadFailure::Unwritable)?;
  let written = write_build(&mut file, found, token, progress).await;

  drop(file);

  if written.is_err() {
    tokio::fs::remove_file(&staged).await.ok();
  }

  written.map(|()| staged)
}

async fn write_build(
  file: &mut tokio::fs::File,
  found: &Found,
  token: &CancelToken,
  progress: &UnboundedSender<Msg>,
) -> Result<(), DownloadFailure> {
  let cancelled = || {
    if token.is_cancelled() {
      return Err(DownloadFailure::Cancelled);
    }

    Ok(())
  };

  cancelled()?;

  let client = client().build().map_err(failed)?;
  let expected = checksum(&client, &found.checksum.url).await?;
  let mut response = client
    .get(&found.build.url)
    .send()
    .await
    .and_then(reqwest::Response::error_for_status)
    .map_err(failed)?;
  let total = response.content_length();
  let mut hasher = Sha256::new();
  let (mut received, mut reported) = (0, 0);

  while let Some(chunk) = response.chunk().await.map_err(failed)? {
    cancelled()?;
    hasher.update(&chunk);
    file.write_all(&chunk).await.map_err(failed)?;

    received += chunk.len() as u64;

    if received - reported >= PROGRESS_STEP {
      reported = received;
      progress
        .unbounded_send(Msg::RivetDownloadProgress { received, total })
        .ok();
    }
  }

  cancelled()?;

  if reported != received {
    progress
      .unbounded_send(Msg::RivetDownloadProgress { received, total })
      .ok();
  }

  file.flush().await.map_err(failed)?;

  if hex(&hasher.finalize()) != expected {
    return Err(DownloadFailure::Mismatch);
  }

  Ok(())
}

fn failed(error: impl std::fmt::Display) -> DownloadFailure {
  DownloadFailure::Failed(error.to_string())
}

/// The SHA-256 the checksum file at `url` gives, in lowercase hex: its first
/// word, as `sha256sum` writes it.
async fn checksum(client: &reqwest::Client, url: &str) -> Result<String, DownloadFailure> {
  let text = client
    .get(url)
    .send()
    .await
    .and_then(reqwest::Response::error_for_status)
    .map_err(failed)?
    .text()
    .await
    .map_err(failed)?;
  let first = text.split_whitespace().next().unwrap_or_default();

  if first.len() != 64 || !first.bytes().all(|byte| byte.is_ascii_hexdigit()) {
    return Err(DownloadFailure::Failed(
      "the release's checksum file holds no SHA-256".to_string(),
    ));
  }

  Ok(first.to_ascii_lowercase())
}

fn hex(bytes: &[u8]) -> String {
  bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Puts the build in `file` in place of the running program, keeping the old
/// build aside, records the update to version `to` for the next start, and
/// starts the new build. When it can't be started, or fails as it starts,
/// the old build goes back in place and nothing is recorded.
pub(crate) async fn replace(config: &Config, file: PathBuf, to: String) -> Result<(), String> {
  let executable = config
    .updater
    .executable
    .clone()
    .ok_or_else(|| "Rivet can't tell where it is installed".to_string())?;
  let (data, start) = (config.data.clone(), config.updater.start);

  tokio::task::spawn_blocking(move || {
    let record = data.join(RECORD);
    let backup = backup_of(&executable);
    let discard = |reason: String| {
      std::fs::remove_file(&file).ok();
      std::fs::remove_file(&record).ok();

      reason
    };

    write_record(
      &record,
      &Record {
        to,
        backup: backup.clone(),
      },
    )
    .map_err(|error| discard(failure(&record, error)))?;
    executable::replace(&executable, &file, &backup)
      .map_err(|error| discard(failure(&executable, error)))?;

    if let Err(error) = start(std::process::Command::new(&executable)) {
      executable::restore(&executable, &backup).ok();

      return Err(discard(format!("the new Rivet did not start: {error}")));
    }

    Ok(())
  })
  .await
  .map_err(|error| error.to_string())?
}

fn write_record(path: &Path, record: &Record) -> io::Result<()> {
  if let Some(folder) = path.parent() {
    std::fs::create_dir_all(folder)?;
  }

  let text = toml::to_string(record).map_err(io::Error::other)?;

  std::fs::write(path, text)
}

/// Settles the update recorded before this start of Rivet at version
/// `running`: when it is the version updated to, the build it replaced is
/// deleted, and otherwise that build is kept to fall back to and the update
/// reported as failed. A start with no update recorded deletes any build an
/// earlier update left aside.
pub(crate) async fn settle(config: &Config, running: String) -> Settled {
  let path = config.data.join(RECORD);
  let record = tokio::fs::read_to_string(&path)
    .await
    .ok()
    .and_then(|text| toml::from_str::<Record>(&text).ok());

  tokio::fs::remove_file(&path).await.ok();

  match record {
    Some(record) if record.to != running => Settled::Failed { to: record.to },
    Some(record) => {
      delete_old(&record.backup).await;

      Settled::Updated(record.to)
    }
    None => {
      if let Some(executable) = &config.updater.executable {
        tokio::fs::remove_file(backup_of(executable)).await.ok();
      }

      Settled::Nothing
    }
  }
}

/// Deletes `backup`, trying again for a while when the old Rivet has not
/// exited yet.
async fn delete_old(backup: &Path) {
  for _ in 0..DELETE_TRIES {
    match tokio::fs::remove_file(backup).await {
      Err(error) if error.kind() != io::ErrorKind::NotFound => {
        tokio::time::sleep(DELETE_PAUSE).await;
      }
      _ => return,
    }
  }
}

#[cfg(test)]
mod tests {
  use crate::Config;
  use crate::tests::{config_in, run_any, run_now, run_reporting};
  use rivet_core::self_update::{Asset, Download, DownloadFailure, Found, Release, Settled};
  use rivet_core::{Command, Msg};
  use rivet_platform::executable::{backup_of, staged_of};
  use std::path::{Path, PathBuf};
  use std::sync::Mutex;
  use tempfile::TempDir;
  use thunderstore_engine::test_support::MockThunderstore;

  const RELEASES: &str = "/repos/endoze/rivet/releases";

  /// The SHA-256 of `b"new build"`.
  const NEW_BUILD_SHA256: &str = "9893f46b04f2b89047dac4860978f401114b41f20aedf9abac1a7bb218496321";

  /// A config whose files sit in `dir`, asking `mock` in place of GitHub,
  /// with the running program at `dir/rivet` holding `old build`.
  fn config(dir: &TempDir, mock: &MockThunderstore) -> Config {
    let executable = dir.path().join("rivet");

    std::fs::write(&executable, "old build").unwrap();

    let mut config = config_in(dir);

    config.updater.github = mock.url();
    config.updater.executable = Some(executable);

    config
  }

  fn executable(config: &Config) -> PathBuf {
    config.updater.executable.clone().unwrap()
  }

  #[test]
  fn the_releases_are_listed_from_github() {
    let dir = TempDir::new().unwrap();
    let mut mock = MockThunderstore::start();
    let config = config(&dir, &mock);
    let listed = mock
      .server_mut()
      .mock("GET", RELEASES)
      .match_header("user-agent", crate::USER_AGENT)
      .match_header("accept", "application/vnd.github+json")
      .with_status(200)
      .with_body(
        r#"[
          {
            "tag_name": "v0.3.0",
            "draft": false,
            "prerelease": true,
            "html_url": "https://github.com/endoze/rivet/releases/tag/v0.3.0",
            "assets": [
              {
                "name": "rivet-linux-x86_64",
                "browser_download_url": "https://github.com/endoze/rivet/releases/download/v0.3.0/rivet-linux-x86_64",
                "size": 10
              }
            ]
          },
          {
            "tag_name": "v0.2.0",
            "draft": true,
            "prerelease": false,
            "html_url": "https://github.com/endoze/rivet/releases/tag/v0.2.0",
            "assets": []
          }
        ]"#,
      )
      .create();

    let answer = run_now(Command::FetchRivetReleases, &config);

    listed.assert();
    assert_eq!(
      answer,
      Msg::RivetReleasesFetched(Ok(vec![
        Release {
          tag: "v0.3.0".to_string(),
          draft: false,
          prerelease: true,
          page: "https://github.com/endoze/rivet/releases/tag/v0.3.0".to_string(),
          assets: vec![Asset {
            name: "rivet-linux-x86_64".to_string(),
            url: "https://github.com/endoze/rivet/releases/download/v0.3.0/rivet-linux-x86_64"
              .to_string(),
          }],
        },
        Release {
          tag: "v0.2.0".to_string(),
          draft: true,
          prerelease: false,
          page: "https://github.com/endoze/rivet/releases/tag/v0.2.0".to_string(),
          assets: Vec::new(),
        },
      ]))
    );
  }

  #[test]
  fn a_rate_limited_or_unreadable_answer_says_why() {
    for (status, body) in [(403, "rate limited"), (200, "not json")] {
      let dir = TempDir::new().unwrap();
      let mut mock = MockThunderstore::start();
      let config = config(&dir, &mock);

      mock
        .server_mut()
        .mock("GET", RELEASES)
        .with_status(status)
        .with_body(body)
        .create();

      let Msg::RivetReleasesFetched(Err(reason)) = run_now(Command::FetchRivetReleases, &config)
      else {
        panic!("a {status} answer was read as releases");
      };

      assert!(!reason.is_empty());

      if status == 403 {
        assert!(reason.contains("403"), "{reason}");
      }
    }
  }

  /// A download, by id 1, of Rivet 0.3.0's build and checksum from `mock`.
  fn download(mock: &MockThunderstore) -> Command {
    Command::DownloadRivet(Download {
      id: 1,
      found: Found {
        version: "0.3.0".to_string(),
        page: "https://github.com/endoze/rivet/releases/tag/v0.3.0".to_string(),
        build: Asset {
          name: "rivet-linux-x86_64".to_string(),
          url: format!("{}/build", mock.url()),
        },
        checksum: Asset {
          name: "rivet-linux-x86_64.sha256".to_string(),
          url: format!("{}/build.sha256", mock.url()),
        },
      },
    })
  }

  /// Serves `build` and a checksum file naming `checksum`.
  fn serve(mock: &mut MockThunderstore, build: &str, checksum: &str) {
    let server = mock.server_mut();

    server
      .mock("GET", "/build")
      .with_status(200)
      .with_body(build)
      .create();
    server
      .mock("GET", "/build.sha256")
      .with_status(200)
      .with_body(format!("{checksum}  rivet-linux-x86_64\n"))
      .create();
  }

  #[test]
  fn a_build_matching_its_checksum_is_downloaded_beside_the_program() {
    let dir = TempDir::new().unwrap();
    let mut mock = MockThunderstore::start();
    let config = config(&dir, &mock);

    serve(&mut mock, "new build", NEW_BUILD_SHA256);

    let (progress, answer) = run_reporting(download(&mock), &config);
    let staged = staged_of(&executable(&config));

    assert_eq!(answer, Msg::RivetDownloaded(Ok(staged.clone())));
    assert_eq!(std::fs::read(&staged).unwrap(), b"new build");
    assert_eq!(
      progress.last(),
      Some(&Msg::RivetDownloadProgress {
        received: 9,
        total: Some(9),
      })
    );
  }

  #[test]
  fn a_build_not_matching_its_checksum_is_discarded() {
    let dir = TempDir::new().unwrap();
    let mut mock = MockThunderstore::start();
    let config = config(&dir, &mock);

    serve(&mut mock, "tampered build", NEW_BUILD_SHA256);

    let answer = run_now(download(&mock), &config);

    assert_eq!(answer, Msg::RivetDownloaded(Err(DownloadFailure::Mismatch)));
    assert!(!staged_of(&executable(&config)).exists());
  }

  #[test]
  fn a_failed_download_is_discarded_and_says_why() {
    let dir = TempDir::new().unwrap();
    let mut mock = MockThunderstore::start();
    let config = config(&dir, &mock);
    let server = mock.server_mut();

    server
      .mock("GET", "/build.sha256")
      .with_status(200)
      .with_body(NEW_BUILD_SHA256)
      .create();
    server.mock("GET", "/build").with_status(404).create();

    let answer = run_now(download(&mock), &config);

    assert!(
      matches!(
        answer,
        Msg::RivetDownloaded(Err(DownloadFailure::Failed(ref reason))) if reason.contains("404")
      ),
      "{answer:?}"
    );
    assert!(!staged_of(&executable(&config)).exists());
  }

  #[test]
  fn a_program_in_a_folder_it_cannot_write_downloads_nothing() {
    let dir = TempDir::new().unwrap();
    let mut mock = MockThunderstore::start();
    let mut config = config(&dir, &mock);
    let fetched = ["/build", "/build.sha256"]
      .map(|path| mock.server_mut().mock("GET", path).expect(0).create());

    config.updater.executable = Some(dir.path().join("no-such-folder").join("rivet"));

    let answer = run_now(download(&mock), &config);

    for mock in fetched {
      mock.assert();
    }

    assert_eq!(
      answer,
      Msg::RivetDownloaded(Err(DownloadFailure::Unwritable))
    );
  }

  #[test]
  fn a_cancelled_download_is_discarded() {
    let dir = TempDir::new().unwrap();
    let mut mock = MockThunderstore::start();
    let config = config(&dir, &mock);

    serve(&mut mock, "new build", NEW_BUILD_SHA256);

    let (_, cancel) = run_any(Command::CancelRivetDownload(1), &config);
    let answer = run_now(download(&mock), &config);

    assert_eq!(cancel, None);
    assert_eq!(
      answer,
      Msg::RivetDownloaded(Err(DownloadFailure::Cancelled))
    );
    assert!(!staged_of(&executable(&config)).exists());
  }

  static SPAWNED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

  fn record_spawn(command: std::process::Command) -> std::io::Result<()> {
    SPAWNED
      .lock()
      .unwrap()
      .push(PathBuf::from(command.get_program()));

    Ok(())
  }

  fn fail_spawn(_: std::process::Command) -> std::io::Result<()> {
    Err(std::io::Error::other("exec format error"))
  }

  /// Puts `new build` beside the program, as a download does.
  fn downloaded(config: &Config) -> PathBuf {
    let staged = staged_of(&executable(config));

    std::fs::write(&staged, "new build").unwrap();

    staged
  }

  fn replace(file: &Path) -> Command {
    Command::ReplaceRivet {
      file: file.to_path_buf(),
      to: "0.3.0".to_string(),
    }
  }

  fn settle(running: &str) -> Command {
    Command::SettleRivetUpdate {
      running: running.to_string(),
    }
  }

  #[test]
  fn an_update_starts_the_new_build_and_the_next_start_deletes_the_old_one() {
    let dir = TempDir::new().unwrap();
    let mock = MockThunderstore::start();
    let mut config = config(&dir, &mock);
    let program = executable(&config);
    let backup = backup_of(&program);

    config.updater.start = record_spawn;

    let replaced = run_now(replace(&downloaded(&config)), &config);

    assert_eq!(replaced, Msg::RivetReplaced(Ok(())));
    assert_eq!(std::fs::read(&program).unwrap(), b"new build");
    assert_eq!(std::fs::read(&backup).unwrap(), b"old build");
    assert!(SPAWNED.lock().unwrap().contains(&program));

    let settled = run_now(settle("0.3.0"), &config);
    let again = run_now(settle("0.3.0"), &config);

    assert_eq!(
      settled,
      Msg::RivetUpdateSettled(Settled::Updated("0.3.0".to_string()))
    );
    assert_eq!(again, Msg::RivetUpdateSettled(Settled::Nothing));
    assert!(!backup.exists(), "the old build was kept");
  }

  #[test]
  fn a_new_build_that_does_not_start_is_swapped_back() {
    let dir = TempDir::new().unwrap();
    let mock = MockThunderstore::start();
    let mut config = config(&dir, &mock);
    let program = executable(&config);

    config.updater.start = fail_spawn;

    let replaced = run_now(replace(&downloaded(&config)), &config);

    assert!(
      matches!(&replaced, Msg::RivetReplaced(Err(reason)) if reason.contains("exec format error")),
      "{replaced:?}"
    );
    assert_eq!(std::fs::read(&program).unwrap(), b"old build");
    assert_eq!(
      run_now(settle("0.2.0"), &config),
      Msg::RivetUpdateSettled(Settled::Nothing)
    );
  }

  #[cfg(unix)]
  #[test]
  fn a_new_build_that_fails_as_it_starts_is_swapped_back() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = TempDir::new().unwrap();
    let mock = MockThunderstore::start();
    let config = config(&dir, &mock);
    let program = executable(&config);
    let staged = staged_of(&program);

    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&staged, "#!/bin/sh\nexit 3\n").unwrap();

    let replaced = run_now(replace(&staged), &config);

    assert!(
      matches!(&replaced, Msg::RivetReplaced(Err(reason)) if reason.contains("exit status: 3")),
      "{replaced:?}"
    );
    assert_eq!(std::fs::read(&program).unwrap(), b"old build");
    assert_eq!(
      run_now(settle("0.2.0"), &config),
      Msg::RivetUpdateSettled(Settled::Nothing)
    );
  }

  #[test]
  fn a_start_of_the_old_version_after_an_update_says_it_failed_and_keeps_the_old_build() {
    let dir = TempDir::new().unwrap();
    let mock = MockThunderstore::start();
    let mut config = config(&dir, &mock);
    let backup = backup_of(&executable(&config));

    config.updater.start = record_spawn;

    run_now(replace(&downloaded(&config)), &config);

    let settled = run_now(settle("0.2.0"), &config);

    assert_eq!(
      settled,
      Msg::RivetUpdateSettled(Settled::Failed {
        to: "0.3.0".to_string()
      })
    );
    assert!(backup.exists(), "the build to fall back to was deleted");
  }
}
