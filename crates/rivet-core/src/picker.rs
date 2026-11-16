//! The game picker: which games it offers and whether a chosen folder holds
//! the game.

use crate::settings::{PickerLayout, Settings};
use crate::{Command, Effect, Screen, SettingsError, State};
use std::collections::BTreeMap;
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
  /// Why the user was last asked to choose a folder by hand, shown until
  /// another game is picked.
  pub fallback: Option<Fallback>,
  /// The cover image on disk of each game that has one cached, by
  /// community slug.
  pub covers: BTreeMap<String, PathBuf>,
  /// Whether a [`Command::CacheCovers`] is running, so no other is sent.
  pub caching_covers: bool,
}

/// A game's cover to cache: `path` is relative to Thunderstore's asset host
/// and to the covers folder in the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cover {
  /// The game's community slug.
  pub game: String,
  pub path: String,
}

/// Shows the picker, asking for the covers of the games it offers that are
/// not cached yet, unless they are already being cached. A cover that could
/// not be fetched is asked for again the next time the picker shows.
pub(crate) fn show(state: &mut State) -> Vec<Effect> {
  state.screen = Screen::Picker;

  if state.picker.caching_covers {
    return Vec::new();
  }

  let covers: Vec<Cover> = games(&state.ecosystem, "")
    .into_iter()
    .filter(|game| !game.meta.icon_url.is_empty())
    .filter(|game| !state.picker.covers.contains_key(&game.slug))
    .map(|game| Cover {
      game: game.slug.clone(),
      path: game.meta.icon_url.clone(),
    })
    .collect();

  if covers.is_empty() {
    return Vec::new();
  }

  state.picker.caching_covers = true;

  vec![Effect::Run(Command::CacheCovers(covers))]
}

/// Records the `covers` now on disk, by community slug.
pub(crate) fn covers_cached(state: &mut State, covers: BTreeMap<String, PathBuf>) -> Vec<Effect> {
  state.picker.covers.extend(covers);

  Vec::new()
}

/// How far choosing a game's folder has got.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Step {
  #[default]
  Idle,
  /// Steam's libraries are being searched for this game.
  Locating(String),
  /// The folder dialog is open for this game.
  Choosing(String),
  /// This folder is being listed to check it holds this game. `from_steam`
  /// when Steam named the folder rather than the user.
  Checking {
    game: String,
    folder: PathBuf,
    from_steam: bool,
  },
}

/// A game whose folder the user must choose by hand, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fallback {
  pub game: String,
  pub reason: NotFound,
}

/// Why Steam could not name a game's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotFound {
  /// The game is not sold on Steam.
  NotOnSteam,
  /// No Steam install was found on this computer.
  NoSteam,
  /// No Steam library has the game installed.
  NotInstalled,
  /// Steam's files could not be read. The detail is the reason.
  Unreadable(String),
}

/// `game`'s Steam app id, or `None` when it is not sold on Steam.
pub(crate) fn steam_app_id(game: &GameDef) -> Option<u32> {
  game.profile().and_then(|profile| profile.steam_app_id())
}

/// Records the picker's list or grid and saves it, when it changed. While the
/// settings file could not be read, nothing is saved, so the defaults shown
/// in its place never overwrite it.
pub(crate) fn layout_chosen(state: &mut State, layout: PickerLayout) -> Vec<Effect> {
  if state.settings.picker_layout == layout {
    return Vec::new();
  }

  state.settings.picker_layout = layout;

  if matches!(state.settings_error, Some(SettingsError::Load(_))) {
    return Vec::new();
  }

  vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
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

/// `games` split into those with a folder in `settings`, then the rest, each
/// in the order given.
pub fn set_up_first<'a>(
  games: Vec<&'a GameDef>,
  settings: &Settings,
) -> (Vec<&'a GameDef>, Vec<&'a GameDef>) {
  games
    .into_iter()
    .partition(|game| settings.folder(&game.slug).is_some())
}

/// A store a game is sold on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Store {
  Steam,
  EpicGamesStore,
  XboxGamePass,
  OculusStore,
  /// EA's store, named `origin` in the game definitions.
  EaApp,
}

/// The stores `game` is sold on, from its own definition and its primary
/// install definition, each once in the order first listed. A platform the
/// definitions name `other`, or one Rivet doesn't know, is left out.
pub fn stores(game: &GameDef) -> Vec<Store> {
  let profile = game
    .profile()
    .map_or(&[][..], |profile| &profile.distributions);
  let mut stores = Vec::new();

  for distribution in game.distributions.iter().chain(profile) {
    let store = match distribution.platform.as_str() {
      "steam" | "steam-direct" => Store::Steam,
      "epic-games-store" => Store::EpicGamesStore,
      "xbox-game-pass" => Store::XboxGamePass,
      "oculus-store" => Store::OculusStore,
      "origin" => Store::EaApp,
      _ => continue,
    };

    if !stores.contains(&store) {
      stores.push(store);
    }
  }

  stores
}

/// Whether the picker offers `game`: its definition has a mod manager install
/// definition (the engine's `GameProfile`) naming a Thunderstore community,
/// without which the app can neither list nor install packages for it.
pub fn offered(game: &GameDef) -> bool {
  game.community().is_some()
}

/// Why a chosen folder was refused as a game's install folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderRefusal {
  /// The folder holds none of the game's executables. `expected` lists the
  /// file names that were looked for.
  MissingExecutable { expected: Vec<String> },
  /// The folder could not be listed. The detail is the system's reason.
  Unreadable(String),
  /// A folder chosen as the Steam install has no [`STEAM_LIBRARY`] in it.
  NotSteam,
}

/// The folder every Steam install keeps its games in, which marks a folder
/// as one.
pub const STEAM_LIBRARY: &str = "steamapps";

/// Accepts `entries`, the file names in a folder, as a Steam install when
/// one of them is [`STEAM_LIBRARY`], ignoring ASCII case as Windows does.
///
/// ```
/// use rivet_core::picker::{FolderRefusal, check_steam_folder};
///
/// assert_eq!(check_steam_folder(&["steamapps".into()]), Ok(()));
/// assert_eq!(check_steam_folder(&[]), Err(FolderRefusal::NotSteam));
/// ```
pub fn check_steam_folder(entries: &[String]) -> Result<(), FolderRefusal> {
  if entries
    .iter()
    .any(|entry| entry.eq_ignore_ascii_case(STEAM_LIBRARY))
  {
    return Ok(());
  }

  Err(FolderRefusal::NotSteam)
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
  fn offers_every_game_it_can_install_and_list_packages_for_sorted_by_name() {
    let ecosystem = test_ecosystem();

    assert_eq!(
      names(games(&ecosystem, "")),
      vec!["Lethal Company", "Risk of Rain 2", "Valheim"]
    );
  }

  #[test]
  fn a_game_names_each_store_it_is_sold_on_once() {
    let ecosystem = Ecosystem::from_json(
      r#"{
        "games": {
          "nine-kings": {
            "label": "nine-kings",
            "meta": { "displayName": "9 Kings" },
            "distributions": [
              { "platform": "steam", "identifier": "1" },
              { "platform": "other", "identifier": "" },
              { "platform": "epic-games-store", "identifier": "2" }
            ],
            "r2modman": [{
              "gameInstanceType": "game",
              "distributions": [
                { "platform": "steam-direct", "identifier": "1" },
                { "platform": "xbox-game-pass", "identifier": "3" }
              ]
            }]
          }
        }
      }"#,
    )
    .unwrap();

    assert_eq!(
      stores(ecosystem.game("nine-kings").unwrap()),
      vec![Store::Steam, Store::EpicGamesStore, Store::XboxGamePass]
    );
  }

  #[test]
  fn a_game_listing_no_store_names_none() {
    let ecosystem = test_ecosystem();

    assert_eq!(stores(ecosystem.game("valheim").unwrap()), vec![]);
  }

  #[test]
  fn games_with_a_folder_are_set_apart_from_the_rest_in_order() {
    let ecosystem = test_ecosystem();
    let mut settings = Settings::default();

    settings.games.insert(
      "valheim".to_string(),
      crate::settings::GameSettings::new("/games/valheim"),
    );

    let (set_up, rest) = set_up_first(games(&ecosystem, ""), &settings);

    assert_eq!(names(set_up), vec!["Valheim"]);
    assert_eq!(names(rest), vec!["Lethal Company", "Risk of Rain 2"]);
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
  fn a_folder_holding_a_steam_library_is_accepted_as_steam() {
    for entries in [&["steamapps", "steam.sh"][..], &["SteamApps"]] {
      let entries: Vec<String> = entries.iter().map(|entry| entry.to_string()).collect();

      assert_eq!(check_steam_folder(&entries), Ok(()), "{entries:?}");
    }
  }

  #[test]
  fn a_folder_without_a_steam_library_is_refused_as_steam() {
    let entries = vec!["Documents".to_string(), "steam.desktop".to_string()];

    assert_eq!(check_steam_folder(&entries), Err(FolderRefusal::NotSteam));
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
