//! Every piece of interface text, kept in one place so translation can be
//! added later.

use rivet_core::SettingsError;
use rivet_core::browse::{SortDirection, SortKey};
use rivet_core::install::{Action, FailedPackage, Planned, Progress, Stage};
use rivet_core::launch::LaunchFailure;
use rivet_core::manage::{Removal, Sort};
use rivet_core::notice::Notice;
use rivet_core::picker::{FolderRefusal, NotFound, Refusal, STEAM_LIBRARY, Store};
use rivet_core::portability::{Destination, Exported, ReadFailure};
use rivet_core::self_update::{CheckFailure, DownloadFailure, Stage as UpdateStage};
use rivet_core::switcher::{NameRefusal, Naming, ProfileChange};
use std::path::{Path, PathBuf};
use thunderstore_engine::ecosystem::LoaderKind;
use thunderstore_engine::profile::launch::LaunchMode;
use thunderstore_engine::profile::layout::BadName;

pub const LOADING: &str = "Loading";
/// Shown on Windows until the manual pass in `docs/windows-manual-check.md`
/// is done.
pub const EXPERIMENTAL: &str = "Experimental Windows build";
pub const PICKER_TITLE: &str = "Choose your game";
pub const PICKER_LIST: &str = "List";
pub const PICKER_GRID: &str = "Grid";
pub const YOUR_GAMES: &str = "Your games";
pub const ALL_GAMES: &str = "All games";
/// The tag on a game Rivet has a folder for, so it is ready to mod.
pub const READY: &str = "Ready";
/// The mark on a game whose mods Rivet cannot install yet.
pub const NO_MOD_SUPPORT: &str = "Mods not supported yet";

/// The stores a game is sold on, joined with middle dots.
///
/// ```
/// use rivet_app::text::stores;
/// use rivet_core::picker::Store;
///
/// assert_eq!(
///   stores(&[Store::Steam, Store::EpicGamesStore]),
///   "Steam · Epic Games Store"
/// );
/// ```
pub fn stores(stores: &[Store]) -> String {
  let names: Vec<&str> = stores
    .iter()
    .map(|store| match store {
      Store::Steam => "Steam",
      Store::EpicGamesStore => "Epic Games Store",
      Store::XboxGamePass => "Xbox Game Pass",
      Store::OculusStore => "Oculus Store",
      Store::EaApp => "EA app",
    })
    .collect();

  names.join(" · ")
}
pub const SEARCH_PLACEHOLDER: &str = "Search games";
pub const NO_GAME_MATCHES: &str = "No game matches your search.";
/// Beside the game's name in the sidebar, so it leaves "game" out.
pub const SWITCH_GAME: &str = "Switch";
pub const REFRESH: &str = "Refresh";
pub const REFRESHING: &str = "Checking Thunderstore for new mods";
pub const LOADING_PACKAGES: &str = "Loading mods";
pub const NO_PACKAGES: &str = "No mods to show.";
pub const PACKAGE_SEARCH_PLACEHOLDER: &str = "Search Thunderstore";
pub const NO_PACKAGE_MATCHES: &str = "No mods match your search and filters.";
pub const SHOW_NSFW: &str = "Show NSFW";
pub const SHOW_DEPRECATED: &str = "Show deprecated";
pub const CLEAR_FILTERS: &str = "Clear filters";

/// The button opening the menu of the list's filters, with how many are on.
///
/// ```
/// use rivet_app::text::filter_menu;
///
/// assert_eq!(filter_menu(0), "Filter");
/// assert_eq!(filter_menu(2), "Filter (2)");
/// ```
pub fn filter_menu(active: usize) -> String {
  match active {
    0 => "Filter".to_string(),
    active => format!("Filter ({active})"),
  }
}

/// The label of the menu choosing Thunderstore's sort, naming the current
/// one, such as `Sort: Downloads ↓`.
///
/// ```
/// use rivet_app::text::sort_menu;
/// use rivet_core::browse::{SortDirection, SortKey};
///
/// assert_eq!(
///   sort_menu(SortKey::Name, SortDirection::Ascending),
///   "Sort: Name ↑"
/// );
/// ```
pub fn sort_menu(key: SortKey, direction: SortDirection) -> String {
  format!("Sort: {}", sort(key, Some(direction)))
}

/// A sort's label, with an arrow for its direction when it is the current
/// sort, such as `Downloads ↓`. Best match has no arrow, as it only sorts one
/// way.
pub fn sort(key: SortKey, current: Option<SortDirection>) -> String {
  let name = match key {
    SortKey::BestMatch => return "Best match".to_string(),
    SortKey::LastUpdated => "Last updated",
    SortKey::Name => "Name",
    SortKey::Downloads => "Downloads",
    SortKey::Rating => "Rating",
  };

  match current {
    Some(SortDirection::Ascending) => format!("{name} ↑"),
    Some(SortDirection::Descending) => format!("{name} ↓"),
    None => name.to_string(),
  }
}

/// A failure as shown to the user: what went wrong, the detail, and what to
/// do next.
pub struct Failure {
  pub headline: String,
  pub detail: String,
  pub next: String,
}

pub fn checking(folder: &Path) -> String {
  format!("Checking {}", folder.display())
}

pub fn locating(game: &str) -> String {
  format!("Looking for {game} in Steam")
}

/// Why the user is asked to choose `game`'s folder by hand.
pub fn fallback(game: &str, reason: &NotFound) -> String {
  match reason {
    NotFound::NotOnSteam => {
      format!("{game} is not a Steam game, so choose the folder it is installed in.")
    }
    NotFound::NoSteam => {
      format!("Steam was not found on this computer, so choose the folder {game} is installed in.")
    }
    NotFound::NotInstalled => {
      format!("Steam does not have {game} installed, so choose the folder it is installed in.")
    }
    NotFound::Unreadable(reason) => format!(
      "Steam's game list could not be read ({reason}), so choose the folder {game} is installed in."
    ),
  }
}

pub fn refusal(game: &str, refusal: &Refusal) -> Failure {
  let folder = refusal.folder.display();

  match &refusal.reason {
    FolderRefusal::MissingExecutable { expected } => Failure {
      headline: format!("This folder does not contain {game}"),
      detail: format!("Looked in {folder} for {}.", expected.join(" or ")),
      next: format!("Choose the folder {game} is installed in."),
    },
    FolderRefusal::Unreadable(reason) => Failure {
      headline: "This folder could not be read".to_string(),
      detail: format!("{folder}: {reason}"),
      next: "Choose another folder, or check that you can open this one.".to_string(),
    },
    FolderRefusal::NotSteam => Failure {
      headline: "This folder is not a Steam install".to_string(),
      detail: format!("Looked in {folder} for {STEAM_LIBRARY}."),
      next: "Choose the folder Steam is installed in.".to_string(),
    },
  }
}

pub const SETTINGS: &str = "Settings";
pub const BACK: &str = "Back";
pub const CHANGE: &str = "Change";
pub const GAME_FOLDER: &str = "Game folder";
pub const STEAM_FOLDER: &str = "Steam folder";
pub const STEAM_FOLDER_FOUND: &str = "Found on this computer";
pub const LAUNCH_ARGUMENTS: &str = "Launch arguments";
pub const LAUNCH_ARGUMENTS_HINT: &str = "Added after the mod loader's own on modded and vanilla launches. Put double quotes around an argument with spaces. Saved when you leave settings or close Rivet.";
pub const EVERY_GAME: &str = "Every game";
pub const RUNS_WITH: &str = "Runs with";
pub const RUNTIME_DETECT: &str = "Detect";
pub const RUNTIME_NATIVE: &str = "Native";
pub const RUNTIME_PROTON: &str = "Proton";
pub const RUNTIME_HINT: &str =
  "Detect picks Proton when the game folder holds only the Windows build. Takes effect at once.";

pub const DISPLAY: &str = "Display";
pub const THEME: &str = "Theme";
pub const THEME_DEFAULT: &str = "Default";
pub const THEME_ONE: &str = "One";
pub const APPEARANCE: &str = "Appearance";
pub const APPEARANCE_SYSTEM: &str = "Match system";
pub const APPEARANCE_LIGHT: &str = "Light";
pub const APPEARANCE_DARK: &str = "Dark";
pub const INTERFACE_SIZE: &str = "Interface size";
pub const INTERFACE_SIZE_AUTO: &str = "Auto";

/// An interface size as settings offer it.
///
/// ```
/// assert_eq!(rivet_app::text::percent(115), "115%");
/// ```
pub fn percent(size: u16) -> String {
  format!("{size}%")
}

pub const LOADER_FILES: &str = "Loader files";
pub const REMOVE: &str = "Remove";

/// How many files launches copied into the game folder and have not been
/// removed, such as `2 files copied into the game folder`.
///
/// ```
/// use rivet_app::text::loader_files;
///
/// assert_eq!(loader_files(0), "None in the game folder");
/// assert_eq!(loader_files(1), "1 file copied into the game folder");
/// assert_eq!(loader_files(3), "3 files copied into the game folder");
/// ```
pub fn loader_files(count: usize) -> String {
  match count {
    0 => "None in the game folder".to_string(),
    1 => "1 file copied into the game folder".to_string(),
    count => format!("{count} files copied into the game folder"),
  }
}

pub const LOADER_FILES_HINT: &str = "Some modded launches copy the mod loader into the game folder. Remove takes out what Rivet copied and leaves anything you changed.";

pub fn settings_title(game: &str) -> String {
  format!("{game} settings")
}

pub fn settings_error(error: &SettingsError) -> Failure {
  match error {
    SettingsError::Load(detail) => Failure {
      headline: "Your settings could not be read".to_string(),
      detail: detail.clone(),
      next: "Choose your game again. Saving replaces the unreadable file.".to_string(),
    },
    SettingsError::Save(detail) => Failure {
      headline: "Your settings could not be saved".to_string(),
      detail: detail.clone(),
      next: "This choice is forgotten when the app closes. Check that the settings folder can be written to.".to_string(),
    },
  }
}

/// The list could not be refreshed, and an older one is shown.
pub fn stale(reason: &str) -> Failure {
  Failure {
    headline: "The mod list may be out of date".to_string(),
    detail: reason.to_string(),
    next: "Check your connection, then press Refresh.".to_string(),
  }
}

/// The list could not be fetched, and none is cached.
pub fn unavailable(reason: &str) -> Failure {
  Failure {
    headline: "The mod list could not be fetched".to_string(),
    detail: reason.to_string(),
    next: "Check your connection, then press Refresh.".to_string(),
  }
}

/// A package's author and latest version, such as `by Owner, 1.2.0`.
pub fn byline(owner: &str, version: &str) -> String {
  format!("by {owner}, {version}")
}

/// A Thunderstore package's author, such as `by Owner`.
pub fn author(owner: &str) -> String {
  format!("by {owner}")
}

/// A download count in at most four characters: ten million and up in whole
/// millions, other millions to one decimal, thousands rounded, and smaller
/// counts in full.
///
/// ```
/// use rivet_app::text::downloads;
///
/// assert_eq!(downloads(950), "950");
/// assert_eq!(downloads(48_600), "49k");
/// assert_eq!(downloads(999_499), "999k");
/// assert_eq!(downloads(999_500), "1M");
/// assert_eq!(downloads(2_000_000), "2M");
/// assert_eq!(downloads(1_250_000), "1.3M");
/// assert_eq!(downloads(9_949_999), "9.9M");
/// assert_eq!(downloads(9_950_000), "10M");
/// assert_eq!(downloads(12_345_678), "12M");
/// ```
pub fn downloads(count: u64) -> String {
  match count {
    0..1_000 => count.to_string(),
    1_000..999_500 => format!("{}k", (count + 500) / 1_000),
    999_500..9_950_000 => {
      let tenths = (count + 50_000) / 100_000;

      match tenths % 10 {
        0 => format!("{}M", tenths / 10),
        tenth => format!("{}.{tenth}M", tenths / 10),
      }
    }
    _ => format!("{}M", (count + 500_000) / 1_000_000),
  }
}

/// `text` as one paragraph: every run of whitespace, line breaks included,
/// becomes a single space, so only wrapping decides where lines break.
///
/// ```
/// use rivet_app::text::one_paragraph;
///
/// assert_eq!(one_paragraph(" Adds\n\n- more\titems "), "Adds - more items");
/// ```
pub fn one_paragraph(text: &str) -> String {
  text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub const INSTALL: &str = "Install";
pub const INSTALLED: &str = "Installed";
/// The detail pane's line in a game whose mods Rivet cannot install.
pub const MODS_UNSUPPORTED: &str =
  "Rivet can't install this game's mods yet. You can still launch it vanilla.";
pub const CANCEL: &str = "Cancel";
pub const DISMISS: &str = "Dismiss";
pub const NO_DEPENDENCIES: &str = "Nothing else comes with it.";
pub const WITH_DEPENDENCIES: &str = "These come with it:";
pub const STARTING_INSTALL: &str = "Starting";

/// The question asked before an install, such as `Install Owner-Mod 1.0.0?`.
pub fn confirm_install(package: &Planned) -> String {
  format!("Install {} {}?", package.full_name, package.version)
}

/// One package in the confirmation list, such as `Owner-Lib 1.0.0`.
pub fn planned(package: &Planned) -> String {
  let line = format!("{} {}", package.full_name, package.version);

  if package.installed {
    format!("{line} (already installed)")
  } else {
    line
  }
}

/// Dependencies the package index does not have, so the install cannot go
/// ahead.
pub fn missing_dependencies(missing: &[String]) -> Failure {
  Failure {
    headline: "This mod cannot be installed".to_string(),
    detail: format!("Thunderstore does not have {}.", missing.join(", ")),
    next: "Press Refresh in case they were added since, or choose another mod.".to_string(),
  }
}

/// A running install, such as `Installing Owner-Mod: downloading Owner-Lib,
/// 40%`.
pub fn installing(package: &str, progress: Option<&Progress>) -> String {
  let Some(progress) = progress else {
    return format!("Installing {package}: {STARTING_INSTALL}");
  };
  let stage = stage(progress.stage);
  let amount = match progress.total {
    Some(total) if total > 0 => format!(", {}%", progress.done.saturating_mul(100) / total),
    _ => String::new(),
  };

  format!("Installing {package}: {stage} {}{amount}", progress.package)
}

/// What an operation is doing to one package, such as `unpacking`.
fn stage(stage: Stage) -> &'static str {
  match stage {
    Stage::Downloading => "downloading",
    Stage::Extracting => "unpacking",
    Stage::Installing => "copying",
    Stage::Uninstalling => "uninstalling",
    Stage::Enabling => "enabling",
    Stage::Disabling => "disabling",
  }
}

/// A finished install, such as `Installed Owner-Mod and 2 dependencies.`.
pub fn installed(package: &str, installed: &[String]) -> String {
  let others = installed.iter().filter(|name| *name != package).count();

  match others {
    0 if installed.is_empty() => format!("{package} was already installed."),
    0 => format!("Installed {package}."),
    1 => format!("Installed {package} and 1 dependency."),
    others => format!("Installed {package} and {others} dependencies."),
  }
}

/// An install that failed, naming what failed and what it changed.
pub fn install_failed(package: &str, failed: &[FailedPackage], installed: &[String]) -> Failure {
  let detail = failed
    .iter()
    .map(|failure| format!("{}: {}", failure.package, failure.reason))
    .collect::<Vec<_>>()
    .join("\n");
  let next = match installed {
    [] => "Nothing in your profile was changed.".to_string(),
    installed => format!(
      "These were installed before it failed: {}.",
      installed.join(", ")
    ),
  };

  Failure {
    headline: format!("{package} could not be installed"),
    detail,
    next,
  }
}

pub const TAB_BROWSE: &str = "Find mods";
pub const TAB_INSTALLED: &str = "In this profile";
pub const INSTALLED_SEARCH_PLACEHOLDER: &str = "Search this profile";
pub const SORT_NAME: &str = "Name";
pub const SORT_AUTHOR: &str = "Author";
pub const SORT_NEWEST: &str = "Newest";

/// The label of the menu choosing the profile's sort, naming the current one.
///
/// ```
/// use rivet_app::text::installed_sort_menu;
/// use rivet_core::manage::Sort;
///
/// assert_eq!(installed_sort_menu(Sort::Newest), "Sort: Newest");
/// ```
pub fn installed_sort_menu(sort: Sort) -> String {
  let name = match sort {
    Sort::Name => SORT_NAME,
    Sort::Author => SORT_AUTHOR,
    Sort::Newest => SORT_NEWEST,
  };

  format!("Sort: {name}")
}
pub const UNINSTALL: &str = "Uninstall";
pub const ENABLE_ALL: &str = "Enable all";
pub const DISABLE_ALL: &str = "Disable all";
pub const UPDATE: &str = "Update";
pub const UPDATE_ALL: &str = "Update all";

/// How many of the profile's mods an update would change, over the list.
///
/// ```
/// use rivet_app::text::updates_available;
///
/// assert_eq!(updates_available(1), "1 update available");
/// assert_eq!(updates_available(2), "2 updates available");
/// ```
pub fn updates_available(count: usize) -> String {
  match count {
    1 => "1 update available".to_string(),
    count => format!("{count} updates available"),
  }
}

pub const DEPRECATED: &str = "Deprecated";
pub const PINNED: &str = "Pinned";
pub const UNPIN: &str = "Unpin";
pub const CHOOSE_VERSION: &str = "Choose version";
pub const VERSION: &str = "Version";
pub const PIN_HINT: &str =
  "A version you choose is pinned, so updates leave it alone until you unpin it.";

/// The version menu's entry for the latest version, such as `Latest
/// (2.0.0)`.
pub fn latest_version(version: &str) -> String {
  format!("Latest ({version})")
}

/// What Update all left alone for being pinned.
///
/// ```
/// use rivet_app::text::pinned_skipped;
///
/// assert_eq!(pinned_skipped(1), "Left 1 pinned mod as it is");
/// assert_eq!(pinned_skipped(2), "Left 2 pinned mods as they are");
/// ```
pub fn pinned_skipped(count: usize) -> String {
  match count {
    1 => "Left 1 pinned mod as it is".to_string(),
    count => format!("Left {count} pinned mods as they are"),
  }
}

pub const UNPIN_TO_UPDATE: &str = "Unpin a mod to update it.";

/// A mod that could not be unpinned, such as `Owner-Mod could not be
/// unpinned`.
pub fn unpin_failed(package: &str) -> String {
  format!("{package} could not be unpinned")
}

pub const PINS_UNREAD: &str = "This profile's pins could not be read";

pub const PINS_UNREAD_NEXT: &str =
  "Mods show as unpinned and can be updated until the pin file is fixed or deleted.";

/// The badge of an installed mod with a newer version, such as `1.2.0
/// available`.
pub fn update_available(version: &str) -> String {
  format!("{version} available")
}

/// The badge of an installed mod missing mods it needs.
///
/// ```
/// use rivet_app::text::missing_badge;
///
/// assert_eq!(missing_badge(&["Owner-Lib".to_string()]), "Missing 1 dependency");
/// assert_eq!(missing_badge(&["A-B".to_string(), "C-D".to_string()]), "Missing 2 dependencies");
/// ```
pub fn missing_badge(missing: &[String]) -> String {
  match missing.len() {
    1 => "Missing 1 dependency".to_string(),
    count => format!("Missing {count} dependencies"),
  }
}
/// The name of a mod's on/off switch.
pub const ENABLED: &str = "Enabled";
pub const MOD_LOADER: &str = "Mod loader";
pub const NO_INSTALLED: &str = "No mods are installed in this profile.";
pub const NO_INSTALLED_MATCHES: &str = "No mod in this profile matches your search and categories.";
pub const PACKAGE_ONLY: &str = "This mod only";
pub const WITH_DEPENDANTS: &str = "Including the mods that need it";
pub const CHECKING_UNINSTALL: &str = "Checking what uninstalling removes";
pub const DELETE_FOLDERS: &str = "Delete them";
pub const KEEP_FOLDERS: &str = "Keep them";
pub const UNTRACKED_LEAD: &str =
  "These folders in the profile belong to no installed mod. Uninstalling can delete them too:";

fn removal_verb(removal: Removal) -> &'static str {
  match removal {
    Removal::Disable => "Disable",
    Removal::Uninstall => "Uninstall",
  }
}

/// The question asked when other mods need the one being disabled or
/// uninstalled, such as `Uninstall Owner-Lib? These mods need it:`.
pub fn dependants_question(removal: Removal, package: &str) -> String {
  format!("{} {package}? These mods need it:", removal_verb(removal))
}

/// The question asked before an uninstall that could delete folders no mod
/// owns, such as `Uninstall Owner-Mod?`.
pub fn untracked_question(packages: &[String]) -> String {
  format!("Uninstall {}?", subject(packages))
}

/// An uninstall the profile refused before anything changed.
pub fn uninstall_refused(packages: &[String], reason: &str) -> Failure {
  Failure {
    headline: format!("{} cannot be uninstalled", subject(packages)),
    detail: reason.to_string(),
    next: "Nothing in your profile was changed.".to_string(),
  }
}

/// The packages an operation names: one by name, more by count.
fn subject(packages: &[String]) -> String {
  match packages {
    [package] => package.clone(),
    packages => format!("{} mods", packages.len()),
  }
}

fn action_verbs(action: &Action) -> (&'static str, &'static str, &'static str) {
  match action {
    Action::Install(_) => ("Installing", "Installed", "installed"),
    Action::Update(_) => ("Updating", "Updated", "updated"),
    Action::Uninstall(_) => ("Uninstalling", "Uninstalled", "uninstalled"),
    Action::SetEnabled { enabled: true, .. } => ("Enabling", "Enabled", "enabled"),
    Action::SetEnabled { enabled: false, .. } => ("Disabling", "Disabled", "disabled"),
    Action::Import(_) => ("Importing", "Imported", "imported"),
  }
}

/// The game and profile an operation changes, such as `Valheim, Default
/// profile`.
pub fn place(game: &str, profile: &str) -> String {
  format!("{game}, {profile} profile")
}

/// How many operations wait behind the running one.
///
/// ```
/// use rivet_app::text::waiting;
///
/// assert_eq!(waiting(1), "1 more waiting");
/// assert_eq!(waiting(3), "3 more waiting");
/// ```
pub fn waiting(count: usize) -> String {
  format!("{count} more waiting")
}

/// A running operation, with the package and stage it is on when known.
///
/// ```
/// use rivet_app::text::running;
/// use rivet_core::install::{Action, Progress, Stage};
///
/// let names = |names: &[&str]| names.iter().map(|name| name.to_string()).collect();
/// let on = |package: &str, stage| Progress {
///   package: package.to_string(),
///   stage,
///   done: 0,
///   total: None,
/// };
/// let disable_one = Action::SetEnabled {
///   packages: names(&["Owner-Mod"]),
///   enabled: false,
/// };
/// let uninstall_two = Action::Uninstall(names(&["Owner-Mod", "Owner-Lib"]));
///
/// assert_eq!(running(&disable_one, None), "Disabling Owner-Mod");
/// assert_eq!(
///   running(&disable_one, Some(&on("Owner-Mod", Stage::Disabling))),
///   "Disabling Owner-Mod"
/// );
/// assert_eq!(
///   running(&uninstall_two, Some(&on("Owner-Lib", Stage::Uninstalling))),
///   "Uninstalling 2 mods: uninstalling Owner-Lib"
/// );
/// ```
pub fn running(action: &Action, progress: Option<&Progress>) -> String {
  if let Action::Install(package) = action {
    return installing(package, progress);
  }

  match progress {
    Some(progress) if action.packages() != std::slice::from_ref(&progress.package) => format!(
      "{}: {} {}",
      changing_packages(action),
      stage(progress.stage),
      progress.package
    ),
    _ => changing_packages(action),
  }
}

/// A running operation asked to stop after the package it is on.
pub fn stopping(action: &Action, progress: Option<&Progress>) -> String {
  format!(
    "{}. Stopping after the current mod.",
    running(action, progress)
  )
}

/// An operation that went through, such as `Enabled Owner-Mod.`.
pub fn finished(action: &Action, changed: &[String]) -> String {
  match action {
    Action::Install(package) => installed(package, changed),
    action => self::changed(action, changed),
  }
}

/// An operation that failed, naming what failed, why, and what to do next.
pub fn operation_failed(action: &Action, failed: &[FailedPackage], changed: &[String]) -> Failure {
  match action {
    Action::Install(package) => install_failed(package, failed, changed),
    Action::Import(_) => import_failed(failed, changed),
    action => change_failed(action, failed, changed),
  }
}

/// An import that did not finish, naming what failed and what it installed.
///
/// ```
/// use rivet_app::text::import_failed;
/// use rivet_core::install::FailedPackage;
///
/// let failed = [FailedPackage {
///   package: "Owner-Gone-1.0.0".to_string(),
///   reason: "not on Thunderstore".to_string(),
/// }];
/// let failure = import_failed(&failed, &["Owner-Lib".to_string()]);
///
/// assert_eq!(failure.headline, "The import did not finish");
/// assert_eq!(failure.detail, "Owner-Gone-1.0.0: not on Thunderstore");
/// assert_eq!(
///   failure.next,
///   "These were installed: Owner-Lib. Config files are restored only once every mod installs."
/// );
/// ```
pub fn import_failed(failed: &[FailedPackage], installed: &[String]) -> Failure {
  let detail = failed
    .iter()
    .map(|failure| format!("{}: {}", failure.package, failure.reason))
    .collect::<Vec<_>>()
    .join("\n");
  let next = match installed {
    [] => "Nothing was installed.".to_string(),
    installed => format!("These were installed: {}.", installed.join(", ")),
  };

  Failure {
    headline: "The import did not finish".to_string(),
    detail,
    next: format!("{next} Config files are restored only once every mod installs."),
  }
}

/// An operation the user cancelled, naming what it changed first.
pub fn cancelled(action: &Action, changed: &[String]) -> String {
  let doing = action_verbs(action).0.to_lowercase();
  let subject = subject(action.packages());

  match changed {
    [] => format!("Cancelled {doing} {subject}. Nothing was changed."),
    changed => format!(
      "Cancelled {doing} {subject} after changing {}.",
      changed.join(", ")
    ),
  }
}

pub const CLOSE_HEADLINE: &str = "Changes to your profiles are still running";
pub const CLOSE_DETAIL: &str = "Closing now could leave a profile half changed.";
pub const CLOSE_WAIT: &str = "Wait for them, then close";
pub const CLOSE_CANCEL: &str = "Cancel after the current mod, then close";
pub const KEEP_OPEN: &str = "Keep Rivet open";
pub const CLOSING_WHEN_DONE: &str = "Rivet closes once the running changes end.";

/// A running uninstall, enable, or disable, such as `Disabling 3 mods`.
pub fn changing_packages(action: &Action) -> String {
  format!("{} {}", action_verbs(action).0, subject(action.packages()))
}

/// A finished uninstall, enable, or disable, such as `Enabled Owner-Mod.`.
pub fn changed(action: &Action, changed: &[String]) -> String {
  if changed.is_empty() {
    return "Nothing needed changing.".to_string();
  }

  format!("{} {}.", action_verbs(action).1, subject(changed))
}

/// An uninstall, enable, or disable that failed, naming what failed and what
/// it changed.
pub fn change_failed(action: &Action, failed: &[FailedPackage], changed: &[String]) -> Failure {
  let detail = failed
    .iter()
    .map(|failure| format!("{}: {}", failure.package, failure.reason))
    .collect::<Vec<_>>()
    .join("\n");
  let next = match changed {
    [] => "Nothing in your profile was changed.".to_string(),
    changed => format!(
      "These were {} before it failed: {}.",
      action_verbs(action).2,
      changed.join(", ")
    ),
  };

  Failure {
    headline: format!(
      "{} could not be {}",
      subject(action.packages()),
      action_verbs(action).2
    ),
    detail,
    next,
  }
}

/// The profile could not be created or read.
pub fn profile_failed(profile: &str, reason: &str) -> Failure {
  Failure {
    headline: format!("The {profile} profile could not be opened"),
    detail: reason.to_string(),
    next: "Mods cannot be installed until it opens. Check that the app's data folder can be written to, then switch game and back.".to_string(),
  }
}

pub const LAUNCH_MODDED: &str = "Launch modded";
pub const LAUNCH_VANILLA: &str = "Launch vanilla";
pub const COPY: &str = "Copy";
pub const CHECK_AGAIN: &str = "Check again";
pub const STEAM_OPTION_LEAD: &str = "Mods load only when Steam's launch options for this game hold this line. Paste it into the game's launch options in Steam, under Properties:";
pub const STEAM_OPTION_CHECKING: &str = "Checking Steam's launch options";
pub const STEAM_OPTION_NOT_SET: &str = "Steam's saved launch options do not have this line. Steam saves them when it closes, so after pasting it, restart Steam. Rivet checks again when you come back to it, or press Check again.";

/// Steam's launch options could not be read, so whether the line is set is
/// unknown.
pub fn steam_option_unknown(reason: &str) -> String {
  format!("Could not tell whether the launch option is set: {reason}")
}

fn mode(mode: LaunchMode) -> &'static str {
  match mode {
    LaunchMode::Vanilla => "vanilla",
    _ => "modded",
  }
}

/// A launch being prepared, such as `Starting Valheim modded`.
pub fn launching(game: &str, launch: LaunchMode) -> String {
  format!("Starting {game} {}", mode(launch))
}

/// A launch that started the game, such as `Started Valheim vanilla.`.
pub fn launched(game: &str, launch: LaunchMode) -> String {
  format!("Started {game} {}.", mode(launch))
}

/// A mod loader's name as its authors write it.
fn loader_name(loader: LoaderKind) -> &'static str {
  match loader {
    LoaderKind::BepInEx => "BepInEx",
    LoaderKind::BepisLoader => "BepisLoader",
    LoaderKind::GdWeave => "GDWeave",
    LoaderKind::GodotMl => "Godot Mod Loader",
    LoaderKind::Lovely => "Lovely",
    LoaderKind::MelonLoader | LoaderKind::RecursiveMelonLoader => "MelonLoader",
    LoaderKind::Northstar => "Northstar",
    LoaderKind::ReturnOfModding => "ReturnOfModding",
    LoaderKind::Rivet => "Rivet",
    LoaderKind::Shimloader => "Shimloader",
    LoaderKind::Umm => "Unity Mod Manager",
    _ => "its mod loader",
  }
}

/// A launch that did not start the game.
pub fn launch_failed(game: &str, launch: LaunchMode, failure: &LaunchFailure) -> Failure {
  let headline = format!("{game} could not be started {}", mode(launch));

  match failure {
    LaunchFailure::NoModLoader(loader) => {
      let loader = loader_name(*loader);

      Failure {
        headline,
        detail: format!("Mods load through {loader}, which this profile does not have."),
        next: format!("Install {loader} from the list, or launch vanilla."),
      }
    }
    LaunchFailure::MissingGameFolder(folder) => Failure {
      headline,
      detail: format!("{} does not exist.", folder.display()),
      next: "Switch game and choose the folder the game is installed in.".to_string(),
    },
    LaunchFailure::Other(reason) => Failure {
      headline,
      detail: reason.clone(),
      next: "Check that the game is installed and Steam is running, then try again.".to_string(),
    },
  }
}

/// The tooltip and accessible name of [`NEW_PROFILE_BUTTON`].
pub const NEW_PROFILE: &str = "New profile…";
/// The label of the button beside the "Profiles" heading that names a new
/// profile.
pub const NEW_PROFILE_BUTTON: &str = "New";
pub const RENAME_PROFILE: &str = "Rename…";
pub const DUPLICATE_PROFILE: &str = "Duplicate…";
pub const DELETE_PROFILE: &str = "Delete…";
pub const PROFILES: &str = "Profiles";
/// The tooltip and accessible name of [`IMPORT_PROFILE_BUTTON`].
pub const IMPORT_PROFILE: &str = "Import a profile";
/// The label of the button beside the "Profiles" heading that opens a menu
/// of the places a profile can be imported from.
pub const IMPORT_PROFILE_BUTTON: &str = "Import";
pub const RUNS_NATIVELY: &str = "Runs natively";
pub const RUNS_UNDER_PROTON: &str = "Runs under Proton";
pub const DELETE: &str = "Delete";
pub const SAVE: &str = "Save";
pub const DELETE_WARNING: &str =
  "Its mods and their config files are deleted with it. This cannot be undone.";

/// What the profile name field asks for.
pub fn name_prompt(naming: &Naming) -> String {
  match naming {
    Naming::Create => "Name the new profile".to_string(),
    Naming::Rename(from) => format!("Rename the {from} profile"),
    Naming::Duplicate(from) => format!("Name the copy of the {from} profile"),
  }
}

/// A profile name that was refused, with the rule it breaks.
pub fn name_refused(refusal: &NameRefusal) -> Failure {
  let detail = match refusal {
    NameRefusal::Taken(existing) => format!("There is already a profile named {existing}."),
    NameRefusal::Invalid(BadName::Empty) => "The name is empty.".to_string(),
    NameRefusal::Invalid(BadName::Escapes) => {
      "A name cannot be . or .. or contain / or \\.".to_string()
    }
    NameRefusal::Invalid(BadName::DrivePrefix) => {
      "A name cannot start with a drive letter and a colon, such as C:.".to_string()
    }
    NameRefusal::Invalid(BadName::Reserved) => {
      "Windows keeps this name for a device, such as CON, NUL, or COM1.".to_string()
    }
    NameRefusal::Invalid(BadName::ForbiddenChar(found)) if found.is_control() => {
      "A name cannot contain a control character.".to_string()
    }
    NameRefusal::Invalid(BadName::ForbiddenChar(found)) => {
      format!("A name cannot contain {found}. Windows does not allow < > : \" | ? * in names.")
    }
    NameRefusal::Invalid(BadName::TrailingDotOrSpace) => {
      "A name cannot end with a dot or a space.".to_string()
    }
  };

  Failure {
    headline: "That name cannot be used".to_string(),
    detail,
    next: "Choose another name.".to_string(),
  }
}

/// How many packages a profile holds, such as `1 mod` or `12 mods`.
pub fn mod_count(count: usize) -> String {
  match count {
    1 => "1 mod".to_string(),
    count => format!("{count} mods"),
  }
}

/// The question asked before deleting a profile.
pub fn confirm_delete(name: &str) -> String {
  format!("Delete the {name} profile?")
}

/// A change to the profiles being made, such as `Renaming Modded to Heavy`.
pub fn changing(change: &ProfileChange) -> String {
  match change {
    ProfileChange::Create(name) => format!("Creating {name}"),
    ProfileChange::Rename { from, to } => format!("Renaming {from} to {to}"),
    ProfileChange::Duplicate { from, to } => format!("Copying {from} to {to}"),
    ProfileChange::Delete(name) => format!("Deleting {name}"),
  }
}

const CHECK_DATA_FOLDER: &str =
  "Check that the app's data folder can be written to, then try again.";

/// The profiles could not be listed.
pub fn profiles_unlisted(reason: &str) -> Failure {
  Failure {
    headline: "Your profiles could not be listed".to_string(),
    detail: reason.to_string(),
    next: CHECK_DATA_FOLDER.to_string(),
  }
}

/// A change to the profiles that was made, such as `Renamed Modded to
/// Heavy`.
pub fn profile_changed(change: &ProfileChange) -> String {
  match change {
    ProfileChange::Create(name) => format!("Created the {name} profile"),
    ProfileChange::Rename { from, to } => format!("Renamed {from} to {to}"),
    ProfileChange::Duplicate { from, to } => format!("Copied {from} to {to}"),
    ProfileChange::Delete(name) => format!("Deleted the {name} profile"),
  }
}

/// A change to the profiles that could not be made, such as `The Modded
/// profile could not be renamed`.
pub fn profile_change_failed(change: &ProfileChange) -> String {
  match change {
    ProfileChange::Create(name) => format!("The {name} profile could not be created"),
    ProfileChange::Rename { from, .. } => format!("The {from} profile could not be renamed"),
    ProfileChange::Duplicate { from, .. } => format!("The {from} profile could not be copied"),
    ProfileChange::Delete(name) => format!("The {name} profile could not be deleted"),
  }
}

pub const LAUNCH_LINE_COPIED: &str = "Launch option copied";
pub const PASTE_IN_STEAM: &str = "Paste it into the game's launch options in Steam.";

/// A notice as shown to the user: a headline, then any lines of detail.
pub struct Told {
  pub title: String,
  pub detail: Vec<String>,
}

/// What `notice` tells the user, naming games through `game_name`.
///
/// ```
/// use rivet_app::text::notice;
/// use rivet_core::install::{Action, FailedPackage};
/// use rivet_core::notice::Notice;
///
/// let failed = Notice::OperationFailed {
///   game: "valheim".to_string(),
///   profile: "Default".to_string(),
///   action: Action::Install("Owner-Mod".to_string()),
///   failed: vec![FailedPackage {
///     package: "Owner-Mod".to_string(),
///     reason: "corrupt archive".to_string(),
///   }],
///   changed: vec!["Owner-Lib".to_string()],
/// };
/// let told = notice(&failed, |_| "Valheim".to_string());
///
/// assert_eq!(told.title, "Owner-Mod could not be installed");
/// assert_eq!(
///   told.detail,
///   [
///     "Valheim, Default profile",
///     "Owner-Mod: corrupt archive",
///     "These were installed before it failed: Owner-Lib.",
///   ]
/// );
///
/// let cancelled = Notice::OperationCancelled {
///   game: "valheim".to_string(),
///   profile: "Default".to_string(),
///   action: Action::Install("Owner-Mod".to_string()),
///   changed: vec!["Owner-Lib".to_string()],
/// };
/// let told = notice(&cancelled, |_| "Valheim".to_string());
///
/// assert_eq!(
///   told.title,
///   "Cancelled installing Owner-Mod after changing Owner-Lib."
/// );
/// assert_eq!(told.detail, ["Valheim, Default profile"]);
/// ```
pub fn notice(notice: &Notice, game_name: impl Fn(&str) -> String) -> Told {
  match notice {
    Notice::OperationFinished {
      game,
      profile,
      action,
      changed,
    } => Told {
      title: finished(action, changed),
      detail: vec![place(&game_name(game), profile)],
    },
    Notice::OperationFailed {
      game,
      profile,
      action,
      failed,
      changed,
    } => {
      let failure = operation_failed(action, failed, changed);
      let detail = std::iter::once(place(&game_name(game), profile))
        .chain(failure.detail.lines().map(str::to_string))
        .chain([failure.next])
        .collect();

      Told {
        title: failure.headline,
        detail,
      }
    }
    Notice::OperationCancelled {
      game,
      profile,
      action,
      changed,
    } => Told {
      title: cancelled(action, changed),
      detail: vec![place(&game_name(game), profile)],
    },
    Notice::ProfileChanged(change) => Told {
      title: profile_changed(change),
      detail: Vec::new(),
    },
    Notice::ProfileChangeFailed { change, reason } => Told {
      title: profile_change_failed(change),
      detail: vec![reason.clone(), CHECK_DATA_FOLDER.to_string()],
    },
    Notice::IndexStale(reason) => {
      let stale = stale(reason);

      Told {
        title: stale.headline,
        detail: vec![stale.detail, stale.next],
      }
    }
    Notice::Launched { game, mode } => Told {
      title: launched(&game_name(game), *mode),
      detail: Vec::new(),
    },
    Notice::LaunchFailed {
      game,
      mode,
      failure,
    } => {
      let failure = launch_failed(&game_name(game), *mode, failure);

      Told {
        title: failure.headline,
        detail: vec![failure.detail, failure.next],
      }
    }
    Notice::LaunchLineCopied => Told {
      title: LAUNCH_LINE_COPIED.to_string(),
      detail: vec![PASTE_IN_STEAM.to_string()],
    },
    Notice::SteamOptionSet(game) => Told {
      title: STEAM_SET_UP.to_string(),
      detail: vec![steam_set_up(&game_name(game))],
    },
    Notice::ProfileExported { profile, exported } => profile_exported(profile, exported),
    Notice::ExportFailed {
      profile,
      to,
      reason,
    } => {
      let failure = export_failed(profile, to, reason);

      Told {
        title: failure.headline,
        detail: vec![failure.detail, failure.next],
      }
    }
    Notice::ImportUnread(failure) => {
      let failure = read_failure(failure);

      Told {
        title: failure.headline,
        detail: vec![failure.detail, failure.next],
      }
    }
    Notice::ImportNeedsGame(file) => Told {
      title: format!("Open a game to import {}", file_name(file)),
      detail: vec![
        "A shared profile goes into the game that's open. Open the game it's for, then open the file again.".to_string(),
      ],
    },
    Notice::ImportBusy(file) => Told {
      title: format!("{} wasn't imported", file_name(file)),
      detail: vec![
        "Another import is under way. Finish or close it, then open the file again.".to_string(),
      ],
    },
    Notice::LoaderFilesRemoved {
      game,
      changed,
      failed,
    } => loader_files_removed(&game_name(game), changed, failed),
    Notice::PinnedSkipped(count) => Told {
      title: pinned_skipped(*count),
      detail: vec![UNPIN_TO_UPDATE.to_string()],
    },
    Notice::UnpinFailed { package, reason } => Told {
      title: unpin_failed(package),
      detail: vec![reason.clone(), CHECK_DATA_FOLDER.to_string()],
    },
    Notice::PinsUnread(reason) => Told {
      title: PINS_UNREAD.to_string(),
      detail: vec![reason.clone(), PINS_UNREAD_NEXT.to_string()],
    },
    Notice::ImportIncomplete { profile, missing } => import_incomplete(profile, missing),
    Notice::R2modmanUnread { profile, failure } => r2modman_unread(profile, failure),
    Notice::RivetUnwritable { version, .. } => Told {
      title: format!("Rivet {version} is out"),
      detail: vec![UNWRITABLE.to_string()],
    },
    Notice::RivetDownloadFailed { version, failure } => Told {
      title: download_failed(version, failure),
      detail: Vec::new(),
    },
    Notice::RestartRefused => Told {
      title: RESTART_REFUSED.to_string(),
      detail: Vec::new(),
    },
    Notice::RivetReplaceFailed(reason) => Told {
      title: "Rivet could not update itself".to_string(),
      detail: vec![reason.clone(), "This version keeps running.".to_string()],
    },
    Notice::RivetUpdated(version) => Told {
      title: format!("Rivet updated to {version}"),
      detail: Vec::new(),
    },
    Notice::RivetUpdateFailed { to } => Told {
      title: format!("The update to Rivet {to} did not start"),
      detail: vec!["The version before it is running instead.".to_string()],
    },
  }
}

/// The last part of `path`, or all of it when it has none.
fn file_name(path: &Path) -> String {
  path
    .file_name()
    .unwrap_or(path.as_os_str())
    .to_string_lossy()
    .into_owned()
}

/// A removal of the loader files launches copied into `game`'s folder,
/// naming the files it left and why.
///
/// ```
/// use rivet_app::text::loader_files_removed;
/// use std::path::PathBuf;
///
/// let changed = [PathBuf::from("/games/Valheim/doorstop_config.ini")];
/// let failed = [(
///   PathBuf::from("/games/Valheim/winhttp.dll"),
///   "permission denied".to_string(),
/// )];
/// let told = loader_files_removed("Valheim", &changed, &failed);
///
/// assert_eq!(told.title, "Some loader files could not be removed from Valheim");
/// assert_eq!(
///   told.detail,
///   [
///     "winhttp.dll: permission denied",
///     "Left doorstop_config.ini, since it changed after Rivet copied it.",
///     "Close the game if it is running, then try again.",
///   ]
/// );
/// assert_eq!(
///   loader_files_removed("Valheim", &[], &[]).title,
///   "Removed the loader files from Valheim's folder"
/// );
/// ```
pub fn loader_files_removed(game: &str, changed: &[PathBuf], failed: &[(PathBuf, String)]) -> Told {
  let name = |path: &PathBuf| {
    path
      .file_name()
      .unwrap_or(path.as_os_str())
      .to_string_lossy()
      .into_owned()
  };
  let title = match failed {
    [] => format!("Removed the loader files from {game}'s folder"),
    _ => format!("Some loader files could not be removed from {game}"),
  };
  let mut detail: Vec<String> = failed
    .iter()
    .map(|(path, reason)| format!("{}: {reason}", name(path)))
    .collect();

  if !changed.is_empty() {
    let names: Vec<String> = changed.iter().map(name).collect();
    let (they, them) = match changed {
      [_] => ("it", "it"),
      _ => ("they", "them"),
    };

    detail.push(format!(
      "Left {}, since {they} changed after Rivet copied {them}.",
      names.join(", ")
    ));
  }

  if !failed.is_empty() {
    detail.push(TRY_AGAIN_AFTER_CLOSING.to_string());
  }

  Told { title, detail }
}

pub const TRY_AGAIN_AFTER_CLOSING: &str = "Close the game if it is running, then try again.";

/// A profile imported without the mods Thunderstore does not have.
///
/// ```
/// use rivet_app::text::import_incomplete;
///
/// let told = import_incomplete("Modded", &["Owner-Gone-1.0.0".to_string()]);
///
/// assert_eq!(told.title, "Modded is imported without some mods");
/// assert_eq!(told.detail, ["Thunderstore does not have Owner-Gone-1.0.0."]);
/// ```
pub fn import_incomplete(profile: &str, missing: &[String]) -> Told {
  Told {
    title: format!("{profile} is imported without some mods"),
    detail: vec![import_missing(missing).detail],
  }
}

/// An r2modman profile that could not be read, so it was not imported.
///
/// ```
/// use rivet_app::text::r2modman_unread;
/// use rivet_core::portability::ReadFailure;
///
/// let told = r2modman_unread(
///   "Modded",
///   &ReadFailure::Unreadable("permission denied".to_string()),
/// );
///
/// assert_eq!(told.title, "The r2modman profile Modded could not be read");
/// assert_eq!(told.detail[0], "permission denied");
/// ```
pub fn r2modman_unread(profile: &str, failure: &ReadFailure) -> Told {
  Told {
    title: format!("The r2modman profile {profile} could not be read"),
    detail: vec![
      read_failure(failure).detail,
      "Check that you can open its folder, then import it again from Import a profile.".to_string(),
    ],
  }
}

pub const IMPORT_R2MODMAN: &str = "Import from r2modman…";
pub const R2MODMAN_PROMPT: &str = "Import your r2modman profiles?";
pub const R2MODMAN_LATER: &str = "You can import them later from Import a profile.";
pub const R2MODMAN_WAITING: &str = "Import can start once the mod list has loaded.";
pub const NOT_NOW: &str = "Not now";

/// What the offer to import r2modman's profiles does, such as `r2modman has
/// profiles for Valheim. The ones you choose are copied in as new profiles,
/// and r2modman's own files are left as they are.`.
pub fn r2modman_lead(game: &str) -> String {
  format!(
    "r2modman has profiles for {game}. The ones you choose are copied in as new profiles, \
     and r2modman's own files are left as they are."
  )
}

pub const SHARE_CODE: &str = "Share as code";
pub const SHARE_FILE: &str = "Share as file…";
pub const IMPORT_CODE: &str = "Import from code…";
pub const IMPORT_FILE: &str = "Import from file…";
pub const IMPORT: &str = "Import";
pub const READ_CODE: &str = "Continue";
pub const IMPORT_CODE_PROMPT: &str = "Import a profile from its code";
pub const IMPORT_CODE_LEAD: &str =
  "Paste the code someone shared from Rivet or r2modman. The profile is imported as a new one.";
pub const READING_SHARED: &str = "Reading the shared profile";

/// A profile being shared, such as `Sharing the Default profile`.
pub fn exporting(profile: &str) -> String {
  format!("Sharing the {profile} profile")
}

/// A profile shared: the code that was copied, or the file written.
///
/// ```
/// use rivet_app::text::profile_exported;
/// use rivet_core::portability::Exported;
/// use std::path::PathBuf;
///
/// let told = profile_exported("Default", &Exported::Code("0193e2b5".to_string()));
///
/// assert_eq!(told.title, "Profile code copied: 0193e2b5");
/// assert_eq!(
///   told.detail,
///   ["Anyone can import the Default profile with it in Rivet or r2modman."]
/// );
///
/// let told = profile_exported("Default", &Exported::File(PathBuf::from("/home/me/Default.r2z")));
///
/// assert_eq!(told.title, "Shared the Default profile as a file");
/// ```
pub fn profile_exported(profile: &str, exported: &Exported) -> Told {
  match exported {
    Exported::Code(code) => Told {
      title: format!("Profile code copied: {code}"),
      detail: vec![format!(
        "Anyone can import the {profile} profile with it in Rivet or r2modman."
      )],
    },
    Exported::File(path) => Told {
      title: format!("Shared the {profile} profile as a file"),
      detail: vec![path.display().to_string()],
    },
  }
}

/// A profile that could not be shared.
pub fn export_failed(profile: &str, to: &Destination, reason: &str) -> Failure {
  let next = match to {
    Destination::Code => {
      "Check your connection, then try again. Thunderstore takes only a few codes a minute."
    }
    Destination::File(_) => "Choose a folder you can save to, then try again.",
  };

  Failure {
    headline: format!("The {profile} profile could not be shared"),
    detail: reason.to_string(),
    next: next.to_string(),
  }
}

/// A shared profile that could not be read, with what to do next.
pub fn read_failure(failure: &ReadFailure) -> Failure {
  match failure {
    ReadFailure::UnknownCode => Failure {
      headline: "No profile has this code".to_string(),
      detail: "Thunderstore does not know it. It may be mistyped, or it may have expired."
        .to_string(),
      next: "Check the code with whoever shared it, or ask them for a new one.".to_string(),
    },
    ReadFailure::Damaged(reason) => Failure {
      headline: "This is not a shared profile".to_string(),
      detail: reason.clone(),
      next: "Choose a .r2z file shared from Rivet or r2modman, or ask for the profile again."
        .to_string(),
    },
    ReadFailure::Unreachable(reason) => Failure {
      headline: "Thunderstore could not be reached".to_string(),
      detail: reason.clone(),
      next: "Check your connection, then try again.".to_string(),
    },
    ReadFailure::Unreadable(reason) => Failure {
      headline: "The file could not be read".to_string(),
      detail: reason.clone(),
      next: "Check that the file exists and that you can open it, then try again.".to_string(),
    },
  }
}

/// The question asked before importing a shared profile, such as `Import
/// Modded as a new profile`.
pub fn import_prompt(shared: &str) -> String {
  format!("Import {shared} as a new profile")
}

/// What an import brings, such as `It installs 12 mods at the versions it
/// lists, with their config files.`.
pub fn import_lead(count: usize) -> String {
  format!(
    "It installs {} at the versions it lists, with their config files.",
    mod_count(count)
  )
}

/// Packages a shared profile lists that Thunderstore does not have.
pub fn import_missing(missing: &[String]) -> Failure {
  Failure {
    headline: "Some mods are left out".to_string(),
    detail: format!("Thunderstore does not have {}.", missing.join(", ")),
    next: "The rest can still be imported. Press Refresh first in case they were added since."
      .to_string(),
  }
}

pub const STEAM_SET_UP: &str = "Steam is set up";

/// What a Steam launch option found set means, such as `Steam now starts
/// Valheim through Rivet, so modded launches load your mods.`.
pub fn steam_set_up(game: &str) -> String {
  format!("Steam now starts {game} through Rivet, so modded launches load your mods.")
}

pub const NO_MOD_SHOWN: &str = "No mod to show.";
pub const README_MISSING: &str = "This mod has no readme.";
/// Reopens the detail pane.
pub const DETAILS: &str = "Details";
pub const HIDE_DETAILS: &str = "Hide details";
pub const DOWNLOADS: &str = "Downloads";
pub const UPDATED: &str = "Updated";
pub const CATEGORIES: &str = "Categories";
pub const NO_CATEGORIES: &str = "None";

/// The detail pane's button updating a mod, such as `Update to 2.0.0`.
pub fn update_to(version: &str) -> String {
  format!("Update to {version}")
}

/// A mod's categories on one line, or [`NO_CATEGORIES`].
///
/// ```
/// use rivet_app::text::categories;
///
/// assert_eq!(categories(&["Tools".to_string(), "Maps".to_string()]), "Tools, Maps");
/// assert_eq!(categories(&[]), "None");
/// ```
pub fn categories(categories: &[String]) -> String {
  match categories {
    [] => NO_CATEGORIES.to_string(),
    categories => categories.join(", "),
  }
}

/// How long before `now` the moment `then` was, both in seconds since the
/// Unix epoch, in its largest whole unit. A month is 30 days and a year 365.
///
/// ```
/// use rivet_app::text::ago;
///
/// const DAY: i64 = 86_400;
///
/// assert_eq!(ago(1_000, 1_030), "just now");
/// assert_eq!(ago(0, 60), "1 minute ago");
/// assert_eq!(ago(0, 5 * 3_600), "5 hours ago");
/// assert_eq!(ago(0, DAY), "1 day ago");
/// assert_eq!(ago(0, 21 * DAY), "3 weeks ago");
/// assert_eq!(ago(0, 29 * DAY), "4 weeks ago");
/// assert_eq!(ago(0, 75 * DAY), "2 months ago");
/// assert_eq!(ago(0, 800 * DAY), "2 years ago");
/// assert_eq!(ago(100, 0), "just now");
/// ```
pub fn ago(then: i64, now: i64) -> String {
  const MINUTE: i64 = 60;
  const HOUR: i64 = 60 * MINUTE;
  const DAY: i64 = 24 * HOUR;
  const WEEK: i64 = 7 * DAY;
  const MONTH: i64 = 30 * DAY;
  const YEAR: i64 = 365 * DAY;

  let elapsed = now - then;
  let (count, unit) = match elapsed {
    ..MINUTE => return "just now".to_string(),
    MINUTE..HOUR => (elapsed / MINUTE, "minute"),
    HOUR..DAY => (elapsed / HOUR, "hour"),
    DAY..WEEK => (elapsed / DAY, "day"),
    WEEK..MONTH => (elapsed / WEEK, "week"),
    MONTH..YEAR => (elapsed / MONTH, "month"),
    YEAR.. => (elapsed / YEAR, "year"),
  };

  match count {
    1 => format!("1 {unit} ago"),
    count => format!("{count} {unit}s ago"),
  }
}

/// The readme of `full_name` is on its way, such as `Loading the readme
/// of Owner-ModA`.
pub fn readme_loading(full_name: &str) -> String {
  format!("Loading the readme of {full_name}")
}

/// The readme could not be fetched, with the reason.
pub fn readme_failed(reason: &str) -> String {
  format!("The readme could not be loaded: {reason}. Select the mod again to retry.")
}

pub const RIVET: &str = "Rivet";
pub const UPDATE_AUTOMATICALLY: &str = "Update Rivet automatically";
pub const UPDATE_AUTOMATICALLY_HINT: &str =
  "Checks GitHub for a newer Rivet each time it starts, and offers to download it.";
pub const CHECK_FOR_UPDATES: &str = "Check for updates";
pub const RESTART_TO_UPDATE: &str = "Restart to update";
pub const OPEN_RELEASE: &str = "Open release";
pub const RESTART_REFUSED: &str = "Rivet can't restart while operations are running or waiting. Restart to update once they finish.";
pub const UNWRITABLE: &str = "Rivet can't replace itself where it is installed, so it was not downloaded. Get it from its release page.";

/// The version of Rivet running, such as `Version 0.2.0`.
pub fn rivet_version(version: &str) -> String {
  format!("Version {version}")
}

/// Where keeping Rivet current has got, shown beside Check for updates, or
/// `None` when there is nothing to say.
///
/// ```
/// use rivet_app::text::update_stage;
/// use rivet_core::self_update::{CheckFailure, Stage};
///
/// assert_eq!(update_stage(&Stage::Idle), None);
/// assert_eq!(
///   update_stage(&Stage::UpToDate).as_deref(),
///   Some("Rivet is up to date.")
/// );
/// assert_eq!(
///   update_stage(&Stage::CheckFailed(CheckFailure::Unreachable(
///     "GitHub answered 403 Forbidden".to_string()
///   )))
///   .as_deref(),
///   Some("Couldn't check for updates: GitHub answered 403 Forbidden.")
/// );
/// ```
pub fn update_stage(stage: &UpdateStage) -> Option<String> {
  let said = match stage {
    UpdateStage::Idle => return None,
    UpdateStage::Checking { .. } => "Checking GitHub for a newer Rivet.".to_string(),
    UpdateStage::UpToDate => "Rivet is up to date.".to_string(),
    UpdateStage::CheckFailed(CheckFailure::Unreachable(reason)) => {
      format!("Couldn't check for updates: {reason}.")
    }
    UpdateStage::CheckFailed(CheckFailure::NoBuild(version)) => {
      format!("Rivet {version} is out, but not for this computer.")
    }
    UpdateStage::Offered(found) => format!("Rivet {} is out.", found.version),
    UpdateStage::Downloading { download, .. } => {
      format!("Rivet {} found. Downloading it.", download.found.version)
    }
    UpdateStage::Ready { version, .. } => format!("Rivet {version} is ready."),
    UpdateStage::Unwritable { version, .. } => format!("Rivet {version} is out. {UNWRITABLE}"),
    UpdateStage::Restarting { version, .. } => format!("Restarting into Rivet {version}."),
  };

  Some(said)
}

/// The title of the modal offering a newer Rivet, such as `Update to Rivet
/// 0.3.0?`.
pub fn rivet_offered(version: &str) -> String {
  format!("Update to Rivet {version}?")
}

/// What going ahead with a newer Rivet does, given the version `running`.
///
/// ```
/// use rivet_app::text::rivet_offer_detail;
///
/// assert_eq!(
///   rivet_offer_detail("0.2.0"),
///   "You have Rivet 0.2.0. The update downloads in the background, then Rivet asks you to restart."
/// );
/// ```
pub fn rivet_offer_detail(running: &str) -> String {
  format!(
    "You have Rivet {running}. The update downloads in the background, then Rivet asks you to restart."
  )
}

/// A newer Rivet's download in the status bar, with how far it has got,
/// such as `Downloading Rivet 0.3.0, 45%`.
///
/// ```
/// use rivet_app::text::rivet_downloading;
///
/// assert_eq!(rivet_downloading("0.3.0", 450, Some(1000)), "Downloading Rivet 0.3.0, 45%");
/// assert_eq!(rivet_downloading("0.3.0", 2_500_000, None), "Downloading Rivet 0.3.0, 2.5 MB");
/// ```
pub fn rivet_downloading(version: &str, received: u64, total: Option<u64>) -> String {
  match total.filter(|total| *total > 0) {
    Some(total) => format!(
      "Downloading Rivet {version}, {}%",
      received.saturating_mul(100) / total
    ),
    None => format!(
      "Downloading Rivet {version}, {:.1} MB",
      received as f64 / 1_000_000.
    ),
  }
}

/// A downloaded Rivet waiting in the status bar, such as `Rivet 0.3.0 is
/// ready`.
pub fn rivet_ready(version: &str) -> String {
  format!("Rivet {version} is ready")
}

/// A newer Rivet's failed download in one line.
fn download_failed(version: &str, failure: &DownloadFailure) -> String {
  let why = match failure {
    DownloadFailure::Unwritable => UNWRITABLE.to_string(),
    DownloadFailure::Cancelled => "it was cancelled.".to_string(),
    DownloadFailure::Mismatch => "it did not match its checksum, so it was deleted.".to_string(),
    DownloadFailure::Failed(reason) => format!("{reason}."),
  };

  format!("Rivet {version} could not be downloaded: {why}")
}
