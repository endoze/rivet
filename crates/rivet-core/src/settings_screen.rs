//! The settings screen of the open game: changing its folder and the Steam
//! folder, each checked as the picker checks a folder, its launch arguments,
//! and whether it runs natively or under Proton on Linux.

use crate::picker::{self, FolderRefusal, Refusal};
use crate::settings::RuntimeChoice;
use crate::{Command, Effect, Screen, State, launch};
use std::path::PathBuf;

/// The settings screen's own state. A fresh one comes with each visit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsScreen {
  pub step: Step,
  /// The last folder refused, shown until another change starts.
  pub refusal: Option<Refusal>,
  /// Whether settings changed here wait to be saved on leaving or on closing
  /// the window.
  pub unsaved: bool,
}

/// A folder the settings screen changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
  /// The open game's install folder.
  GameFolder,
  /// The Steam install, for every game.
  SteamFolder,
}

/// How far changing a folder has got.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Step {
  #[default]
  Idle,
  /// The folder dialog is open for this folder.
  Choosing(Target),
  /// This folder is being listed to check it can be this target.
  Checking { target: Target, folder: PathBuf },
}

/// Shows the settings of the open game, filling in its launch arguments.
pub(crate) fn pressed(state: &mut State) -> Vec<Effect> {
  let Screen::Game(game) = &state.screen else {
    return Vec::new();
  };
  let arguments = state.settings.launch_arguments(game).to_string();

  state.screen = Screen::Settings(game.clone());
  state.settings_screen = SettingsScreen::default();

  vec![Effect::FillLaunchArguments(arguments)]
}

/// Returns to the game, saving what changed here.
pub(crate) fn closed(state: &mut State) -> Vec<Effect> {
  let Screen::Settings(game) = &state.screen else {
    return Vec::new();
  };
  let game = game.clone();
  let effects = save_unsaved(state);

  state.screen = Screen::Game(game);
  state.settings_screen = SettingsScreen::default();

  effects
}

/// Saves the settings when the settings screen changed them without saving.
pub(crate) fn save_unsaved(state: &mut State) -> Vec<Effect> {
  if !std::mem::take(&mut state.settings_screen.unsaved) {
    return Vec::new();
  }

  vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
}

/// Records the open game's launch arguments as typed. They are saved on
/// leaving the settings or closing the window, rather than once per key.
pub(crate) fn arguments_changed(state: &mut State, arguments: String) -> Vec<Effect> {
  let Screen::Settings(game) = &state.screen else {
    return Vec::new();
  };
  let Some(settings) = state.settings.games.get_mut(game) else {
    return Vec::new();
  };

  if settings.launch_arguments != arguments {
    settings.launch_arguments = arguments;
    state.settings_screen.unsaved = true;
  }

  Vec::new()
}

/// Records how the open game runs on Linux, saves it, and works out launching
/// for the game again, so the launch option line and launches follow at once.
pub(crate) fn runtime_chosen(state: &mut State, choice: RuntimeChoice) -> Vec<Effect> {
  let Screen::Settings(game) = state.screen.clone() else {
    return Vec::new();
  };
  let Some(settings) = state.settings.games.get_mut(&game) else {
    return Vec::new();
  };

  if settings.runtime == choice {
    return Vec::new();
  }

  settings.runtime = choice;
  state.settings_screen.unsaved = false;

  let mut effects = vec![Effect::Run(Command::SaveSettings(state.settings.clone()))];

  effects.extend(launch::open(state, &game));

  effects
}

/// Asks for a new `target` folder, unless another change is under way.
pub(crate) fn change_pressed(state: &mut State, target: Target) -> Vec<Effect> {
  let screen = &mut state.settings_screen;

  if screen.step != Step::Idle {
    return Vec::new();
  }

  screen.refusal = None;
  screen.step = Step::Choosing(target);

  vec![Effect::ChooseFolder]
}

pub(crate) fn folder_chosen(state: &mut State, folder: Option<PathBuf>) -> Vec<Effect> {
  let screen = &mut state.settings_screen;
  let Step::Choosing(target) = screen.step else {
    return Vec::new();
  };

  match folder {
    Some(folder) => {
      screen.step = Step::Checking {
        target,
        folder: folder.clone(),
      };

      vec![Effect::Run(Command::ListFolder(folder))]
    }
    None => {
      screen.step = Step::Idle;

      Vec::new()
    }
  }
}

/// Applies a listing of the folder being checked: one that can be its
/// target replaces it and is saved, and one that cannot is refused.
pub(crate) fn folder_listed(
  state: &mut State,
  folder: PathBuf,
  entries: Result<Vec<String>, String>,
) -> Vec<Effect> {
  let Screen::Settings(game) = state.screen.clone() else {
    return Vec::new();
  };
  let Step::Checking {
    target,
    folder: checking,
  } = &state.settings_screen.step
  else {
    return Vec::new();
  };

  if *checking != folder {
    return Vec::new();
  }

  let target = *target;

  state.settings_screen.step = Step::Idle;

  let Some(definition) = state.ecosystem.game(&game) else {
    return Vec::new();
  };
  let verdict = entries
    .map_err(FolderRefusal::Unreadable)
    .and_then(|entries| match target {
      Target::GameFolder => picker::check_folder(definition, &entries),
      Target::SteamFolder => picker::check_steam_folder(&entries),
    });

  if let Err(reason) = verdict {
    state.settings_screen.refusal = Some(Refusal {
      game,
      folder,
      reason,
    });

    return Vec::new();
  }

  match target {
    Target::GameFolder => {
      if let Some(settings) = state.settings.games.get_mut(&game) {
        settings.folder = folder;
      }
    }
    Target::SteamFolder => state.settings.steam_folder = Some(folder),
  }

  state.settings_screen.unsaved = false;

  let mut effects = vec![Effect::Run(Command::SaveSettings(state.settings.clone()))];

  effects.extend(match target {
    Target::GameFolder => launch::open(state, &game),
    Target::SteamFolder => launch::check_steam_option(state),
  });

  effects
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::picker::{FolderRefusal, Refusal};
  use crate::settings::{GameSettings, RuntimeChoice, Settings};
  use crate::{Command, Effect, Msg, Screen, State, test_ecosystem, update};
  use std::path::PathBuf;
  use thunderstore_engine::profile::launch::{
    HostOs, LaunchContext, LaunchMode, Runtime, StorePlatform,
  };

  const LETHAL: &str = "lethal-company";

  /// A Linux state with `game` open from `/games/<game>`, recorded with
  /// `-console` as its launch arguments and `Modded` as its profile, and its
  /// folder listed.
  fn opened(game: &str) -> State {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: Some(game.to_string()),
      games: [(
        game.to_string(),
        GameSettings {
          profile: Some("Modded".to_string()),
          launch_arguments: "-console".to_string(),
          ..GameSettings::new(PathBuf::from("/games").join(game))
        },
      )]
      .into(),
      ..Settings::default()
    };

    state.host = Some(HostOs::Linux);

    update(&mut state, Msg::Started);
    update(&mut state, Msg::SettingsLoaded(Ok(settings)));
    update(
      &mut state,
      Msg::GameFolderListed {
        game: game.to_string(),
        entries: Ok(vec!["Lethal Company.exe".to_string()]),
      },
    );

    state
  }

  /// As [`opened`], with the settings screen shown.
  fn on_settings(game: &str) -> State {
    let mut state = opened(game);

    update(&mut state, Msg::SettingsPressed);

    state
  }

  fn listed(folder: &str, entries: &[&str]) -> Msg {
    Msg::FolderListed {
      folder: PathBuf::from(folder),
      entries: Ok(entries.iter().map(|entry| entry.to_string()).collect()),
    }
  }

  /// `state` with `target` changed to `folder`, the folder holding
  /// `entries`, and the effects of the listing.
  fn change(state: &mut State, target: Target, folder: &str, entries: &[&str]) -> Vec<Effect> {
    update(state, Msg::ChangeFolderPressed(target));
    update(state, Msg::FolderChosen(Some(PathBuf::from(folder))));

    update(state, listed(folder, entries))
  }

  #[test]
  fn settings_open_from_a_game_with_its_launch_arguments_filled_in() {
    let mut state = opened("valheim");

    let effects = update(&mut state, Msg::SettingsPressed);

    assert_eq!(state.screen, Screen::Settings("valheim".to_string()));
    assert_eq!(
      effects,
      vec![Effect::FillLaunchArguments("-console".to_string())]
    );
  }

  #[test]
  fn changing_a_folder_asks_for_one_then_lists_it() {
    let mut state = on_settings("valheim");

    assert_eq!(
      update(&mut state, Msg::ChangeFolderPressed(Target::GameFolder)),
      vec![Effect::ChooseFolder]
    );
    assert_eq!(
      update(
        &mut state,
        Msg::FolderChosen(Some(PathBuf::from("/moved/valheim")))
      ),
      vec![Effect::Run(Command::ListFolder(PathBuf::from(
        "/moved/valheim"
      )))]
    );
    assert_eq!(
      state.settings_screen.step,
      Step::Checking {
        target: Target::GameFolder,
        folder: PathBuf::from("/moved/valheim"),
      }
    );
  }

  #[test]
  fn a_game_folder_holding_the_game_is_saved_keeping_the_games_other_settings() {
    let mut state = on_settings("valheim");

    let effects = change(
      &mut state,
      Target::GameFolder,
      "/moved/valheim",
      &["valheim.x86_64"],
    );
    let expected = GameSettings {
      profile: Some("Modded".to_string()),
      launch_arguments: "-console".to_string(),
      ..GameSettings::new("/moved/valheim")
    };

    assert_eq!(state.settings.games["valheim"], expected);
    assert_eq!(state.settings_screen.step, Step::Idle);
    assert_eq!(
      effects,
      vec![
        Effect::Run(Command::SaveSettings(state.settings.clone())),
        Effect::Run(Command::ListGameFolder {
          game: "valheim".to_string(),
          folder: PathBuf::from("/moved/valheim"),
        }),
      ]
    );
  }

  #[test]
  fn a_game_folder_without_the_game_is_refused_as_the_picker_refuses_it() {
    let mut state = on_settings("valheim");

    let effects = change(&mut state, Target::GameFolder, "/home/me", &["Documents"]);

    assert_eq!(effects, vec![]);
    assert_eq!(
      state.settings.folder("valheim"),
      Some(PathBuf::from("/games/valheim").as_path())
    );
    assert_eq!(
      state.settings_screen.refusal,
      Some(Refusal {
        game: "valheim".to_string(),
        folder: PathBuf::from("/home/me"),
        reason: FolderRefusal::MissingExecutable {
          expected: vec!["valheim.exe".to_string(), "valheim.x86_64".to_string()],
        },
      })
    );
  }

  #[test]
  fn a_folder_that_cannot_be_listed_is_refused_with_the_reason() {
    let mut state = on_settings("valheim");

    update(&mut state, Msg::ChangeFolderPressed(Target::SteamFolder));
    update(&mut state, Msg::FolderChosen(Some(PathBuf::from("/root"))));
    update(
      &mut state,
      Msg::FolderListed {
        folder: PathBuf::from("/root"),
        entries: Err("permission denied".to_string()),
      },
    );

    assert_eq!(
      state.settings_screen.refusal.map(|refusal| refusal.reason),
      Some(FolderRefusal::Unreadable("permission denied".to_string()))
    );
    assert_eq!(state.settings.steam_folder, None);
  }

  #[test]
  fn a_steam_folder_without_a_steam_library_is_refused() {
    let mut state = on_settings("valheim");

    let effects = change(&mut state, Target::SteamFolder, "/home/me", &["Documents"]);

    assert_eq!(effects, vec![]);
    assert_eq!(state.settings.steam_folder, None);
    assert_eq!(
      state.settings_screen.refusal.map(|refusal| refusal.reason),
      Some(FolderRefusal::NotSteam)
    );
  }

  #[test]
  fn a_steam_folder_is_saved_and_steams_launch_options_are_read_from_it() {
    let mut state = on_settings(LETHAL);

    let effects = change(
      &mut state,
      Target::SteamFolder,
      "/mnt/Steam",
      &["steamapps"],
    );

    assert_eq!(
      state.settings.steam_folder,
      Some(PathBuf::from("/mnt/Steam"))
    );
    assert_eq!(
      effects,
      vec![
        Effect::Run(Command::SaveSettings(state.settings.clone())),
        Effect::Run(Command::ReadSteamOption {
          game: LETHAL.to_string(),
          app_id: 1966720,
          context: LaunchContext::new(
            HostOs::Linux,
            StorePlatform::Steam,
            Runtime::Proton,
            LaunchMode::Vanilla,
          ),
          steam: Some(PathBuf::from("/mnt/Steam")),
        }),
      ]
    );
  }

  #[test]
  fn a_new_change_clears_the_last_refusal() {
    let mut state = on_settings("valheim");

    change(&mut state, Target::SteamFolder, "/home/me", &[]);
    update(&mut state, Msg::ChangeFolderPressed(Target::GameFolder));

    assert_eq!(state.settings_screen.refusal, None);
  }

  #[test]
  fn cancelling_the_folder_dialog_changes_nothing() {
    let mut state = on_settings("valheim");
    let before = state.settings.clone();

    update(&mut state, Msg::ChangeFolderPressed(Target::GameFolder));

    let effects = update(&mut state, Msg::FolderChosen(None));

    assert_eq!(effects, vec![]);
    assert_eq!(state.settings_screen.step, Step::Idle);
    assert_eq!(state.settings, before);
  }

  #[test]
  fn changing_a_folder_while_another_change_runs_is_ignored() {
    let mut state = on_settings("valheim");

    update(&mut state, Msg::ChangeFolderPressed(Target::GameFolder));

    let effects = update(&mut state, Msg::ChangeFolderPressed(Target::SteamFolder));

    assert_eq!(effects, vec![]);
    assert_eq!(
      state.settings_screen.step,
      Step::Choosing(Target::GameFolder)
    );
  }

  #[test]
  fn launch_arguments_typed_in_settings_are_saved_on_leaving_them() {
    let mut state = on_settings("valheim");

    let typed = update(
      &mut state,
      Msg::LaunchArgumentsChanged("-console -windowed".to_string()),
    );
    let left = update(&mut state, Msg::SettingsClosed);

    assert_eq!(typed, vec![]);
    assert_eq!(state.screen, Screen::Game("valheim".to_string()));
    assert_eq!(
      state.settings.launch_arguments("valheim"),
      "-console -windowed"
    );
    assert_eq!(
      left,
      vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
    );
  }

  #[test]
  fn closing_the_window_from_settings_saves_typed_launch_arguments_before_closing() {
    let mut state = on_settings("valheim");

    update(
      &mut state,
      Msg::LaunchArgumentsChanged("-console -windowed".to_string()),
    );

    let closing = update(&mut state, Msg::CloseRequested);

    assert_eq!(
      state.settings.launch_arguments("valheim"),
      "-console -windowed"
    );
    assert_eq!(
      closing,
      vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
    );
    assert_eq!(
      update(&mut state, Msg::SettingsSaved(Ok(()))),
      vec![Effect::CloseWindow]
    );
  }

  #[test]
  fn closing_the_window_waits_for_a_settings_save_still_being_written() {
    let mut state = on_settings("valheim");

    update(
      &mut state,
      Msg::LaunchArgumentsChanged("-console -windowed".to_string()),
    );
    update(&mut state, Msg::SettingsClosed);

    assert_eq!(update(&mut state, Msg::CloseRequested), vec![]);
    assert_eq!(
      update(&mut state, Msg::SettingsSaved(Ok(()))),
      vec![Effect::CloseWindow]
    );
  }

  #[test]
  fn leaving_settings_without_changing_anything_saves_nothing() {
    let mut state = on_settings("valheim");

    update(
      &mut state,
      Msg::LaunchArgumentsChanged("-console".to_string()),
    );

    assert_eq!(update(&mut state, Msg::SettingsClosed), vec![]);
    assert_eq!(state.screen, Screen::Game("valheim".to_string()));
  }

  fn runtime(state: &State) -> Option<Runtime> {
    state
      .launcher
      .as_ref()
      .and_then(|launcher| launcher.runtime)
  }

  #[test]
  fn choosing_native_saves_it_and_drops_the_proton_launch_line_at_once() {
    let mut state = on_settings(LETHAL);
    let steam = |state: &State| {
      state
        .launcher
        .as_ref()
        .is_some_and(|launcher| launcher.steam.is_some())
    };

    assert!(steam(&state));

    let effects = update(&mut state, Msg::RuntimeChosen(RuntimeChoice::Native));

    assert_eq!(state.settings.runtime(LETHAL), RuntimeChoice::Native);
    assert_eq!(runtime(&state), Some(Runtime::Native));
    assert!(!steam(&state));
    assert_eq!(
      effects,
      vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
    );
  }

  #[test]
  fn choosing_proton_reads_steams_launch_options_for_the_proton_line() {
    let mut state = on_settings(LETHAL);

    update(&mut state, Msg::RuntimeChosen(RuntimeChoice::Native));

    let effects = update(&mut state, Msg::RuntimeChosen(RuntimeChoice::Proton));

    assert_eq!(runtime(&state), Some(Runtime::Proton));
    assert_eq!(
      effects,
      vec![
        Effect::Run(Command::SaveSettings(state.settings.clone())),
        Effect::Run(Command::ReadSteamOption {
          game: LETHAL.to_string(),
          app_id: 1966720,
          context: LaunchContext::new(
            HostOs::Linux,
            StorePlatform::Steam,
            Runtime::Proton,
            LaunchMode::Vanilla,
          ),
          steam: None,
        }),
      ]
    );
  }

  #[test]
  fn choosing_detect_again_lists_the_game_folder_to_tell() {
    let mut state = on_settings(LETHAL);

    update(&mut state, Msg::RuntimeChosen(RuntimeChoice::Proton));

    let effects = update(&mut state, Msg::RuntimeChosen(RuntimeChoice::Detect));

    assert_eq!(state.settings.runtime(LETHAL), RuntimeChoice::Detect);
    assert_eq!(runtime(&state), None);
    assert_eq!(
      effects,
      vec![
        Effect::Run(Command::SaveSettings(state.settings.clone())),
        Effect::Run(Command::ListGameFolder {
          game: LETHAL.to_string(),
          folder: PathBuf::from("/games").join(LETHAL),
        }),
      ]
    );
  }

  #[test]
  fn choosing_the_runtime_already_set_changes_nothing() {
    let mut state = on_settings(LETHAL);

    let effects = update(&mut state, Msg::RuntimeChosen(RuntimeChoice::Detect));

    assert_eq!(effects, vec![]);
    assert_eq!(runtime(&state), Some(Runtime::Proton));
  }

  #[test]
  fn a_listing_that_ends_after_leaving_settings_is_ignored() {
    let mut state = on_settings("valheim");

    update(&mut state, Msg::ChangeFolderPressed(Target::GameFolder));
    update(
      &mut state,
      Msg::FolderChosen(Some(PathBuf::from("/moved/valheim"))),
    );
    update(&mut state, Msg::SettingsClosed);

    let effects = update(&mut state, listed("/moved/valheim", &["valheim.x86_64"]));

    assert_eq!(effects, vec![]);
    assert_eq!(
      state.settings.folder("valheim"),
      Some(PathBuf::from("/games/valheim").as_path())
    );
  }
}
