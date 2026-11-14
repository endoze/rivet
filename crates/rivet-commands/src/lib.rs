//! Runs [`Command`]s on a Tokio thread of their own and answers each with a
//! [`Msg`], so the UI thread never waits on disk or network.

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use rivet_core::settings::Settings;
use rivet_core::{Command, Msg};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Where commands read and write the app's files.
#[derive(Debug, Clone)]
pub struct Paths {
  /// The settings TOML file. Its folder is created on the first save.
  pub settings: PathBuf,
}

/// Runs one command to completion and returns the message that answers it.
pub async fn run(command: Command, paths: &Paths) -> Msg {
  match command {
    Command::LoadSettings => Msg::SettingsLoaded(load_settings(&paths.settings).await),
    Command::SaveSettings(settings) => {
      Msg::SettingsSaved(save_settings(&paths.settings, &settings).await)
    }
    Command::ListFolder(folder) => {
      let entries = list_folder(&folder).await;

      Msg::FolderListed { folder, entries }
    }
  }
}

async fn load_settings(file: &Path) -> Result<Settings, String> {
  match tokio::fs::read_to_string(file).await {
    Ok(text) => toml::from_str(&text).map_err(|error| failure(file, error)),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
    Err(error) => Err(failure(file, error)),
  }
}

/// Held for the whole of a save. Commands run concurrently, and two saves
/// sharing the partial file would rename each other's half-written content.
static SAVING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn save_settings(file: &Path, settings: &Settings) -> Result<(), String> {
  let text = toml::to_string(settings).map_err(|error| failure(file, error))?;
  let partial = file.with_extension("toml.partial");
  let _saving = SAVING.lock().await;

  if let Some(folder) = file.parent() {
    tokio::fs::create_dir_all(folder)
      .await
      .map_err(|error| failure(folder, error))?;
  }

  // Written aside and renamed over the old file, so a crash mid-write leaves
  // the previous settings intact.
  tokio::fs::write(&partial, text)
    .await
    .map_err(|error| failure(&partial, error))?;

  tokio::fs::rename(&partial, file)
    .await
    .map_err(|error| failure(file, error))
}

async fn list_folder(folder: &Path) -> Result<Vec<String>, String> {
  let mut reader = tokio::fs::read_dir(folder)
    .await
    .map_err(|error| error.to_string())?;
  let mut names = Vec::new();

  while let Some(entry) = reader
    .next_entry()
    .await
    .map_err(|error| error.to_string())?
  {
    names.push(entry.file_name().to_string_lossy().into_owned());
  }

  Ok(names)
}

fn failure(path: &Path, error: impl std::fmt::Display) -> String {
  format!("{}: {error}", path.display())
}

/// Starts a Tokio runtime for commands, on threads named `commands`.
///
/// Commands sent on the returned sender run concurrently on that runtime, and
/// their answers arrive on the returned receiver. The runtime stops once the
/// sender is dropped.
///
/// # Panics
///
/// Panics if the runtime or the thread that owns it cannot be started.
pub fn spawn(paths: Paths) -> (UnboundedSender<Command>, UnboundedReceiver<Msg>) {
  let (commands, mut pending) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let paths = Arc::new(paths);
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .thread_name("commands")
    .enable_all()
    .build()
    .expect("tokio runtime");

  std::thread::Builder::new()
    .name("commands".into())
    .spawn(move || {
      runtime.block_on(async move {
        while let Some(command) = pending.next().await {
          let answers = answers.clone();
          let paths = paths.clone();

          tokio::spawn(async move {
            answers.unbounded_send(run(command, &paths).await).ok();
          });
        }
      });
    })
    .expect("commands thread");

  (commands, inbox)
}

#[cfg(test)]
mod tests {
  use super::*;
  use rivet_core::settings::GameSettings;
  use tempfile::TempDir;

  fn paths_in(dir: &TempDir) -> Paths {
    Paths {
      settings: dir.path().join("config").join("settings.toml"),
    }
  }

  fn run_now(command: Command, paths: &Paths) -> Msg {
    tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .unwrap()
      .block_on(run(command, paths))
  }

  fn valheim_settings() -> Settings {
    Settings {
      last_game: Some("valheim".to_string()),
      games: [(
        "valheim".to_string(),
        GameSettings {
          folder: PathBuf::from("/games/Valheim"),
        },
      )]
      .into(),
    }
  }

  #[test]
  fn loading_without_a_settings_file_gives_the_defaults() {
    let dir = TempDir::new().unwrap();

    let answer = run_now(Command::LoadSettings, &paths_in(&dir));

    assert_eq!(answer, Msg::SettingsLoaded(Ok(Settings::default())));
  }

  #[test]
  fn saved_settings_load_back_after_a_restart() {
    let dir = TempDir::new().unwrap();
    let paths = paths_in(&dir);

    let saved = run_now(Command::SaveSettings(valheim_settings()), &paths);
    let loaded = run_now(Command::LoadSettings, &paths);

    assert_eq!(saved, Msg::SettingsSaved(Ok(())));
    assert_eq!(loaded, Msg::SettingsLoaded(Ok(valheim_settings())));
  }

  #[test]
  fn settings_are_stored_as_toml() {
    let dir = TempDir::new().unwrap();
    let paths = paths_in(&dir);

    run_now(Command::SaveSettings(valheim_settings()), &paths);

    assert_eq!(
      std::fs::read_to_string(&paths.settings).unwrap(),
      "last_game = \"valheim\"\n\n[games.valheim]\nfolder = \"/games/Valheim\"\n"
    );
  }

  #[test]
  fn a_malformed_settings_file_is_reported() {
    let dir = TempDir::new().unwrap();
    let paths = paths_in(&dir);

    std::fs::create_dir_all(paths.settings.parent().unwrap()).unwrap();
    std::fs::write(&paths.settings, "last_game = [").unwrap();

    let answer = run_now(Command::LoadSettings, &paths);

    assert!(matches!(answer, Msg::SettingsLoaded(Err(_))), "{answer:?}");
  }

  #[test]
  fn listing_a_folder_answers_with_its_entry_names() {
    let dir = TempDir::new().unwrap();

    std::fs::write(dir.path().join("valheim.x86_64"), "").unwrap();
    std::fs::create_dir(dir.path().join("valheim_Data")).unwrap();

    let answer = run_now(
      Command::ListFolder(dir.path().to_path_buf()),
      &paths_in(&dir),
    );

    let Msg::FolderListed { folder, entries } = answer else {
      panic!("unexpected answer {answer:?}");
    };
    let mut entries = entries.unwrap();

    entries.sort();

    assert_eq!(folder, dir.path());
    assert_eq!(entries, vec!["valheim.x86_64", "valheim_Data"]);
  }

  #[test]
  fn listing_a_missing_folder_answers_with_why() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("missing");

    let answer = run_now(Command::ListFolder(missing.clone()), &paths_in(&dir));

    assert!(
      matches!(&answer, Msg::FolderListed { folder, entries: Err(_) } if *folder == missing),
      "{answer:?}"
    );
  }

  #[test]
  fn a_command_sent_to_the_commands_thread_is_answered() {
    let dir = TempDir::new().unwrap();
    let (commands, mut inbox) = spawn(paths_in(&dir));

    commands.unbounded_send(Command::LoadSettings).unwrap();

    let answer = futures::executor::block_on(inbox.next());

    assert_eq!(answer, Some(Msg::SettingsLoaded(Ok(Settings::default()))));
  }

  #[test]
  fn the_inbox_closes_when_the_commands_sender_is_dropped() {
    let dir = TempDir::new().unwrap();
    let (commands, mut inbox) = spawn(paths_in(&dir));

    drop(commands);

    let answer = futures::executor::block_on(inbox.next());

    assert_eq!(answer, None);
  }
}
