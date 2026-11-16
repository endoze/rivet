//! What the app remembers between runs. The shell stores it as TOML in the
//! platform's config location.

use crate::install::DEFAULT_PROFILE;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thunderstore_engine::profile::launch::Runtime;

/// The settings file's contents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
  /// The community slug of the game open when the app last ran.
  pub last_game: Option<String>,
  /// The Steam install the user chose, searched for games and read for
  /// launch options instead of the installs found on the computer. `None`
  /// uses the ones found.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub steam_folder: Option<PathBuf>,
  /// Per-game settings, keyed by community slug.
  pub games: BTreeMap<String, GameSettings>,
}

/// Settings for one game. Build one with [`GameSettings::new`], so a field
/// added later needs no change where they are made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameSettings {
  /// The folder the game is installed in.
  pub folder: PathBuf,
  /// The profile selected when the game was last open, or `None` for the
  /// default profile.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub profile: Option<String>,
  /// The user's launch arguments as typed, added after the mod loader's own
  /// on modded and vanilla launches. Split by [`split_arguments`].
  #[serde(default, skip_serializing_if = "String::is_empty")]
  pub launch_arguments: String,
  /// Whether the game runs natively or under Proton on Linux, or
  /// [`RuntimeChoice::Detect`] to tell from its folder.
  #[serde(default, skip_serializing_if = "RuntimeChoice::is_detect")]
  pub runtime: RuntimeChoice,
}

impl GameSettings {
  /// Settings for a game installed in `folder`, with everything else unset.
  ///
  /// ```
  /// use rivet_core::settings::{GameSettings, RuntimeChoice};
  ///
  /// let valheim = GameSettings::new("/games/valheim");
  ///
  /// assert_eq!(valheim.profile, None);
  /// assert_eq!(valheim.launch_arguments, "");
  /// assert_eq!(valheim.runtime, RuntimeChoice::Detect);
  /// ```
  pub fn new(folder: impl Into<PathBuf>) -> Self {
    Self {
      folder: folder.into(),
      profile: None,
      launch_arguments: String::new(),
      runtime: RuntimeChoice::Detect,
    }
  }
}

/// How the user wants a game run on Linux. Other hosts run every game
/// natively, whatever is chosen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeChoice {
  /// Tell from the game folder: Proton when it holds only the Windows build.
  #[default]
  Detect,
  Native,
  Proton,
}

impl RuntimeChoice {
  /// The runtime this choice forces, or `None` when it is detected.
  ///
  /// ```
  /// use rivet_core::settings::RuntimeChoice;
  /// use thunderstore_engine::profile::launch::Runtime;
  ///
  /// assert_eq!(RuntimeChoice::Proton.forced(), Some(Runtime::Proton));
  /// assert_eq!(RuntimeChoice::Detect.forced(), None);
  /// ```
  pub fn forced(self) -> Option<Runtime> {
    match self {
      Self::Detect => None,
      Self::Native => Some(Runtime::Native),
      Self::Proton => Some(Runtime::Proton),
    }
  }

  fn is_detect(&self) -> bool {
    *self == Self::Detect
  }
}

/// Splits launch arguments typed on one line into separate arguments, at
/// whitespace outside double quotes. The quotes themselves are dropped, so
/// `"My Logs"` is one argument. No other character is special.
///
/// ```
/// use rivet_core::settings::split_arguments;
///
/// assert_eq!(split_arguments(r#"-log "My Logs""#), vec!["-log", "My Logs"]);
/// ```
pub fn split_arguments(line: &str) -> Vec<String> {
  let mut arguments = Vec::new();
  let mut current: Option<String> = None;
  let mut quoted = false;

  for character in line.chars() {
    match character {
      '"' => {
        quoted = !quoted;
        current.get_or_insert_default();
      }
      character if character.is_whitespace() && !quoted => {
        arguments.extend(current.take());
      }
      character => current.get_or_insert_default().push(character),
    }
  }

  arguments.extend(current);

  arguments
}

impl Settings {
  /// The launch arguments recorded for `game` as typed, empty when none are.
  pub fn launch_arguments(&self, game: &str) -> &str {
    self
      .games
      .get(game)
      .map_or("", |settings| settings.launch_arguments.as_str())
  }

  /// How `game` is set to run on Linux, [`RuntimeChoice::Detect`] when no
  /// choice is recorded.
  pub fn runtime(&self, game: &str) -> RuntimeChoice {
    self
      .games
      .get(game)
      .map_or(RuntimeChoice::Detect, |settings| settings.runtime)
  }

  /// The install folder recorded for `game`, if any.
  pub fn folder(&self, game: &str) -> Option<&Path> {
    self
      .games
      .get(game)
      .map(|settings| settings.folder.as_path())
  }

  /// The profile to open `game` with: the one last selected, or
  /// [`DEFAULT_PROFILE`].
  ///
  /// ```
  /// use rivet_core::settings::Settings;
  ///
  /// assert_eq!(Settings::default().profile("valheim"), "Default");
  /// ```
  pub fn profile(&self, game: &str) -> &str {
    self
      .games
      .get(game)
      .and_then(|settings| settings.profile.as_deref())
      .unwrap_or(DEFAULT_PROFILE)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn split(line: &str) -> Vec<String> {
    split_arguments(line)
  }

  #[test]
  fn arguments_are_split_on_whitespace() {
    assert_eq!(
      split("  -console\t-windowed  "),
      vec!["-console", "-windowed"]
    );
    assert_eq!(split(""), Vec::<String>::new());
  }

  #[test]
  fn double_quotes_keep_spaces_inside_one_argument() {
    assert_eq!(
      split(r#"-log "C:/My Logs/out.txt" -name="Big Bob""#),
      vec!["-log", "C:/My Logs/out.txt", "-name=Big Bob"]
    );
  }

  #[test]
  fn a_game_with_no_launch_arguments_launches_with_none() {
    let mut settings = Settings::default();

    settings
      .games
      .insert("valheim".to_string(), GameSettings::new("/games/valheim"));

    assert_eq!(settings.launch_arguments("valheim"), "");
    assert_eq!(settings.launch_arguments("riskofrain2"), "");
  }

  #[test]
  fn settings_without_the_newer_fields_still_read() {
    let settings: Settings = serde_json::from_value(serde_json::json!({
      "games": { "valheim": { "folder": "/games/valheim" } }
    }))
    .unwrap();

    assert_eq!(settings.steam_folder, None);
    assert_eq!(
      settings.games["valheim"],
      GameSettings::new("/games/valheim")
    );
  }
}
