//! Runs [`Command`]s on a Tokio thread of their own and answers each with a
//! [`Msg`], so the UI thread never waits on disk or network.

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use rivet_core::browse::{Fetch, Index};
use rivet_core::install::{FailedPackage, Install, Installed, Outcome, Progress, Stage};
use rivet_core::settings::Settings;
use rivet_core::{Command, Msg};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thunderstore_engine::client::{Refreshed, ThunderstoreClient};
use thunderstore_engine::ecosystem::Ecosystem;
use thunderstore_engine::profile::modlist::{self, ProfileMod};
use thunderstore_engine::profile::{self, layout};
use thunderstore_engine::progress::{self as engine, OperationId, ProgressEvent, StageUpdate};

/// Where commands read and write the app's files, and the Thunderstore host
/// they fetch from.
#[derive(Debug, Clone)]
pub struct Config {
  /// The settings TOML file. Its folder is created on the first save.
  pub settings: PathBuf,
  /// The folder package indexes and downloaded archives are cached in.
  pub cache: PathBuf,
  /// The folder profiles and the extracted packages they are installed from
  /// are kept in. The engine records absolute paths into the extracted
  /// packages, so they cannot live in `cache`.
  pub data: PathBuf,
  /// The Thunderstore host, such as [`THUNDERSTORE`].
  pub thunderstore: String,
  /// The game definitions installs are routed by.
  pub ecosystem: Ecosystem,
}

/// The public Thunderstore host.
pub const THUNDERSTORE: &str = "https://thunderstore.io";

/// The `User-Agent` sent with every request the app makes.
pub const USER_AGENT: &str = concat!("rivet/", env!("CARGO_PKG_VERSION"));

/// Runs one command to completion and returns the message that answers it.
/// Messages it sends before the answer, such as install progress, go to
/// `progress`.
pub async fn run(command: Command, config: &Config, progress: &UnboundedSender<Msg>) -> Msg {
  match command {
    Command::LoadSettings => Msg::SettingsLoaded(load_settings(&config.settings).await),
    Command::SaveSettings(settings) => {
      Msg::SettingsSaved(save_settings(&config.settings, &settings).await)
    }
    Command::ListFolder(folder) => {
      let entries = list_folder(&folder).await;

      Msg::FolderListed { folder, entries }
    }
    Command::ReadCachedIndex(community) => {
      let index = match client(config, &community) {
        Ok(client) => client.cached_index().await,
        Err(error) => Err(error),
      };

      Msg::CachedIndexRead {
        community,
        index: index
          .map(|index| index.map(Index::from))
          .map_err(|error| error.to_string()),
      }
    }
    Command::RefreshIndex { refresh, fetch } => {
      let index = refresh_index(config, &refresh.community, fetch).await;

      Msg::IndexRefreshed {
        refresh,
        index: index.map_err(|error| error.to_string()),
      }
    }
    Command::OpenProfile { game, profile } => {
      let installed = open_profile(config, &game, &profile).await;

      Msg::ProfileOpened {
        game,
        profile,
        installed,
      }
    }
    Command::Install(install) => {
      let operation = install.operation;
      let outcome = install_package(config, install, progress.clone()).await;

      Msg::InstallFinished {
        operation,
        outcome: outcome.map_err(|error| error.to_string()),
      }
    }
  }
}

/// Reads `game`'s `name` profile, creating it first if it does not exist.
async fn open_profile(config: &Config, game: &str, name: &str) -> Result<Installed, String> {
  let (data, game, name) = (config.data.clone(), game.to_string(), name.to_string());

  tokio::task::spawn_blocking(move || {
    let folder = layout::profile_dir(&data, &game, &name);

    if !folder.is_dir() {
      layout::create(&data, &game, &name)?;
    }

    modlist::read(&folder).map(|recorded| installed(&recorded))
  })
  .await
  .map_err(|error| error.to_string())?
  .map_err(|error| error.to_string())
}

/// Installs `install.package` and everything it needs, sending its progress
/// to `progress`. An error means nothing was installed. When the profile's
/// record cannot be read back after the install, the outcome names what the
/// install reported installing, with the read failure as its profile.
async fn install_package(
  config: &Config,
  install: Install,
  progress: UnboundedSender<Msg>,
) -> thunderstore_engine::Result<Outcome> {
  let folder = layout::profile_dir(&config.data, &install.game, &install.profile);
  let before = modlist::read(&folder)?;
  let client = client(config, &install.community)?;
  let requested = [install.package.clone()];
  let batch = profile::plan_install_batch_from(&before, &requested, &requested);
  let id = install.operation;
  let operation =
    engine::Operation::new(OperationId(id)).with_progress(move |event: ProgressEvent| {
      if let Some(update) = progress_of(event) {
        progress
          .unbounded_send(Msg::InstallProgress {
            operation: id,
            progress: update,
          })
          .ok();
      }
    });
  let outcome = profile::install_batch_with(
    &folder,
    &config.data,
    &config.ecosystem,
    &install.index.0,
    &client,
    &install.game,
    &batch,
    modlist::now_millis(),
    &operation,
  )
  .await?;
  let before = installed(&before);
  let (changed, after) = match modlist::read(&folder) {
    Ok(after) => {
      let after = installed(&after);
      let changed = after
        .iter()
        .filter(|(name, version)| before.get(*name) != Some(version))
        .map(|(name, _)| name.clone())
        .collect();

      (changed, Ok(after))
    }
    Err(error) => (outcome.succeeded, Err(error.to_string())),
  };

  Ok(Outcome {
    installed: changed,
    failed: outcome
      .failed
      .into_iter()
      .map(|(package, error)| FailedPackage {
        package,
        reason: error.to_string(),
      })
      .collect(),
    profile: after,
  })
}

/// The progress an engine event reports, or `None` for the end of a stage,
/// which the start of the next one replaces.
fn progress_of(event: ProgressEvent) -> Option<Progress> {
  let stage = match event.stage {
    engine::Stage::Downloading => Stage::Downloading,
    engine::Stage::Extracting => Stage::Extracting,
    engine::Stage::Installing => Stage::Installing,
    _ => return None,
  };
  let (done, total) = match event.update {
    StageUpdate::Started { total } => (0, total),
    StageUpdate::Advanced { done, total } => (done, total),
    _ => return None,
  };

  Some(Progress {
    package: event.package,
    stage,
    done,
    total,
  })
}

fn installed(recorded: &[ProfileMod]) -> Installed {
  recorded
    .iter()
    .map(|entry| (entry.name.clone(), entry.version_number.to_string()))
    .collect()
}

/// Fetches `community`'s index as `fetch` asks, with `None` when the cache is
/// current.
async fn refresh_index(
  config: &Config,
  community: &str,
  fetch: Fetch,
) -> thunderstore_engine::Result<Option<Index>> {
  let client = client(config, community)?;

  match fetch {
    Fetch::Always => Ok(Some(client.refresh_index().await?.into())),
    Fetch::IfStale => match client.refresh_if_stale().await? {
      Refreshed::Updated(index) => Ok(Some(index.into())),
      Refreshed::Unchanged => Ok(None),
    },
  }
}

/// A Thunderstore client for one community. Building one reads and sends
/// nothing.
fn client(config: &Config, community: &str) -> thunderstore_engine::Result<ThunderstoreClient> {
  ThunderstoreClient::builder()
    .base_url(&config.thunderstore)
    .community(community)
    .cache_dir(&config.cache)
    .user_agent(USER_AGENT)
    .build()
}

async fn load_settings(file: &Path) -> Result<Settings, String> {
  match tokio::fs::read_to_string(file).await {
    Ok(text) => toml::from_str(&text).map_err(|error| failure(file, error)),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
    Err(error) => Err(failure(file, error)),
  }
}

/// Held for the whole of a save. Commands run concurrently, and two saves
/// sharing the partial file would rename each other's half-written content.
static SAVING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn save_settings(file: &Path, settings: &Settings) -> Result<(), String> {
  let text = toml::to_string(settings).map_err(|error| failure(file, error))?;
  let partial = file.with_extension("toml.partial");
  let _saving = SAVING.lock().await;

  if let Some(folder) = file.parent() {
    tokio::fs::create_dir_all(folder)
      .await
      .map_err(|error| failure(folder, error))?;
  }

  // Written aside and renamed over the old file, so a crash mid-write leaves
  // the previous settings intact.
  tokio::fs::write(&partial, text)
    .await
    .map_err(|error| failure(&partial, error))?;

  tokio::fs::rename(&partial, file)
    .await
    .map_err(|error| failure(file, error))
}

async fn list_folder(folder: &Path) -> Result<Vec<String>, String> {
  let mut reader = tokio::fs::read_dir(folder)
    .await
    .map_err(|error| error.to_string())?;
  let mut names = Vec::new();

  while let Some(entry) = reader
    .next_entry()
    .await
    .map_err(|error| error.to_string())?
  {
    names.push(entry.file_name().to_string_lossy().into_owned());
  }

  Ok(names)
}

fn failure(path: &Path, error: impl std::fmt::Display) -> String {
  format!("{}: {error}", path.display())
}

/// Starts a Tokio runtime for commands, on threads named `commands`.
///
/// Commands sent on the returned sender run concurrently on that runtime, and
/// their answers arrive on the returned receiver. The runtime stops once the
/// sender is dropped.
///
/// # Panics
///
/// Panics if the runtime or the thread that owns it cannot be started.
pub fn spawn(config: Config) -> (UnboundedSender<Command>, UnboundedReceiver<Msg>) {
  let (commands, mut pending) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let config = Arc::new(config);
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .thread_name("commands")
    .enable_all()
    .build()
    .expect("tokio runtime");

  std::thread::Builder::new()
    .name("commands".into())
    .spawn(move || {
      runtime.block_on(async move {
        while let Some(command) = pending.next().await {
          let answers = answers.clone();
          let config = config.clone();

          tokio::spawn(async move {
            let answer = run(command, &config, &answers).await;

            answers.unbounded_send(answer).ok();
          });
        }
      });
    })
    .expect("commands thread");

  (commands, inbox)
}

#[cfg(test)]
mod tests {
  use super::*;
  use rivet_core::browse::{Fetch, Refresh};
  use rivet_core::install::{DEFAULT_PROFILE, Install, Installed, Outcome, Stage};
  use rivet_core::settings::GameSettings;
  use tempfile::TempDir;
  use thunderstore_engine::models::PackageIndex;
  use thunderstore_engine::test_support::{MockThunderstore, index_json};

  /// A config whose files sit in `dir`, against the public Thunderstore.
  fn config_in(dir: &TempDir) -> Config {
    Config {
      settings: dir.path().join("config").join("settings.toml"),
      cache: dir.path().join("cache"),
      data: dir.path().join("data"),
      thunderstore: THUNDERSTORE.to_string(),
      ecosystem: Ecosystem::bundled(),
    }
  }

  /// A mock Thunderstore serving its package index, and its Last-Modified to a
  /// HEAD, as the valheim community's,
  /// and answering `404` for the `gone` community.
  fn mock_thunderstore() -> MockThunderstore {
    let mut mock = MockThunderstore::start();
    let url = mock.url();
    let server = mock.server_mut();

    server
      .mock("GET", "/c/valheim/api/v1/package/")
      .with_status(200)
      .with_header("Content-Type", "application/json")
      .with_header("Last-Modified", "Wed, 21 Feb 2024 15:30:45 GMT")
      .with_body(index_json(&url))
      .create();
    server
      .mock("HEAD", "/c/valheim/api/v1/package/")
      .with_status(200)
      .with_header("Last-Modified", "Wed, 21 Feb 2024 15:30:45 GMT")
      .create();
    server
      .mock("GET", "/c/gone/api/v1/package/")
      .with_status(404)
      .create();

    mock
  }

  fn config_for(dir: &TempDir, mock: &MockThunderstore) -> Config {
    Config {
      thunderstore: mock.url(),
      ..config_in(dir)
    }
  }

  fn run_now(command: Command, config: &Config) -> Msg {
    run_reporting(command, config).1
  }

  /// Runs `command`, returning the messages it sent along the way and its
  /// answer.
  fn run_reporting(command: Command, config: &Config) -> (Vec<Msg>, Msg) {
    let (progress, mut sent) = unbounded();
    let answer = tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .unwrap()
      .block_on(run(command, config, &progress));

    drop(progress);

    let along_the_way = futures::executor::block_on_stream(&mut sent).collect();

    (along_the_way, answer)
  }

  fn package_count(index: &Index) -> usize {
    index.0.len()
  }

  #[test]
  fn reading_the_cache_before_any_fetch_finds_nothing() {
    let dir = TempDir::new().unwrap();
    let mock = mock_thunderstore();

    let answer = run_now(
      Command::ReadCachedIndex("valheim".to_string()),
      &config_for(&dir, &mock),
    );

    assert_eq!(
      answer,
      Msg::CachedIndexRead {
        community: "valheim".to_string(),
        index: Ok(None),
      }
    );
  }

  #[test]
  fn a_refresh_fetches_the_index_and_caches_it_for_the_next_start() {
    let dir = TempDir::new().unwrap();
    let mock = mock_thunderstore();
    let config = config_for(&dir, &mock);

    let fetched = run_now(refresh("valheim", Fetch::Always), &config);

    drop(mock);

    let cached = run_now(Command::ReadCachedIndex("valheim".to_string()), &config);

    let Msg::IndexRefreshed {
      refresh,
      index: Ok(Some(fetched)),
    } = fetched
    else {
      panic!("unexpected answer {fetched:?}");
    };
    let Msg::CachedIndexRead {
      index: Ok(Some(cached)),
      ..
    } = cached
    else {
      panic!("unexpected answer {cached:?}");
    };

    assert_eq!(refresh, refresh_of("valheim"));
    assert_eq!(package_count(&fetched), 5);
    assert_eq!(package_count(&cached), 5);
  }

  #[test]
  fn a_failed_refresh_answers_with_why() {
    let dir = TempDir::new().unwrap();
    let mock = mock_thunderstore();

    let answer = run_now(refresh("gone", Fetch::Always), &config_for(&dir, &mock));

    assert!(
      matches!(&answer, Msg::IndexRefreshed { refresh, index: Err(_) } if *refresh == refresh_of("gone")),
      "{answer:?}"
    );
  }

  #[test]
  fn a_staleness_check_downloads_only_when_the_cache_is_out_of_date() {
    let dir = TempDir::new().unwrap();
    let mock = mock_thunderstore();
    let config = config_for(&dir, &mock);

    let first = run_now(refresh("valheim", Fetch::IfStale), &config);
    let second = run_now(refresh("valheim", Fetch::IfStale), &config);

    assert!(
      matches!(&first, Msg::IndexRefreshed { index: Ok(Some(index)), .. } if package_count(index) == 5),
      "{first:?}"
    );
    assert_eq!(
      second,
      Msg::IndexRefreshed {
        refresh: refresh_of("valheim"),
        index: Ok(None),
      }
    );
  }

  #[test]
  fn a_staleness_check_that_cannot_reach_thunderstore_answers_with_why() {
    let dir = TempDir::new().unwrap();
    let mock = mock_thunderstore();
    let config = config_for(&dir, &mock);

    run_now(refresh("valheim", Fetch::Always), &config);
    drop(mock);

    let answer = run_now(refresh("valheim", Fetch::IfStale), &config);

    assert!(
      matches!(&answer, Msg::IndexRefreshed { index: Err(_), .. }),
      "{answer:?}"
    );
  }

  fn refresh_of(community: &str) -> Refresh {
    Refresh {
      community: community.to_string(),
      id: 7,
    }
  }

  fn refresh(community: &str, fetch: Fetch) -> Command {
    Command::RefreshIndex {
      refresh: refresh_of(community),
      fetch,
    }
  }

  fn valheim_settings() -> Settings {
    Settings {
      last_game: Some("valheim".to_string()),
      games: [(
        "valheim".to_string(),
        GameSettings {
          folder: PathBuf::from("/games/Valheim"),
        },
      )]
      .into(),
    }
  }

  fn open_profile(game: &str) -> Command {
    Command::OpenProfile {
      game: game.to_string(),
      profile: DEFAULT_PROFILE.to_string(),
    }
  }

  fn installed(packages: &[(&str, &str)]) -> Installed {
    packages
      .iter()
      .map(|(name, version)| (name.to_string(), version.to_string()))
      .collect()
  }

  fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
  }

  /// The mock's package index, plus `Owner-ModE`, which needs `Owner-ModA` and
  /// whose archive the mock answers with `404`.
  fn mock_index(mock: &mut MockThunderstore) -> Index {
    let url = mock.url();
    let mut packages: Vec<serde_json::Value> = serde_json::from_str(&index_json(&url)).unwrap();
    let mut broken = packages
      .iter()
      .find(|package| package["full_name"] == "Owner-ModC")
      .unwrap()
      .clone();

    broken["name"] = "ModE".into();
    broken["full_name"] = "Owner-ModE".into();
    broken["versions"][0]["name"] = "ModE".into();
    broken["versions"][0]["full_name"] = "Owner-ModE".into();
    broken["versions"][0]["download_url"] = format!("{url}/dl/ModE.zip").into();
    packages.push(broken);
    mock
      .server_mut()
      .mock("GET", "/dl/ModE.zip")
      .with_status(404)
      .create();

    let packages: Vec<thunderstore_engine::models::Package> =
      serde_json::from_value(serde_json::Value::Array(packages)).unwrap();

    PackageIndex::from(packages).into()
  }

  fn install(package: &str, index: &Index) -> Command {
    Command::Install(Install {
      operation: 3,
      game: "valheim".to_string(),
      community: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      package: package.to_string(),
      index: index.clone(),
    })
  }

  fn default_profile(config: &Config) -> PathBuf {
    config
      .data
      .join("valheim")
      .join("profiles")
      .join(DEFAULT_PROFILE)
  }

  #[test]
  fn opening_a_game_without_a_default_profile_creates_it() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    let answer = run_now(open_profile("valheim"), &config);

    assert_eq!(
      answer,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(Installed::new()),
      }
    );
    assert!(default_profile(&config).is_dir());
  }

  #[test]
  fn installing_a_package_brings_its_dependencies_into_the_profile() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let index = mock_index(&mut mock);

    run_now(open_profile("valheim"), &config);

    let (progress, answer) = run_reporting(install("Owner-ModC", &index), &config);
    let reopened = run_now(open_profile("valheim"), &config);
    let profile = installed(&[("Owner-ModA", "1.0.0"), ("Owner-ModC", "1.0.0")]);

    assert_eq!(
      answer,
      Msg::InstallFinished {
        operation: 3,
        outcome: Ok(Outcome {
          installed: names(&["Owner-ModA", "Owner-ModC"]),
          failed: vec![],
          profile: Ok(profile.clone()),
        }),
      }
    );
    assert_eq!(
      reopened,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(profile),
      }
    );
    assert!(
      default_profile(&config)
        .join("BepInEx/plugins/Owner-ModC/ModC.dll")
        .is_file()
    );

    let stages: Vec<(String, Stage)> = progress
      .into_iter()
      .map(|msg| match msg {
        Msg::InstallProgress {
          operation: 3,
          progress,
        } => (progress.package, progress.stage),
        other => panic!("unexpected message {other:?}"),
      })
      .collect();

    for stage in [Stage::Downloading, Stage::Extracting, Stage::Installing] {
      assert!(
        stages.contains(&("Owner-ModC".to_string(), stage)),
        "{stage:?} missing from {stages:?}"
      );
    }
  }

  #[test]
  fn a_failed_download_names_the_package_and_leaves_the_profile_as_it_was() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let index = mock_index(&mut mock);

    run_now(open_profile("valheim"), &config);
    run_now(install("Owner-ModB", &index), &config);

    let answer = run_now(install("Owner-ModE", &index), &config);

    let Msg::InstallFinished {
      operation: 3,
      outcome: Ok(outcome),
    } = answer
    else {
      panic!("unexpected answer {answer:?}");
    };

    assert_eq!(outcome.installed, Vec::<String>::new());
    assert_eq!(outcome.profile, Ok(installed(&[("Owner-ModB", "1.0.0")])));
    assert_eq!(
      outcome
        .failed
        .iter()
        .map(|failure| failure.package.as_str())
        .collect::<Vec<_>>(),
      ["Owner-ModE"]
    );
    assert!(
      outcome.failed[0].reason.contains("ModE"),
      "{:?}",
      outcome.failed[0].reason
    );
  }

  #[test]
  fn loading_without_a_settings_file_gives_the_defaults() {
    let dir = TempDir::new().unwrap();

    let answer = run_now(Command::LoadSettings, &config_in(&dir));

    assert_eq!(answer, Msg::SettingsLoaded(Ok(Settings::default())));
  }

  #[test]
  fn saved_settings_load_back_after_a_restart() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    let saved = run_now(Command::SaveSettings(valheim_settings()), &config);
    let loaded = run_now(Command::LoadSettings, &config);

    assert_eq!(saved, Msg::SettingsSaved(Ok(())));
    assert_eq!(loaded, Msg::SettingsLoaded(Ok(valheim_settings())));
  }

  #[test]
  fn settings_are_stored_as_toml() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    run_now(Command::SaveSettings(valheim_settings()), &config);

    assert_eq!(
      std::fs::read_to_string(&config.settings).unwrap(),
      "last_game = \"valheim\"\n\n[games.valheim]\nfolder = \"/games/Valheim\"\n"
    );
  }

  #[test]
  fn a_malformed_settings_file_is_reported() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    std::fs::create_dir_all(config.settings.parent().unwrap()).unwrap();
    std::fs::write(&config.settings, "last_game = [").unwrap();

    let answer = run_now(Command::LoadSettings, &config);

    assert!(matches!(answer, Msg::SettingsLoaded(Err(_))), "{answer:?}");
  }

  #[test]
  fn listing_a_folder_answers_with_its_entry_names() {
    let dir = TempDir::new().unwrap();

    std::fs::write(dir.path().join("valheim.x86_64"), "").unwrap();
    std::fs::create_dir(dir.path().join("valheim_Data")).unwrap();

    let answer = run_now(
      Command::ListFolder(dir.path().to_path_buf()),
      &config_in(&dir),
    );

    let Msg::FolderListed { folder, entries } = answer else {
      panic!("unexpected answer {answer:?}");
    };
    let mut entries = entries.unwrap();

    entries.sort();

    assert_eq!(folder, dir.path());
    assert_eq!(entries, vec!["valheim.x86_64", "valheim_Data"]);
  }

  #[test]
  fn listing_a_missing_folder_answers_with_why() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("missing");

    let answer = run_now(Command::ListFolder(missing.clone()), &config_in(&dir));

    assert!(
      matches!(&answer, Msg::FolderListed { folder, entries: Err(_) } if *folder == missing),
      "{answer:?}"
    );
  }

  #[test]
  fn a_command_sent_to_the_commands_thread_is_answered() {
    let dir = TempDir::new().unwrap();
    let (commands, mut inbox) = spawn(config_in(&dir));

    commands.unbounded_send(Command::LoadSettings).unwrap();

    let answer = futures::executor::block_on(inbox.next());

    assert_eq!(answer, Some(Msg::SettingsLoaded(Ok(Settings::default()))));
  }

  #[test]
  fn the_inbox_closes_when_the_commands_sender_is_dropped() {
    let dir = TempDir::new().unwrap();
    let (commands, mut inbox) = spawn(config_in(&dir));

    drop(commands);

    let answer = futures::executor::block_on(inbox.next());

    assert_eq!(answer, None);
  }
}
