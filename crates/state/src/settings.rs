//! What the app remembers between runs. The shell stores it as TOML in the
//! platform's config location.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The settings file's contents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
  /// The community slug of the game open when the app last ran.
  pub last_game: Option<String>,
  /// Per-game settings, keyed by community slug.
  pub games: BTreeMap<String, GameSettings>,
}

/// Settings for one game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameSettings {
  /// The folder the game is installed in.
  pub folder: PathBuf,
}

impl Settings {
  /// The install folder recorded for `game`, if any.
  pub fn folder(&self, game: &str) -> Option<&Path> {
    self
      .games
      .get(game)
      .map(|settings| settings.folder.as_path())
  }
}
