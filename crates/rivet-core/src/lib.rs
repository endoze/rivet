//! The pure core of the app: all state, every message that can change it, and
//! the effects the shell must run. Nothing here does IO.

pub mod picker;
pub mod settings;

use picker::{FolderRefusal, Picker, Refusal, Step};
use settings::{GameSettings, Settings};
use std::path::PathBuf;
use thunderstore_engine::ecosystem::Ecosystem;

/// Everything the app knows.
#[derive(Debug)]
pub struct State {
  /// The game definitions.
  pub ecosystem: Ecosystem,
  pub settings: Settings,
  pub screen: Screen,
  pub picker: Picker,
  /// The last failure to read or write the settings file, if unresolved.
  pub settings_error: Option<SettingsError>,
}

impl State {
  /// A state that has not loaded its settings yet.
  pub fn new(ecosystem: Ecosystem) -> Self {
    Self {
      ecosystem,
      settings: Settings::default(),
      screen: Screen::Starting,
      picker: Picker::default(),
      settings_error: None,
    }
  }
}

/// What fills the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
  /// The settings are still loading.
  Starting,
  /// The game picker.
  Picker,
  /// One game, by community slug.
  Game(String),
}

/// A failure to read or write the settings file, with the system's reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
  Load(String),
  Save(String),
}

/// Something that happened: user input, or the result of an [`Effect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
  /// The window opened.
  Started,
  /// The settings file was read. A missing file reads as the defaults.
  SettingsLoaded(Result<Settings, String>),
  SettingsSaved(Result<(), String>),
  /// The picker's search text changed.
  SearchChanged(String),
  /// A game in the picker was clicked, by community slug.
  GamePicked(String),
  /// The folder dialog closed, with `None` if it was cancelled.
  FolderChosen(Option<PathBuf>),
  /// A folder was listed: the names of its entries, or why it could not be.
  FolderListed {
    folder: PathBuf,
    entries: Result<Vec<String>, String>,
  },
  SwitchGamePressed,
}

/// Work the shell must do outside the pure core. Each effect is answered with
/// a [`Msg`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
  /// Ask the user for a folder. Needs the window, so the shell runs it.
  /// Answered with [`Msg::FolderChosen`].
  ChooseFolder,
  /// Work for the commands thread.
  Run(Command),
}

/// Effects the commands thread runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
  /// Answered with [`Msg::SettingsLoaded`].
  LoadSettings,
  /// Answered with [`Msg::SettingsSaved`].
  SaveSettings(Settings),
  /// Answered with [`Msg::FolderListed`].
  ListFolder(PathBuf),
}

/// Applies one message to the state and returns the effects to run.
///
/// ```
/// use rivet_core::{Command, Effect, Msg, Screen, State, update};
/// use thunderstore_engine::ecosystem::Ecosystem;
///
/// let mut state = State::new(Ecosystem::bundled());
/// let effects = update(&mut state, Msg::Started);
///
/// assert_eq!(state.screen, Screen::Starting);
/// assert_eq!(effects, vec![Effect::Run(Command::LoadSettings)]);
/// ```
pub fn update(state: &mut State, msg: Msg) -> Vec<Effect> {
  match msg {
    Msg::Started => {
      state.screen = Screen::Starting;

      vec![Effect::Run(Command::LoadSettings)]
    }
    Msg::SettingsLoaded(Ok(settings)) => {
      state.settings = settings;
      state.screen = match state.settings.last_game.clone() {
        Some(game) if offered(state, &game) && state.settings.folder(&game).is_some() => {
          Screen::Game(game)
        }
        _ => Screen::Picker,
      };

      Vec::new()
    }
    Msg::SettingsLoaded(Err(detail)) => {
      state.settings_error = Some(SettingsError::Load(detail));
      state.screen = Screen::Picker;

      Vec::new()
    }
    Msg::SearchChanged(query) => {
      state.picker.query = query;

      Vec::new()
    }
    Msg::GamePicked(game) => pick_game(state, game),
    Msg::FolderChosen(folder) => folder_chosen(state, folder),
    Msg::FolderListed { folder, entries } => folder_listed(state, folder, entries),
    Msg::SwitchGamePressed => {
      state.screen = Screen::Picker;
      state.picker.step = Step::Idle;
      state.picker.refusal = None;

      Vec::new()
    }
    Msg::SettingsSaved(result) => {
      state.settings_error = result.err().map(SettingsError::Save);

      Vec::new()
    }
  }
}

/// Whether the picker offers `game`, a community slug.
fn offered(state: &State, game: &str) -> bool {
  state.ecosystem.game(game).is_some_and(picker::offered)
}

fn pick_game(state: &mut State, game: String) -> Vec<Effect> {
  if !offered(state, &game) || matches!(state.picker.step, Step::Choosing(_)) {
    return Vec::new();
  }

  state.picker.refusal = None;

  if state.settings.folder(&game).is_some() {
    state.picker.step = Step::Idle;

    return open_game(state, game);
  }

  state.picker.step = Step::Choosing(game);

  vec![Effect::ChooseFolder]
}

fn folder_chosen(state: &mut State, folder: Option<PathBuf>) -> Vec<Effect> {
  let Step::Choosing(game) = &state.picker.step else {
    return Vec::new();
  };

  match folder {
    Some(folder) => {
      state.picker.step = Step::Checking {
        game: game.clone(),
        folder: folder.clone(),
      };

      vec![Effect::Run(Command::ListFolder(folder))]
    }
    None => {
      state.picker.step = Step::Idle;

      Vec::new()
    }
  }
}

fn folder_listed(
  state: &mut State,
  folder: PathBuf,
  entries: Result<Vec<String>, String>,
) -> Vec<Effect> {
  let Step::Checking {
    game,
    folder: checking,
  } = &state.picker.step
  else {
    return Vec::new();
  };

  if *checking != folder {
    return Vec::new();
  }

  let game = game.clone();

  state.picker.step = Step::Idle;

  let Some(definition) = state.ecosystem.game(&game) else {
    return Vec::new();
  };
  let verdict = entries
    .map_err(FolderRefusal::Unreadable)
    .and_then(|entries| picker::check_folder(definition, &entries));

  match verdict {
    Ok(()) => {
      state
        .settings
        .games
        .insert(game.clone(), GameSettings { folder });

      open_game(state, game)
    }
    Err(reason) => {
      state.picker.refusal = Some(Refusal {
        game,
        folder,
        reason,
      });

      Vec::new()
    }
  }
}

/// Shows `game` and saves it as the game to reopen on the next start.
fn open_game(state: &mut State, game: String) -> Vec<Effect> {
  state.settings.last_game = Some(game.clone());
  state.screen = Screen::Game(game);

  vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
}

#[cfg(test)]
pub(crate) fn test_ecosystem() -> thunderstore_engine::ecosystem::Ecosystem {
  thunderstore_engine::ecosystem::Ecosystem::from_json(
    r#"{
      "games": {
        "valheim": {
          "label": "valheim",
          "meta": { "displayName": "Valheim" },
          "r2modman": [
            { "gameInstanceType": "game", "exeNames": ["valheim.exe", "valheim.x86_64"] },
            { "gameInstanceType": "server", "exeNames": ["valheim_server.exe"] }
          ]
        },
        "riskofrain2": {
          "label": "riskofrain2",
          "meta": { "displayName": "Risk of Rain 2" },
          "r2modman": [{ "gameInstanceType": "game", "exeNames": ["Risk of Rain 2.exe"] }]
        },
        "lethal-company": {
          "label": "lethal-company",
          "meta": { "displayName": "Lethal Company" },
          "r2modman": [{ "gameInstanceType": "game", "exeNames": ["Lethal Company.exe"] }]
        },
        "no-install-definition": {
          "label": "no-install-definition",
          "meta": { "displayName": "A Game Without An Install Definition" },
          "r2modman": null
        }
      }
    }"#,
  )
  .expect("test ecosystem parses")
}

#[cfg(test)]
mod tests {
  use super::*;

  fn started() -> State {
    let mut state = State::new(test_ecosystem());

    update(&mut state, Msg::Started);

    state
  }

  fn settings_with(game: &str, folder: &str) -> Settings {
    Settings {
      last_game: Some(game.to_string()),
      games: [(
        game.to_string(),
        GameSettings {
          folder: PathBuf::from(folder),
        },
      )]
      .into(),
    }
  }

  #[test]
  fn starting_loads_the_settings() {
    let mut state = State::new(test_ecosystem());

    let effects = update(&mut state, Msg::Started);

    assert_eq!(state.screen, Screen::Starting);
    assert_eq!(effects, vec![Effect::Run(Command::LoadSettings)]);
  }

  #[test]
  fn the_first_start_opens_the_picker() {
    let mut state = started();

    let effects = update(&mut state, Msg::SettingsLoaded(Ok(Settings::default())));

    assert_eq!(state.screen, Screen::Picker);
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn a_later_start_opens_the_last_game() {
    let mut state = started();
    let settings = settings_with("valheim", "/games/valheim");

    let effects = update(&mut state, Msg::SettingsLoaded(Ok(settings.clone())));

    assert_eq!(state.screen, Screen::Game("valheim".to_string()));
    assert_eq!(state.settings, settings);
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn a_last_game_that_no_longer_has_a_definition_opens_the_picker() {
    let mut state = started();

    update(
      &mut state,
      Msg::SettingsLoaded(Ok(settings_with("gone", "/games/gone"))),
    );

    assert_eq!(state.screen, Screen::Picker);
  }

  #[test]
  fn unreadable_settings_open_the_picker_and_report_why() {
    let mut state = started();

    update(&mut state, Msg::SettingsLoaded(Err("bad toml".to_string())));

    assert_eq!(state.screen, Screen::Picker);
    assert_eq!(state.settings, Settings::default());
    assert_eq!(
      state.settings_error,
      Some(SettingsError::Load("bad toml".to_string()))
    );
  }

  fn on_picker() -> State {
    let mut state = started();

    update(&mut state, Msg::SettingsLoaded(Ok(Settings::default())));

    state
  }

  fn checking(game: &str, folder: &str) -> State {
    let mut state = on_picker();

    update(&mut state, Msg::GamePicked(game.to_string()));
    update(&mut state, Msg::FolderChosen(Some(PathBuf::from(folder))));

    state
  }

  fn listed(folder: &str, entries: &[&str]) -> Msg {
    Msg::FolderListed {
      folder: PathBuf::from(folder),
      entries: Ok(entries.iter().map(|entry| entry.to_string()).collect()),
    }
  }

  #[test]
  fn typing_in_search_sets_the_query() {
    let mut state = on_picker();

    let effects = update(&mut state, Msg::SearchChanged("val".to_string()));

    assert_eq!(state.picker.query, "val");
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn picking_a_game_without_a_folder_asks_for_one() {
    let mut state = on_picker();

    let effects = update(&mut state, Msg::GamePicked("valheim".to_string()));

    assert_eq!(state.picker.step, Step::Choosing("valheim".to_string()));
    assert_eq!(effects, vec![Effect::ChooseFolder]);
  }

  #[test]
  fn picking_a_game_with_a_known_folder_opens_it_and_remembers_it_as_last() {
    let mut state = started();
    let mut settings = settings_with("valheim", "/games/valheim");

    settings.last_game = Some("riskofrain2".to_string());

    settings.games.insert(
      "riskofrain2".to_string(),
      GameSettings {
        folder: PathBuf::from("/games/ror2"),
      },
    );
    update(&mut state, Msg::SettingsLoaded(Ok(settings)));
    update(&mut state, Msg::SwitchGamePressed);

    let effects = update(&mut state, Msg::GamePicked("valheim".to_string()));

    assert_eq!(state.screen, Screen::Game("valheim".to_string()));
    assert_eq!(state.settings.last_game.as_deref(), Some("valheim"));
    assert_eq!(
      effects,
      vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
    );
  }

  #[test]
  fn picking_a_game_the_picker_does_not_offer_is_ignored() {
    let mut state = on_picker();

    let effects = update(
      &mut state,
      Msg::GamePicked("no-install-definition".to_string()),
    );

    assert_eq!(state.picker.step, Step::Idle);
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn a_chosen_folder_is_listed() {
    let mut state = on_picker();

    update(&mut state, Msg::GamePicked("valheim".to_string()));

    let effects = update(
      &mut state,
      Msg::FolderChosen(Some(PathBuf::from("/games/valheim"))),
    );

    assert_eq!(
      state.picker.step,
      Step::Checking {
        game: "valheim".to_string(),
        folder: PathBuf::from("/games/valheim"),
      }
    );
    assert_eq!(
      effects,
      vec![Effect::Run(Command::ListFolder(PathBuf::from(
        "/games/valheim"
      )))]
    );
  }

  #[test]
  fn cancelling_the_folder_dialog_leaves_the_picker_idle() {
    let mut state = on_picker();

    update(&mut state, Msg::GamePicked("valheim".to_string()));

    let effects = update(&mut state, Msg::FolderChosen(None));

    assert_eq!(state.picker.step, Step::Idle);
    assert_eq!(state.screen, Screen::Picker);
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn a_folder_holding_the_game_opens_it_and_saves_the_choice() {
    let mut state = checking("valheim", "/games/valheim");

    let effects = update(
      &mut state,
      listed("/games/valheim", &["valheim.x86_64", "valheim_Data"]),
    );

    assert_eq!(state.screen, Screen::Game("valheim".to_string()));
    assert_eq!(state.picker.step, Step::Idle);
    assert_eq!(state.settings, settings_with("valheim", "/games/valheim"));
    assert_eq!(
      effects,
      vec![Effect::Run(Command::SaveSettings(settings_with(
        "valheim",
        "/games/valheim"
      )))]
    );
  }

  #[test]
  fn a_folder_without_the_game_is_refused_and_nothing_is_saved() {
    let mut state = checking("valheim", "/home/me");

    let effects = update(&mut state, listed("/home/me", &["Documents"]));

    assert_eq!(state.screen, Screen::Picker);
    assert_eq!(state.picker.step, Step::Idle);
    assert_eq!(
      state.picker.refusal,
      Some(Refusal {
        game: "valheim".to_string(),
        folder: PathBuf::from("/home/me"),
        reason: FolderRefusal::MissingExecutable {
          expected: vec!["valheim.exe".to_string(), "valheim.x86_64".to_string()],
        },
      })
    );
    assert_eq!(state.settings, Settings::default());
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn a_folder_that_cannot_be_listed_is_refused_with_the_reason() {
    let mut state = checking("valheim", "/root");

    update(
      &mut state,
      Msg::FolderListed {
        folder: PathBuf::from("/root"),
        entries: Err("permission denied".to_string()),
      },
    );

    assert_eq!(
      state.picker.refusal.map(|refusal| refusal.reason),
      Some(FolderRefusal::Unreadable("permission denied".to_string()))
    );
  }

  #[test]
  fn picking_again_clears_the_last_refusal() {
    let mut state = checking("valheim", "/home/me");

    update(&mut state, listed("/home/me", &[]));
    update(&mut state, Msg::GamePicked("valheim".to_string()));

    assert_eq!(state.picker.refusal, None);
  }

  #[test]
  fn a_listing_for_a_folder_no_longer_being_checked_is_ignored() {
    let mut state = checking("valheim", "/games/valheim");

    update(&mut state, Msg::GamePicked("riskofrain2".to_string()));

    let effects = update(&mut state, listed("/games/valheim", &["valheim.exe"]));

    assert_eq!(state.screen, Screen::Picker);
    assert_eq!(state.picker.step, Step::Choosing("riskofrain2".to_string()));
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn picking_another_game_while_the_folder_dialog_is_open_is_ignored() {
    let mut state = on_picker();

    update(&mut state, Msg::GamePicked("valheim".to_string()));

    let effects = update(&mut state, Msg::GamePicked("riskofrain2".to_string()));

    assert_eq!(state.picker.step, Step::Choosing("valheim".to_string()));
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn switching_game_returns_to_the_picker_keeping_the_search() {
    let mut state = on_picker();

    update(&mut state, Msg::SearchChanged("val".to_string()));
    update(&mut state, Msg::GamePicked("valheim".to_string()));
    update(
      &mut state,
      Msg::FolderChosen(Some(PathBuf::from("/games/valheim"))),
    );
    update(&mut state, listed("/games/valheim", &["valheim.exe"]));

    let effects = update(&mut state, Msg::SwitchGamePressed);

    assert_eq!(state.screen, Screen::Picker);
    assert_eq!(state.picker.query, "val");
    assert_eq!(state.settings.last_game.as_deref(), Some("valheim"));
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn a_failed_save_is_reported_and_a_later_save_clears_it() {
    let mut state = on_picker();

    update(&mut state, Msg::SettingsSaved(Err("disk full".to_string())));

    assert_eq!(
      state.settings_error,
      Some(SettingsError::Save("disk full".to_string()))
    );

    update(&mut state, Msg::SettingsSaved(Ok(())));

    assert_eq!(state.settings_error, None);
  }
}
