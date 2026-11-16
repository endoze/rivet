//! What the app remembers between runs. The shell stores it as TOML in the
//! platform's config location.

use crate::install::DEFAULT_PROFILE;
use crate::loader_files::LoaderFile;
use crate::manage::Tab;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thunderstore_engine::profile::launch::Runtime;

/// The interface sizes settings offer, as percentages of the spec's sizes.
pub const INTERFACE_SIZES: [u16; 5] = [90, 100, 115, 130, 150];

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
  /// Whether Rivet looks light or dark, or follows the system.
  #[serde(skip_serializing_if = "Appearance::is_system")]
  pub appearance: Appearance,
  /// The family of themes Rivet is drawn in. [`Settings::appearance`] picks
  /// its light or dark member.
  #[serde(skip_serializing_if = "ThemeFamily::is_default")]
  pub theme: ThemeFamily,
  /// How large Rivet draws its interface, as a percentage of the spec's
  /// sizes, one of [`INTERFACE_SIZES`]. `None` picks one for the display the
  /// window is on.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub interface_size: Option<u16>,
  /// Whether the game picker shows its games as a list or a grid of covers.
  #[serde(skip_serializing_if = "PickerLayout::is_list")]
  pub picker_layout: PickerLayout,
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
  /// The files launches copied into the game's folder and have not been
  /// removed since, so they can be.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub loader_files: Vec<LoaderFile>,
  /// Whether the user answered the offer to import r2modman's profiles of
  /// the game, so it is not made again on its own.
  #[serde(default, skip_serializing_if = "std::ops::Not::not")]
  pub r2modman_answered: bool,
  /// The view the game was left on, which it opens on next.
  #[serde(default, skip_serializing_if = "Tab::is_installed")]
  pub tab: Tab,
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
  /// assert!(valheim.loader_files.is_empty());
  /// ```
  pub fn new(folder: impl Into<PathBuf>) -> Self {
    Self {
      folder: folder.into(),
      profile: None,
      launch_arguments: String::new(),
      runtime: RuntimeChoice::Detect,
      loader_files: Vec::new(),
      r2modman_answered: false,
      tab: Tab::Installed,
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

/// Whether Rivet looks light or dark. The theme in use is the light or dark
/// member of its family, picked by this.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
  /// Light or dark as the system is, following it when it changes.
  #[default]
  System,
  Light,
  Dark,
}

impl Appearance {
  fn is_system(&self) -> bool {
    *self == Self::System
  }
}

/// A family of themes, each with a light and a dark member.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeFamily {
  /// Rivet's own look: Default Light and Default Dark.
  #[default]
  Default,
  /// One Light and One Dark, from Atom's One palettes.
  One,
}

impl ThemeFamily {
  fn is_default(&self) -> bool {
    *self == Self::Default
  }
}

/// How the game picker lays out its games.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PickerLayout {
  /// One row per game: its cover, then its name.
  #[default]
  List,
  /// Cover tiles with the game's name under each, wrapping to the width.
  Grid,
}

impl PickerLayout {
  fn is_list(&self) -> bool {
    *self == Self::List
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

  /// The view to open `game` on: the one it was left on, or
  /// [`Tab::Installed`].
  pub fn tab(&self, game: &str) -> Tab {
    self
      .games
      .get(game)
      .map_or(Tab::Installed, |settings| settings.tab)
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
    assert_eq!(settings.appearance, Appearance::System);
    assert_eq!(
      settings.games["valheim"],
      GameSettings::new("/games/valheim")
    );
  }

  #[test]
  fn the_appearance_matches_the_system_until_one_is_chosen() {
    assert_eq!(Settings::default().appearance, Appearance::System);
  }

  #[test]
  fn a_chosen_appearance_is_saved_by_name_and_read_back() {
    let settings = Settings {
      appearance: Appearance::Dark,
      ..Settings::default()
    };
    let saved = serde_json::to_value(&settings).unwrap();

    assert_eq!(saved["appearance"], "dark");
    assert_eq!(serde_json::from_value::<Settings>(saved).unwrap(), settings);
  }

  #[test]
  fn the_theme_is_the_default_family_until_one_is_chosen() {
    let older: Settings = serde_json::from_value(serde_json::json!({ "games": {} })).unwrap();

    assert_eq!(Settings::default().theme, ThemeFamily::Default);
    assert_eq!(older.theme, ThemeFamily::Default);
    assert_eq!(
      serde_json::to_value(Settings::default())
        .unwrap()
        .get("theme"),
      None
    );
  }

  #[test]
  fn a_chosen_theme_is_saved_by_name_and_read_back() {
    let settings = Settings {
      theme: ThemeFamily::One,
      ..Settings::default()
    };
    let saved = serde_json::to_value(&settings).unwrap();

    assert_eq!(saved["theme"], "one");
    assert_eq!(serde_json::from_value::<Settings>(saved).unwrap(), settings);
  }

  #[test]
  fn the_interface_size_follows_the_display_until_one_is_chosen() {
    let older: Settings = serde_json::from_value(serde_json::json!({ "games": {} })).unwrap();

    assert_eq!(Settings::default().interface_size, None);
    assert_eq!(older.interface_size, None);
    assert!(
      serde_json::to_value(Settings::default()).unwrap()["interface_size"].is_null(),
      "an unchosen size is written to the file"
    );
  }

  #[test]
  fn a_game_opens_on_its_profile_until_find_mods_is_saved_by_name() {
    let mut valheim = GameSettings::new("/games/valheim");

    assert_eq!(valheim.tab, Tab::Installed);
    assert_eq!(
      serde_json::to_value(&valheim).unwrap().get("tab"),
      None,
      "the default view is written to the file"
    );

    valheim.tab = Tab::Browse;

    let saved = serde_json::to_value(&valheim).unwrap();

    assert_eq!(saved["tab"], "browse");
    assert_eq!(
      serde_json::from_value::<GameSettings>(saved).unwrap(),
      valheim
    );
  }

  #[test]
  fn the_picker_lists_games_until_a_grid_is_chosen() {
    let older: Settings = serde_json::from_value(serde_json::json!({ "games": {} })).unwrap();

    assert_eq!(Settings::default().picker_layout, PickerLayout::List);
    assert_eq!(older.picker_layout, PickerLayout::List);
    assert_eq!(
      serde_json::to_value(Settings::default())
        .unwrap()
        .get("picker_layout"),
      None
    );
  }

  #[test]
  fn a_chosen_picker_layout_is_saved_by_name_and_read_back() {
    let settings = Settings {
      picker_layout: PickerLayout::Grid,
      ..Settings::default()
    };
    let saved = serde_json::to_value(&settings).unwrap();

    assert_eq!(saved["picker_layout"], "grid");
    assert_eq!(serde_json::from_value::<Settings>(saved).unwrap(), settings);
  }

  #[test]
  fn a_chosen_interface_size_is_saved_as_a_percentage_and_read_back() {
    let settings = Settings {
      interface_size: Some(130),
      ..Settings::default()
    };
    let saved = serde_json::to_value(&settings).unwrap();

    assert_eq!(saved["interface_size"], 130);
    assert_eq!(serde_json::from_value::<Settings>(saved).unwrap(), settings);
  }
}
