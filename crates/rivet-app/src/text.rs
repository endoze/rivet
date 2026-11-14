//! Every piece of interface text, kept in one place so translation can be
//! added later.

use rivet_core::SettingsError;
use rivet_core::install::{FailedPackage, Planned, Progress, Stage};
use rivet_core::launch::LaunchFailure;
use rivet_core::picker::{FolderRefusal, Refusal};
use std::path::Path;
use thunderstore_engine::ecosystem::LoaderKind;
use thunderstore_engine::profile::launch::LaunchMode;

pub const LOADING: &str = "Loading";
pub const PICKER_TITLE: &str = "Choose your game";
pub const SEARCH_PLACEHOLDER: &str = "Search games";
pub const NO_GAME_MATCHES: &str = "No game matches your search.";
pub const SWITCH_GAME: &str = "Switch game";
pub const REFRESH: &str = "Refresh";
pub const REFRESHING: &str = "Checking Thunderstore for new mods";
pub const LOADING_PACKAGES: &str = "Loading mods";
pub const NO_PACKAGES: &str = "No mods to show.";

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
  }
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
  let stage = match progress.stage {
    Stage::Downloading => "downloading",
    Stage::Extracting => "unpacking",
    Stage::Installing => "copying",
  };
  let amount = match progress.total {
    Some(total) if total > 0 => format!(", {}%", progress.done.saturating_mul(100) / total),
    _ => String::new(),
  };

  format!("Installing {package}: {stage} {}{amount}", progress.package)
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
    [] => "Nothing in your profile was changed. Check your connection and try again.".to_string(),
    installed => format!(
      "These were installed before it failed: {}. Installing again finishes the rest.",
      installed.join(", ")
    ),
  };

  Failure {
    headline: format!("{package} could not be installed"),
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
pub const STEAM_OPTION_LEAD: &str = "Steam must start this game through Rivet for mods to load. Paste this line into the game's launch options in Steam, under Properties:";
pub const STEAM_OPTION_CHECKING: &str = "Checking Steam's launch options";
pub const STEAM_OPTION_SET: &str = "The launch option is set.";
pub const STEAM_OPTION_NOT_SET: &str = "Steam's saved launch options do not have this line. Steam saves them when it closes, so after pasting it, restart Steam and press Check again.";

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
