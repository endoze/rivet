//! Every piece of interface text, kept in one place so translation can be
//! added later.

use rivet_core::SettingsError;
use rivet_core::browse::{SortDirection, SortKey};
use rivet_core::install::{Action, FailedPackage, Planned, Progress, Stage};
use rivet_core::launch::LaunchFailure;
use rivet_core::manage::Removal;
use rivet_core::notice::Notice;
use rivet_core::picker::{FolderRefusal, NotFound, Refusal, STEAM_LIBRARY};
use rivet_core::portability::{Destination, Exported, ReadFailure};
use rivet_core::switcher::{NameRefusal, Naming, ProfileChange};
use std::path::Path;
use thunderstore_engine::ecosystem::LoaderKind;
use thunderstore_engine::profile::launch::LaunchMode;
use thunderstore_engine::profile::layout::BadName;

pub const LOADING: &str = "Loading";
pub const PICKER_TITLE: &str = "Choose your game";
pub const SEARCH_PLACEHOLDER: &str = "Search games";
pub const NO_GAME_MATCHES: &str = "No game matches your search.";
pub const SWITCH_GAME: &str = "Switch game";
pub const REFRESH: &str = "Refresh";
pub const REFRESHING: &str = "Checking Thunderstore for new mods";
pub const LOADING_PACKAGES: &str = "Loading mods";
pub const NO_PACKAGES: &str = "No mods to show.";
pub const PACKAGE_SEARCH_PLACEHOLDER: &str = "Search mods by name, author, or description";
pub const NO_PACKAGE_MATCHES: &str = "No mods match your search and filters.";
pub const SORT_BY: &str = "Sort by";
pub const SHOW_NSFW: &str = "Show NSFW";
pub const SHOW_DEPRECATED: &str = "Show deprecated";
pub const CLEAR_FILTERS: &str = "Clear";

/// A sort button's label, with an arrow for its direction when it is the
/// current sort, such as `Downloads ↓`. Best match has no arrow, as it only
/// sorts one way.
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

pub fn installed_in(folder: &Path) -> String {
  format!("Installed in {}", folder.display())
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

pub const TAB_BROWSE: &str = "Browse";
pub const TAB_INSTALLED: &str = "Installed";
pub const INSTALLED_SEARCH_PLACEHOLDER: &str = "Search installed mods";
pub const SORT_NAME: &str = "Name";
pub const SORT_AUTHOR: &str = "Author";
pub const SORT_NEWEST: &str = "Newest";
pub const ENABLE: &str = "Enable";
pub const DISABLE: &str = "Disable";
pub const UNINSTALL: &str = "Uninstall";
pub const ENABLE_ALL: &str = "Enable all";
pub const DISABLE_ALL: &str = "Disable all";
pub const UPDATE: &str = "Update";
pub const UPDATE_ALL: &str = "Update all";
pub const DEPRECATED: &str = "Deprecated";

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
pub const ENABLED: &str = "Enabled";
pub const DISABLED: &str = "Disabled";
pub const MOD_LOADER: &str = "Mod loader";
pub const NO_INSTALLED: &str = "No mods are installed in this profile.";
pub const NO_INSTALLED_MATCHES: &str = "No installed mod matches your search.";
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

/// The open profile lacks the mod loader its game's packages load through.
pub fn missing_loader(profile: &str, loader: LoaderKind) -> Failure {
  let loader = loader_name(loader);

  Failure {
    headline: format!("The {profile} profile has no mod loader"),
    detail: format!(
      "This game's mods load through {loader}, which is not installed, so a modded launch would start the game without them."
    ),
    next: format!("Install {loader}, or any mod that needs it, from the list."),
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

pub const NEW_PROFILE: &str = "New profile…";
pub const RENAME_PROFILE: &str = "Rename…";
pub const DUPLICATE_PROFILE: &str = "Duplicate…";
pub const DELETE_PROFILE: &str = "Delete…";
pub const PROFILE_ACTIONS: &str = "Profile actions";
/// Between the game and the profile in the header, read as "Valheim's
/// Default profile".
pub const HEADER_SEPARATOR: &str = "›";
pub const DELETE: &str = "Delete";
pub const SAVE: &str = "Save";
pub const DELETE_WARNING: &str =
  "Its mods and their config files are deleted with it. This cannot be undone.";

/// What the profile name field asks for.
pub fn name_prompt(naming: Naming, from: &str) -> String {
  match naming {
    Naming::Create => "Name the new profile".to_string(),
    Naming::Rename => format!("Rename the {from} profile"),
    Naming::Duplicate => format!("Name the copy of the {from} profile"),
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
  }
}

pub const EXPORT_CODE: &str = "Export as code";
pub const EXPORT_FILE: &str = "Export as file…";
pub const IMPORT_CODE: &str = "Import from code…";
pub const IMPORT_FILE: &str = "Import from file…";
pub const IMPORT: &str = "Import";
pub const READ_CODE: &str = "Continue";
pub const IMPORT_CODE_PROMPT: &str = "Import a profile from its code";
pub const IMPORT_CODE_LEAD: &str =
  "Paste the code someone shared from Rivet or r2modman. The profile is imported as a new one.";
pub const READING_SHARED: &str = "Reading the shared profile";

/// An export being made, such as `Exporting the Default profile`.
pub fn exporting(profile: &str) -> String {
  format!("Exporting the {profile} profile")
}

/// A finished export: the code that was copied, or the file written.
///
/// ```
/// use rivet_app::text::profile_exported;
/// use rivet_core::portability::Exported;
///
/// let told = profile_exported("Default", &Exported::Code("0193e2b5".to_string()));
///
/// assert_eq!(told.title, "Profile code copied: 0193e2b5");
/// assert_eq!(
///   told.detail,
///   ["Anyone can import the Default profile with it in Rivet or r2modman."]
/// );
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
      title: format!("Exported the {profile} profile"),
      detail: vec![path.display().to_string()],
    },
  }
}

/// An export that could not be made.
pub fn export_failed(profile: &str, to: &Destination, reason: &str) -> Failure {
  let next = match to {
    Destination::Code => {
      "Check your connection, then try again. Thunderstore takes only a few codes a minute."
    }
    Destination::File(_) => "Choose a folder you can save to, then try again.",
  };

  Failure {
    headline: format!("The {profile} profile could not be exported"),
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
      headline: "This is not a profile export".to_string(),
      detail: reason.clone(),
      next: "Choose a .r2z file exported from Rivet or r2modman, or ask for the profile again."
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

pub const NO_README_SELECTED: &str = "Select a mod to read its readme.";
pub const README_MISSING: &str = "This mod has no readme.";

/// The readme of `full_name` is on its way, such as `Loading the readme
/// of Owner-ModA`.
pub fn readme_loading(full_name: &str) -> String {
  format!("Loading the readme of {full_name}")
}

/// The readme could not be fetched, with the reason.
pub fn readme_failed(reason: &str) -> String {
  format!("The readme could not be loaded: {reason}. Select the mod again to retry.")
}
