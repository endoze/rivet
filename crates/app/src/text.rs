//! Every piece of interface text, kept in one place so translation can be
//! added later.

use state::SettingsError;
use state::picker::{FolderRefusal, Refusal};
use std::path::Path;

pub const LOADING: &str = "Loading";
pub const PICKER_TITLE: &str = "Choose your game";
pub const SEARCH_PLACEHOLDER: &str = "Search games";
pub const NO_GAME_MATCHES: &str = "No game matches your search.";
pub const SWITCH_GAME: &str = "Switch game";

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
