//! The record of files launches copied into each game's folder, and removing
//! them from the settings screen to leave the folder as it was before.

use crate::notice::Notice;
use crate::{Command, Effect, Screen, State};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A file a launch copied into a game folder, as it was when copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoaderFile {
  pub path: PathBuf,
  /// The SHA-256 of what was copied, in lowercase hex, so a file changed
  /// since is left alone. Empty when it could not be read after copying,
  /// which no file matches.
  pub sha256: String,
}

/// What one launch copied into `game`'s folder, for
/// [`crate::Msg::LoaderFilesCopied`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
  /// The game, by its key in the game definitions.
  pub game: String,
  /// Files the launch created.
  pub created: Vec<LoaderFile>,
  /// Files already in the folder that the launch overwrote. Each may be the
  /// player's own, so only one already recorded stays recorded.
  pub replaced: Vec<LoaderFile>,
}

/// Records what a launch copied into a game folder and saves it: every file
/// it created, and the new contents of recorded files it overwrote.
pub(crate) fn copied(state: &mut State, copied: Copied) -> Vec<Effect> {
  let Some(settings) = state.settings.games.get_mut(&copied.game) else {
    return Vec::new();
  };

  let before = settings.loader_files.clone();
  let copies = copied
    .created
    .into_iter()
    .map(|file| (file, true))
    .chain(copied.replaced.into_iter().map(|file| (file, false)));

  for (file, created) in copies {
    match settings
      .loader_files
      .iter_mut()
      .find(|recorded| recorded.path == file.path)
    {
      Some(recorded) => *recorded = file,
      None if created => settings.loader_files.push(file),
      None => {}
    }
  }

  if settings.loader_files == before {
    return Vec::new();
  }

  vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
}

/// What became of one recorded file when its removal was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileRemoval {
  Removed,
  /// It was already gone.
  Missing,
  /// It changed since it was copied, so it was left in place.
  Changed,
  /// It could not be removed, with the reason.
  Failed(String),
}

impl State {
  /// Whether a game's loader files are being removed, so no launch can start.
  pub fn is_removing_loader_files(&self) -> bool {
    self.loader_removal.is_some()
  }

  /// The files launches copied into `game`'s folder and have not been
  /// removed since.
  pub fn loader_files(&self, game: &str) -> &[LoaderFile] {
    self
      .settings
      .games
      .get(game)
      .map_or(&[], |settings| settings.loader_files.as_slice())
  }

  /// Whether removing `game`'s loader files would start: some are recorded,
  /// and no removal or launch is under way.
  pub fn can_remove_loader_files(&self, game: &str) -> bool {
    !self.loader_files(game).is_empty() && !self.is_removing_loader_files() && !self.is_launching()
  }
}

/// Removes the files launches copied into the folder of the game whose
/// settings are shown, unless [`State::can_remove_loader_files`] says not.
pub(crate) fn remove_pressed(state: &mut State) -> Vec<Effect> {
  let Screen::Settings(game) = &state.screen else {
    return Vec::new();
  };

  if !state.can_remove_loader_files(game) {
    return Vec::new();
  }

  let game = game.clone();
  let files = state.loader_files(&game).to_vec();

  state.loader_removal = Some(game.clone());

  vec![Effect::Run(Command::RemoveLoaderFiles { game, files })]
}

/// Applies the end of a removal of `game`'s loader files: each file removed,
/// already gone, or changed since it was copied is no longer recorded, and
/// each that could not be removed stays recorded to try again.
pub(crate) fn removed(
  state: &mut State,
  game: String,
  files: Vec<(PathBuf, FileRemoval)>,
) -> Vec<Effect> {
  if state.loader_removal.as_ref() == Some(&game) {
    state.loader_removal = None;
  }

  let mut changed = Vec::new();
  let mut failed = Vec::new();
  let mut forgotten = Vec::new();

  for (path, removal) in files {
    match removal {
      FileRemoval::Removed | FileRemoval::Missing => forgotten.push(path),
      FileRemoval::Changed => {
        forgotten.push(path.clone());
        changed.push(path);
      }
      FileRemoval::Failed(reason) => failed.push((path, reason)),
    }
  }

  let mut effects = Vec::new();

  if let Some(settings) = state.settings.games.get_mut(&game) {
    settings
      .loader_files
      .retain(|file| !forgotten.contains(&file.path));
    effects.push(Effect::Run(Command::SaveSettings(state.settings.clone())));
  }

  effects.push(Effect::Notify(Notice::LoaderFilesRemoved {
    game,
    changed,
    failed,
  }));

  effects
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::settings::{GameSettings, Settings};
  use crate::{Msg, test_ecosystem, update};
  use thunderstore_engine::profile::launch::{HostOs, LaunchMode};

  /// A Linux state with valheim open from `/games/valheim`, its record
  /// holding `recorded`.
  fn opened(recorded: &[LoaderFile]) -> State {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: Some("valheim".to_string()),
      games: [(
        "valheim".to_string(),
        GameSettings {
          loader_files: recorded.to_vec(),
          ..GameSettings::new("/games/valheim")
        },
      )]
      .into(),
      ..Settings::default()
    };

    state.host = Some(HostOs::Linux);

    update(&mut state, Msg::Started);
    update(&mut state, Msg::SettingsLoaded(Ok(settings)));

    state
  }

  fn file(name: &str, sha256: &str) -> LoaderFile {
    LoaderFile {
      path: PathBuf::from("/games/valheim").join(name),
      sha256: sha256.to_string(),
    }
  }

  fn copied_into_valheim(created: &[LoaderFile], replaced: &[LoaderFile]) -> Msg {
    Msg::LoaderFilesCopied(Copied {
      game: "valheim".to_string(),
      created: created.to_vec(),
      replaced: replaced.to_vec(),
    })
  }

  fn recorded(state: &State) -> Vec<LoaderFile> {
    state.settings.games["valheim"].loader_files.clone()
  }

  #[test]
  fn files_a_launch_created_are_recorded_for_the_game_and_saved() {
    let mut state = opened(&[]);
    let created = [file("winhttp.dll", "aa"), file("doorstop_config.ini", "bb")];

    let effects = update(&mut state, copied_into_valheim(&created, &[]));

    assert_eq!(recorded(&state), created);
    assert_eq!(
      effects,
      vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
    );
  }

  #[test]
  fn an_overwritten_file_stays_recorded_only_if_a_launch_copied_it_before() {
    let mut state = opened(&[file("winhttp.dll", "old")]);
    let replaced = [file("winhttp.dll", "new"), file("players-own.ini", "cc")];

    update(&mut state, copied_into_valheim(&[], &replaced));

    assert_eq!(recorded(&state), [file("winhttp.dll", "new")]);
  }

  #[test]
  fn a_file_created_again_is_recorded_once() {
    let mut state = opened(&[file("winhttp.dll", "old")]);

    update(
      &mut state,
      copied_into_valheim(&[file("winhttp.dll", "new")], &[]),
    );

    assert_eq!(recorded(&state), [file("winhttp.dll", "new")]);
  }

  #[test]
  fn overwriting_only_the_players_own_files_records_and_saves_nothing() {
    let mut state = opened(&[]);

    let effects = update(
      &mut state,
      copied_into_valheim(&[], &[file("players-own.ini", "cc")]),
    );

    assert_eq!(effects, vec![]);
    assert_eq!(recorded(&state), []);
  }

  /// As [`opened`], with the settings screen shown.
  fn on_settings(recorded: &[LoaderFile]) -> State {
    let mut state = opened(recorded);

    update(&mut state, Msg::SettingsPressed);

    state
  }

  fn remove(files: &[LoaderFile]) -> Effect {
    Effect::Run(Command::RemoveLoaderFiles {
      game: "valheim".to_string(),
      files: files.to_vec(),
    })
  }

  #[test]
  fn removing_sends_every_recorded_file_to_be_removed() {
    let files = [file("winhttp.dll", "aa"), file("doorstop_config.ini", "bb")];
    let mut state = on_settings(&files);

    let effects = update(&mut state, Msg::RemoveLoaderFilesPressed);

    assert_eq!(effects, vec![remove(&files)]);
    assert!(state.is_removing_loader_files());
  }

  #[test]
  fn removing_with_nothing_recorded_does_nothing() {
    let mut state = on_settings(&[]);

    assert_eq!(update(&mut state, Msg::RemoveLoaderFilesPressed), vec![]);
    assert!(!state.is_removing_loader_files());
  }

  #[test]
  fn removing_again_while_a_removal_runs_does_nothing() {
    let mut state = on_settings(&[file("winhttp.dll", "aa")]);

    update(&mut state, Msg::RemoveLoaderFilesPressed);

    assert_eq!(update(&mut state, Msg::RemoveLoaderFilesPressed), vec![]);
  }

  /// As [`opened`], with valheim's folder listed and its profile open, so it
  /// can launch.
  fn launchable(recorded: &[LoaderFile]) -> State {
    let mut state = opened(recorded);

    update(
      &mut state,
      Msg::GameFolderListed {
        game: "valheim".to_string(),
        entries: Ok(vec!["valheim.x86_64".to_string()]),
      },
    );
    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: crate::install::DEFAULT_PROFILE.to_string(),
        installed: Ok(crate::install::Installed::new()),
      },
    );

    state
  }

  #[test]
  fn removing_waits_for_a_launch_being_prepared() {
    let mut state = launchable(&[file("winhttp.dll", "aa")]);

    update(&mut state, Msg::LaunchPressed(LaunchMode::Modded));
    update(&mut state, Msg::SettingsPressed);

    assert_eq!(update(&mut state, Msg::RemoveLoaderFilesPressed), vec![]);
    assert!(!state.is_removing_loader_files());
  }

  #[test]
  fn a_launch_waits_for_a_running_removal() {
    let mut state = launchable(&[file("winhttp.dll", "aa")]);

    assert!(state.can_launch());

    update(&mut state, Msg::SettingsPressed);
    update(&mut state, Msg::RemoveLoaderFilesPressed);
    update(&mut state, Msg::SettingsClosed);

    assert!(!state.can_launch());
    assert_eq!(
      update(&mut state, Msg::LaunchPressed(LaunchMode::Modded)),
      vec![]
    );
  }

  #[test]
  fn a_removal_forgets_each_file_dealt_with_and_keeps_those_that_failed() {
    let files = [
      file("winhttp.dll", "aa"),
      file("doorstop_config.ini", "bb"),
      file(".doorstop_version", "cc"),
      file("changelog.txt", "dd"),
    ];
    let mut state = on_settings(&files);

    update(&mut state, Msg::RemoveLoaderFilesPressed);

    let effects = update(
      &mut state,
      Msg::LoaderFilesRemoved {
        game: "valheim".to_string(),
        files: vec![
          (files[0].path.clone(), FileRemoval::Removed),
          (files[1].path.clone(), FileRemoval::Missing),
          (files[2].path.clone(), FileRemoval::Changed),
          (
            files[3].path.clone(),
            FileRemoval::Failed("in use".to_string()),
          ),
        ],
      },
    );

    assert_eq!(recorded(&state), [files[3].clone()]);
    assert!(!state.is_removing_loader_files());
    assert_eq!(
      effects,
      vec![
        Effect::Run(Command::SaveSettings(state.settings.clone())),
        Effect::Notify(Notice::LoaderFilesRemoved {
          game: "valheim".to_string(),
          changed: vec![files[2].path.clone()],
          failed: vec![(files[3].path.clone(), "in use".to_string())],
        }),
      ]
    );
  }

  #[test]
  fn a_removal_that_ends_after_leaving_the_game_still_updates_its_record() {
    let mut state = on_settings(&[file("winhttp.dll", "aa")]);

    update(&mut state, Msg::RemoveLoaderFilesPressed);
    update(&mut state, Msg::SettingsClosed);
    update(&mut state, Msg::SwitchGamePressed);
    update(
      &mut state,
      Msg::LoaderFilesRemoved {
        game: "valheim".to_string(),
        files: vec![(file("winhttp.dll", "aa").path, FileRemoval::Removed)],
      },
    );

    assert_eq!(recorded(&state), []);
    assert!(!state.is_removing_loader_files());
  }
}
