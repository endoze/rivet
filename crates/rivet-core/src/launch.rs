//! Launching the open game modded or vanilla, the Steam launch option a native
//! Linux game needs before a modded launch loads packages, and whether the
//! profile has the mod loader those packages need.

use crate::install::Profile;
use crate::notice::Notice;
use crate::settings::split_arguments;
use crate::{Command, Effect, Screen, State};
use std::path::{Path, PathBuf};
use thunderstore_engine::ecosystem::{Ecosystem, LoaderKind};
use thunderstore_engine::profile::launch::{
  HostOs, LaunchContext, LaunchFacts, LaunchMode, Runtime, StorePlatform, launch_plan_from,
};

/// Start the open game from a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
  /// The [`Attempt`] id the result carries.
  pub id: u64,
  /// The game, by its key in the game definitions.
  pub game: String,
  pub profile: String,
  /// The folder the game is installed in.
  pub folder: PathBuf,
  pub context: LaunchContext,
}

/// Launching for the open game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launcher {
  /// The game, by its key in the game definitions.
  pub game: String,
  /// Whether the game runs natively or under Proton. `None` while its folder
  /// is read to tell.
  pub runtime: Option<Runtime>,
  /// The line Steam's launch options must hold for mods to load. `None` when
  /// the game needs none there.
  pub steam: Option<SteamOption>,
}

/// The line to paste into the game's Steam launch options, and whether it is
/// there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamOption {
  /// `None` until it has been worked out.
  pub line: Option<String>,
  /// What the last read of Steam's launch options found, kept while another
  /// read runs. `None` until the first read ends.
  pub found: Option<OptionStatus>,
  /// Whether a read of Steam's launch options is running.
  pub checking: bool,
}

impl SteamOption {
  /// Whether the user must be shown the line: the last read found Steam
  /// without it, or could not tell. Nothing needs showing before the first
  /// read ends or once the line is set, so a game set up for mods opens
  /// without it flashing up.
  pub fn needs_attention(&self) -> bool {
    matches!(
      self.found,
      Some(OptionStatus::NotSet | OptionStatus::Unknown(_))
    )
  }
}

/// Whether Steam's launch options for the game hold the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionStatus {
  Set,
  NotSet,
  /// Steam's settings could not be read, with the reason.
  Unknown(String),
}

/// One press of a launch button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
  /// Unique per app run, so the result can be matched to it.
  pub id: u64,
  /// The game, by its key in the game definitions.
  pub game: String,
  pub mode: LaunchMode,
  pub status: AttemptStatus,
}

/// Where an [`Attempt`] is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptStatus {
  Preparing,
  /// The game was started.
  Started,
  Failed(LaunchFailure),
}

/// Why a launch did not start the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchFailure {
  /// A modded launch found no mod loader in the profile.
  NoModLoader(LoaderKind),
  /// The game folder does not exist.
  MissingGameFolder(PathBuf),
  /// Anything else, with the reason.
  Other(String),
}

/// What Steam's launch options for a game were found to be, for
/// [`crate::Msg::SteamOptionRead`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamOptionRead {
  /// The game, by its key in the game definitions.
  pub game: String,
  /// How the game runs in the context the line was worked out for.
  pub runtime: Runtime,
  /// The line the launch options must hold, or `None` when they need none.
  pub line: Option<String>,
  /// The launch options Steam holds, `None` when there are none, or why they
  /// could not be read.
  pub current: Result<Option<String>, String>,
}

/// Sets up launching for `game`. On Linux it runs as its settings choose, or,
/// when they leave it to be detected, its folder is listed first to tell
/// whether it runs under Proton. Called again when either setting changes.
pub(crate) fn open(state: &mut State, game: &str) -> Vec<Effect> {
  state.launcher = Some(Launcher {
    game: game.to_string(),
    runtime: None,
    steam: None,
  });

  let forced = state.settings.runtime(game).forced();
  let folder = state.settings.folder(game).map(Path::to_path_buf);

  match (state.host, forced, folder) {
    (Some(HostOs::Linux), Some(runtime), _) => runtime_found(state, runtime),
    (Some(HostOs::Linux), None, Some(folder)) => vec![Effect::Run(Command::ListGameFolder {
      game: game.to_string(),
      folder,
    })],
    _ => runtime_found(state, Runtime::Native),
  }
}

/// Applies a listing of the open game's folder: the game runs under Proton
/// when the folder holds only its Windows build. A folder that cannot be
/// listed counts as native, and the launch says why it fails.
pub(crate) fn game_folder_listed(
  state: &mut State,
  game: String,
  entries: Result<Vec<String>, String>,
) -> Vec<Effect> {
  let waiting = state
    .launcher
    .as_ref()
    .is_some_and(|launcher| launcher.game == game && launcher.runtime.is_none());

  if !waiting {
    return Vec::new();
  }

  let exe_names = state
    .ecosystem
    .require_profile(&game)
    .map(|profile| profile.exe_names.clone())
    .unwrap_or_default();
  let runtime = entries
    .map(|entries| runtime_of(&exe_names, &entries))
    .unwrap_or(Runtime::Native);

  runtime_found(state, runtime)
}

/// How a game whose executables are `exe_names` runs from a folder holding
/// `entries`: under Proton when the folder holds only the Windows ones, which
/// is the build Steam installs for Proton. Names match ignoring ASCII case.
///
/// ```
/// use rivet_core::launch::runtime_of;
/// use thunderstore_engine::profile::launch::Runtime;
///
/// let exe_names = ["valheim.exe".to_string(), "valheim.x86_64".to_string()];
///
/// assert_eq!(runtime_of(&exe_names, &["valheim.exe".into()]), Runtime::Proton);
/// assert_eq!(runtime_of(&exe_names, &["valheim.x86_64".into()]), Runtime::Native);
/// ```
pub fn runtime_of(exe_names: &[String], entries: &[String]) -> Runtime {
  let present = |windows: bool| {
    exe_names
      .iter()
      .filter(|exe| exe.to_ascii_lowercase().ends_with(".exe") == windows)
      .any(|exe| entries.iter().any(|entry| entry.eq_ignore_ascii_case(exe)))
  };

  match (present(true), present(false)) {
    (true, false) => Runtime::Proton,
    _ => Runtime::Native,
  }
}

/// Records how the open game runs, then reads its Steam launch options when
/// they must hold a line for mods to load.
fn runtime_found(state: &mut State, runtime: Runtime) -> Vec<Effect> {
  let Some(launcher) = state.launcher.as_mut() else {
    return Vec::new();
  };

  launcher.runtime = Some(runtime);

  let game = launcher.game.clone();

  if steam_line_app_id(state, &game).is_none() {
    return Vec::new();
  }

  if let Some(launcher) = state.launcher.as_mut() {
    launcher.steam = Some(SteamOption {
      line: None,
      found: None,
      checking: true,
    });
  }

  check_steam_option(state)
}

/// How `game` launches in `mode` on this host: natively or under Proton as
/// its folder showed, through Steam when it has a Steam app id and directly
/// otherwise. `None` where launching is not supported, or before the folder
/// has been read.
fn context(state: &State, game: &str, mode: LaunchMode) -> Option<LaunchContext> {
  let host = state.host?;
  let runtime = state
    .launcher
    .as_ref()
    .filter(|launcher| launcher.game == game)?
    .runtime?;
  let store = match steam_app_id(&state.ecosystem, game) {
    Some(_) => StorePlatform::Steam,
    None => StorePlatform::Other,
  };

  Some(LaunchContext::new(host, store, runtime, mode))
}

fn steam_app_id(ecosystem: &Ecosystem, game: &str) -> Option<u32> {
  ecosystem
    .require_profile(game)
    .ok()
    .and_then(|profile| profile.steam_app_id())
}

/// `game`'s Steam app id when Steam's launch options must hold a line for a
/// modded launch to load packages: the wrapper for a native Linux game, or
/// the DLL override under Proton.
fn steam_line_app_id(state: &State, game: &str) -> Option<u32> {
  let context = context(state, game, LaunchMode::Vanilla)?;
  let plan = launch_plan_from(
    Path::new(""),
    &LaunchFacts::default(),
    &state.ecosystem,
    game,
    &context,
  )
  .ok()?;

  plan
    .steam_launch_options(Path::new(""))
    .and(steam_app_id(&state.ecosystem, game))
}

impl State {
  /// Whether a launch is being prepared, so no install or other launch can
  /// start.
  pub fn is_launching(&self) -> bool {
    self
      .launch
      .as_ref()
      .is_some_and(|attempt| attempt.status == AttemptStatus::Preparing)
  }

  /// Whether a launch button would start a launch: the open profile has been
  /// read, the game's folder has shown whether it runs under Proton, no
  /// operation on the profile is running or waiting, no other launch is being
  /// prepared, no uninstall is being checked, no profile change is being
  /// made, and no loader files are being removed.
  pub fn can_launch(&self) -> bool {
    let ready = self.profile.as_ref().and_then(Profile::installed).is_some();
    let runtime = self
      .launcher
      .as_ref()
      .is_some_and(|launcher| launcher.runtime.is_some());

    self.host.is_some()
      && ready
      && runtime
      && !self.is_launching()
      && !self.is_profile_busy()
      && !self.is_checking()
      && !self.is_changing_profile()
      && !self.is_removing_loader_files()
  }
}

/// Launches the open game from its profile in `mode`, unless
/// [`State::can_launch`] says it cannot.
pub(crate) fn launch_pressed(state: &mut State, mode: LaunchMode) -> Vec<Effect> {
  if !state.can_launch() {
    return Vec::new();
  }

  let Screen::Game(game) = &state.screen else {
    return Vec::new();
  };
  let Some(folder) = state.settings.folder(game) else {
    return Vec::new();
  };
  let Some(profile) = state.profile.as_ref() else {
    return Vec::new();
  };
  let Some(context) = context(state, game, mode) else {
    return Vec::new();
  };
  let arguments = split_arguments(state.settings.launch_arguments(game));
  let launch = Launch {
    id: state.launches + 1,
    game: game.clone(),
    profile: profile.name.clone(),
    folder: folder.to_path_buf(),
    context: context.with_extra_args(arguments),
  };

  state.launches = launch.id;
  state.launch = Some(Attempt {
    id: launch.id,
    game: launch.game.clone(),
    mode,
    status: AttemptStatus::Preparing,
  });

  vec![Effect::Run(Command::Launch(launch))]
}

/// Applies the result of launch `id`, saying whether it started the game,
/// then reads Steam's launch option again, since the user may have set it
/// since it was last read.
pub(crate) fn launch_finished(
  state: &mut State,
  id: u64,
  result: Result<(), LaunchFailure>,
) -> Vec<Effect> {
  let Some(attempt) = state
    .launch
    .as_mut()
    .filter(|attempt| attempt.id == id && attempt.status == AttemptStatus::Preparing)
  else {
    return Vec::new();
  };

  let (status, told) = match result {
    Ok(()) => (
      AttemptStatus::Started,
      Notice::Launched {
        game: attempt.game.clone(),
        mode: attempt.mode,
      },
    ),
    Err(failure) => (
      AttemptStatus::Failed(failure.clone()),
      Notice::LaunchFailed {
        game: attempt.game.clone(),
        mode: attempt.mode,
        failure,
      },
    ),
  };

  attempt.status = status;

  let mut effects = vec![Effect::Notify(told)];

  effects.extend(check_steam_option(state));

  effects
}

/// Puts the open game's Steam launch option line on the clipboard, once it
/// has been worked out, and says so.
pub(crate) fn copy_line(state: &State) -> Vec<Effect> {
  let Some(line) = state
    .launcher
    .as_ref()
    .and_then(|launcher| launcher.steam.as_ref())
    .and_then(|steam| steam.line.clone())
  else {
    return Vec::new();
  };

  vec![
    Effect::CopyToClipboard(line),
    Effect::Notify(Notice::LaunchLineCopied),
  ]
}

/// Applies what Steam's launch options for a game were found to be. The line
/// is set when the options hold it anywhere, beside any of the user's own.
///
/// A line found set after the last read found it missing, or could not tell,
/// tells the user Steam is now set up. A read for a runtime the game no longer
/// runs under is ignored.
pub(crate) fn steam_option_read(state: &mut State, read: SteamOptionRead) -> Vec<Effect> {
  let Some(launcher) = state
    .launcher
    .as_mut()
    .filter(|launcher| launcher.game == read.game && launcher.runtime == Some(read.runtime))
  else {
    return Vec::new();
  };
  let Some(line) = read.line else {
    launcher.steam = None;

    return Vec::new();
  };
  let Some(steam) = launcher.steam.as_mut() else {
    return Vec::new();
  };
  let found = match read.current {
    Ok(Some(options)) if options.contains(&line) => OptionStatus::Set,
    Ok(_) => OptionStatus::NotSet,
    Err(reason) => OptionStatus::Unknown(reason),
  };
  let now_set = found == OptionStatus::Set && steam.needs_attention();

  steam.found = Some(found);
  steam.checking = false;
  steam.line = Some(line);

  if !now_set {
    return Vec::new();
  }

  vec![Effect::Notify(Notice::SteamOptionSet(read.game))]
}

/// Reads Steam's launch options again when the window comes back into focus
/// and the last read found them without the line, since the user may have
/// left to paste it into Steam.
pub(crate) fn window_focused(state: &mut State) -> Vec<Effect> {
  let waiting = state
    .launcher
    .as_ref()
    .and_then(|launcher| launcher.steam.as_ref())
    .is_some_and(|steam| !steam.checking && steam.found == Some(OptionStatus::NotSet));

  if !waiting {
    return Vec::new();
  }

  check_steam_option(state)
}

/// Reads Steam's launch options for the open game again, when it has a line
/// to set, keeping what the last read found until this one ends.
pub(crate) fn check_steam_option(state: &mut State) -> Vec<Effect> {
  let Some(launcher) = state
    .launcher
    .as_ref()
    .filter(|launcher| launcher.steam.is_some())
  else {
    return Vec::new();
  };
  let game = launcher.game.clone();
  let Some(app_id) = steam_app_id(&state.ecosystem, &game) else {
    return Vec::new();
  };
  let Some(context) = context(state, &game, LaunchMode::Vanilla) else {
    return Vec::new();
  };

  if let Some(steam) = state
    .launcher
    .as_mut()
    .and_then(|launcher| launcher.steam.as_mut())
  {
    steam.checking = true;
  }

  vec![Effect::Run(Command::ReadSteamOption {
    game,
    app_id,
    context,
    steam: state.settings.steam_folder.clone(),
  })]
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::install::{DEFAULT_PROFILE, Installed};
  use crate::settings::{GameSettings, RuntimeChoice, Settings};
  use crate::{Msg, update};
  use thunderstore_engine::ecosystem::Ecosystem;
  use thunderstore_engine::profile::launch::HostOs;

  const VALHEIM: u32 = 892970;
  const LETHAL: u32 = 1966720;
  const LINE: &str = r#""/data/valheim/steam-wrapper.sh" %command%"#;

  /// Valheim as a BepInEx game on Steam with Linux and Windows builds, a
  /// BepInEx game on Steam with only a Windows build, a BepInEx game with no
  /// Steam app id, and a game with no mod loader.
  fn ecosystem() -> Ecosystem {
    Ecosystem::from_json(
      r#"{
        "games": {
          "valheim": {
            "label": "valheim",
            "meta": { "displayName": "Valheim" },
            "r2modman": [{
              "gameInstanceType": "game",
              "packageLoader": "bepinex",
              "exeNames": ["valheim.exe", "valheim.x86_64"],
              "distributions": [{ "platform": "steam", "identifier": "892970" }],
              "packageIndex": "https://thunderstore.io/c/valheim/api/v1/package/",
              "installRules": [{ "route": "BepInEx/plugins", "isDefaultLocation": true }]
            }]
          },
          "lethal": {
            "label": "lethal",
            "meta": { "displayName": "Lethal Company" },
            "r2modman": [{
              "gameInstanceType": "game",
              "packageLoader": "bepinex",
              "exeNames": ["Lethal Company.exe"],
              "distributions": [{ "platform": "steam", "identifier": "1966720" }],
              "packageIndex": "https://thunderstore.io/c/lethal-company/api/v1/package/",
              "installRules": [{ "route": "BepInEx/plugins", "isDefaultLocation": true }]
            }]
          },
          "elsewhere": {
            "label": "elsewhere",
            "meta": { "displayName": "Elsewhere" },
            "r2modman": [{
              "gameInstanceType": "game",
              "packageLoader": "bepinex",
              "exeNames": ["elsewhere.x86_64"],
              "packageIndex": "https://thunderstore.io/c/elsewhere/api/v1/package/",
              "installRules": [{ "route": "BepInEx/plugins", "isDefaultLocation": true }]
            }]
          },
          "plain": {
            "label": "plain",
            "meta": { "displayName": "Plain" },
            "r2modman": [{
              "gameInstanceType": "game",
              "packageLoader": "none",
              "exeNames": ["plain.x86_64"],
              "distributions": [{ "platform": "steam", "identifier": "7" }],
              "packageIndex": "https://thunderstore.io/c/plain/api/v1/package/"
            }]
          }
        },
        "modloaderPackages": [
          { "packageId": "denikson-BepInExPack_Valheim", "rootFolder": "BepInExPack_Valheim", "loader": "bepinex" }
        ]
      }"#,
    )
    .expect("launch ecosystem parses")
  }

  /// A Linux state that started with `game` open, its cache and profile not
  /// yet read and its folder holding its native build, and the effects of
  /// opening it.
  fn opened(game: &str) -> (State, Vec<Effect>) {
    opened_holding(game, &[&format!("{game}.x86_64")])
  }

  /// As [`opened`], with the game's folder holding `entries`.
  fn opened_holding(game: &str, entries: &[&str]) -> (State, Vec<Effect>) {
    let (mut state, mut effects) = started(game);
    let listed = Msg::GameFolderListed {
      game: game.to_string(),
      entries: Ok(entries.iter().map(|entry| entry.to_string()).collect()),
    };

    effects.extend(update(&mut state, listed));

    (state, effects)
  }

  /// A Linux state that started with `game` open and its folder not yet
  /// listed, and the effects of opening it.
  fn started(game: &str) -> (State, Vec<Effect>) {
    started_on(HostOs::Linux, game)
  }

  /// As [`started`], on `host`.
  fn started_on(host: HostOs, game: &str) -> (State, Vec<Effect>) {
    started_choosing(host, game, RuntimeChoice::Detect)
  }

  /// As [`started_on`], with the game set to run as `choice`.
  fn started_choosing(host: HostOs, game: &str, choice: RuntimeChoice) -> (State, Vec<Effect>) {
    let mut state = State::new(ecosystem());
    let settings = Settings {
      last_game: Some(game.to_string()),
      games: [(
        game.to_string(),
        GameSettings {
          runtime: choice,
          ..GameSettings::new(PathBuf::from("/games").join(game))
        },
      )]
      .into(),
      ..Settings::default()
    };

    state.host = Some(host);

    update(&mut state, Msg::Started);

    let effects = update(&mut state, Msg::SettingsLoaded(Ok(settings)));

    (state, effects)
  }

  fn read_steam_option(game: &str, app_id: u32) -> Effect {
    read_steam_option_under(game, app_id, Runtime::Native)
  }

  fn read_steam_option_under(game: &str, app_id: u32, runtime: Runtime) -> Effect {
    Effect::Run(Command::ReadSteamOption {
      game: game.to_string(),
      app_id,
      context: LaunchContext::new(
        HostOs::Linux,
        StorePlatform::Steam,
        runtime,
        LaunchMode::Vanilla,
      ),
      steam: None,
    })
  }

  #[test]
  fn steams_launch_options_are_read_from_the_steam_folder_chosen_in_settings() {
    let (mut state, _) = opened("valheim");

    update(&mut state, read("valheim", Ok(None)));

    state.settings.steam_folder = Some(PathBuf::from("/mnt/Steam"));

    let effects = update(&mut state, Msg::SteamOptionCheckPressed);

    assert_eq!(
      effects,
      vec![Effect::Run(Command::ReadSteamOption {
        game: "valheim".to_string(),
        app_id: VALHEIM,
        context: context(StorePlatform::Steam, LaunchMode::Vanilla),
        steam: Some(PathBuf::from("/mnt/Steam")),
      })]
    );
  }

  fn runtime(state: &State) -> Option<Runtime> {
    state
      .launcher
      .as_ref()
      .and_then(|launcher| launcher.runtime)
  }

  #[test]
  fn opening_a_game_on_linux_lists_its_folder_before_reading_steam() {
    let (state, effects) = started("valheim");

    assert!(
      effects.contains(&Effect::Run(Command::ListGameFolder {
        game: "valheim".to_string(),
        folder: PathBuf::from("/games/valheim"),
      })),
      "{effects:?}"
    );
    assert!(
      !effects
        .iter()
        .any(|effect| matches!(effect, Effect::Run(Command::ReadSteamOption { .. }))),
      "{effects:?}"
    );
    assert_eq!(runtime(&state), None);
  }

  #[test]
  fn a_folder_holding_only_the_windows_build_runs_under_proton() {
    let (state, effects) = opened_holding("lethal", &["Lethal Company.exe", "MonoBleedingEdge"]);

    assert_eq!(runtime(&state), Some(Runtime::Proton));
    assert!(
      effects.contains(&read_steam_option_under("lethal", LETHAL, Runtime::Proton)),
      "{effects:?}"
    );
    assert_eq!(
      steam_option(&state),
      Some(SteamOption {
        line: None,
        found: None,
        checking: true,
      })
    );
  }

  #[test]
  fn a_folder_holding_the_linux_build_runs_natively() {
    for entries in [
      &["valheim.x86_64"][..],
      &["valheim.x86_64", "valheim.exe"],
      &["Valheim.X86_64"],
    ] {
      let (state, _) = opened_holding("valheim", entries);

      assert_eq!(runtime(&state), Some(Runtime::Native), "{entries:?}");
    }
  }

  #[test]
  fn a_folder_that_cannot_be_listed_runs_natively() {
    let (mut state, _) = started("lethal");

    update(
      &mut state,
      Msg::GameFolderListed {
        game: "lethal".to_string(),
        entries: Err("permission denied".to_string()),
      },
    );

    assert_eq!(runtime(&state), Some(Runtime::Native));
  }

  #[test]
  fn a_listing_for_a_game_no_longer_open_is_ignored() {
    let (mut state, _) = started("valheim");
    let effects = update(
      &mut state,
      Msg::GameFolderListed {
        game: "lethal".to_string(),
        entries: Ok(vec!["Lethal Company.exe".to_string()]),
      },
    );

    assert_eq!(effects, vec![]);
    assert_eq!(runtime(&state), None);
  }

  #[test]
  fn a_windows_host_runs_games_natively_without_listing_their_folder() {
    let (state, effects) = started_on(HostOs::Windows, "lethal");

    assert!(
      !effects
        .iter()
        .any(|effect| matches!(effect, Effect::Run(Command::ListGameFolder { .. }))),
      "{effects:?}"
    );
    assert_eq!(runtime(&state), Some(Runtime::Native));
    assert_eq!(steam_option(&state), None);
  }

  fn lists_game_folder(effects: &[Effect]) -> bool {
    effects
      .iter()
      .any(|effect| matches!(effect, Effect::Run(Command::ListGameFolder { .. })))
  }

  #[test]
  fn a_game_set_to_proton_runs_under_proton_without_listing_its_folder() {
    let (state, effects) = started_choosing(HostOs::Linux, "valheim", RuntimeChoice::Proton);

    assert!(!lists_game_folder(&effects), "{effects:?}");
    assert_eq!(runtime(&state), Some(Runtime::Proton));
    assert!(
      effects.contains(&read_steam_option_under(
        "valheim",
        VALHEIM,
        Runtime::Proton
      )),
      "{effects:?}"
    );
  }

  #[test]
  fn a_windows_only_game_set_to_native_runs_natively_without_listing_its_folder() {
    let (state, effects) = started_choosing(HostOs::Linux, "lethal", RuntimeChoice::Native);

    assert!(!lists_game_folder(&effects), "{effects:?}");
    assert_eq!(runtime(&state), Some(Runtime::Native));
    assert!(
      effects.contains(&read_steam_option_under("lethal", LETHAL, Runtime::Native)),
      "{effects:?}"
    );
  }

  #[test]
  fn a_game_set_to_proton_runs_natively_off_linux() {
    let (state, _) = started_choosing(HostOs::Windows, "lethal", RuntimeChoice::Proton);

    assert_eq!(runtime(&state), Some(Runtime::Native));
  }

  #[test]
  fn a_launch_waits_until_the_folder_shows_how_the_game_runs() {
    let (mut state, _) = started("lethal");

    update(&mut state, profile_opened("lethal", &[]));

    assert!(!state.can_launch());
    assert_eq!(
      update(&mut state, Msg::LaunchPressed(LaunchMode::Modded)),
      vec![]
    );

    update(
      &mut state,
      Msg::GameFolderListed {
        game: "lethal".to_string(),
        entries: Ok(vec!["Lethal Company.exe".to_string()]),
      },
    );

    assert!(state.can_launch());
  }

  #[test]
  fn a_proton_game_launches_through_steam_under_proton() {
    let (mut state, _) = opened_holding("lethal", &["Lethal Company.exe"]);

    update(&mut state, profile_opened("lethal", &[]));

    assert_eq!(
      update(&mut state, Msg::LaunchPressed(LaunchMode::Modded)),
      vec![launch(
        1,
        "lethal",
        LaunchContext::new(
          HostOs::Linux,
          StorePlatform::Steam,
          Runtime::Proton,
          LaunchMode::Modded,
        ),
      )]
    );
  }

  #[test]
  fn a_read_with_no_line_to_set_drops_the_launch_option() {
    let (mut state, _) = opened("valheim");

    update(
      &mut state,
      Msg::SteamOptionRead(SteamOptionRead {
        game: "valheim".to_string(),
        runtime: Runtime::Native,
        line: None,
        current: Ok(None),
      }),
    );

    assert_eq!(steam_option(&state), None);
  }

  #[test]
  fn opening_a_game_steam_runs_through_a_wrapper_reads_its_launch_option() {
    let (state, effects) = opened("valheim");

    assert!(
      effects.contains(&read_steam_option("valheim", VALHEIM)),
      "{effects:?}"
    );
    assert_eq!(
      state.launcher.and_then(|launcher| launcher.steam),
      Some(SteamOption {
        line: None,
        found: None,
        checking: true,
      })
    );
  }

  #[test]
  fn a_game_without_a_steam_wrapper_has_no_launch_option_to_set() {
    for game in ["plain", "elsewhere"] {
      let (state, effects) = opened(game);

      assert!(
        !effects
          .iter()
          .any(|effect| matches!(effect, Effect::Run(Command::ReadSteamOption { .. }))),
        "{game}: {effects:?}"
      );
      assert_eq!(state.launcher.unwrap().steam, None, "{game}");
    }
  }

  fn read(game: &str, current: Result<Option<&str>, &str>) -> Msg {
    Msg::SteamOptionRead(SteamOptionRead {
      game: game.to_string(),
      runtime: Runtime::Native,
      line: Some(LINE.to_string()),
      current: current
        .map(|options| options.map(str::to_string))
        .map_err(str::to_string),
    })
  }

  fn steam_option(state: &State) -> Option<SteamOption> {
    state
      .launcher
      .as_ref()
      .and_then(|launcher| launcher.steam.clone())
  }

  /// The line, worked out, with what the last read found and no read
  /// running.
  fn shown(found: OptionStatus) -> Option<SteamOption> {
    Some(SteamOption {
      line: Some(LINE.to_string()),
      found: Some(found),
      checking: false,
    })
  }

  #[test]
  fn a_read_for_the_runtime_left_behind_landing_last_is_ignored() {
    let proton = r#"WINEDLLOVERRIDES="winhttp=n,b" %command%"#;
    let (mut state, _) = opened("valheim");

    update(&mut state, Msg::SettingsPressed);

    let effects = update(&mut state, Msg::RuntimeChosen(RuntimeChoice::Proton));

    assert!(
      effects.contains(&read_steam_option_under(
        "valheim",
        VALHEIM,
        Runtime::Proton
      )),
      "{effects:?}"
    );

    update(
      &mut state,
      Msg::SteamOptionRead(SteamOptionRead {
        game: "valheim".to_string(),
        runtime: Runtime::Proton,
        line: Some(proton.to_string()),
        current: Ok(Some(proton.to_string())),
      }),
    );

    assert_eq!(update(&mut state, read("valheim", Ok(None))), vec![]);
    assert_eq!(runtime(&state), Some(Runtime::Proton));
    assert_eq!(
      steam_option(&state),
      Some(SteamOption {
        line: Some(proton.to_string()),
        found: Some(OptionStatus::Set),
        checking: false,
      })
    );
  }

  /// As [`shown`], with another read running.
  fn rechecking(found: OptionStatus) -> Option<SteamOption> {
    shown(found).map(|steam| SteamOption {
      checking: true,
      ..steam
    })
  }

  #[test]
  fn the_launch_option_is_set_when_steams_options_hold_the_line() {
    let (mut state, _) = opened("valheim");
    let options = format!("PULSE_LATENCY_MSEC=120 {LINE}");

    update(&mut state, read("valheim", Ok(Some(&options))));

    assert_eq!(steam_option(&state), shown(OptionStatus::Set));
  }

  #[test]
  fn the_launch_option_is_not_set_when_steams_options_lack_the_line() {
    for current in [None, Some("%command% -console")] {
      let (mut state, _) = opened("valheim");

      update(&mut state, read("valheim", Ok(current)));

      assert_eq!(steam_option(&state), shown(OptionStatus::NotSet));
    }
  }

  #[test]
  fn unreadable_steam_settings_still_show_the_line_and_say_why() {
    let (mut state, _) = opened("valheim");

    update(&mut state, read("valheim", Err("Steam was not found")));

    assert_eq!(
      steam_option(&state),
      shown(OptionStatus::Unknown("Steam was not found".to_string()))
    );
  }

  #[test]
  fn a_launch_option_read_for_another_game_is_ignored() {
    let (mut state, _) = opened("valheim");

    update(&mut state, read("elsewhere", Ok(Some(LINE))));

    assert_eq!(
      steam_option(&state),
      Some(SteamOption {
        line: None,
        found: None,
        checking: true,
      })
    );
  }

  #[test]
  fn checking_again_rereads_steams_options_keeping_the_line() {
    let (mut state, _) = opened("valheim");

    update(&mut state, read("valheim", Ok(None)));

    let effects = update(&mut state, Msg::SteamOptionCheckPressed);

    assert_eq!(effects, vec![read_steam_option("valheim", VALHEIM)]);
    assert_eq!(steam_option(&state), rechecking(OptionStatus::NotSet));
  }

  fn needs_attention(state: &State) -> bool {
    steam_option(state).is_some_and(|steam| steam.needs_attention())
  }

  #[test]
  fn the_line_needs_attention_only_when_steam_lacks_it_or_cannot_be_read() {
    let (mut state, _) = opened("valheim");

    assert!(!needs_attention(&state), "before the first read ends");

    for (current, attention) in [
      (Ok(Some(LINE)), false),
      (Ok(None), true),
      (Err("Steam was not found"), true),
    ] {
      update(&mut state, read("valheim", current));

      assert_eq!(needs_attention(&state), attention, "{current:?}");

      update(&mut state, Msg::SteamOptionCheckPressed);

      assert_eq!(
        needs_attention(&state),
        attention,
        "{current:?}, checking again"
      );
    }
  }

  fn steam_set_up() -> Effect {
    Effect::Notify(Notice::SteamOptionSet("valheim".to_string()))
  }

  #[test]
  fn a_line_found_set_after_being_found_missing_says_steam_is_set_up() {
    for missing in [Ok(None), Err("Steam was not found")] {
      let (mut state, _) = opened("valheim");

      update(&mut state, read("valheim", missing));
      update(&mut state, Msg::SteamOptionCheckPressed);

      assert_eq!(
        update(&mut state, read("valheim", Ok(Some(LINE)))),
        vec![steam_set_up()],
        "{missing:?}"
      );
    }
  }

  #[test]
  fn a_line_already_set_is_not_announced() {
    let (mut state, _) = opened("valheim");

    assert_eq!(update(&mut state, read("valheim", Ok(Some(LINE)))), vec![]);

    update(&mut state, Msg::SteamOptionCheckPressed);

    assert_eq!(update(&mut state, read("valheim", Ok(Some(LINE)))), vec![]);
  }

  #[test]
  fn regaining_focus_checks_again_only_while_steam_lacks_the_line() {
    let (mut state, _) = opened("valheim");

    assert_eq!(
      update(&mut state, Msg::WindowFocused),
      vec![],
      "while checking"
    );

    update(&mut state, read("valheim", Ok(None)));

    assert_eq!(
      update(&mut state, Msg::WindowFocused),
      vec![read_steam_option("valheim", VALHEIM)]
    );
    assert_eq!(
      update(&mut state, Msg::WindowFocused),
      vec![],
      "while checking again"
    );

    update(&mut state, read("valheim", Ok(Some(LINE))));

    assert_eq!(update(&mut state, Msg::WindowFocused), vec![], "once set");

    update(&mut state, Msg::SteamOptionCheckPressed);
    update(&mut state, read("valheim", Err("Steam was not found")));

    assert_eq!(
      update(&mut state, Msg::WindowFocused),
      vec![],
      "when unreadable"
    );
  }

  /// A Linux state with `game` open and its profile holding `packages`.
  fn ready(game: &str, packages: &[&str]) -> State {
    let (mut state, _) = opened(game);

    update(&mut state, profile_opened(game, packages));

    state
  }

  /// `game`'s default profile opened, holding `packages`.
  fn profile_opened(game: &str, packages: &[&str]) -> Msg {
    let packages: Vec<(&str, &str)> = packages.iter().map(|name| (*name, "1.0.0")).collect();
    let installed: Installed = crate::test_installed(&packages);

    Msg::ProfileOpened {
      game: game.to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: Ok(installed),
    }
  }

  fn context(store: StorePlatform, mode: LaunchMode) -> LaunchContext {
    LaunchContext::new(HostOs::Linux, store, Runtime::Native, mode)
  }

  fn launch(id: u64, game: &str, context: LaunchContext) -> Effect {
    Effect::Run(Command::Launch(Launch {
      id,
      game: game.to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      folder: PathBuf::from("/games").join(game),
      context,
    }))
  }

  fn attempt(state: &State) -> Option<Attempt> {
    state.launch.clone()
  }

  #[test]
  fn launching_modded_starts_the_game_through_steam() {
    let mut state = ready("valheim", &["denikson-BepInExPack_Valheim"]);

    let effects = update(&mut state, Msg::LaunchPressed(LaunchMode::Modded));

    assert_eq!(
      effects,
      vec![launch(
        1,
        "valheim",
        context(StorePlatform::Steam, LaunchMode::Modded)
      )]
    );
    assert_eq!(
      attempt(&state),
      Some(Attempt {
        id: 1,
        game: "valheim".to_string(),
        mode: LaunchMode::Modded,
        status: AttemptStatus::Preparing,
      })
    );
  }

  #[test]
  fn launching_vanilla_starts_the_game_and_leaves_the_profile_as_it_was() {
    let mut state = ready("valheim", &["denikson-BepInExPack_Valheim"]);
    let profile = state.profile.clone();

    let effects = update(&mut state, Msg::LaunchPressed(LaunchMode::Vanilla));

    update(
      &mut state,
      Msg::LaunchFinished {
        launch: 1,
        result: Ok(()),
      },
    );

    assert_eq!(
      effects,
      vec![launch(
        1,
        "valheim",
        context(StorePlatform::Steam, LaunchMode::Vanilla)
      )]
    );
    assert_eq!(state.profile, profile);
    assert_eq!(
      attempt(&state).map(|attempt| attempt.status),
      Some(AttemptStatus::Started)
    );
  }

  #[test]
  fn modded_and_vanilla_launches_pass_the_games_launch_arguments() {
    for mode in [LaunchMode::Modded, LaunchMode::Vanilla] {
      let mut state = ready("valheim", &["denikson-BepInExPack_Valheim"]);

      if let Some(valheim) = state.settings.games.get_mut("valheim") {
        valheim.launch_arguments = r#"-console -log "My Logs""#.to_string();
      }

      let effects = update(&mut state, Msg::LaunchPressed(mode));
      let arguments = vec![
        "-console".to_string(),
        "-log".to_string(),
        "My Logs".to_string(),
      ];

      assert_eq!(
        effects,
        vec![launch(
          1,
          "valheim",
          context(StorePlatform::Steam, mode).with_extra_args(arguments)
        )],
        "{mode:?}"
      );
    }
  }

  #[test]
  fn a_game_without_a_steam_app_id_is_started_directly() {
    let mut state = ready("elsewhere", &[]);

    let effects = update(&mut state, Msg::LaunchPressed(LaunchMode::Modded));

    assert_eq!(
      effects,
      vec![launch(
        1,
        "elsewhere",
        context(StorePlatform::Other, LaunchMode::Modded)
      )]
    );
  }

  #[test]
  fn a_launch_waits_for_the_profile_to_open() {
    let (mut state, _) = opened("valheim");

    let effects = update(&mut state, Msg::LaunchPressed(LaunchMode::Vanilla));

    assert_eq!(effects, vec![]);
    assert_eq!(attempt(&state), None);
  }

  #[test]
  fn a_second_launch_waits_for_the_first_to_end() {
    let mut state = ready("valheim", &[]);

    update(&mut state, Msg::LaunchPressed(LaunchMode::Vanilla));

    let effects = update(&mut state, Msg::LaunchPressed(LaunchMode::Modded));

    assert_eq!(effects, vec![]);
    assert_eq!(attempt(&state).map(|attempt| attempt.id), Some(1));
  }

  /// Reads a package index holding `Owner-Mod` into `state`'s open valheim.
  fn load_index(state: &mut State) {
    let packages: Vec<thunderstore_engine::models::Package> =
      serde_json::from_value(serde_json::json!([{
        "name": "Mod",
        "full_name": "Owner-Mod",
        "owner": "Owner",
        "date_created": "2024-01-01T00:00:00Z",
        "date_updated": "2024-01-01T00:00:00Z",
        "categories": [],
        "versions": [{
          "version_number": "1.0.0",
          "dependencies": [],
          "download_url": "https://example.com/Owner-Mod-1.0.0.zip",
          "date_created": "2024-01-01T00:00:00Z"
        }]
      }]))
      .unwrap();

    update(
      state,
      Msg::CachedIndexRead {
        community: "valheim".to_string(),
        index: Ok(Some(
          thunderstore_engine::models::PackageIndex::from(packages).into(),
        )),
      },
    );
  }

  #[test]
  fn a_launch_waits_for_a_running_install() {
    let mut state = ready("valheim", &[]);

    load_index(&mut state);
    update(&mut state, Msg::InstallPressed("Owner-Mod".to_string()));
    update(&mut state, Msg::InstallConfirmed);

    let effects = update(&mut state, Msg::LaunchPressed(LaunchMode::Vanilla));

    assert!(state.is_busy());
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn an_install_waits_for_a_preparing_launch() {
    let mut state = ready("valheim", &[]);

    load_index(&mut state);
    update(&mut state, Msg::LaunchPressed(LaunchMode::Modded));
    update(&mut state, Msg::InstallPressed("Owner-Mod".to_string()));

    assert_eq!(state.confirmation, None);
  }

  #[test]
  fn an_install_confirmed_after_a_launch_was_pressed_waits_for_it() {
    let mut state = ready("valheim", &[]);

    load_index(&mut state);
    update(&mut state, Msg::InstallPressed("Owner-Mod".to_string()));
    update(&mut state, Msg::LaunchPressed(LaunchMode::Modded));

    let effects = update(&mut state, Msg::InstallConfirmed);

    assert_eq!(effects, vec![]);
    assert!(state.queue.operations.is_empty());
  }

  #[test]
  fn a_preparing_launch_survives_leaving_and_reopening_its_game() {
    let mut state = ready("valheim", &[]);

    load_index(&mut state);
    update(&mut state, Msg::LaunchPressed(LaunchMode::Vanilla));
    update(&mut state, Msg::SwitchGamePressed);
    update(&mut state, Msg::GamePicked("valheim".to_string()));
    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(Installed::new()),
      },
    );
    load_index(&mut state);

    let relaunch = update(&mut state, Msg::LaunchPressed(LaunchMode::Modded));

    update(&mut state, Msg::InstallPressed("Owner-Mod".to_string()));

    assert_eq!(relaunch, vec![]);
    assert_eq!(state.confirmation, None);

    update(
      &mut state,
      Msg::LaunchFinished {
        launch: 1,
        result: Ok(()),
      },
    );

    assert_eq!(
      attempt(&state).map(|attempt| (attempt.id, attempt.status)),
      Some((1, AttemptStatus::Started))
    );
  }

  #[test]
  fn a_host_that_cannot_launch_ignores_the_buttons() {
    let mut state = ready("valheim", &[]);

    state.host = None;

    let effects = update(&mut state, Msg::LaunchPressed(LaunchMode::Vanilla));

    assert_eq!(effects, vec![]);
  }

  #[test]
  fn a_modded_launch_without_a_mod_loader_fails_naming_the_loader() {
    let mut state = ready("valheim", &[]);

    update(&mut state, Msg::LaunchPressed(LaunchMode::Modded));
    update(
      &mut state,
      Msg::LaunchFinished {
        launch: 1,
        result: Err(LaunchFailure::NoModLoader(LoaderKind::BepInEx)),
      },
    );

    assert_eq!(
      attempt(&state),
      Some(Attempt {
        id: 1,
        game: "valheim".to_string(),
        mode: LaunchMode::Modded,
        status: AttemptStatus::Failed(LaunchFailure::NoModLoader(LoaderKind::BepInEx)),
      })
    );
  }

  #[test]
  fn a_finished_launch_checks_steams_launch_option_again() {
    let mut state = ready("valheim", &["denikson-BepInExPack_Valheim"]);

    update(&mut state, read("valheim", Ok(None)));
    update(&mut state, Msg::LaunchPressed(LaunchMode::Modded));

    let effects = update(
      &mut state,
      Msg::LaunchFinished {
        launch: 1,
        result: Ok(()),
      },
    );

    assert_eq!(
      effects,
      vec![
        Effect::Notify(Notice::Launched {
          game: "valheim".to_string(),
          mode: LaunchMode::Modded,
        }),
        read_steam_option("valheim", VALHEIM),
      ]
    );
    assert_eq!(steam_option(&state), rechecking(OptionStatus::NotSet));
  }

  #[test]
  fn a_failed_launch_is_told_as_an_error() {
    let mut state = ready("valheim", &[]);

    update(&mut state, Msg::LaunchPressed(LaunchMode::Modded));

    let effects = update(
      &mut state,
      Msg::LaunchFinished {
        launch: 1,
        result: Err(LaunchFailure::NoModLoader(LoaderKind::BepInEx)),
      },
    );
    let told = Notice::LaunchFailed {
      game: "valheim".to_string(),
      mode: LaunchMode::Modded,
      failure: LaunchFailure::NoModLoader(LoaderKind::BepInEx),
    };

    assert_eq!(told.level(), crate::notice::Level::Error);
    assert_eq!(effects.first(), Some(&Effect::Notify(told)));
  }

  #[test]
  fn copying_the_launch_option_line_puts_it_on_the_clipboard_and_says_so() {
    let (mut state, _) = opened("valheim");

    assert_eq!(update(&mut state, Msg::LaunchLineCopyPressed), vec![]);

    update(&mut state, read("valheim", Ok(None)));

    assert_eq!(
      update(&mut state, Msg::LaunchLineCopyPressed),
      vec![
        Effect::CopyToClipboard(LINE.to_string()),
        Effect::Notify(Notice::LaunchLineCopied),
      ]
    );
  }

  #[test]
  fn checking_again_for_a_game_without_a_wrapper_does_nothing() {
    let (mut state, _) = opened("plain");

    let effects = update(&mut state, Msg::SteamOptionCheckPressed);

    assert_eq!(effects, vec![]);
  }
}
