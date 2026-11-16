//! Updates: the badges the Installed tab marks packages with, and updating
//! one package or every outdated one.

use crate::browse::Index;
use crate::install::{Action, Installed, Profile};
use crate::queue::{self, Work};
use crate::{Effect, State};
use std::collections::BTreeMap;
use thunderstore_engine::profile::modlist::SemVer;

/// What the Installed tab marks on one package.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Badges {
  /// The newer version the index has, when the package is outdated.
  pub update: Option<String>,
  /// The index marks the package deprecated.
  pub deprecated: bool,
  /// The packages it requires that the profile does not hold, by
  /// `Owner-Name`.
  pub missing: Vec<String>,
}

impl Badges {
  fn is_empty(&self) -> bool {
    *self == Badges::default()
  }
}

/// The badges of every package of `installed` that has one, by `Owner-Name`.
///
/// A package is outdated when the version `index` published last is newer
/// than the installed one, the version an update installs. Without an index,
/// only missing dependencies are known.
pub fn badges(installed: &Installed, index: Option<&Index>) -> BTreeMap<String, Badges> {
  installed
    .iter()
    .map(|(full_name, package)| {
      let missing = package
        .dependencies
        .iter()
        .filter(|needed| !installed.contains_key(*needed))
        .cloned()
        .collect();
      let listed = index.and_then(|index| index.package(full_name));
      let update = listed
        .and_then(|row| row.version_number())
        .filter(|latest| SemVer::parse(latest) > SemVer::parse(&package.version))
        .map(str::to_string);
      let badge = Badges {
        update,
        deprecated: listed.is_some_and(|row| row.is_deprecated()),
        missing,
      };

      (full_name.clone(), badge)
    })
    .filter(|(_, badge)| !badge.is_empty())
    .collect()
}

/// Update packages of a game's profile to the version the index published
/// last, with what that version needs. A disabled package stays disabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
  /// The operation id its progress and result carry.
  pub operation: u64,
  /// The game, by its key in the game definitions.
  pub game: String,
  /// The Thunderstore community the packages come from.
  pub community: String,
  pub profile: String,
  /// The packages to update, by `Owner-Name`.
  pub packages: Vec<String>,
  /// The index the badges were worked out from, so the update installs the
  /// versions the user saw.
  pub index: Index,
}

impl State {
  /// The badges of the open profile's packages against the shown package
  /// list, as [`badges`] works them out. Empty until the profile is read.
  pub fn badges(&self) -> BTreeMap<String, Badges> {
    let Some(installed) = self.profile.as_ref().and_then(Profile::installed) else {
      return BTreeMap::new();
    };
    let index = self
      .browse
      .as_ref()
      .and_then(|browse| browse.index.as_ref());

    badges(installed, index)
  }
}

/// Updates `full_name` when it is outdated.
pub(crate) fn update_pressed(state: &mut State, full_name: String) -> Vec<Effect> {
  let outdated = state
    .badges()
    .get(&full_name)
    .is_some_and(|badge| badge.update.is_some());

  if !outdated {
    return Vec::new();
  }

  start(state, vec![full_name])
}

/// Updates every outdated package of the open profile as one operation.
pub(crate) fn update_all(state: &mut State) -> Vec<Effect> {
  let outdated = state
    .badges()
    .into_iter()
    .filter(|(_, badge)| badge.update.is_some())
    .map(|(full_name, _)| full_name)
    .collect();

  start(state, outdated)
}

/// Updates `packages` of the open profile as one operation, when a change can
/// start and the package list is loaded.
fn start(state: &mut State, packages: Vec<String>) -> Vec<Effect> {
  if packages.is_empty() || !state.can_manage() {
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
  let action = Action::Update(packages.clone());

  queue::enqueue(state, game.clone(), name.clone(), action, |operation| {
    Work::Update(Update {
      operation,
      game,
      community,
      profile: name,
      packages,
      index,
    })
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::install::{DEFAULT_PROFILE, Outcome};
  use crate::settings::{GameSettings, Settings};
  use crate::{Command, Msg, test_ecosystem, update};
  use thunderstore_engine::models::PackageIndex;

  /// A package published at each of `versions`, in order, the last most
  /// recently.
  fn listed(full_name: &str, versions: &[&str], deprecated: bool) -> serde_json::Value {
    let (owner, name) = full_name.split_once('-').unwrap();
    let versions: Vec<serde_json::Value> = versions
      .iter()
      .enumerate()
      .map(|(day, version)| {
        serde_json::json!({
          "version_number": version,
          "dependencies": [],
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
      "is_deprecated": deprecated,
      "categories": [],
      "versions": versions,
    })
  }

  fn index(packages: Vec<serde_json::Value>) -> Index {
    let packages: Vec<thunderstore_engine::models::Package> =
      serde_json::from_value(serde_json::Value::Array(packages)).unwrap();

    PackageIndex::from(packages).into()
  }

  fn update_to(version: &str) -> Badges {
    Badges {
      update: Some(version.to_string()),
      ..Badges::default()
    }
  }

  #[test]
  fn an_outdated_package_carries_the_newer_version() {
    let installed = crate::test_installed(&[("Owner-Old", "1.0.0"), ("Owner-Current", "2.0.0")]);
    let index = index(vec![
      listed("Owner-Old", &["1.0.0", "1.2.0"], false),
      listed("Owner-Current", &["1.0.0", "2.0.0"], false),
    ]);

    assert_eq!(
      badges(&installed, Some(&index)),
      [("Owner-Old".to_string(), update_to("1.2.0"))].into()
    );
  }

  #[test]
  fn a_version_newer_only_in_a_later_component_is_an_update() {
    let installed = crate::test_installed(&[("Owner-Mod", "1.0.9")]);
    let index = index(vec![listed("Owner-Mod", &["1.0.9", "1.0.10"], false)]);

    assert_eq!(
      badges(&installed, Some(&index)),
      [("Owner-Mod".to_string(), update_to("1.0.10"))].into()
    );
  }

  #[test]
  fn an_older_version_published_last_is_not_an_update() {
    let installed = crate::test_installed(&[("Owner-Mod", "2.0.0")]);
    let index = index(vec![listed("Owner-Mod", &["2.0.0", "1.5.0"], false)]);

    assert_eq!(badges(&installed, Some(&index)), BTreeMap::new());
  }

  #[test]
  fn a_deprecated_package_is_marked_deprecated() {
    let installed = crate::test_installed(&[("Owner-Old", "1.0.0"), ("Owner-Fine", "1.0.0")]);
    let index = index(vec![
      listed("Owner-Old", &["1.0.0"], true),
      listed("Owner-Fine", &["1.0.0"], false),
    ]);

    assert_eq!(
      badges(&installed, Some(&index)),
      [(
        "Owner-Old".to_string(),
        Badges {
          deprecated: true,
          ..Badges::default()
        }
      )]
      .into()
    );
  }

  #[test]
  fn a_package_needing_one_the_profile_lacks_names_it_even_without_an_index() {
    let mut installed = crate::test_installed(&[("Owner-Lib", "1.0.0"), ("Owner-Pack", "1.0.0")]);

    installed.get_mut("Owner-Pack").unwrap().dependencies =
      vec!["Owner-Lib".to_string(), "Owner-Gone".to_string()];

    assert_eq!(
      badges(&installed, None),
      [(
        "Owner-Pack".to_string(),
        Badges {
          missing: vec!["Owner-Gone".to_string()],
          ..Badges::default()
        }
      )]
      .into()
    );
  }

  #[test]
  fn a_package_the_index_lacks_has_no_update_or_deprecation() {
    let installed = crate::test_installed(&[("Owner-Local", "1.0.0")]);
    let index = index(vec![listed("Owner-Other", &["9.0.0"], true)]);

    assert_eq!(badges(&installed, Some(&index)), BTreeMap::new());
  }

  /// A state with valheim open, its list holding `Owner-Old` at 1.0.0 and
  /// 1.2.0, `Owner-Stale` at 1.0.0 and 3.0.0, and `Owner-Current` at 2.0.0,
  /// and its Default profile holding each at 1.0.0 but `Owner-Current` at
  /// 2.0.0.
  fn ready() -> State {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: Some("valheim".to_string()),
      games: [("valheim".to_string(), GameSettings::new("/games/valheim"))].into(),
      ..Settings::default()
    };
    let index = index(vec![
      listed("Owner-Old", &["1.0.0", "1.2.0"], false),
      listed("Owner-Stale", &["1.0.0", "3.0.0"], false),
      listed("Owner-Current", &["2.0.0"], false),
    ]);

    update(&mut state, Msg::Started);
    update(&mut state, Msg::SettingsLoaded(Ok(settings)));
    update(
      &mut state,
      Msg::CachedIndexRead {
        community: "valheim".to_string(),
        index: Ok(Some(index)),
      },
    );
    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(crate::test_installed(&[
          ("Owner-Old", "1.0.0"),
          ("Owner-Stale", "1.0.0"),
          ("Owner-Current", "2.0.0"),
        ])),
      },
    );

    state
  }

  fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
  }

  /// The command updating `packages` in valheim's Default profile as
  /// `operation`, from the index `state` shows.
  fn updating(state: &State, operation: u64, packages: &[&str]) -> Effect {
    let index = state.browse.as_ref().unwrap().index.clone().unwrap();

    Effect::Run(Command::Update(Update {
      operation,
      game: "valheim".to_string(),
      community: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      packages: names(packages),
      index,
    }))
  }

  fn action(state: &State) -> Option<Action> {
    state
      .queue
      .operations
      .last()
      .map(|operation| operation.action.clone())
  }

  #[test]
  fn the_state_badges_the_open_profile_against_the_shown_list() {
    let state = ready();

    assert_eq!(
      state.badges(),
      [
        ("Owner-Old".to_string(), update_to("1.2.0")),
        ("Owner-Stale".to_string(), update_to("3.0.0")),
      ]
      .into()
    );
  }

  #[test]
  fn updating_an_outdated_package_updates_it_alone() {
    let mut state = ready();

    let effects = update(&mut state, Msg::UpdatePressed("Owner-Old".to_string()));

    assert_eq!(effects, vec![updating(&state, 1, &["Owner-Old"])]);
    assert_eq!(action(&state), Some(Action::Update(names(&["Owner-Old"]))));
  }

  #[test]
  fn updating_a_current_package_does_nothing() {
    let mut state = ready();

    let effects = update(&mut state, Msg::UpdatePressed("Owner-Current".to_string()));

    assert_eq!(effects, vec![]);
    assert!(state.queue.operations.is_empty());
  }

  #[test]
  fn update_all_updates_every_outdated_package_in_one_operation() {
    let mut state = ready();

    let effects = update(&mut state, Msg::UpdateAllPressed);

    assert_eq!(
      effects,
      vec![updating(&state, 1, &["Owner-Old", "Owner-Stale"])]
    );
    assert_eq!(state.queue.operations.len(), 1);
    assert_eq!(
      action(&state),
      Some(Action::Update(names(&["Owner-Old", "Owner-Stale"])))
    );
  }

  #[test]
  fn update_all_with_nothing_outdated_does_nothing() {
    let mut state = ready();

    update(&mut state, Msg::UpdatePressed("Owner-Old".to_string()));
    update(
      &mut state,
      Msg::OperationFinished {
        operation: 1,
        outcome: Ok(Outcome {
          changed: names(&["Owner-Old", "Owner-Stale"]),
          failed: vec![],
          cancelled: vec![],
          profile: Ok(crate::test_installed(&[
            ("Owner-Old", "1.2.0"),
            ("Owner-Stale", "3.0.0"),
            ("Owner-Current", "2.0.0"),
          ])),
        }),
      },
    );

    assert_eq!(state.badges(), BTreeMap::new());
    assert_eq!(update(&mut state, Msg::UpdateAllPressed), vec![]);
    assert!(state.queue.operations.is_empty());
  }

  #[test]
  fn nothing_is_updated_until_the_profiles_operation_ends() {
    let mut state = ready();

    update(&mut state, Msg::UpdatePressed("Owner-Old".to_string()));

    assert_eq!(
      update(&mut state, Msg::UpdatePressed("Owner-Stale".to_string())),
      vec![]
    );
    assert_eq!(update(&mut state, Msg::UpdateAllPressed), vec![]);
    assert_eq!(state.queue.operations.len(), 1);
  }

  #[test]
  fn nothing_is_updated_before_the_list_is_loaded() {
    let mut state = ready();

    state.browse.as_mut().unwrap().index = None;

    assert_eq!(state.badges(), BTreeMap::new());
    assert_eq!(update(&mut state, Msg::UpdateAllPressed), vec![]);
  }
}
