//! A later launch of Rivet handing over to the one already running.

use crate::browse::Status;
use crate::install::ProfileStatus;
use crate::notice::Notice;
use crate::portability::{self, Source};
use crate::{Effect, Screen, State};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use url::Url;

/// What a launch of Rivet asks the running one to do: come to the front,
/// and open what the launch was started with.
///
/// Fields a newer Rivet adds are ignored by an older one, so each new field
/// needs a default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
  /// The Wayland activation token the desktop gave the later launch, which
  /// lets the running Rivet take focus.
  pub activation_token: Option<String>,
  /// What the launch was started with, as given: a file path, made absolute
  /// by the launch, or a URI. Read with [`Open::of`].
  pub open: Option<String>,
}

/// What a launch was started with, as Rivet acts on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Open {
  /// A `.r2z` profile export to import.
  File(PathBuf),
  /// Anything else, as given, which Rivet ignores.
  Other(String),
}

impl Open {
  /// Reads `argument`, a path or a URI. A desktop may pass a file as a
  /// `file://` URI.
  ///
  /// ```
  /// use rivet_core::handoff::Open;
  /// use std::path::PathBuf;
  ///
  /// if cfg!(unix) {
  ///   assert_eq!(
  ///     Open::of("file:///home/me/My%20Mods.r2z"),
  ///     Open::File(PathBuf::from("/home/me/My Mods.r2z"))
  ///   );
  /// }
  ///
  /// assert_eq!(Open::of("notes.txt"), Open::Other("notes.txt".to_string()));
  /// ```
  pub fn of(argument: &str) -> Self {
    // A Windows path such as `C:\a.r2z` parses as a URI with a one-letter
    // scheme.
    let path = match Url::parse(argument) {
      Ok(url) if url.scheme() == "file" => url.to_file_path().ok(),
      Ok(url) if url.scheme().len() > 1 => None,
      _ => Some(PathBuf::from(argument)),
    };

    match path {
      Some(path) if is_profile_export(&path) => Open::File(path),
      _ => Open::Other(argument.to_string()),
    }
  }
}

fn is_profile_export(path: &Path) -> bool {
  path
    .extension()
    .is_some_and(|extension| extension.eq_ignore_ascii_case("r2z"))
}

/// Applies a request a later launch handed over: brings the window to the
/// front, and opens what it was started with once Rivet is ready.
pub(crate) fn handed_off(state: &mut State, request: Request) -> Vec<Effect> {
  launched(state, request);

  vec![Effect::FocusWindow]
}

/// Keeps what `request` opens until Rivet is ready for it, which
/// [`open_when_ready`] checks after each message.
pub(crate) fn launched(state: &mut State, request: Request) -> Vec<Effect> {
  if let Some(open) = request.open {
    state.opening = Some(Open::of(&open));
  }

  Vec::new()
}

/// Acts on what a launch was started with, once Rivet is ready for it.
pub(crate) fn open_when_ready(state: &mut State) -> Vec<Effect> {
  if !state
    .opening
    .as_ref()
    .is_some_and(|open| is_ready(state, open))
  {
    return Vec::new();
  }

  let Some(open) = state.opening.take() else {
    return Vec::new();
  };

  match open {
    Open::File(path) => import_file(state, path),
    Open::Other(_) => Vec::new(),
  }
}

/// Whether Rivet can act on `open`: its settings have loaded and, for a
/// file, the open game's package list and profile have too.
fn is_ready(state: &State, open: &Open) -> bool {
  if state.screen == Screen::Starting {
    return false;
  }

  match open {
    Open::File(_) => !is_game_loading(state),
    Open::Other(_) => true,
  }
}

/// Whether the open game's package list is on its way with none shown yet,
/// or its profile is still being read.
fn is_game_loading(state: &State) -> bool {
  let listing = state.browse.as_ref().is_some_and(|browse| {
    browse.index.is_none() && matches!(browse.status, Status::ReadingCache | Status::Refreshing(_))
  });
  let opening = state
    .profile
    .as_ref()
    .is_some_and(|profile| profile.status == ProfileStatus::Opening);

  listing || opening
}

/// Reads the profile export at `path` to import into the open game, as
/// **Import** > file does, or tells why it can't. Nothing while Rivet
/// restarts to update.
fn import_file(state: &mut State, path: PathBuf) -> Vec<Effect> {
  if state.is_restarting() {
    return Vec::new();
  }

  if state.importing.is_some() {
    return vec![Effect::Notify(Notice::ImportBusy(path))];
  }

  if !state.can_import_profile() {
    return vec![Effect::Notify(Notice::ImportNeedsGame(path))];
  }

  portability::read(state, Source::File(path))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::install::DEFAULT_PROFILE;
  use crate::notice::Notice;
  use crate::portability::{Importing, Source};
  use crate::settings::{GameSettings, Settings};
  use crate::{Command, Effect, Msg, State, test_ecosystem, test_installed, update};
  use std::path::PathBuf;
  use thunderstore_engine::ecosystem::Ecosystem;

  #[test]
  fn a_later_launch_brings_the_window_to_the_front() {
    let mut state = State::new(Ecosystem::bundled());

    let effects = update(&mut state, Msg::HandedOff(Request::default()));

    assert_eq!(effects, vec![Effect::FocusWindow]);
  }

  /// A state that has loaded settings with `game` as the last game, but has
  /// not yet read its package list or opened its profile.
  fn loading(game: &str) -> State {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: Some(game.to_string()),
      games: [(
        game.to_string(),
        GameSettings::new(format!("/games/{game}")),
      )]
      .into(),
      ..Settings::default()
    };

    update(&mut state, Msg::Started);
    update(&mut state, Msg::SettingsLoaded(Ok(settings)));

    state
  }

  /// Answers what [`loading`] waits on for `game`: its cached package list
  /// and its profile.
  fn finish_loading(state: &mut State, game: &str) -> Vec<Effect> {
    let mut effects = update(
      state,
      Msg::CachedIndexRead {
        community: game.to_string(),
        index: Ok(Some(crate::test_shared_index())),
      },
    );

    effects.extend(update(
      state,
      Msg::ProfileOpened {
        game: game.to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(test_installed(&[])),
      },
    ));
    effects.extend(update(
      state,
      Msg::ProfilesListed {
        game: game.to_string(),
        names: Ok(vec![DEFAULT_PROFILE.to_string()]),
      },
    ));

    effects
  }

  /// A state with `game` open and ready to import into.
  fn ready_in(game: &str) -> State {
    let mut state = loading(game);

    finish_loading(&mut state, game);

    state
  }

  fn opening(open: &str) -> Request {
    Request {
      open: Some(open.to_string()),
      ..Request::default()
    }
  }

  fn read_file(path: &str) -> Effect {
    Effect::Run(Command::ReadShared {
      community: "valheim".to_string(),
      source: Source::File(PathBuf::from(path)),
    })
  }

  #[test]
  fn a_file_handed_over_with_a_game_open_is_read_for_import() {
    let mut state = ready_in("valheim");

    let effects = update(&mut state, Msg::HandedOff(opening("/home/me/Shared.r2z")));

    assert_eq!(
      effects,
      vec![Effect::FocusWindow, read_file("/home/me/Shared.r2z")]
    );
  }

  fn needs_game(path: &str) -> Effect {
    Effect::Notify(Notice::ImportNeedsGame(PathBuf::from(path)))
  }

  #[test]
  #[cfg(unix)]
  fn a_file_uri_opens_the_file_it_names() {
    let mut state = ready_in("valheim");

    let effects = update(
      &mut state,
      Msg::HandedOff(opening("file:///home/me/My%20Mods.r2z")),
    );

    assert_eq!(
      effects,
      vec![Effect::FocusWindow, read_file("/home/me/My Mods.r2z")]
    );
  }

  #[test]
  fn what_a_launch_opens_is_read_as_a_file_only_for_a_profile_export() {
    let cases = [
      (
        "/home/me/Shared.r2z",
        Open::File(PathBuf::from("/home/me/Shared.r2z")),
      ),
      (
        "/home/me/SHARED.R2Z",
        Open::File(PathBuf::from("/home/me/SHARED.R2Z")),
      ),
      (
        "/home/me/notes.txt",
        Open::Other("/home/me/notes.txt".to_string()),
      ),
      (
        "ror2mm://v1/install/thunderstore.io/Owner/Mod/1.0.0/",
        Open::Other("ror2mm://v1/install/thunderstore.io/Owner/Mod/1.0.0/".to_string()),
      ),
    ];

    for (argument, open) in cases {
      assert_eq!(Open::of(argument), open, "{argument}");
    }
  }

  #[test]
  #[cfg(unix)]
  fn a_file_uri_names_a_file_only_on_this_machine() {
    let cases = [
      (
        "file://localhost/home/me/Shared.r2z",
        Open::File(PathBuf::from("/home/me/Shared.r2z")),
      ),
      (
        "file://elsewhere/home/me/Shared.r2z",
        Open::Other("file://elsewhere/home/me/Shared.r2z".to_string()),
      ),
    ];

    for (argument, open) in cases {
      assert_eq!(Open::of(argument), open, "{argument}");
    }
  }

  #[test]
  #[cfg(windows)]
  fn a_windows_file_uri_or_path_opens_the_file_it_names() {
    let cases = [
      (
        "file:///C:/Users/me/My%20Mods.r2z",
        Open::File(PathBuf::from(r"C:\Users\me\My Mods.r2z")),
      ),
      (
        r"C:\Users\me\Shared.r2z",
        Open::File(PathBuf::from(r"C:\Users\me\Shared.r2z")),
      ),
    ];

    for (argument, open) in cases {
      assert_eq!(Open::of(argument), open, "{argument}");
    }
  }

  #[test]
  fn a_link_handed_over_only_brings_the_window_to_the_front() {
    let mut state = ready_in("valheim");

    let effects = update(
      &mut state,
      Msg::HandedOff(opening(
        "ror2mm://v1/install/thunderstore.io/Owner/Mod/1.0.0/",
      )),
    );

    assert_eq!(effects, vec![Effect::FocusWindow]);
    assert_eq!(state.opening, None);
  }

  #[test]
  fn a_file_handed_over_with_no_game_open_asks_to_open_one_and_imports_nothing() {
    let mut state = ready_in("valheim");

    update(&mut state, Msg::SwitchGamePressed);

    let effects = update(&mut state, Msg::HandedOff(opening("/home/me/Shared.r2z")));

    assert_eq!(
      effects,
      vec![Effect::FocusWindow, needs_game("/home/me/Shared.r2z")]
    );
    assert_eq!(state.importing, None);
  }

  #[test]
  fn a_file_handed_over_to_a_game_without_profile_imports_asks_to_open_another() {
    let mut state = ready_in("balatro");

    let effects = update(&mut state, Msg::HandedOff(opening("/home/me/Shared.r2z")));

    assert_eq!(
      effects,
      vec![Effect::FocusWindow, needs_game("/home/me/Shared.r2z")]
    );
    assert_eq!(state.importing, None);
  }

  #[test]
  fn a_file_handed_over_during_another_import_leaves_that_import_alone() {
    let mut state = ready_in("valheim");

    update(&mut state, Msg::ImportCodePressed);

    let effects = update(&mut state, Msg::HandedOff(opening("/home/me/Shared.r2z")));

    assert_eq!(
      effects,
      vec![
        Effect::FocusWindow,
        Effect::Notify(Notice::ImportBusy(PathBuf::from("/home/me/Shared.r2z")))
      ]
    );
    assert_eq!(state.importing, Some(Importing::AskingCode(None)));
  }

  #[test]
  fn a_file_handed_over_while_rivet_restarts_to_update_is_not_imported() {
    let mut state = ready_in("valheim");

    state.self_update.stage = crate::self_update::Stage::Restarting {
      version: "0.3.0".to_string(),
      file: PathBuf::from("/opt/rivet/rivet.new"),
      replacing: true,
    };

    let effects = update(&mut state, Msg::HandedOff(opening("/home/me/Shared.r2z")));

    assert_eq!(effects, vec![Effect::FocusWindow]);
    assert_eq!(state.importing, None);
  }

  #[test]
  fn a_file_passed_at_first_launch_waits_until_the_last_game_has_loaded() {
    let mut state = State::new(test_ecosystem());

    update(&mut state, Msg::Started);

    assert_eq!(
      update(&mut state, Msg::Launched(opening("/home/me/Shared.r2z"))),
      vec![]
    );

    let settings = Settings {
      last_game: Some("valheim".to_string()),
      games: [(
        "valheim".to_string(),
        GameSettings::new("/games/valheim".to_string()),
      )]
      .into(),
      ..Settings::default()
    };
    let effects = update(&mut state, Msg::SettingsLoaded(Ok(settings)));

    assert!(!effects.contains(&read_file("/home/me/Shared.r2z")));

    let effects = finish_loading(&mut state, "valheim");

    assert!(
      effects.contains(&read_file("/home/me/Shared.r2z")),
      "{effects:?}"
    );
    assert!(!effects.contains(&Effect::FocusWindow));
  }

  #[test]
  fn a_file_passed_at_first_launch_with_no_game_to_open_asks_to_open_one() {
    let mut state = State::new(test_ecosystem());

    update(&mut state, Msg::Started);
    update(&mut state, Msg::Launched(opening("/home/me/Shared.r2z")));

    let effects = update(&mut state, Msg::SettingsLoaded(Ok(Settings::default())));

    assert!(
      effects.contains(&needs_game("/home/me/Shared.r2z")),
      "{effects:?}"
    );

    let effects = update(&mut state, Msg::SearchChanged("val".to_string()));

    assert_eq!(effects, vec![], "the file was acted on twice");
  }
}
