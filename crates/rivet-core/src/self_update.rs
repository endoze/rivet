//! Rivet keeping itself current: checking GitHub for a newer release,
//! downloading this platform's build of it, and restarting into it once
//! nothing is running.

use crate::notice::Notice;
use crate::{Command, Effect, State, queue, settings_screen};
use std::path::PathBuf;
use thunderstore_engine::profile::modlist::SemVer;

/// The version of Rivet running: the workspace version in `Cargo.toml`, which
/// each release is tagged with.
pub const RUNNING: &str = env!("CARGO_PKG_VERSION");

/// The name of this platform's build among a release's assets, or `None`
/// where Rivet publishes no build. Its checksum is the asset of the same name
/// ending in `.sha256`.
///
/// ```
/// use rivet_core::self_update::platform_asset;
///
/// if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
///   assert_eq!(platform_asset(), Some("rivet-linux-x86_64"));
/// }
/// ```
pub fn platform_asset() -> Option<&'static str> {
  if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("rivet-linux-x86_64")
  } else if cfg!(all(windows, target_arch = "x86_64")) {
    Some("rivet-windows-x86_64.exe")
  } else {
    None
  }
}

/// A release of Rivet as GitHub's releases API lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
  /// The tag it was released from, such as `v0.2.0`.
  pub tag: String,
  /// A draft is unpublished, and never offered.
  pub draft: bool,
  /// A prerelease is never offered.
  pub prerelease: bool,
  /// The release's web page.
  pub page: String,
  pub assets: Vec<Asset>,
}

/// A file attached to a [`Release`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
  pub name: String,
  /// Where it downloads from.
  pub url: String,
}

/// A newer release, with this platform's build of it and that build's
/// checksum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
  /// Its version, without the tag's leading `v`.
  pub version: String,
  /// The release's web page.
  pub page: String,
  pub build: Asset,
  /// A text file whose first word is the build's SHA-256, in hex.
  pub checksum: Asset,
}

/// Download `found` beside the running program. Answered with any number of
/// [`crate::Msg::RivetDownloadProgress`], then
/// [`crate::Msg::RivetDownloaded`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
  /// The id a cancel names it by.
  pub id: u64,
  pub found: Found,
}

/// Why a download of a newer Rivet left nothing to restart into. Whatever it
/// downloaded is deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadFailure {
  /// Rivet cannot write beside itself, as in a Nix store path or a system
  /// folder, so nothing was downloaded.
  Unwritable,
  /// The user cancelled it.
  Cancelled,
  /// The build did not match its checksum.
  Mismatch,
  /// The download failed, with the reason.
  Failed(String),
}

/// What a start found of the update before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settled {
  /// No update was waiting to be settled.
  Nothing,
  /// Rivet restarted into this version.
  Updated(String),
  /// Rivet restarted into an update to this version, but the version running
  /// is another: the update did not start.
  Failed { to: String },
}

/// Why a check for a newer Rivet found nothing to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckFailure {
  /// GitHub could not be asked, or its answer could not be read, with the
  /// reason.
  Unreachable(String),
  /// The newer version, by number, has no build for this platform.
  NoBuild(String),
}

/// How far keeping Rivet current has got.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Stage {
  #[default]
  Idle,
  /// GitHub is being asked for its releases. A check the user asked for has
  /// its result shown.
  Checking { manual: bool },
  /// A check the user asked for found no newer release.
  UpToDate,
  /// A check the user asked for failed.
  CheckFailed(CheckFailure),
  /// A check found this newer release, and asks whether to download it.
  Offered(Found),
  /// A newer release is downloading, `received` bytes of `total` so far.
  Downloading {
    download: Download,
    received: u64,
    total: Option<u64>,
    cancelling: bool,
  },
  /// A newer release, this version, is downloaded to `file`, waiting for the
  /// user to restart into it.
  Ready { version: String, file: PathBuf },
  /// A newer release, this version, is out, but Rivet cannot replace itself
  /// where it is installed; its page is at `page`.
  Unwritable { version: String, page: String },
  /// Restarting into the release in `file`: waiting for settings saves when
  /// `replacing` is false, and replacing the program once they are written.
  Restarting {
    version: String,
    file: PathBuf,
    replacing: bool,
  },
}

/// Keeping Rivet current, while it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfUpdate {
  /// The version running, [`RUNNING`] outside tests.
  pub running: String,
  /// This platform's build among a release's assets, [`platform_asset`]
  /// outside tests.
  pub asset: Option<String>,
  pub stage: Stage,
  /// How many downloads have been started, so each gets its own id.
  pub downloads: u64,
}

impl Default for SelfUpdate {
  fn default() -> Self {
    Self {
      running: RUNNING.to_string(),
      asset: platform_asset().map(str::to_string),
      stage: Stage::Idle,
      downloads: 0,
    }
  }
}

impl SelfUpdate {
  /// Whether a check for updates can start now: none is running and no newer
  /// release is offered, downloading or downloaded.
  pub fn can_check(&self) -> bool {
    matches!(
      self.stage,
      Stage::Idle | Stage::UpToDate | Stage::CheckFailed(_) | Stage::Unwritable { .. }
    )
  }
}

impl State {
  /// Whether "Restart to update" can be pressed: a newer Rivet is downloaded,
  /// and no operation is running or waiting and no game is being started.
  pub fn can_restart_to_update(&self) -> bool {
    matches!(self.self_update.stage, Stage::Ready { .. }) && !self.is_busy() && !self.is_launching()
  }

  /// Whether Rivet is restarting into a newer release, so no operation or
  /// launch may start: the new Rivet would run beside it on the same
  /// profiles and settings.
  pub fn is_restarting(&self) -> bool {
    matches!(self.self_update.stage, Stage::Restarting { .. })
  }
}

/// Records whether Rivet checks for a newer release on each start, and saves
/// it.
pub(crate) fn automatically_chosen(state: &mut State, on: bool) -> Vec<Effect> {
  if state.settings.update_automatically == on {
    return Vec::new();
  }

  state.settings.update_automatically = on;
  state.settings_screen.unsaved = false;

  vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
}

/// The check a start makes, when the user turned updating automatically on.
pub(crate) fn started(state: &mut State) -> Vec<Effect> {
  if !state.settings.update_automatically || !state.self_update.can_check() {
    return Vec::new();
  }

  state.self_update.stage = Stage::Checking { manual: false };

  vec![Effect::Run(Command::FetchRivetReleases)]
}

/// Checks for a newer release whether or not updating automatically is on,
/// showing the result. A check already running has its result shown.
pub(crate) fn check_pressed(state: &mut State) -> Vec<Effect> {
  let update = &mut state.self_update;

  if let Stage::Checking { manual } = &mut update.stage {
    *manual = true;

    return Vec::new();
  }

  if !update.can_check() {
    return Vec::new();
  }

  update.stage = Stage::Checking { manual: true };

  vec![Effect::Run(Command::FetchRivetReleases)]
}

/// Applies GitHub's releases: the newest published one, when newer than the
/// one running, is offered for download. An automatic check that finds
/// nothing or fails shows nothing.
pub(crate) fn releases_fetched(
  state: &mut State,
  releases: Result<Vec<Release>, String>,
) -> Vec<Effect> {
  let update = &mut state.self_update;
  let Stage::Checking { manual } = update.stage else {
    return Vec::new();
  };
  let shown = |stage: Stage| if manual { stage } else { Stage::Idle };
  let releases = match releases {
    Ok(releases) => releases,
    Err(reason) => {
      update.stage = shown(Stage::CheckFailed(CheckFailure::Unreachable(reason)));

      return Vec::new();
    }
  };
  let running = SemVer::parse(&update.running);
  let newest = releases
    .into_iter()
    .filter(|release| !release.draft && !release.prerelease)
    .map(|release| (SemVer::parse(version_of(&release.tag)), release))
    .filter(|(version, _)| *version > running)
    .max_by_key(|(version, _)| *version);
  let Some((_, release)) = newest else {
    update.stage = shown(Stage::UpToDate);

    return Vec::new();
  };
  let version = version_of(&release.tag).to_string();
  let Some(found) = update
    .asset
    .as_deref()
    .and_then(|asset| found(&release, &version, asset))
  else {
    update.stage = shown(Stage::CheckFailed(CheckFailure::NoBuild(version)));

    return Vec::new();
  };

  update.stage = Stage::Offered(found);

  Vec::new()
}

/// Downloads the release a check offered.
pub(crate) fn confirmed(state: &mut State) -> Vec<Effect> {
  let update = &mut state.self_update;
  let Stage::Offered(found) = &update.stage else {
    return Vec::new();
  };
  let found = found.clone();

  update.downloads += 1;

  let download = Download {
    id: update.downloads,
    found,
  };

  update.stage = Stage::Downloading {
    download: download.clone(),
    received: 0,
    total: None,
    cancelling: false,
  };

  vec![Effect::Run(Command::DownloadRivet(download))]
}

/// Turns down the release a check offered: nothing downloads, and the next
/// check offers it again.
pub(crate) fn dismissed(state: &mut State) -> Vec<Effect> {
  if matches!(state.self_update.stage, Stage::Offered(_)) {
    state.self_update.stage = Stage::Idle;
  }

  Vec::new()
}

/// `tag` without its leading `v`.
fn version_of(tag: &str) -> &str {
  tag.strip_prefix('v').unwrap_or(tag)
}

/// `release` at `version` with its `asset` and that asset's checksum, or
/// `None` when it lacks either.
fn found(release: &Release, version: &str, asset: &str) -> Option<Found> {
  let named = |name: &str| {
    release
      .assets
      .iter()
      .find(|candidate| candidate.name == name)
      .cloned()
  };

  Some(Found {
    version: version.to_string(),
    page: release.page.clone(),
    build: named(asset)?,
    checksum: named(&format!("{asset}.sha256"))?,
  })
}

pub(crate) fn download_progress(state: &mut State, got: u64, of: Option<u64>) -> Vec<Effect> {
  if let Stage::Downloading {
    received, total, ..
  } = &mut state.self_update.stage
  {
    *received = got;
    *total = of;
  }

  Vec::new()
}

/// Cancels the running download, which answers once it has stopped.
pub(crate) fn cancel_pressed(state: &mut State) -> Vec<Effect> {
  let Stage::Downloading {
    download,
    cancelling,
    ..
  } = &mut state.self_update.stage
  else {
    return Vec::new();
  };

  if *cancelling {
    return Vec::new();
  }

  *cancelling = true;

  vec![Effect::Run(Command::CancelRivetDownload(download.id))]
}

/// Applies the end of a download: a build checked against its checksum waits
/// for a restart, and anything else is reported in one line, except a
/// cancel. When Rivet cannot write where it is installed, the release's page
/// is offered instead.
pub(crate) fn downloaded(
  state: &mut State,
  result: Result<PathBuf, DownloadFailure>,
) -> Vec<Effect> {
  let Stage::Downloading { download, .. } = &state.self_update.stage else {
    return Vec::new();
  };
  let Found { version, page, .. } = download.found.clone();

  match result {
    Ok(file) => {
      state.self_update.stage = Stage::Ready { version, file };

      Vec::new()
    }
    Err(DownloadFailure::Unwritable) => {
      state.self_update.stage = Stage::Unwritable {
        version: version.clone(),
        page: page.clone(),
      };

      vec![Effect::Notify(Notice::RivetUnwritable { version, page })]
    }
    Err(DownloadFailure::Cancelled) => {
      state.self_update.stage = Stage::Idle;

      Vec::new()
    }
    Err(failure) => {
      state.self_update.stage = Stage::Idle;

      vec![Effect::Notify(Notice::RivetDownloadFailed {
        version,
        failure,
      })]
    }
  }
}

/// Restarts into the downloaded release, unless an operation is running or
/// waiting or a game is being started. Settings not yet saved are saved
/// first.
pub(crate) fn restart_pressed(state: &mut State) -> Vec<Effect> {
  let Stage::Ready { version, file } = &state.self_update.stage else {
    return Vec::new();
  };

  if !state.can_restart_to_update() {
    return vec![Effect::Notify(Notice::RestartRefused)];
  }

  let (version, file) = (version.clone(), file.clone());
  let mut effects = settings_screen::save_unsaved(state);

  state.self_update.stage = Stage::Restarting {
    version,
    file,
    replacing: false,
  };

  if effects.is_empty() {
    effects.extend(saves_written(state));
  }

  effects
}

/// Replaces the program once every settings save is written, when restarting
/// into a release waits for them.
pub(crate) fn saves_written(state: &mut State) -> Vec<Effect> {
  if state.saves > 0 {
    return Vec::new();
  }

  let Stage::Restarting {
    version,
    file,
    replacing,
  } = &mut state.self_update.stage
  else {
    return Vec::new();
  };

  if *replacing {
    return Vec::new();
  }

  *replacing = true;

  vec![Effect::Run(Command::ReplaceRivet {
    file: file.clone(),
    to: version.clone(),
  })]
}

/// Closes the window once the new Rivet has started, or reports why it could
/// not be put in place, leaving the one running as it was.
pub(crate) fn replaced(state: &mut State, result: Result<(), String>) -> Vec<Effect> {
  if !matches!(state.self_update.stage, Stage::Restarting { .. }) {
    return Vec::new();
  }

  match result {
    Ok(()) => {
      state.closing = Some(queue::Closing::Waiting);

      queue::close_when_done(state)
    }
    Err(reason) => {
      state.self_update.stage = Stage::Idle;

      vec![Effect::Notify(Notice::RivetReplaceFailed(reason))]
    }
  }
}

/// Tells the user how the update before this start went.
pub(crate) fn settled(settled: Settled) -> Vec<Effect> {
  match settled {
    Settled::Nothing => Vec::new(),
    Settled::Updated(version) => vec![Effect::Notify(Notice::RivetUpdated(version))],
    Settled::Failed { to } => vec![Effect::Notify(Notice::RivetUpdateFailed { to })],
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::install::DEFAULT_PROFILE;
  use crate::settings::{GameSettings, Settings};
  use crate::{Msg, Screen, test_ecosystem, test_installed, update};
  use thunderstore_engine::profile::launch::{HostOs, LaunchMode};

  const BUILD: &str = "rivet-linux-x86_64";

  /// A Linux state running Rivet 0.2.0, with valheim set up and open last,
  /// and updating automatically turned `on`, after its settings loaded.
  fn started(on: bool) -> (State, Vec<Effect>) {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: Some("valheim".to_string()),
      games: [("valheim".to_string(), GameSettings::new("/games/valheim"))].into(),
      update_automatically: on,
      ..Settings::default()
    };

    state.host = Some(HostOs::Linux);
    state.self_update.running = "0.2.0".to_string();
    state.self_update.asset = Some(BUILD.to_string());

    update(&mut state, Msg::Started);

    let effects = update(&mut state, Msg::SettingsLoaded(Ok(settings)));

    (state, effects)
  }

  fn asset(name: &str) -> Asset {
    Asset {
      name: name.to_string(),
      url: format!("https://github.com/endoze/rivet/releases/download/{name}"),
    }
  }

  /// A published release tagged `tag`, with the Linux build and its checksum.
  fn release(tag: &str) -> Release {
    Release {
      tag: tag.to_string(),
      draft: false,
      prerelease: false,
      page: format!("https://github.com/endoze/rivet/releases/tag/{tag}"),
      assets: vec![asset(BUILD), asset(&format!("{BUILD}.sha256"))],
    }
  }

  fn fetch() -> Effect {
    Effect::Run(Command::FetchRivetReleases)
  }

  fn found(version: &str) -> Found {
    Found {
      version: version.to_string(),
      page: format!("https://github.com/endoze/rivet/releases/tag/v{version}"),
      build: asset(BUILD),
      checksum: asset(&format!("{BUILD}.sha256")),
    }
  }

  fn download(id: u64, version: &str) -> Effect {
    Effect::Run(Command::DownloadRivet(Download {
      id,
      found: found(version),
    }))
  }

  fn checks(effects: &[Effect]) -> usize {
    effects.iter().filter(|effect| **effect == fetch()).count()
  }

  #[test]
  fn a_start_with_updating_on_asks_github_once() {
    let (mut state, effects) = started(true);

    assert_eq!(checks(&effects), 1);
    assert_eq!(state.self_update.stage, Stage::Checking { manual: false });

    update(
      &mut state,
      Msg::RivetReleasesFetched(Ok(vec![release("v0.2.0")])),
    );

    let switched = [
      update(&mut state, Msg::SwitchGamePressed),
      update(&mut state, Msg::GamePicked("valheim".to_string())),
      update(&mut state, Msg::WindowFocused),
    ];

    assert_eq!(
      switched
        .iter()
        .map(|effects| checks(effects))
        .sum::<usize>(),
      0
    );
  }

  #[test]
  fn a_start_with_updating_off_checks_nothing() {
    let (state, effects) = started(false);

    assert_eq!(checks(&effects), 0);
    assert_eq!(state.self_update.stage, Stage::Idle);
  }

  #[test]
  fn a_newer_release_found_on_start_asks_before_downloading() {
    let (mut state, _) = started(true);

    let effects = update(
      &mut state,
      Msg::RivetReleasesFetched(Ok(vec![release("v0.1.0"), release("v0.3.0")])),
    );

    assert_eq!(effects, vec![]);
    assert_eq!(state.self_update.stage, Stage::Offered(found("0.3.0")));
  }

  #[test]
  fn going_ahead_with_the_offer_downloads_its_build_for_this_platform() {
    let (mut state, _) = started(true);

    update(
      &mut state,
      Msg::RivetReleasesFetched(Ok(vec![release("v0.3.0")])),
    );

    let effects = update(&mut state, Msg::UpdateRivetConfirmed);

    assert_eq!(effects, vec![download(1, "0.3.0")]);
    assert!(matches!(
      state.self_update.stage,
      Stage::Downloading {
        received: 0,
        cancelling: false,
        ..
      }
    ));
  }

  #[test]
  fn the_same_or_an_older_release_found_on_start_shows_and_changes_nothing() {
    for latest in ["v0.2.0", "v0.1.9"] {
      let (mut state, _) = started(true);

      let effects = update(
        &mut state,
        Msg::RivetReleasesFetched(Ok(vec![release(latest)])),
      );

      assert_eq!(effects, vec![], "{latest}");
      assert_eq!(state.self_update.stage, Stage::Idle, "{latest}");
    }
  }

  #[test]
  fn a_failed_check_on_start_shows_and_changes_nothing() {
    let (mut state, _) = started(true);
    let settings = state.settings.clone();

    let effects = update(
      &mut state,
      Msg::RivetReleasesFetched(Err("rate limited".to_string())),
    );

    assert_eq!(effects, vec![]);
    assert_eq!(state.self_update.stage, Stage::Idle);
    assert_eq!(state.settings, settings);
  }

  #[test]
  fn drafts_and_prereleases_are_never_offered() {
    let (mut state, _) = started(true);
    let draft = Release {
      draft: true,
      ..release("v0.4.0")
    };
    let prerelease = Release {
      prerelease: true,
      ..release("v0.5.0")
    };

    let effects = update(
      &mut state,
      Msg::RivetReleasesFetched(Ok(vec![draft, prerelease, release("v0.3.0")])),
    );

    assert_eq!(effects, vec![]);
    assert_eq!(state.self_update.stage, Stage::Offered(found("0.3.0")));
  }

  #[test]
  fn checking_by_hand_with_updating_off_says_rivet_is_up_to_date() {
    let (mut state, _) = started(false);

    let asked = update(&mut state, Msg::CheckForUpdatesPressed);

    update(
      &mut state,
      Msg::RivetReleasesFetched(Ok(vec![release("v0.2.0")])),
    );

    assert_eq!(asked, vec![fetch()]);
    assert_eq!(state.self_update.stage, Stage::UpToDate);
  }

  #[test]
  fn checking_by_hand_says_why_the_check_failed() {
    let (mut state, _) = started(false);

    update(&mut state, Msg::CheckForUpdatesPressed);
    update(
      &mut state,
      Msg::RivetReleasesFetched(Err("offline".to_string())),
    );

    assert_eq!(
      state.self_update.stage,
      Stage::CheckFailed(CheckFailure::Unreachable("offline".to_string()))
    );
  }

  #[test]
  fn checking_by_hand_asks_before_downloading_a_newer_release() {
    let (mut state, _) = started(false);

    update(&mut state, Msg::CheckForUpdatesPressed);

    let effects = update(
      &mut state,
      Msg::RivetReleasesFetched(Ok(vec![release("v1.0.0")])),
    );

    assert_eq!(effects, vec![]);
    assert_eq!(state.self_update.stage, Stage::Offered(found("1.0.0")));
    assert_eq!(
      update(&mut state, Msg::UpdateRivetConfirmed),
      vec![download(1, "1.0.0")]
    );
  }

  #[test]
  fn turning_down_the_offer_downloads_nothing_and_the_next_check_offers_it_again() {
    let (mut state, _) = started(true);

    update(
      &mut state,
      Msg::RivetReleasesFetched(Ok(vec![release("v0.3.0")])),
    );

    let dismissed = update(&mut state, Msg::UpdateRivetDismissed);

    assert_eq!(dismissed, vec![]);
    assert_eq!(state.self_update.stage, Stage::Idle);
    assert_eq!(
      update(&mut state, Msg::CheckForUpdatesPressed),
      vec![fetch()]
    );

    update(
      &mut state,
      Msg::RivetReleasesFetched(Ok(vec![release("v0.3.0")])),
    );

    assert_eq!(state.self_update.stage, Stage::Offered(found("0.3.0")));
  }

  #[test]
  fn checking_by_hand_while_an_offer_is_open_asks_github_nothing() {
    let (mut state, _) = started(true);

    update(
      &mut state,
      Msg::RivetReleasesFetched(Ok(vec![release("v0.3.0")])),
    );

    assert_eq!(update(&mut state, Msg::CheckForUpdatesPressed), vec![]);
    assert_eq!(state.self_update.stage, Stage::Offered(found("0.3.0")));
  }

  #[test]
  fn checking_by_hand_during_the_start_check_shows_its_result_without_asking_again() {
    let (mut state, _) = started(true);

    let asked = update(&mut state, Msg::CheckForUpdatesPressed);

    update(
      &mut state,
      Msg::RivetReleasesFetched(Ok(vec![release("v0.2.0")])),
    );

    assert_eq!(asked, vec![]);
    assert_eq!(state.self_update.stage, Stage::UpToDate);
  }

  #[test]
  fn a_newer_release_without_a_build_for_this_platform_is_named_when_checked_by_hand() {
    let (mut state, _) = started(false);
    let windows_only = Release {
      assets: vec![asset("rivet-windows-x86_64.exe")],
      ..release("v0.3.0")
    };

    update(&mut state, Msg::CheckForUpdatesPressed);

    let effects = update(
      &mut state,
      Msg::RivetReleasesFetched(Ok(vec![windows_only])),
    );

    assert_eq!(effects, vec![]);
    assert_eq!(
      state.self_update.stage,
      Stage::CheckFailed(CheckFailure::NoBuild("0.3.0".to_string()))
    );
  }

  #[test]
  fn turning_updating_on_saves_it_and_waits_for_the_next_start() {
    let (mut state, _) = started(false);

    let effects = update(&mut state, Msg::UpdateAutomaticallyChosen(true));

    assert!(state.settings.update_automatically);
    assert_eq!(
      effects,
      vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
    );
  }

  /// A state downloading Rivet 0.3.0, found on start and gone ahead with.
  fn downloading() -> State {
    let (mut state, _) = started(true);

    update(
      &mut state,
      Msg::RivetReleasesFetched(Ok(vec![release("v0.3.0")])),
    );
    update(&mut state, Msg::UpdateRivetConfirmed);

    state
  }

  #[test]
  fn the_download_reports_its_progress() {
    let mut state = downloading();

    update(
      &mut state,
      Msg::RivetDownloadProgress {
        received: 512,
        total: Some(2048),
      },
    );

    assert!(matches!(
      state.self_update.stage,
      Stage::Downloading {
        received: 512,
        total: Some(2048),
        ..
      }
    ));
  }

  #[test]
  fn a_cancelled_download_stops_and_says_nothing() {
    let mut state = downloading();

    let cancel = update(&mut state, Msg::RivetDownloadCancelPressed);
    let again = update(&mut state, Msg::RivetDownloadCancelPressed);
    let ended = update(
      &mut state,
      Msg::RivetDownloaded(Err(DownloadFailure::Cancelled)),
    );

    assert_eq!(cancel, vec![Effect::Run(Command::CancelRivetDownload(1))]);
    assert_eq!(again, vec![]);
    assert_eq!(ended, vec![]);
    assert_eq!(state.self_update.stage, Stage::Idle);
  }

  #[test]
  fn a_build_that_does_not_match_its_checksum_is_reported_in_one_notice() {
    let mut state = downloading();

    let effects = update(
      &mut state,
      Msg::RivetDownloaded(Err(DownloadFailure::Mismatch)),
    );

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::RivetDownloadFailed {
        version: "0.3.0".to_string(),
        failure: DownloadFailure::Mismatch,
      })]
    );
    assert_eq!(state.self_update.stage, Stage::Idle);
  }

  #[test]
  fn rivet_unable_to_write_where_it_is_installed_links_to_the_release() {
    let mut state = downloading();
    let page = "https://github.com/endoze/rivet/releases/tag/v0.3.0".to_string();

    let effects = update(
      &mut state,
      Msg::RivetDownloaded(Err(DownloadFailure::Unwritable)),
    );

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::RivetUnwritable {
        version: "0.3.0".to_string(),
        page: page.clone(),
      })]
    );
    assert_eq!(
      state.self_update.stage,
      Stage::Unwritable {
        version: "0.3.0".to_string(),
        page,
      }
    );
  }

  /// A state with Rivet 0.3.0 downloaded to `/opt/rivet/rivet.new`.
  fn ready() -> State {
    let mut state = downloading();

    update(
      &mut state,
      Msg::RivetDownloaded(Ok(PathBuf::from("/opt/rivet/rivet.new"))),
    );

    state
  }

  fn replace() -> Effect {
    Effect::Run(Command::ReplaceRivet {
      file: PathBuf::from("/opt/rivet/rivet.new"),
      to: "0.3.0".to_string(),
    })
  }

  #[test]
  fn a_downloaded_build_waits_for_a_restart() {
    let state = ready();

    assert_eq!(
      state.self_update.stage,
      Stage::Ready {
        version: "0.3.0".to_string(),
        file: PathBuf::from("/opt/rivet/rivet.new"),
      }
    );
    assert!(state.can_restart_to_update());
  }

  #[test]
  fn restarting_puts_the_new_build_in_place_then_closes() {
    let mut state = ready();

    let restarting = update(&mut state, Msg::RestartToUpdatePressed);
    let replaced = update(&mut state, Msg::RivetReplaced(Ok(())));

    assert_eq!(restarting, vec![replace()]);
    assert_eq!(replaced, vec![Effect::CloseWindow]);
  }

  #[test]
  fn restarting_is_refused_while_an_operation_is_queued() {
    let mut state = ready();

    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(test_installed(&[("Owner-Solo", "1.0.0")])),
      },
    );
    update(&mut state, Msg::DisableAllPressed);

    assert!(state.is_busy(), "no operation was queued");
    assert!(!state.can_restart_to_update());

    let effects = update(&mut state, Msg::RestartToUpdatePressed);

    assert_eq!(effects, vec![Effect::Notify(Notice::RestartRefused)]);
    assert!(matches!(state.self_update.stage, Stage::Ready { .. }));
  }

  #[test]
  fn no_operation_or_launch_starts_while_restarting() {
    let mut state = ready();

    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(test_installed(&[("Owner-Solo", "1.0.0")])),
      },
    );
    update(
      &mut state,
      Msg::GameFolderListed {
        game: "valheim".to_string(),
        entries: Ok(vec!["valheim.x86_64".to_string()]),
      },
    );

    assert!(state.can_install() && state.can_launch());

    update(&mut state, Msg::RestartToUpdatePressed);

    let disabled = update(&mut state, Msg::DisableAllPressed);
    let launched = update(&mut state, Msg::LaunchPressed(LaunchMode::Modded));

    assert!(!state.can_install());
    assert!(!state.can_launch());
    assert_eq!(disabled, vec![]);
    assert_eq!(launched, vec![]);
    assert!(!state.is_busy());
  }

  #[test]
  fn restarting_saves_unsaved_settings_before_replacing_the_program() {
    let mut state = ready();

    update(&mut state, Msg::SettingsPressed);
    update(
      &mut state,
      Msg::LaunchArgumentsChanged("-console".to_string()),
    );

    let restarting = update(&mut state, Msg::RestartToUpdatePressed);
    let saved = update(&mut state, Msg::SettingsSaved(Ok(())));

    assert_eq!(
      restarting,
      vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
    );
    assert_eq!(saved, vec![replace()]);
    assert_eq!(state.screen, Screen::Settings("valheim".to_string()));
  }

  #[test]
  fn a_build_that_cannot_be_put_in_place_is_reported_and_rivet_keeps_running() {
    let mut state = ready();

    update(&mut state, Msg::RestartToUpdatePressed);

    let effects = update(
      &mut state,
      Msg::RivetReplaced(Err("permission denied".to_string())),
    );

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::RivetReplaceFailed(
        "permission denied".to_string()
      ))]
    );
    assert_eq!(state.closing, None);
    assert_eq!(state.self_update.stage, Stage::Idle);
  }

  #[test]
  fn the_start_after_an_update_says_how_it_went() {
    let mut state = State::new(test_ecosystem());

    let updated = update(
      &mut state,
      Msg::RivetUpdateSettled(Settled::Updated("0.3.0".to_string())),
    );
    let failed = update(
      &mut state,
      Msg::RivetUpdateSettled(Settled::Failed {
        to: "0.3.0".to_string(),
      }),
    );
    let nothing = update(&mut state, Msg::RivetUpdateSettled(Settled::Nothing));

    assert_eq!(
      updated,
      vec![Effect::Notify(Notice::RivetUpdated("0.3.0".to_string()))]
    );
    assert_eq!(
      failed,
      vec![Effect::Notify(Notice::RivetUpdateFailed {
        to: "0.3.0".to_string()
      })]
    );
    assert_eq!(nothing, vec![]);
  }
}
