//! The game picker: which games it offers and whether a chosen folder holds
//! the game.

use std::path::PathBuf;
use thunderstore_engine::ecosystem::{Ecosystem, GameDef};

/// The picker's own state. It outlives visits to the picker, so the search
/// text is still there on return.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Picker {
  pub query: String,
  pub step: Step,
  /// The last folder refused, shown until another game or folder is picked.
  pub refusal: Option<Refusal>,
}

/// How far choosing a game's folder has got.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Step {
  #[default]
  Idle,
  /// The folder dialog is open for this game.
  Choosing(String),
  /// This folder is being listed to check it holds this game.
  Checking { game: String, folder: PathBuf },
}

/// A folder refused as a game's install folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
  pub game: String,
  pub folder: PathBuf,
  pub reason: FolderRefusal,
}

/// The games the picker offers for `query`, sorted by display name.
///
/// Only [`offered`] games are listed. `query` matches the display name,
/// ignoring case.
pub fn games<'a>(ecosystem: &'a Ecosystem, query: &str) -> Vec<&'a GameDef> {
  let query = query.to_lowercase();
  let mut games: Vec<&GameDef> = ecosystem
    .games()
    .filter(|game| offered(game))
    .filter(|game| game.display_name().to_lowercase().contains(&query))
    .collect();

  games.sort_by_cached_key(|game| game.display_name().to_lowercase());

  games
}

/// Whether the picker offers `game`: its definition has a mod manager install
/// definition (the engine's `GameProfile`), without which the app cannot
/// install packages for it.
pub fn offered(game: &GameDef) -> bool {
  game.profile().is_some()
}

/// Why a chosen folder was refused as a game's install folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderRefusal {
  /// The folder holds none of the game's executables. `expected` lists the
  /// file names that were looked for.
  MissingExecutable { expected: Vec<String> },
  /// The folder could not be listed. The detail is the system's reason.
  Unreadable(String),
}

/// Accepts `entries`, the file names in a folder, as an install folder of
/// `game` when one of them is an executable named by the game's primary
/// install definition.
/// Names are compared ignoring ASCII case, as Windows does.
///
/// ```
/// use rivet_core::picker::{FolderRefusal, check_folder};
/// use thunderstore_engine::ecosystem::Ecosystem;
///
/// let ecosystem = Ecosystem::bundled();
/// let valheim = ecosystem.game("valheim").unwrap();
///
/// assert_eq!(check_folder(valheim, &["valheim.x86_64".into()]), Ok(()));
/// assert!(matches!(
///   check_folder(valheim, &[]),
///   Err(FolderRefusal::MissingExecutable { .. })
/// ));
/// ```
pub fn check_folder(game: &GameDef, entries: &[String]) -> Result<(), FolderRefusal> {
  let expected = game
    .profile()
    .map(|profile| profile.exe_names.clone())
    .unwrap_or_default();
  let found = entries
    .iter()
    .any(|entry| expected.iter().any(|exe| entry.eq_ignore_ascii_case(exe)));

  if found {
    return Ok(());
  }

  Err(FolderRefusal::MissingExecutable { expected })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_ecosystem;

  fn names(games: Vec<&GameDef>) -> Vec<&str> {
    games.into_iter().map(GameDef::display_name).collect()
  }

  #[test]
  fn offers_every_game_it_can_install_for_sorted_by_name() {
    let ecosystem = test_ecosystem();

    assert_eq!(
      names(games(&ecosystem, "")),
      vec!["Lethal Company", "Risk of Rain 2", "Valheim"]
    );
  }

  #[test]
  fn search_narrows_by_name_ignoring_case() {
    let ecosystem = test_ecosystem();

    assert_eq!(names(games(&ecosystem, "RAIN")), vec!["Risk of Rain 2"]);
  }

  #[test]
  fn a_folder_holding_any_of_the_games_executables_is_accepted() {
    let ecosystem = test_ecosystem();
    let valheim = ecosystem.game("valheim").unwrap();
    let entries = vec!["valheim_Data".to_string(), "valheim.x86_64".to_string()];

    assert_eq!(check_folder(valheim, &entries), Ok(()));
  }

  #[test]
  fn executable_names_match_ignoring_case() {
    let ecosystem = test_ecosystem();
    let valheim = ecosystem.game("valheim").unwrap();

    assert_eq!(check_folder(valheim, &["Valheim.EXE".to_string()]), Ok(()));
  }

  #[test]
  fn a_folder_without_the_executable_is_refused_naming_what_was_looked_for() {
    let ecosystem = test_ecosystem();
    let valheim = ecosystem.game("valheim").unwrap();
    let entries = vec!["valheim_server.exe".to_string(), "readme.txt".to_string()];

    assert_eq!(
      check_folder(valheim, &entries),
      Err(FolderRefusal::MissingExecutable {
        expected: vec!["valheim.exe".to_string(), "valheim.x86_64".to_string()],
      })
    );
  }
}
