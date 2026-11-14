//! The open game's profile and installing a package into it: which packages
//! an install brings, the confirmation, and the running operation.

use crate::browse::Index;
use crate::{Command, Effect, State};
use std::collections::BTreeMap;
use thunderstore_engine::extract::parse_package_filename;
use thunderstore_engine::models::{DependencyGraph, PackageIndex};

/// The profile every game has, created the first time the game is opened.
pub const DEFAULT_PROFILE: &str = "Default";

/// The packages recorded in a profile: each `Owner-Name` with its version.
pub type Installed = BTreeMap<String, String>;

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
}

impl Confirmation {
  /// Whether the install can go ahead.
  pub fn can_confirm(&self) -> bool {
    self.missing.is_empty()
  }
}

/// An install the user confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
  /// Unique per app run, so progress and the result can be matched to it.
  pub id: u64,
  /// The game, by its key in the game definitions.
  pub game: String,
  pub profile: String,
  /// The package the user asked for, as `Owner-Name`.
  pub package: String,
  pub status: OperationStatus,
}

/// Where an [`Operation`] is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationStatus {
  /// Running, with the latest progress once there is some.
  Running(Option<Progress>),
  /// Every package installed. `installed` lists those that changed.
  Finished { installed: Vec<String> },
  /// Something failed. `installed` lists the packages that changed before or
  /// despite the failure, and is empty when the profile is as it was.
  Failed {
    failed: Vec<FailedPackage>,
    installed: Vec<String>,
  },
}

/// How far an install has got with one package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
  /// The package, as `Owner-Name`.
  pub package: String,
  pub stage: Stage,
  /// Units done: bytes when downloading, archive entries when extracting,
  /// files when installing.
  pub done: u64,
  /// Units expected, or `None` when unknown.
  pub total: Option<u64>,
}

/// The step an install is at for one package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
  Downloading,
  Extracting,
  Installing,
}

/// A package an install could not install, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedPackage {
  /// The package, as `Owner-Name`.
  pub package: String,
  pub reason: String,
}

/// What an install did to the profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
  /// The packages added or changed, by `Owner-Name`, sorted.
  pub installed: Vec<String>,
  /// What failed. Empty when the install succeeded.
  pub failed: Vec<FailedPackage>,
  /// Every package in the profile afterwards, or why the profile could not be
  /// read back after the install.
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

  /// Whether an install is running, so no other can start.
  pub fn is_busy(&self) -> bool {
    self
      .operation
      .as_ref()
      .is_some_and(|operation| matches!(operation.status, OperationStatus::Running(_)))
  }

  /// Whether an install can start: the open profile has been read, and no
  /// install is running, no launch is being prepared, and no profile change
  /// is being made.
  pub fn can_install(&self) -> bool {
    let ready = self.profile.as_ref().and_then(Profile::installed).is_some();

    ready && !self.is_busy() && !self.is_launching() && !self.is_changing_profile()
  }
}

/// Opens `game`'s `name` profile as the selected one, creating it if the game
/// does not have it.
pub(crate) fn open_profile(state: &mut State, game: &str, name: &str) -> Vec<Effect> {
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

  crate::switcher::profile_opened(state, &game)
}

/// Asks to confirm installing `full_name` with everything it needs. Ignored
/// until the list and the profile are loaded, and for a package the list
/// does not have.
pub(crate) fn install_pressed(state: &mut State, full_name: String) -> Vec<Effect> {
  if !state.can_install() {
    return Vec::new();
  }

  let Some(installed) = state.profile.as_ref().and_then(Profile::installed) else {
    return Vec::new();
  };
  let Some(index) = state
    .browse
    .as_ref()
    .and_then(|browse| browse.index.as_ref())
  else {
    return Vec::new();
  };

  state.confirmation = plan(&index.0, installed, &full_name);

  Vec::new()
}

/// The packages installing `full_name` brings, or `None` when the index does
/// not have it.
fn plan(index: &PackageIndex, installed: &Installed, full_name: &str) -> Option<Confirmation> {
  let resolution = DependencyGraph::new(vec![full_name.to_string()]).resolve_report(index);
  let mut planned: Vec<Planned> = resolution
    .urls
    .keys()
    .filter_map(|file| parse_package_filename(file))
    .map(|(name, version)| Planned {
      installed: installed.get(&name) == Some(&version),
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
  let community = browse.community.clone();
  let Some(confirmation) = state.confirmation.take() else {
    return Vec::new();
  };

  state.operations += 1;

  let operation = Operation {
    id: state.operations,
    game: profile.game.clone(),
    profile: profile.name.clone(),
    package: confirmation.package.full_name,
    status: OperationStatus::Running(None),
  };
  let install = Install {
    operation: operation.id,
    game: operation.game.clone(),
    community,
    profile: operation.profile.clone(),
    package: operation.package.clone(),
    index,
  };

  state.operation = Some(operation);

  vec![Effect::Run(Command::Install(install))]
}

/// The running operation with id `id`, if there is one.
fn running(state: &mut State, id: u64) -> Option<&mut Operation> {
  state.operation.as_mut().filter(|operation| {
    operation.id == id && matches!(operation.status, OperationStatus::Running(_))
  })
}

pub(crate) fn install_progress(state: &mut State, id: u64, progress: Progress) -> Vec<Effect> {
  if let Some(operation) = running(state, id) {
    operation.status = OperationStatus::Running(Some(progress));
  }

  Vec::new()
}

/// Applies the result of operation `id`: what it did, or why it could not run
/// at all, which leaves the profile as it was.
pub(crate) fn install_finished(
  state: &mut State,
  id: u64,
  outcome: Result<Outcome, String>,
) -> Vec<Effect> {
  let Some(operation) = running(state, id) else {
    return Vec::new();
  };

  let Outcome {
    installed,
    failed,
    profile,
  } = match outcome {
    Ok(outcome) => outcome,
    Err(reason) => {
      operation.status = OperationStatus::Failed {
        failed: vec![FailedPackage {
          package: operation.package.clone(),
          reason,
        }],
        installed: Vec::new(),
      };

      return Vec::new();
    }
  };

  operation.status = if failed.is_empty() {
    OperationStatus::Finished { installed }
  } else {
    OperationStatus::Failed { failed, installed }
  };

  let (game, name) = (operation.game.clone(), operation.profile.clone());

  if let Some(open) = state
    .profile
    .as_mut()
    .filter(|open| open.game == game && open.name == name)
  {
    open.status = match profile {
      Ok(installed) => ProfileStatus::Ready(installed),
      Err(reason) => ProfileStatus::Failed(reason),
    };
  }

  Vec::new()
}

/// Clears the result of an install that has ended.
pub(crate) fn operation_dismissed(state: &mut State) -> Vec<Effect> {
  if !state.is_busy() {
    state.operation = None;
  }

  Vec::new()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::browse::Index;
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
        GameSettings {
          folder: PathBuf::from("/games").join(game),
          profile: None,
        },
      )]
      .into(),
    };

    update(&mut state, Msg::Started);

    let effects = update(&mut state, Msg::SettingsLoaded(Ok(settings)));

    (state, effects)
  }

  fn installed(packages: &[(&str, &str)]) -> Installed {
    packages
      .iter()
      .map(|(name, version)| (name.to_string(), version.to_string()))
      .collect()
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
    assert_eq!(state.operation, None);
  }

  #[test]
  fn dismissing_the_confirmation_installs_nothing() {
    let mut state = ready(&[]);

    press(&mut state, "Owner-Pack");

    let effects = update(&mut state, Msg::InstallDismissed);

    assert_eq!(effects, vec![]);
    assert_eq!(state.confirmation, None);
    assert_eq!(state.operation, None);
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
      .operation
      .as_ref()
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
    Msg::InstallFinished {
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
        index,
      }))]
    );
    assert_eq!(state.confirmation, None);
    assert_eq!(status(&state), Some(OperationStatus::Running(None)));
  }

  #[test]
  fn install_is_ignored_while_an_install_runs() {
    let mut state = installing();

    press(&mut state, "Owner-Core");

    assert_eq!(state.confirmation, None);
  }

  #[test]
  fn progress_shows_the_package_and_stage_being_worked_on() {
    let mut state = installing();

    update(
      &mut state,
      Msg::InstallProgress {
        operation: 1,
        progress: progress("Owner-Lib", Stage::Extracting, 4),
      },
    );
    update(
      &mut state,
      Msg::InstallProgress {
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
  fn a_finished_install_marks_the_new_packages_installed() {
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
          installed: names(&["Owner-Core", "Owner-Lib", "Owner-Pack"]),
          failed: vec![],
          profile: Ok(profile),
        }),
      ),
    );

    assert_eq!(effects, vec![]);
    assert_eq!(
      status(&state),
      Some(OperationStatus::Finished {
        installed: names(&["Owner-Core", "Owner-Lib", "Owner-Pack"]),
      })
    );
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

    update(
      &mut state,
      finished(
        1,
        Ok(Outcome {
          installed: names(&["Owner-Core"]),
          failed: vec![failure.clone()],
          profile: Ok(installed(&[("Owner-Core", "1.0.0")])),
        }),
      ),
    );

    assert_eq!(
      status(&state),
      Some(OperationStatus::Failed {
        failed: vec![failure],
        installed: names(&["Owner-Core"]),
      })
    );
    assert!(state.is_installed("Owner-Core"));
    assert!(!state.is_installed("Owner-Pack"));
  }

  #[test]
  fn an_install_that_could_not_run_names_the_package_and_changes_no_marks() {
    let mut state = ready(&[("Owner-Core", "1.0.0")]);

    press(&mut state, "Owner-Pack");
    update(&mut state, Msg::InstallConfirmed);
    update(&mut state, finished(1, Err("disk full")));

    assert_eq!(
      status(&state),
      Some(OperationStatus::Failed {
        failed: vec![FailedPackage {
          package: "Owner-Pack".to_string(),
          reason: "disk full".to_string(),
        }],
        installed: vec![],
      })
    );
    assert!(state.is_installed("Owner-Core"));
  }

  #[test]
  fn an_install_whose_profile_cannot_be_read_back_names_what_it_installed() {
    let mut state = ready(&[("Owner-Core", "1.0.0")]);

    press(&mut state, "Owner-Pack");
    update(&mut state, Msg::InstallConfirmed);
    update(
      &mut state,
      finished(
        1,
        Ok(Outcome {
          installed: names(&["Owner-Lib", "Owner-Pack"]),
          failed: vec![],
          profile: Err("mods.yml: permission denied".to_string()),
        }),
      ),
    );

    assert_eq!(
      status(&state),
      Some(OperationStatus::Finished {
        installed: names(&["Owner-Lib", "Owner-Pack"]),
      })
    );
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
          installed: names(&["Owner-Pack"]),
          failed: vec![],
          profile: Ok(installed(&[("Owner-Pack", "2.0.0")])),
        }),
      ),
    );

    assert_eq!(
      status(&state),
      Some(OperationStatus::Finished {
        installed: names(&["Owner-Pack"]),
      })
    );
    assert_eq!(state.profile, None);
  }

  #[test]
  fn a_finished_install_can_be_dismissed_and_another_started() {
    let mut state = installing();

    update(&mut state, finished(1, Err("offline")));
    update(&mut state, Msg::OperationDismissed);

    assert_eq!(state.operation, None);

    press(&mut state, "Owner-Core");
    update(&mut state, Msg::InstallConfirmed);

    assert_eq!(state.operation.map(|operation| operation.id), Some(2));
  }

  #[test]
  fn a_running_install_cannot_be_dismissed() {
    let mut state = installing();

    update(&mut state, Msg::OperationDismissed);

    assert_eq!(status(&state), Some(OperationStatus::Running(None)));
  }

  #[test]
  fn leaving_the_game_drops_an_unconfirmed_install() {
    let mut state = ready(&[]);

    press(&mut state, "Owner-Pack");
    update(&mut state, Msg::SwitchGamePressed);

    assert_eq!(state.confirmation, None);
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
