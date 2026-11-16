//! The open game's profile and installing a package into it: which packages
//! an install brings, the confirmation, and the operations that change a
//! profile.

use crate::browse::Index;
use crate::pins;
use crate::queue::{self, Work};
use crate::{Command, Effect, State};
use std::collections::BTreeMap;
use thunderstore_engine::extract::parse_package_filename;
use thunderstore_engine::models::DependencyGraph;

/// The profile every game has, created the first time the game is opened.
pub const DEFAULT_PROFILE: &str = "Default";

/// The packages recorded in a profile, by `Owner-Name`.
pub type Installed = BTreeMap<String, InstalledPackage>;

/// One package recorded in a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
  /// The owner, as Thunderstore spells it.
  pub author: String,
  pub version: String,
  pub enabled: bool,
  /// The packages it requires, by `Owner-Name`.
  pub dependencies: Vec<String>,
  /// When it was installed, in Unix milliseconds.
  pub installed_at: u64,
  /// The user chose this version, so updates leave it alone. See
  /// [`crate::pins`].
  pub pinned: bool,
}

impl InstalledPackage {
  /// The name of `full_name` without its owner, such as `Mod` for
  /// `Owner-Mod`.
  ///
  /// ```
  /// use rivet_core::install::InstalledPackage;
  ///
  /// let package = InstalledPackage {
  ///   author: "Owner".to_string(),
  ///   version: "1.0.0".to_string(),
  ///   enabled: true,
  ///   dependencies: vec![],
  ///   installed_at: 0,
  ///   pinned: false,
  /// };
  ///
  /// assert_eq!(package.name("Owner-Mod"), "Mod");
  /// ```
  pub fn name<'a>(&self, full_name: &'a str) -> &'a str {
    full_name
      .strip_prefix(self.author.as_str())
      .and_then(|rest| rest.strip_prefix('-'))
      .unwrap_or(full_name)
  }
}

/// The open game's selected profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
  /// The game, by its key in the game definitions.
  pub game: String,
  pub name: String,
  pub status: ProfileStatus,
}

/// Whether a profile's packages are known yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileStatus {
  /// The profile is being created if missing, then read.
  Opening,
  Ready(Installed),
  /// The profile could not be created or read, with the reason.
  Failed(String),
}

impl Profile {
  /// The profile's packages, once it has been read.
  pub fn installed(&self) -> Option<&Installed> {
    match &self.status {
      ProfileStatus::Ready(installed) => Some(installed),
      _ => None,
    }
  }
}

/// One package an install will bring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
  /// The package, as `Owner-Name`.
  pub full_name: String,
  /// The version that will be installed.
  pub version: String,
  /// The profile already holds this version, so it is left as it is.
  pub installed: bool,
}

/// An install the user has asked for but not yet confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmation {
  /// The package the user asked for.
  pub package: Planned,
  /// Every package it needs, directly or through others, by name.
  pub dependencies: Vec<Planned>,
  /// Dependencies the package index does not have, by `Owner-Name`. The
  /// install cannot go ahead while any are missing.
  pub missing: Vec<String>,
  /// Every version the package has published, the most recent first.
  pub versions: Vec<String>,
  /// The version the user chose, which the install pins, or `None` for the
  /// latest, which leaves the package unpinned.
  pub chosen: Option<String>,
}

impl Confirmation {
  /// Whether the install can go ahead.
  pub fn can_confirm(&self) -> bool {
    self.missing.is_empty()
  }
}

/// A change to a profile the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
  /// Unique per app run, so progress and the result can be matched to it.
  pub id: u64,
  /// The game, by its key in the game definitions.
  pub game: String,
  pub profile: String,
  pub action: Action,
  pub work: Work,
  pub status: OperationStatus,
}

/// What an [`Operation`] changes, naming packages as `Owner-Name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
  /// Install the package the user asked for, with everything it needs.
  Install(String),
  /// Update these packages to their latest versions, with what they need.
  Update(Vec<String>),
  /// Uninstall these packages, the one the user named first.
  Uninstall(Vec<String>),
  /// Enable or disable these packages, the one the user named first.
  SetEnabled {
    packages: Vec<String>,
    enabled: bool,
  },
  /// Create the profile from a shared one, installing these packages.
  Import(Vec<String>),
}

impl Action {
  /// The packages the user asked to change. An install's dependencies are not
  /// among them.
  pub fn packages(&self) -> &[String] {
    match self {
      Action::Install(package) => std::slice::from_ref(package),
      Action::Update(packages)
      | Action::Uninstall(packages)
      | Action::SetEnabled { packages, .. }
      | Action::Import(packages) => packages,
    }
  }
}

/// Where an [`Operation`] is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationStatus {
  /// Waiting for the operations ahead of it to end.
  Queued,
  /// Running, with the latest progress once there is some.
  Running(Option<Progress>),
  /// Running, and asked to stop after the package it is on.
  Cancelling(Option<Progress>),
}

/// How far an operation has got with one package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
  /// The package, as `Owner-Name`.
  pub package: String,
  pub stage: Stage,
  /// Units done: bytes when downloading, archive entries when extracting,
  /// files when installing. Always `0` for a stage without units.
  pub done: u64,
  /// Units expected, or `None` when unknown or the stage has no units.
  pub total: Option<u64>,
}

/// The step an operation is at for one package. An install downloads,
/// extracts, then installs each package; the other stages have no units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
  Downloading,
  Extracting,
  Installing,
  Uninstalling,
  Enabling,
  Disabling,
}

/// A package an operation could not change, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedPackage {
  /// The package as `Owner-Name`, a folder an uninstall could not delete, or
  /// the profile an import could not create or restore config files into.
  pub package: String,
  pub reason: String,
}

/// What an operation did to the profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
  /// The packages added, changed, or removed, by `Owner-Name`, sorted.
  pub changed: Vec<String>,
  /// What failed. Empty when the operation succeeded.
  pub failed: Vec<FailedPackage>,
  /// What was left alone because the operation was cancelled: packages by
  /// `Owner-Name` (an install's requested package may carry its version),
  /// and folders an uninstall kept, by path. Empty when it ran to the end.
  pub cancelled: Vec<String>,
  /// Every package in the profile afterwards, or why the profile could not be
  /// read back after the operation.
  pub profile: Result<Installed, String>,
}

/// Install `package` and everything it needs into a game's profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
  /// The [`Operation`] id its progress and result carry.
  pub operation: u64,
  /// The game, by its key in the game definitions.
  pub game: String,
  /// The Thunderstore community the packages come from.
  pub community: String,
  pub profile: String,
  /// The package, as `Owner-Name`.
  pub package: String,
  /// The version the user chose, installed exactly and pinned, or `None` to
  /// install the latest and unpin it.
  pub version: Option<String>,
  /// The index the confirmation was planned from, so the install brings what
  /// the user saw.
  pub index: Index,
}

impl State {
  /// Whether the open profile holds `full_name`, an `Owner-Name`.
  pub fn is_installed(&self, full_name: &str) -> bool {
    self
      .profile
      .as_ref()
      .and_then(Profile::installed)
      .is_some_and(|installed| installed.contains_key(full_name))
  }

  /// Whether a change to the open profile can be asked for: the profile has
  /// been read, no uninstall is being checked, no launch is being prepared,
  /// no profile change is being made, and no export is reading it. A change
  /// asked for while another runs waits its turn.
  pub fn can_install(&self) -> bool {
    let ready = self.profile.as_ref().and_then(Profile::installed).is_some();

    ready
      && !self.is_checking()
      && !self.is_launching()
      && !self.is_changing_profile()
      && !self.is_profile_exporting()
  }
}

/// Opens `game`'s `name` profile as the selected one, creating it if the game
/// does not have it.
pub(crate) fn open_profile(state: &mut State, game: &str, name: &str) -> Vec<Effect> {
  state.manage.pending = None;
  state.profile = Some(Profile {
    game: game.to_string(),
    name: name.to_string(),
    status: ProfileStatus::Opening,
  });

  vec![Effect::Run(Command::OpenProfile {
    game: game.to_string(),
    profile: name.to_string(),
  })]
}

/// Applies the answer to opening `game`'s `name` profile, then lists the
/// game's profiles if they are not listed yet.
pub(crate) fn profile_opened(
  state: &mut State,
  game: String,
  name: String,
  installed: Result<Installed, String>,
) -> Vec<Effect> {
  let Some(profile) = state.profile.as_mut().filter(|profile| {
    profile.game == game && profile.name == name && profile.status == ProfileStatus::Opening
  }) else {
    return Vec::new();
  };

  profile.status = match installed {
    Ok(installed) => ProfileStatus::Ready(installed),
    Err(reason) => ProfileStatus::Failed(reason),
  };

  if let Some(count) = profile.installed().map(Installed::len) {
    crate::switcher::counted(state, &game, &name, count);
  }

  crate::switcher::profile_opened(state, &game)
}

/// Asks to confirm installing `full_name` with everything it needs, at the
/// version it is pinned at while the list still publishes it, or else the
/// latest. Ignored until the list and the profile are loaded, and for a
/// package the list does not have.
pub(crate) fn install_pressed(state: &mut State, full_name: String) -> Vec<Effect> {
  if !state.can_install() {
    return Vec::new();
  }

  let Some((installed, index)) = state.planning() else {
    return Vec::new();
  };
  let published = index.versions(&full_name);
  let pinned = installed
    .get(&full_name)
    .filter(|package| package.pinned && published.contains(&package.version))
    .map(|package| package.version.as_str());

  state.confirmation = plan(index, installed, &full_name, pinned);

  Vec::new()
}

/// Plans the waiting install again at `version`, or at the latest when
/// `None`. A version the package has not published is ignored.
pub(crate) fn version_chosen(state: &mut State, version: Option<String>) -> Vec<Effect> {
  let Some(confirmation) = state.confirmation.as_ref() else {
    return Vec::new();
  };

  if version
    .as_ref()
    .is_some_and(|version| !confirmation.versions.contains(version))
  {
    return Vec::new();
  }

  let Some((installed, index)) = state.planning() else {
    return Vec::new();
  };

  if let Some(planned) = plan(
    index,
    installed,
    &confirmation.package.full_name,
    version.as_deref(),
  ) {
    state.confirmation = Some(planned);
  }

  Vec::new()
}

impl State {
  /// The open profile's packages and the shown index an install is planned
  /// from, once both are loaded.
  fn planning(&self) -> Option<(&Installed, &Index)> {
    let installed = self.profile.as_ref().and_then(Profile::installed)?;
    let index = self.browse.as_ref()?.index.as_ref()?;

    Some((installed, index))
  }
}

/// The packages installing `full_name` at `chosen`, or at the latest when
/// `None`, brings, holding pinned packages where they are. `None` when the
/// index does not have it.
fn plan(
  index: &Index,
  installed: &Installed,
  full_name: &str,
  chosen: Option<&str>,
) -> Option<Confirmation> {
  let requested = match chosen {
    Some(version) => format!("{full_name}-{version}"),
    None => full_name.to_string(),
  };
  let resolution = DependencyGraph::new(vec![requested])
    .pins(pins::held(installed, index, &[full_name.to_string()]))
    .resolve_report(&index.0);
  let mut planned: Vec<Planned> = resolution
    .urls
    .keys()
    .filter_map(|file| parse_package_filename(file))
    .map(|(name, version)| Planned {
      installed: installed
        .get(&name)
        .is_some_and(|package| package.version == version),
      full_name: name,
      version,
    })
    .collect();

  planned.sort_by(|a, b| a.full_name.cmp(&b.full_name));

  let position = planned
    .iter()
    .position(|planned| planned.full_name == full_name)?;
  let package = planned.remove(position);

  Some(Confirmation {
    package,
    dependencies: planned,
    missing: resolution.unresolved,
    versions: index.versions(full_name),
    chosen: chosen.map(str::to_string),
  })
}

/// Starts the confirmed install, unless something it needs is missing or
/// [`State::can_install`] says it cannot.
pub(crate) fn install_confirmed(state: &mut State) -> Vec<Effect> {
  let confirmed = state
    .confirmation
    .as_ref()
    .is_some_and(Confirmation::can_confirm);

  if !confirmed || !state.can_install() {
    return Vec::new();
  }

  let Some(profile) = state.profile.as_ref() else {
    return Vec::new();
  };
  let Some(browse) = state.browse.as_ref() else {
    return Vec::new();
  };
  let Some(index) = browse.index.clone() else {
    return Vec::new();
  };
  let (game, name) = (profile.game.clone(), profile.name.clone());
  let community = browse.community.clone();
  let Some(confirmation) = state.confirmation.take() else {
    return Vec::new();
  };
  let package = confirmation.package.full_name;
  let action = Action::Install(package.clone());

  queue::enqueue(state, game.clone(), name.clone(), action, |operation| {
    Work::Install(Install {
      operation,
      game,
      community,
      profile: name,
      package,
      version: confirmation.chosen,
      index,
    })
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::browse::Index;
  use crate::notice::Notice;
  use crate::settings::{GameSettings, Settings};
  use crate::{Command, Effect, Msg, State, test_ecosystem, update};
  use std::path::PathBuf;

  /// A state that started with `game` open, its cache and profile not yet
  /// read.
  fn opened(game: &str) -> (State, Vec<Effect>) {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: Some(game.to_string()),
      games: [(
        game.to_string(),
        GameSettings::new(PathBuf::from("/games").join(game)),
      )]
      .into(),
      ..Settings::default()
    };

    update(&mut state, Msg::Started);

    let effects = update(&mut state, Msg::SettingsLoaded(Ok(settings)));

    (state, effects)
  }

  fn installed(packages: &[(&str, &str)]) -> Installed {
    crate::test_installed(packages)
  }

  fn action(state: &State) -> Option<Action> {
    state
      .queue
      .operations
      .last()
      .map(|operation| operation.action.clone())
  }

  fn profile_opened(game: &str, packages: Result<Installed, &str>) -> Msg {
    Msg::ProfileOpened {
      game: game.to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: packages.map_err(str::to_string),
    }
  }

  /// A one-version package at `version` depending on `dependencies`, each an
  /// `Owner-Name-Version`.
  fn package(full_name: &str, version: &str, dependencies: &[&str]) -> serde_json::Value {
    let (owner, name) = full_name.split_once('-').unwrap();

    serde_json::json!({
      "name": name,
      "full_name": full_name,
      "owner": owner,
      "date_created": "2024-01-01T00:00:00Z",
      "date_updated": "2024-01-01T00:00:00Z",
      "categories": [],
      "versions": [{
        "version_number": version,
        "dependencies": dependencies,
        "download_url": format!("https://example.com/{full_name}-{version}.zip"),
        "date_created": "2024-01-01T00:00:00Z"
      }]
    })
  }

  /// `Owner-Pack` needs `Owner-Lib`, which needs `Owner-Core`. `Owner-Broken`
  /// needs `Owner-Gone`, which is not in the index.
  fn index() -> Index {
    let packages: Vec<thunderstore_engine::models::Package> =
      serde_json::from_value(serde_json::json!([
        package("Owner-Pack", "2.0.0", &["Owner-Lib-1.0.0"]),
        package("Owner-Lib", "1.1.0", &["Owner-Core-1.0.0"]),
        package("Owner-Core", "1.0.0", &[]),
        package("Owner-Broken", "1.0.0", &["Owner-Gone-1.0.0"]),
      ]))
      .unwrap();

    thunderstore_engine::models::PackageIndex::from(packages).into()
  }

  /// A state with valheim open, its list and profile loaded.
  fn ready(packages: &[(&str, &str)]) -> State {
    let (mut state, _) = opened("valheim");

    update(
      &mut state,
      Msg::CachedIndexRead {
        community: "valheim".to_string(),
        index: Ok(Some(index())),
      },
    );
    update(
      &mut state,
      profile_opened("valheim", Ok(installed(packages))),
    );

    state
  }

  fn planned(full_name: &str, version: &str, installed: bool) -> Planned {
    Planned {
      full_name: full_name.to_string(),
      version: version.to_string(),
      installed,
    }
  }

  fn press(state: &mut State, full_name: &str) -> Vec<Effect> {
    update(state, Msg::InstallPressed(full_name.to_string()))
  }

  #[test]
  fn pressing_install_asks_to_confirm_the_package_and_every_dependency() {
    let mut state = ready(&[]);

    let effects = press(&mut state, "Owner-Pack");

    assert_eq!(effects, vec![]);
    assert_eq!(
      state.confirmation,
      Some(Confirmation {
        package: planned("Owner-Pack", "2.0.0", false),
        dependencies: vec![
          planned("Owner-Core", "1.0.0", false),
          planned("Owner-Lib", "1.1.0", false),
        ],
        missing: vec![],
        versions: names(&["2.0.0"]),
        chosen: None,
      })
    );
  }

  #[test]
  fn the_confirmation_marks_dependencies_already_installed_at_that_version() {
    let mut state = ready(&[("Owner-Core", "1.0.0"), ("Owner-Lib", "1.0.0")]);

    press(&mut state, "Owner-Pack");

    assert_eq!(
      state
        .confirmation
        .map(|confirmation| confirmation.dependencies),
      Some(vec![
        planned("Owner-Core", "1.0.0", true),
        planned("Owner-Lib", "1.1.0", false),
      ])
    );
  }

  #[test]
  fn a_package_with_a_missing_dependency_names_it_and_cannot_be_confirmed() {
    let mut state = ready(&[]);

    press(&mut state, "Owner-Broken");

    let confirmation = state.confirmation.clone().unwrap();

    assert_eq!(confirmation.missing, vec!["Owner-Gone".to_string()]);
    assert!(!confirmation.can_confirm());
    assert_eq!(update(&mut state, Msg::InstallConfirmed), vec![]);
    assert!(state.queue.operations.is_empty());
  }

  #[test]
  fn dismissing_the_confirmation_installs_nothing() {
    let mut state = ready(&[]);

    press(&mut state, "Owner-Pack");

    let effects = update(&mut state, Msg::InstallDismissed);

    assert_eq!(effects, vec![]);
    assert_eq!(state.confirmation, None);
    assert!(state.queue.operations.is_empty());
  }

  #[test]
  fn install_does_nothing_until_the_profile_is_open() {
    let (mut state, _) = opened("valheim");

    update(
      &mut state,
      Msg::CachedIndexRead {
        community: "valheim".to_string(),
        index: Ok(Some(index())),
      },
    );
    press(&mut state, "Owner-Pack");

    assert_eq!(state.confirmation, None);
  }

  fn status(state: &State) -> Option<OperationStatus> {
    state
      .queue
      .operations
      .last()
      .map(|operation| operation.status.clone())
  }

  /// A state with valheim open and `Owner-Pack` being installed as
  /// operation 1.
  fn installing() -> State {
    let mut state = ready(&[]);

    press(&mut state, "Owner-Pack");
    update(&mut state, Msg::InstallConfirmed);

    state
  }

  fn progress(package: &str, stage: Stage, done: u64) -> Progress {
    Progress {
      package: package.to_string(),
      stage,
      done,
      total: Some(10),
    }
  }

  fn finished(operation: u64, outcome: Result<Outcome, &str>) -> Msg {
    Msg::OperationFinished {
      operation,
      outcome: outcome.map_err(str::to_string),
    }
  }

  fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
  }

  #[test]
  fn confirming_installs_into_the_open_profile() {
    let mut state = ready(&[]);

    press(&mut state, "Owner-Pack");

    let effects = update(&mut state, Msg::InstallConfirmed);
    let index = state.browse.as_ref().unwrap().index.clone().unwrap();

    assert_eq!(
      effects,
      vec![Effect::Run(Command::Install(Install {
        operation: 1,
        game: "valheim".to_string(),
        community: "valheim".to_string(),
        profile: "Default".to_string(),
        package: "Owner-Pack".to_string(),
        version: None,
        index,
      }))]
    );
    assert_eq!(state.confirmation, None);
    assert_eq!(
      action(&state),
      Some(Action::Install("Owner-Pack".to_string()))
    );
    assert_eq!(status(&state), Some(OperationStatus::Running(None)));
  }

  #[test]
  fn an_install_confirmed_while_another_runs_waits_its_turn() {
    let mut state = installing();

    press(&mut state, "Owner-Core");

    assert!(state.confirmation.is_some());
    assert_eq!(update(&mut state, Msg::InstallConfirmed), vec![]);
    assert_eq!(
      action(&state),
      Some(Action::Install("Owner-Core".to_string()))
    );
    assert_eq!(status(&state), Some(OperationStatus::Queued));
  }

  #[test]
  fn progress_shows_the_package_and_stage_being_worked_on() {
    let mut state = installing();

    update(
      &mut state,
      Msg::OperationProgress {
        operation: 1,
        progress: progress("Owner-Lib", Stage::Extracting, 4),
      },
    );
    update(
      &mut state,
      Msg::OperationProgress {
        operation: 2,
        progress: progress("Owner-Other", Stage::Installing, 9),
      },
    );

    assert_eq!(
      status(&state),
      Some(OperationStatus::Running(Some(progress(
        "Owner-Lib",
        Stage::Extracting,
        4
      ))))
    );
  }

  #[test]
  fn a_finished_install_marks_the_new_packages_installed_and_says_what_it_installed() {
    let mut state = installing();
    let profile = installed(&[
      ("Owner-Core", "1.0.0"),
      ("Owner-Lib", "1.1.0"),
      ("Owner-Pack", "2.0.0"),
    ]);

    let effects = update(
      &mut state,
      finished(
        1,
        Ok(Outcome {
          changed: names(&["Owner-Core", "Owner-Lib", "Owner-Pack"]),
          failed: vec![],
          cancelled: vec![],
          profile: Ok(profile),
        }),
      ),
    );

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::OperationFinished {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        action: Action::Install("Owner-Pack".to_string()),
        changed: names(&["Owner-Core", "Owner-Lib", "Owner-Pack"]),
      })]
    );
    assert_eq!(status(&state), None);
    assert!(state.is_installed("Owner-Pack"));
    assert!(state.is_installed("Owner-Core"));
  }

  #[test]
  fn a_partly_failed_install_names_what_failed_and_what_was_installed() {
    let mut state = installing();
    let failure = FailedPackage {
      package: "Owner-Lib".to_string(),
      reason: "corrupt archive".to_string(),
    };

    let effects = update(
      &mut state,
      finished(
        1,
        Ok(Outcome {
          changed: names(&["Owner-Core"]),
          failed: vec![failure.clone()],
          cancelled: vec![],
          profile: Ok(installed(&[("Owner-Core", "1.0.0")])),
        }),
      ),
    );

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::OperationFailed {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        action: Action::Install("Owner-Pack".to_string()),
        failed: vec![failure],
        changed: names(&["Owner-Core"]),
      })]
    );
    assert_eq!(status(&state), None);
    assert!(state.is_installed("Owner-Core"));
    assert!(!state.is_installed("Owner-Pack"));
  }

  #[test]
  fn an_install_that_could_not_run_names_the_package_and_changes_no_marks() {
    let mut state = ready(&[("Owner-Core", "1.0.0")]);

    press(&mut state, "Owner-Pack");
    update(&mut state, Msg::InstallConfirmed);

    let effects = update(&mut state, finished(1, Err("disk full")));

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::OperationFailed {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        action: Action::Install("Owner-Pack".to_string()),
        failed: vec![FailedPackage {
          package: "Owner-Pack".to_string(),
          reason: "disk full".to_string(),
        }],
        changed: vec![],
      })]
    );
    assert_eq!(status(&state), None);
    assert!(state.is_installed("Owner-Core"));
  }

  #[test]
  fn an_install_whose_profile_cannot_be_read_back_names_what_it_installed() {
    let mut state = ready(&[("Owner-Core", "1.0.0")]);

    press(&mut state, "Owner-Pack");
    update(&mut state, Msg::InstallConfirmed);

    let effects = update(
      &mut state,
      finished(
        1,
        Ok(Outcome {
          changed: names(&["Owner-Lib", "Owner-Pack"]),
          failed: vec![],
          cancelled: vec![],
          profile: Err("mods.yml: permission denied".to_string()),
        }),
      ),
    );

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::OperationFinished {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        action: Action::Install("Owner-Pack".to_string()),
        changed: names(&["Owner-Lib", "Owner-Pack"]),
      })]
    );
    assert_eq!(status(&state), None);
    assert_eq!(
      state.profile.map(|profile| profile.status),
      Some(ProfileStatus::Failed(
        "mods.yml: permission denied".to_string()
      ))
    );
  }

  #[test]
  fn an_install_keeps_running_after_leaving_its_game() {
    let mut state = installing();

    update(&mut state, Msg::SwitchGamePressed);
    update(
      &mut state,
      finished(
        1,
        Ok(Outcome {
          changed: names(&["Owner-Pack"]),
          failed: vec![],
          cancelled: vec![],
          profile: Ok(installed(&[("Owner-Pack", "2.0.0")])),
        }),
      ),
    );

    assert_eq!(status(&state), None);
    assert_eq!(state.profile, None);
  }

  #[test]
  fn a_failed_install_leaves_the_queue_and_another_can_start() {
    let mut state = installing();

    update(&mut state, finished(1, Err("offline")));

    assert!(state.queue.operations.is_empty());

    press(&mut state, "Owner-Core");
    update(&mut state, Msg::InstallConfirmed);

    assert_eq!(
      state.queue.operations.last().map(|operation| operation.id),
      Some(2)
    );
  }

  #[test]
  fn leaving_the_game_drops_an_unconfirmed_install() {
    let mut state = ready(&[]);

    press(&mut state, "Owner-Pack");
    update(&mut state, Msg::SwitchGamePressed);

    assert_eq!(state.confirmation, None);
  }

  /// A package published at each `(version, dependencies)`, in order, the
  /// last most recently.
  fn published(full_name: &str, versions: &[(&str, &[&str])]) -> serde_json::Value {
    let (owner, name) = full_name.split_once('-').unwrap();
    let versions: Vec<serde_json::Value> = versions
      .iter()
      .enumerate()
      .map(|(day, (version, dependencies))| {
        serde_json::json!({
          "version_number": version,
          "dependencies": dependencies,
          "download_url": format!("https://example.com/{full_name}-{version}.zip"),
          "date_created": format!("2024-01-{:02}T00:00:00Z", day + 1),
        })
      })
      .collect();

    serde_json::json!({
      "name": name,
      "full_name": full_name,
      "owner": owner,
      "date_created": "2024-01-01T00:00:00Z",
      "date_updated": "2024-01-01T00:00:00Z",
      "categories": [],
      "versions": versions,
    })
  }

  /// A state with valheim open, its profile holding `packages`, and its list
  /// holding `Owner-Pack` at 1.0.0 needing nothing and at 2.0.0 and 2.1.0
  /// needing `Owner-Lib`, which is at 1.0.0 and 1.1.0.
  fn versioned(packages: &[(&str, &str)]) -> State {
    let mut state = ready(packages);
    let packages: Vec<thunderstore_engine::models::Package> =
      serde_json::from_value(serde_json::json!([
        published(
          "Owner-Pack",
          &[
            ("1.0.0", &[]),
            ("2.0.0", &["Owner-Lib-1.0.0"]),
            ("2.1.0", &["Owner-Lib-1.0.0"]),
          ]
        ),
        published("Owner-Lib", &[("1.0.0", &[]), ("1.1.0", &[])]),
      ]))
      .unwrap();
    let index: Index = thunderstore_engine::models::PackageIndex::from(packages).into();

    state.browse.as_mut().unwrap().index = Some(index);

    state
  }

  fn choose(state: &mut State, version: Option<&str>) -> Vec<Effect> {
    update(state, Msg::VersionChosen(version.map(str::to_string)))
  }

  #[test]
  fn the_confirmation_offers_every_published_version_with_the_latest_chosen() {
    let mut state = versioned(&[]);

    press(&mut state, "Owner-Pack");

    let confirmation = state.confirmation.unwrap();

    assert_eq!(confirmation.versions, names(&["2.1.0", "2.0.0", "1.0.0"]));
    assert_eq!(confirmation.chosen, None);
    assert_eq!(confirmation.package, planned("Owner-Pack", "2.1.0", false));
  }

  #[test]
  fn choosing_a_version_plans_that_version_with_what_it_needs() {
    let mut state = versioned(&[]);

    press(&mut state, "Owner-Pack");

    assert_eq!(choose(&mut state, Some("1.0.0")), vec![]);

    let confirmation = state.confirmation.clone().unwrap();

    assert_eq!(confirmation.chosen.as_deref(), Some("1.0.0"));
    assert_eq!(confirmation.package, planned("Owner-Pack", "1.0.0", false));
    assert_eq!(confirmation.dependencies, vec![]);

    choose(&mut state, None);

    let confirmation = state.confirmation.unwrap();

    assert_eq!(confirmation.chosen, None);
    assert_eq!(confirmation.package, planned("Owner-Pack", "2.1.0", false));
    assert_eq!(
      confirmation.dependencies,
      vec![planned("Owner-Lib", "1.1.0", false)]
    );
  }

  #[test]
  fn a_version_the_package_does_not_publish_cannot_be_chosen() {
    let mut state = versioned(&[]);

    press(&mut state, "Owner-Pack");
    choose(&mut state, Some("9.9.9"));

    assert_eq!(state.confirmation.unwrap().chosen, None);
  }

  #[test]
  fn confirming_a_chosen_version_installs_exactly_that_version() {
    let mut state = versioned(&[]);

    press(&mut state, "Owner-Pack");
    choose(&mut state, Some("2.0.0"));

    let effects = update(&mut state, Msg::InstallConfirmed);
    let index = state.browse.as_ref().unwrap().index.clone().unwrap();

    assert_eq!(
      effects,
      vec![Effect::Run(Command::Install(Install {
        operation: 1,
        game: "valheim".to_string(),
        community: "valheim".to_string(),
        profile: "Default".to_string(),
        package: "Owner-Pack".to_string(),
        version: Some("2.0.0".to_string()),
        index,
      }))]
    );
  }

  /// Pins `full_name` in the open profile at the version it is at.
  fn pin(state: &mut State, full_name: &str) {
    let Some(ProfileStatus::Ready(installed)) = state.profile.as_mut().map(|open| &mut open.status)
    else {
      panic!("the profile is not open");
    };

    installed.get_mut(full_name).unwrap().pinned = true;
  }

  #[test]
  fn the_confirmation_for_a_pinned_package_starts_at_its_pinned_version() {
    let mut state = versioned(&[("Owner-Pack", "2.0.0"), ("Owner-Lib", "1.0.0")]);

    pin(&mut state, "Owner-Pack");
    press(&mut state, "Owner-Pack");

    let confirmation = state.confirmation.unwrap();

    assert_eq!(confirmation.chosen.as_deref(), Some("2.0.0"));
    assert_eq!(confirmation.package, planned("Owner-Pack", "2.0.0", true));
  }

  #[test]
  fn a_pinned_version_the_list_no_longer_publishes_confirms_the_latest() {
    let mut state = versioned(&[("Owner-Pack", "1.5.0")]);

    pin(&mut state, "Owner-Pack");
    press(&mut state, "Owner-Pack");

    let confirmation = state.confirmation.unwrap();

    assert_eq!(confirmation.chosen, None);
    assert_eq!(confirmation.package, planned("Owner-Pack", "2.1.0", false));
  }

  #[test]
  fn the_confirmation_holds_pinned_dependencies_at_their_version() {
    let mut state = versioned(&[("Owner-Lib", "1.0.0")]);

    pin(&mut state, "Owner-Lib");
    press(&mut state, "Owner-Pack");

    assert_eq!(
      state.confirmation.unwrap().dependencies,
      vec![planned("Owner-Lib", "1.0.0", true)]
    );
  }

  #[test]
  fn opening_a_game_opens_its_default_profile() {
    let (state, effects) = opened("valheim");

    assert!(effects.contains(&Effect::Run(Command::OpenProfile {
      game: "valheim".to_string(),
      profile: "Default".to_string(),
    })));
    assert_eq!(state.profile.as_ref().and_then(Profile::installed), None);
  }

  #[test]
  fn an_opened_profile_marks_its_packages_installed() {
    let (mut state, _) = opened("valheim");

    let effects = update(
      &mut state,
      profile_opened("valheim", Ok(installed(&[("Owner-ModA", "1.0.0")]))),
    );

    assert_eq!(
      effects,
      vec![Effect::Run(Command::ListProfiles("valheim".to_string()))]
    );
    assert!(state.is_installed("Owner-ModA"));
    assert!(!state.is_installed("Owner-ModB"));
  }

  #[test]
  fn a_profile_that_cannot_be_opened_says_why() {
    let (mut state, _) = opened("valheim");

    update(
      &mut state,
      profile_opened("valheim", Err("permission denied")),
    );

    assert_eq!(
      state.profile.map(|profile| profile.status),
      Some(ProfileStatus::Failed("permission denied".to_string()))
    );
  }

  #[test]
  fn a_profile_answer_for_a_game_no_longer_open_is_ignored() {
    let (mut state, _) = opened("valheim");

    update(&mut state, Msg::SwitchGamePressed);
    update(
      &mut state,
      profile_opened("valheim", Ok(installed(&[("Owner-ModA", "1.0.0")]))),
    );

    assert_eq!(state.profile, None);
    assert!(!state.is_installed("Owner-ModA"));
  }
}
