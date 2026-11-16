//! The Installed tab: the open profile's packages, searched and sorted, and
//! enabling, disabling, and uninstalling them with the questions that asks.

use crate::install::{Action, Installed, InstalledPackage, Profile};
use crate::queue::{self, Work};
use crate::{Command, Effect, State};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// The tab shown in a game. A game opens on the one it was left on, saved
/// by name in its settings, or else on [`Tab::Installed`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
  /// The game's package list.
  Browse,
  /// The open profile's packages.
  #[default]
  Installed,
}

impl Tab {
  pub(crate) fn is_installed(&self) -> bool {
    *self == Self::Installed
  }
}

/// The Installed tab's search, order, and the change waiting on an answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manage {
  pub query: String,
  pub sort: Sort,
  pub pending: Option<Pending>,
}

/// The order of the Installed tab.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Sort {
  /// By name without the owner, ignoring case.
  #[default]
  Name,
  /// By owner, then name, ignoring case.
  Author,
  /// Most recently installed first.
  Newest,
}

/// A change that takes a package's dependants with it, or leaves them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removal {
  Disable,
  Uninstall,
}

/// The user's answer to whether a change takes the package's dependants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
  PackageOnly,
  WithDependants,
}

/// The user's answer to whether an uninstall deletes folders no package
/// records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folders {
  Delete,
  Keep,
}

/// A change in the Installed tab that waits on the user or a check before it
/// can start. Packages are named as `Owner-Name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
  /// Other packages depend on `package`. Asks whether to change it alone or
  /// with `dependants`.
  Dependants {
    removal: Removal,
    package: String,
    dependants: Vec<String>,
  },
  /// Checking what uninstalling `packages` would delete.
  Checking { packages: Vec<String> },
  /// The profile holds `folders` that no package records, which the
  /// uninstall of `packages` would delete. Asks whether to delete them.
  Untracked {
    packages: Vec<String>,
    folders: Vec<PathBuf>,
  },
  /// Uninstalling `packages` cannot go ahead, with the reason.
  Refused {
    packages: Vec<String>,
    reason: String,
  },
}

/// Uninstall packages from a game's profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uninstall {
  /// The operation id its result carries.
  pub operation: u64,
  /// The game, by its key in the game definitions.
  pub game: String,
  pub profile: String,
  /// The packages to remove, by `Owner-Name`. Nothing else is removed.
  pub packages: Vec<String>,
  /// The folders no package records that the user agreed to delete. Empty
  /// keeps every one.
  pub delete: Vec<PathBuf>,
}

/// Enable or disable packages in a game's profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetEnabled {
  /// The operation id its result carries.
  pub operation: u64,
  /// The game, by its key in the game definitions.
  pub game: String,
  pub profile: String,
  /// The packages to change, by `Owner-Name`.
  pub packages: Vec<String>,
  pub enabled: bool,
}

/// The packages of `installed` whose name or owner contains `query`, ignoring
/// case, in `sort` order.
///
/// ```
/// use rivet_core::install::{Installed, InstalledPackage};
/// use rivet_core::manage::{Sort, rows};
///
/// let package = |author: &str| InstalledPackage {
///   author: author.to_string(),
///   version: "1.0.0".to_string(),
///   enabled: true,
///   dependencies: vec![],
///   installed_at: 0,
///   pinned: false,
/// };
/// let installed: Installed = [
///   ("Bob-Zip".to_string(), package("Bob")),
///   ("Amy-Map".to_string(), package("Amy")),
/// ]
/// .into();
///
/// let names: Vec<&str> = rows(&installed, "", Sort::Name)
///   .into_iter()
///   .map(|(full_name, _)| full_name)
///   .collect();
///
/// assert_eq!(names, ["Amy-Map", "Bob-Zip"]);
/// ```
pub fn rows<'a>(
  installed: &'a Installed,
  query: &str,
  sort: Sort,
) -> Vec<(&'a str, &'a InstalledPackage)> {
  let query = query.trim().to_lowercase();
  let mut rows: Vec<(&str, &InstalledPackage)> = installed
    .iter()
    .map(|(full_name, package)| (full_name.as_str(), package))
    .filter(|(full_name, _)| found_by(full_name, &query))
    .collect();
  let name =
    |(full_name, package): &(&str, &InstalledPackage)| package.name(full_name).to_lowercase();

  match sort {
    Sort::Name => rows.sort_by_cached_key(name),
    Sort::Author => rows.sort_by_cached_key(|row| (row.1.author.to_lowercase(), name(row))),
    Sort::Newest => rows.sort_by_key(|(_, package)| std::cmp::Reverse(package.installed_at)),
  }

  rows
}

/// Whether the Installed tab's search finds `full_name`, the search already
/// trimmed and lowercased.
fn found_by(full_name: &str, query: &str) -> bool {
  full_name.to_lowercase().contains(query)
}

/// Every package of `installed` that requires `full_name`, directly or
/// through others, sorted. `full_name` itself is never listed.
pub fn dependants(installed: &Installed, full_name: &str) -> Vec<String> {
  closure(full_name, |name| {
    installed
      .iter()
      .filter(|(_, package)| package.dependencies.iter().any(|needed| needed == name))
      .map(|(dependant, _)| dependant.clone())
      .collect()
  })
}

/// Every package of `installed` that `full_name` requires, directly or
/// through others, sorted. Requirements the profile does not hold are left
/// out, and `full_name` itself is never listed.
pub fn dependencies(installed: &Installed, full_name: &str) -> Vec<String> {
  closure(full_name, |name| {
    installed
      .get(name)
      .map(|package| {
        package
          .dependencies
          .iter()
          .filter(|needed| installed.contains_key(*needed))
          .cloned()
          .collect()
      })
      .unwrap_or_default()
  })
}

/// Every name reachable from `start` through `next`, sorted, without `start`.
fn closure(start: &str, next: impl Fn(&str) -> Vec<String>) -> Vec<String> {
  let mut found = BTreeSet::new();
  let mut todo = vec![start.to_string()];

  while let Some(name) = todo.pop() {
    for reached in next(&name) {
      if reached != start && found.insert(reached.clone()) {
        todo.push(reached);
      }
    }
  }

  found.into_iter().collect()
}

impl State {
  /// Whether `full_name` can be enabled and disabled. Mod loaders cannot.
  pub fn can_toggle(&self, full_name: &str) -> bool {
    self.ecosystem.modloader_package(full_name).is_none()
  }

  /// The open profile's packages the Installed tab lists: [`rows`] for its
  /// search and order, kept to those the package list puts in a chosen
  /// category when any is chosen. A package the list lacks is in none. Empty
  /// until the profile is read.
  pub fn installed_rows(&self) -> Vec<(&str, &InstalledPackage)> {
    let Some(installed) = self.profile.as_ref().and_then(Profile::installed) else {
      return Vec::new();
    };
    let mut rows = rows(installed, &self.manage.query, self.manage.sort);

    rows.retain(|(full_name, _)| self.in_chosen_category(full_name));

    rows
  }

  /// The open profile's `full_name` (`Owner-Name`) when the Installed tab
  /// lists it, without listing and sorting the whole profile as
  /// [`State::installed_rows`] does.
  pub(crate) fn installed_listing(&self, full_name: &str) -> Option<&InstalledPackage> {
    let package = self
      .profile
      .as_ref()
      .and_then(Profile::installed)?
      .get(full_name)?;
    let query = self.manage.query.trim().to_lowercase();

    (found_by(full_name, &query) && self.in_chosen_category(full_name)).then_some(package)
  }

  /// Whether the package list puts `full_name` in a chosen category, or
  /// none is chosen. A package the list lacks is in none.
  fn in_chosen_category(&self, full_name: &str) -> bool {
    let Some(browse) = self.browse.as_ref() else {
      return true;
    };
    let chosen = &browse.filters.categories;

    if chosen.is_empty() {
      return true;
    }

    browse
      .index
      .as_ref()
      .and_then(|index| index.package(full_name))
      .is_some_and(|row| row.categories().any(|category| chosen.contains(category)))
  }

  /// Whether an uninstall is being checked, so no other change can start.
  pub fn is_checking(&self) -> bool {
    matches!(self.manage.pending, Some(Pending::Checking { .. }))
  }

  /// Whether the open profile's packages can be enabled, disabled, or
  /// uninstalled: as for [`State::can_install`], and no operation on the
  /// profile is running or waiting. Those changes are planned from the
  /// packages shown, which an operation still to end would make stale.
  pub fn can_manage(&self) -> bool {
    self.can_install() && !self.is_profile_busy()
  }

  /// The open profile's packages, once they are known and a change can start.
  fn changeable(&self) -> Option<&Installed> {
    if !self.can_manage() {
      return None;
    }

    self.profile.as_ref().and_then(Profile::installed)
  }
}

/// Enables `full_name` and the disabled packages it requires.
pub(crate) fn enable_pressed(state: &mut State, full_name: String) -> Vec<Effect> {
  let Some(installed) = state.changeable() else {
    return Vec::new();
  };

  if !installed.contains_key(&full_name) {
    return Vec::new();
  }

  let packages: Vec<String> = std::iter::once(full_name.clone())
    .chain(dependencies(installed, &full_name))
    .filter(|name| installed.get(name).is_some_and(|package| !package.enabled))
    .filter(|name| state.can_toggle(name))
    .collect();

  set_enabled(state, packages, true)
}

/// Disables `full_name`, first asking whether to disable its enabled
/// dependants too when it has any.
pub(crate) fn disable_pressed(state: &mut State, full_name: String) -> Vec<Effect> {
  let Some(installed) = state.changeable() else {
    return Vec::new();
  };
  let enabled = |name: &str| installed.get(name).is_some_and(|package| package.enabled);

  if !enabled(&full_name) || !state.can_toggle(&full_name) {
    return Vec::new();
  }

  let dependants: Vec<String> = dependants(installed, &full_name)
    .into_iter()
    .filter(|name| enabled(name) && state.can_toggle(name))
    .collect();

  if dependants.is_empty() {
    return set_enabled(state, vec![full_name], false);
  }

  state.manage.pending = Some(Pending::Dependants {
    removal: Removal::Disable,
    package: full_name,
    dependants,
  });

  Vec::new()
}

/// Uninstalls `full_name`, first asking whether to uninstall its dependants
/// too when it has any.
pub(crate) fn uninstall_pressed(state: &mut State, full_name: String) -> Vec<Effect> {
  let Some(installed) = state.changeable() else {
    return Vec::new();
  };

  if !installed.contains_key(&full_name) {
    return Vec::new();
  }

  let dependants = dependants(installed, &full_name);

  if dependants.is_empty() {
    return check_uninstall(state, vec![full_name]);
  }

  state.manage.pending = Some(Pending::Dependants {
    removal: Removal::Uninstall,
    package: full_name,
    dependants,
  });

  Vec::new()
}

/// Enables every disabled package, or disables every enabled one. Mod loaders
/// are left alone.
pub(crate) fn set_all(state: &mut State, enabled: bool) -> Vec<Effect> {
  let Some(installed) = state.changeable() else {
    return Vec::new();
  };
  let packages: Vec<String> = installed
    .iter()
    .filter(|(name, package)| package.enabled != enabled && state.can_toggle(name))
    .map(|(name, _)| name.clone())
    .collect();

  set_enabled(state, packages, enabled)
}

/// Goes ahead with the change waiting on the dependants question, taking the
/// dependants only when `scope` says so.
pub(crate) fn dependants_chosen(state: &mut State, scope: Scope) -> Vec<Effect> {
  if state.changeable().is_none() {
    return Vec::new();
  }

  let Some(Pending::Dependants {
    removal,
    package,
    dependants,
  }) = state.manage.pending.take()
  else {
    return Vec::new();
  };
  let mut packages = vec![package];

  if scope == Scope::WithDependants {
    packages.extend(dependants);
  }

  match removal {
    Removal::Disable => set_enabled(state, packages, false),
    Removal::Uninstall => check_uninstall(state, packages),
  }
}

/// Asks the commands thread what uninstalling `packages` would delete.
fn check_uninstall(state: &mut State, packages: Vec<String>) -> Vec<Effect> {
  let Some(profile) = state.profile.as_ref() else {
    return Vec::new();
  };
  let command = Command::CheckUninstall {
    game: profile.game.clone(),
    profile: profile.name.clone(),
    packages: packages.clone(),
  };

  state.manage.pending = Some(Pending::Checking { packages });

  vec![Effect::Run(command)]
}

/// Applies the check of an uninstall: starts it when there are no folders to
/// ask about, asks when there are, or reports why it cannot go ahead.
pub(crate) fn uninstall_checked(
  state: &mut State,
  game: String,
  name: String,
  packages: Vec<String>,
  untracked: Result<Vec<PathBuf>, String>,
) -> Vec<Effect> {
  let awaited = state
    .profile
    .as_ref()
    .is_some_and(|profile| profile.game == game && profile.name == name)
    && matches!(&state.manage.pending, Some(Pending::Checking { packages: checking }) if *checking == packages);

  if !awaited {
    return Vec::new();
  }

  match untracked {
    Ok(folders) if folders.is_empty() => {
      state.manage.pending = None;

      uninstall(state, packages, Vec::new())
    }
    Ok(folders) => {
      state.manage.pending = Some(Pending::Untracked { packages, folders });

      Vec::new()
    }
    Err(reason) => {
      state.manage.pending = Some(Pending::Refused { packages, reason });

      Vec::new()
    }
  }
}

/// Goes ahead with the uninstall waiting on the folders question, deleting
/// the folders only when `folders` says so.
pub(crate) fn folders_chosen(state: &mut State, folders: Folders) -> Vec<Effect> {
  if state.changeable().is_none() {
    return Vec::new();
  }

  let Some(Pending::Untracked {
    packages,
    folders: untracked,
  }) = state.manage.pending.take()
  else {
    return Vec::new();
  };
  let delete = match folders {
    Folders::Delete => untracked,
    Folders::Keep => Vec::new(),
  };

  uninstall(state, packages, delete)
}

fn uninstall(state: &mut State, packages: Vec<String>, delete: Vec<PathBuf>) -> Vec<Effect> {
  let Some(profile) = state.profile.as_ref() else {
    return Vec::new();
  };
  let (game, name) = (profile.game.clone(), profile.name.clone());
  let action = Action::Uninstall(packages.clone());

  queue::enqueue(state, game.clone(), name.clone(), action, |operation| {
    Work::Uninstall(Uninstall {
      operation,
      game,
      profile: name,
      packages,
      delete,
    })
  })
}

fn set_enabled(state: &mut State, packages: Vec<String>, enabled: bool) -> Vec<Effect> {
  if packages.is_empty() {
    return Vec::new();
  }

  let Some(profile) = state.profile.as_ref() else {
    return Vec::new();
  };
  let (game, name) = (profile.game.clone(), profile.name.clone());
  let action = Action::SetEnabled {
    packages: packages.clone(),
    enabled,
  };

  queue::enqueue(state, game.clone(), name.clone(), action, |operation| {
    Work::SetEnabled(SetEnabled {
      operation,
      game,
      profile: name,
      packages,
      enabled,
    })
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::install::{DEFAULT_PROFILE, Outcome};
  use crate::settings::{GameSettings, Settings};
  use crate::{Msg, test_ecosystem, update};

  const LOADER: &str = "denikson-BepInExPack_Valheim";

  /// An enabled package of `full_name` needing `dependencies`.
  fn package(full_name: &str, dependencies: &[&str]) -> (String, InstalledPackage) {
    let mut installed = crate::test_installed(&[(full_name, "1.0.0")]);
    let (name, mut package) = installed.pop_first().unwrap();

    package.dependencies = dependencies.iter().map(|name| name.to_string()).collect();

    (name, package)
  }

  fn disabled(full_name: &str, dependencies: &[&str]) -> (String, InstalledPackage) {
    let (name, mut package) = package(full_name, dependencies);

    package.enabled = false;

    (name, package)
  }

  /// `Owner-Pack` needs `Owner-Lib`, which needs `Owner-Core` and the loader.
  /// `Owner-Solo` needs nothing and nothing needs it.
  fn profile() -> Installed {
    [
      package(LOADER, &[]),
      package("Owner-Core", &[]),
      package("Owner-Lib", &["Owner-Core", LOADER]),
      package("Owner-Pack", &["Owner-Lib"]),
      package("Owner-Solo", &[]),
    ]
    .into()
  }

  /// A state with valheim open and its Default profile holding `installed`.
  fn ready(installed: Installed) -> State {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: Some("valheim".to_string()),
      games: [("valheim".to_string(), GameSettings::new("/games/valheim"))].into(),
      ..Settings::default()
    };

    update(&mut state, Msg::Started);
    update(&mut state, Msg::SettingsLoaded(Ok(settings)));
    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(installed),
      },
    );

    state
  }

  fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
  }

  fn set_enabled(operation: u64, packages: &[&str], enabled: bool) -> Effect {
    Effect::Run(Command::SetEnabled(SetEnabled {
      operation,
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      packages: names(packages),
      enabled,
    }))
  }

  fn check(packages: &[&str]) -> Effect {
    Effect::Run(Command::CheckUninstall {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      packages: names(packages),
    })
  }

  fn checked(packages: &[&str], untracked: Result<Vec<PathBuf>, &str>) -> Msg {
    Msg::UninstallChecked {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      packages: names(packages),
      untracked: untracked.map_err(str::to_string),
    }
  }

  fn uninstall(operation: u64, packages: &[&str], delete: &[&str]) -> Effect {
    Effect::Run(Command::Uninstall(Uninstall {
      operation,
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      packages: names(packages),
      delete: delete.iter().map(PathBuf::from).collect(),
    }))
  }

  fn dependants_question(removal: Removal, package: &str, dependants: &[&str]) -> Option<Pending> {
    Some(Pending::Dependants {
      removal,
      package: package.to_string(),
      dependants: names(dependants),
    })
  }

  fn listed(state: &State) -> Vec<&str> {
    state
      .installed_rows()
      .into_iter()
      .map(|(full_name, _)| full_name)
      .collect()
  }

  /// `full_name` as the package list has it, in `categories`.
  fn in_categories(full_name: &str, categories: &[&str]) -> serde_json::Value {
    let mut package = crate::test_listed(full_name, &["1.0.0"], false);

    package["categories"] = categories.into();

    package
  }

  #[test]
  fn choosing_categories_narrows_the_profile_list_to_mods_in_any_of_them() {
    let mut state = ready(
      [
        package("Owner-Map", &[]),
        package("Owner-Boat", &[]),
        package("Owner-Unlisted", &[]),
      ]
      .into(),
    );
    let index = crate::test_index(vec![
      in_categories("Owner-Map", &["Maps"]),
      in_categories("Owner-Boat", &["Mods", "Tools"]),
    ]);

    update(
      &mut state,
      Msg::CachedIndexRead {
        community: "valheim".to_string(),
        index: Ok(Some(index)),
      },
    );

    assert_eq!(
      listed(&state),
      ["Owner-Boat", "Owner-Map", "Owner-Unlisted"]
    );

    update(&mut state, Msg::CategoryToggled("Tools".to_string()));

    assert_eq!(listed(&state), ["Owner-Boat"]);

    update(&mut state, Msg::CategoryToggled("Maps".to_string()));
    update(&mut state, Msg::InstalledSearchChanged("map".to_string()));

    assert_eq!(listed(&state), ["Owner-Map"]);
  }

  #[test]
  fn choosing_a_tab_shows_it() {
    let mut state = ready(profile());

    assert_eq!(state.tab, Tab::Installed);

    update(&mut state, Msg::TabChosen(Tab::Browse));

    assert_eq!(state.tab, Tab::Browse);
  }

  #[test]
  fn the_list_is_sorted_by_name_and_narrowed_by_search() {
    let mut state = ready(
      [
        package("Zed-Alpha", &[]),
        package("Amy-Zulu", &[]),
        package("Bob-mike", &[]),
      ]
      .into(),
    );

    assert_eq!(listed(&state), ["Zed-Alpha", "Bob-mike", "Amy-Zulu"]);

    update(&mut state, Msg::InstalledSearchChanged("ZU".to_string()));

    assert_eq!(listed(&state), ["Amy-Zulu"]);

    update(&mut state, Msg::InstalledSearchChanged("zed".to_string()));

    assert_eq!(listed(&state), ["Zed-Alpha"]);
  }

  #[test]
  fn the_list_sorts_by_author_or_by_newest() {
    let mut first = package("Zed-Alpha", &[]);
    let mut second = package("Amy-Zulu", &[]);
    let mut third = package("Amy-Bravo", &[]);

    (
      first.1.installed_at,
      second.1.installed_at,
      third.1.installed_at,
    ) = (3, 1, 2);

    let mut state = ready([first, second, third].into());

    update(&mut state, Msg::InstalledSortChosen(Sort::Author));

    assert_eq!(listed(&state), ["Amy-Bravo", "Amy-Zulu", "Zed-Alpha"]);

    update(&mut state, Msg::InstalledSortChosen(Sort::Newest));

    assert_eq!(listed(&state), ["Zed-Alpha", "Amy-Bravo", "Amy-Zulu"]);
  }

  #[test]
  fn dependants_include_packages_that_need_it_through_others() {
    let installed = profile();

    assert_eq!(
      dependants(&installed, "Owner-Core"),
      names(&["Owner-Lib", "Owner-Pack"])
    );
    assert_eq!(dependants(&installed, "Owner-Solo"), Vec::<String>::new());
  }

  #[test]
  fn dependencies_include_what_it_needs_through_others_that_the_profile_holds() {
    let mut installed = profile();

    installed.insert(
      "Owner-Pack".to_string(),
      package("Owner-Pack", &["Owner-Lib", "Owner-Gone"]).1,
    );

    assert_eq!(
      dependencies(&installed, "Owner-Pack"),
      names(&["Owner-Core", "Owner-Lib", LOADER])
    );
  }

  #[test]
  fn a_dependency_cycle_never_lists_the_package_itself() {
    let installed: Installed = [
      package("Owner-A", &["Owner-B"]),
      package("Owner-B", &["Owner-A"]),
    ]
    .into();

    assert_eq!(dependants(&installed, "Owner-A"), names(&["Owner-B"]));
    assert_eq!(dependencies(&installed, "Owner-A"), names(&["Owner-B"]));
  }

  #[test]
  fn disabling_a_package_nothing_needs_disables_it_at_once() {
    let mut state = ready(profile());

    let effects = update(&mut state, Msg::DisablePressed("Owner-Solo".to_string()));

    assert_eq!(effects, vec![set_enabled(1, &["Owner-Solo"], false)]);
    assert_eq!(
      state
        .queue
        .operations
        .last()
        .map(|operation| operation.action.clone()),
      Some(Action::SetEnabled {
        packages: names(&["Owner-Solo"]),
        enabled: false,
      })
    );
  }

  #[test]
  fn disabling_a_package_others_need_asks_first_and_changes_nothing() {
    let mut state = ready(profile());

    let effects = update(&mut state, Msg::DisablePressed("Owner-Core".to_string()));

    assert_eq!(effects, vec![]);
    assert!(state.queue.operations.is_empty());
    assert_eq!(
      state.manage.pending,
      dependants_question(Removal::Disable, "Owner-Core", &["Owner-Lib", "Owner-Pack"])
    );
  }

  #[test]
  fn disabling_this_package_only_leaves_its_dependants_enabled() {
    let mut state = ready(profile());

    update(&mut state, Msg::DisablePressed("Owner-Core".to_string()));

    let effects = update(&mut state, Msg::DependantsChosen(Scope::PackageOnly));

    assert_eq!(effects, vec![set_enabled(1, &["Owner-Core"], false)]);
    assert_eq!(state.manage.pending, None);
  }

  #[test]
  fn disabling_with_dependants_disables_them_too() {
    let mut state = ready(profile());

    update(&mut state, Msg::DisablePressed("Owner-Core".to_string()));

    let effects = update(&mut state, Msg::DependantsChosen(Scope::WithDependants));

    assert_eq!(
      effects,
      vec![set_enabled(
        1,
        &["Owner-Core", "Owner-Lib", "Owner-Pack"],
        false
      )]
    );
  }

  #[test]
  fn disabling_does_not_ask_about_dependants_already_disabled() {
    let mut installed = profile();

    installed.extend([
      disabled("Owner-Lib", &["Owner-Core"]),
      disabled("Owner-Pack", &["Owner-Lib"]),
    ]);

    let mut state = ready(installed);

    let effects = update(&mut state, Msg::DisablePressed("Owner-Core".to_string()));

    assert_eq!(effects, vec![set_enabled(1, &["Owner-Core"], false)]);
  }

  #[test]
  fn a_mod_loader_cannot_be_disabled() {
    let mut state = ready(profile());

    let effects = update(&mut state, Msg::DisablePressed(LOADER.to_string()));

    assert!(!state.can_toggle(LOADER));
    assert_eq!(effects, vec![]);
    assert_eq!(state.manage.pending, None);
  }

  #[test]
  fn enabling_a_package_enables_its_disabled_dependencies() {
    let mut installed = profile();

    installed.extend([
      disabled("Owner-Core", &[]),
      disabled("Owner-Lib", &["Owner-Core", LOADER]),
      disabled("Owner-Pack", &["Owner-Lib"]),
      disabled("Owner-Solo", &[]),
    ]);

    let mut state = ready(installed);

    let effects = update(&mut state, Msg::EnablePressed("Owner-Pack".to_string()));

    assert_eq!(
      effects,
      vec![set_enabled(
        1,
        &["Owner-Pack", "Owner-Core", "Owner-Lib"],
        true
      )]
    );
  }

  #[test]
  fn enabling_leaves_enabled_dependencies_out() {
    let mut installed = profile();

    installed.extend([disabled("Owner-Pack", &["Owner-Lib"])]);

    let mut state = ready(installed);

    let effects = update(&mut state, Msg::EnablePressed("Owner-Pack".to_string()));

    assert_eq!(effects, vec![set_enabled(1, &["Owner-Pack"], true)]);
  }

  #[test]
  fn enable_all_and_disable_all_change_every_package_but_the_loader() {
    let mut installed = profile();

    installed.extend([disabled("Owner-Solo", &[])]);

    let mut state = ready(installed);

    let enable = update(&mut state, Msg::EnableAllPressed);

    update(
      &mut state,
      Msg::OperationFinished {
        operation: 1,
        outcome: Ok(Outcome {
          changed: names(&["Owner-Solo"]),
          failed: vec![],
          cancelled: vec![],
          profile: Ok(profile()),
        }),
      },
    );

    let disable = update(&mut state, Msg::DisableAllPressed);

    assert_eq!(enable, vec![set_enabled(1, &["Owner-Solo"], true)]);
    assert_eq!(
      disable,
      vec![set_enabled(
        2,
        &["Owner-Core", "Owner-Lib", "Owner-Pack", "Owner-Solo"],
        false
      )]
    );
  }

  #[test]
  fn enable_all_with_nothing_disabled_does_nothing() {
    let mut state = ready(profile());

    assert_eq!(update(&mut state, Msg::EnableAllPressed), vec![]);
    assert!(state.queue.operations.is_empty());
  }

  #[test]
  fn a_finished_toggle_shows_the_profile_it_left() {
    let mut state = ready(profile());
    let mut after = profile();

    after.extend([disabled("Owner-Solo", &[])]);

    update(&mut state, Msg::DisablePressed("Owner-Solo".to_string()));
    update(
      &mut state,
      Msg::OperationFinished {
        operation: 1,
        outcome: Ok(Outcome {
          changed: names(&["Owner-Solo"]),
          failed: vec![],
          cancelled: vec![],
          profile: Ok(after.clone()),
        }),
      },
    );

    assert!(state.queue.operations.is_empty());
    assert_eq!(
      state.profile.as_ref().and_then(Profile::installed),
      Some(&after)
    );
  }

  #[test]
  fn nothing_is_enabled_disabled_or_uninstalled_until_the_profiles_operation_ends() {
    let mut installed = profile();

    installed.extend([disabled("Owner-Solo", &[])]);

    let mut state = ready(installed.clone());

    update(&mut state, Msg::DisablePressed("Owner-Pack".to_string()));

    assert!(!state.can_manage());
    assert!(state.can_install());
    assert_eq!(
      update(&mut state, Msg::EnablePressed("Owner-Solo".to_string())),
      vec![]
    );
    assert_eq!(
      update(&mut state, Msg::DisablePressed("Owner-Lib".to_string())),
      vec![]
    );
    assert_eq!(
      update(&mut state, Msg::UninstallPressed("Owner-Core".to_string())),
      vec![]
    );
    assert_eq!(update(&mut state, Msg::EnableAllPressed), vec![]);
    assert_eq!(state.manage.pending, None);
    assert_eq!(state.queue.operations.len(), 1);

    update(
      &mut state,
      Msg::OperationFinished {
        operation: 1,
        outcome: Ok(Outcome {
          changed: names(&["Owner-Pack"]),
          failed: vec![],
          cancelled: vec![],
          profile: Ok(installed),
        }),
      },
    );

    assert!(state.can_manage());
    assert_eq!(
      update(&mut state, Msg::EnablePressed("Owner-Solo".to_string())),
      vec![set_enabled(2, &["Owner-Solo"], true)]
    );
  }

  #[test]
  fn uninstalling_a_package_nothing_needs_checks_it_alone() {
    let mut state = ready(profile());

    let effects = update(&mut state, Msg::UninstallPressed("Owner-Pack".to_string()));

    assert_eq!(effects, vec![check(&["Owner-Pack"])]);
    assert_eq!(
      state.manage.pending,
      Some(Pending::Checking {
        packages: names(&["Owner-Pack"]),
      })
    );
    assert!(!state.can_install());
  }

  #[test]
  fn uninstalling_a_package_others_need_asks_first() {
    let mut state = ready(profile());

    let effects = update(&mut state, Msg::UninstallPressed("Owner-Core".to_string()));

    assert_eq!(effects, vec![]);
    assert_eq!(
      state.manage.pending,
      dependants_question(
        Removal::Uninstall,
        "Owner-Core",
        &["Owner-Lib", "Owner-Pack"]
      )
    );
  }

  #[test]
  fn uninstalling_this_package_only_names_only_it() {
    let mut state = ready(profile());

    update(&mut state, Msg::UninstallPressed("Owner-Core".to_string()));

    let effects = update(&mut state, Msg::DependantsChosen(Scope::PackageOnly));

    assert_eq!(effects, vec![check(&["Owner-Core"])]);
  }

  #[test]
  fn uninstalling_with_dependants_names_them_too() {
    let mut state = ready(profile());

    update(&mut state, Msg::UninstallPressed("Owner-Lib".to_string()));

    let effects = update(&mut state, Msg::DependantsChosen(Scope::WithDependants));

    assert_eq!(effects, vec![check(&["Owner-Lib", "Owner-Pack"])]);
  }

  #[test]
  fn dismissing_the_dependants_question_changes_nothing() {
    let mut state = ready(profile());

    update(&mut state, Msg::UninstallPressed("Owner-Core".to_string()));

    let effects = update(&mut state, Msg::PendingDismissed);

    assert_eq!(effects, vec![]);
    assert_eq!(state.manage.pending, None);
    assert_eq!(
      update(&mut state, Msg::DependantsChosen(Scope::WithDependants)),
      vec![]
    );
  }

  #[test]
  fn an_uninstall_with_no_untracked_folders_removes_only_what_was_named() {
    let mut state = ready(profile());

    update(&mut state, Msg::UninstallPressed("Owner-Pack".to_string()));

    let effects = update(&mut state, checked(&["Owner-Pack"], Ok(vec![])));

    assert_eq!(effects, vec![uninstall(1, &["Owner-Pack"], &[])]);
    assert_eq!(state.manage.pending, None);
    assert_eq!(
      state
        .queue
        .operations
        .last()
        .map(|operation| operation.action.clone()),
      Some(Action::Uninstall(names(&["Owner-Pack"])))
    );
  }

  #[test]
  fn an_uninstall_that_would_delete_untracked_folders_shows_them_and_waits() {
    let mut state = ready(profile());

    update(&mut state, Msg::UninstallPressed("Owner-Pack".to_string()));

    let effects = update(
      &mut state,
      checked(
        &["Owner-Pack"],
        Ok(vec![PathBuf::from("BepInEx/plugins/Stray")]),
      ),
    );

    assert_eq!(effects, vec![]);
    assert!(state.queue.operations.is_empty());
    assert_eq!(
      state.manage.pending,
      Some(Pending::Untracked {
        packages: names(&["Owner-Pack"]),
        folders: vec![PathBuf::from("BepInEx/plugins/Stray")],
      })
    );
  }

  #[test]
  fn agreeing_to_delete_the_folders_passes_them_on() {
    let mut state = ready(profile());

    update(&mut state, Msg::UninstallPressed("Owner-Pack".to_string()));
    update(
      &mut state,
      checked(
        &["Owner-Pack"],
        Ok(vec![PathBuf::from("BepInEx/plugins/Stray")]),
      ),
    );

    let effects = update(&mut state, Msg::FoldersChosen(Folders::Delete));

    assert_eq!(
      effects,
      vec![uninstall(1, &["Owner-Pack"], &["BepInEx/plugins/Stray"])]
    );
  }

  #[test]
  fn keeping_the_folders_uninstalls_without_deleting_them() {
    let mut state = ready(profile());

    update(&mut state, Msg::UninstallPressed("Owner-Pack".to_string()));
    update(
      &mut state,
      checked(
        &["Owner-Pack"],
        Ok(vec![PathBuf::from("BepInEx/plugins/Stray")]),
      ),
    );

    let effects = update(&mut state, Msg::FoldersChosen(Folders::Keep));

    assert_eq!(effects, vec![uninstall(1, &["Owner-Pack"], &[])]);
  }

  #[test]
  fn dismissing_the_folders_question_uninstalls_nothing() {
    let mut state = ready(profile());

    update(&mut state, Msg::UninstallPressed("Owner-Pack".to_string()));
    update(
      &mut state,
      checked(
        &["Owner-Pack"],
        Ok(vec![PathBuf::from("BepInEx/plugins/Stray")]),
      ),
    );
    update(&mut state, Msg::PendingDismissed);

    assert_eq!(
      update(&mut state, Msg::FoldersChosen(Folders::Delete)),
      vec![]
    );
    assert!(state.queue.operations.is_empty());
  }

  #[test]
  fn an_uninstall_the_check_refuses_says_why_and_starts_nothing() {
    let mut state = ready([package(LOADER, &[])].into());

    update(&mut state, Msg::UninstallPressed(LOADER.to_string()));

    let effects = update(&mut state, checked(&[LOADER], Err("no install record")));

    assert_eq!(effects, vec![]);
    assert!(state.queue.operations.is_empty());
    assert_eq!(
      state.manage.pending,
      Some(Pending::Refused {
        packages: names(&[LOADER]),
        reason: "no install record".to_string(),
      })
    );
    assert!(state.can_install());
  }

  #[test]
  fn a_check_no_longer_awaited_is_ignored() {
    let mut state = ready(profile());

    update(&mut state, Msg::UninstallPressed("Owner-Pack".to_string()));
    update(&mut state, Msg::PendingDismissed);

    let effects = update(&mut state, checked(&["Owner-Pack"], Ok(vec![])));

    assert_eq!(effects, vec![]);
    assert!(state.queue.operations.is_empty());
  }

  #[test]
  fn switching_profile_drops_the_question() {
    let mut state = ready(profile());

    update(
      &mut state,
      Msg::ProfilesListed {
        game: "valheim".to_string(),
        names: Ok(names(&[DEFAULT_PROFILE, "Modded"])),
      },
    );
    update(&mut state, Msg::UninstallPressed("Owner-Core".to_string()));
    update(&mut state, Msg::ProfilePicked("Modded".to_string()));

    assert_eq!(state.manage.pending, None);
  }

  #[test]
  fn leaving_the_game_drops_the_question() {
    let mut state = ready(profile());

    update(&mut state, Msg::UninstallPressed("Owner-Core".to_string()));
    update(&mut state, Msg::SwitchGamePressed);

    assert_eq!(state.manage.pending, None);
  }
}
