//! Launching the open game modded or vanilla, the Steam launch option a native
//! Linux game needs before a modded launch loads packages, and whether the
//! profile has the mod loader those packages need.

use crate::install::Profile;
use crate::notice::Notice;
use crate::{Command, Effect, Screen, State};
use std::path::{Path, PathBuf};
use thunderstore_engine::Error;
use thunderstore_engine::ecosystem::{Ecosystem, LoaderKind};
use thunderstore_engine::profile::launch::{
  LaunchContext, LaunchFacts, LaunchMode, Runtime, StorePlatform, launch_plan_from,
  prepare_plan_from,
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
  /// The line Steam's launch options must hold, for a game launched through
  /// Steam's wrapper. `None` when the game needs no wrapper.
  pub steam: Option<SteamOption>,
}

/// The line to paste into the game's Steam launch options, and whether it is
/// there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamOption {
  /// `None` until it has been worked out.
  pub line: Option<String>,
  pub status: OptionStatus,
}

/// Whether Steam's launch options for the game hold the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionStatus {
  Checking,
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
  /// The line the launch options must hold.
  pub line: String,
  /// The launch options Steam holds, `None` when there are none, or why they
  /// could not be read.
  pub current: Result<Option<String>, String>,
}

/// Sets up launching for `game`, reading its Steam launch option when Steam
/// runs it through a wrapper.
pub(crate) fn open(state: &mut State, game: &str) -> Vec<Effect> {
  let app_id = steam_wrapped(state, game);

  state.launcher = Some(Launcher {
    game: game.to_string(),
    steam: app_id.map(|_| SteamOption {
      line: None,
      status: OptionStatus::Checking,
    }),
  });

  app_id
    .map(|app_id| {
      Effect::Run(Command::ReadSteamOption {
        game: game.to_string(),
        app_id,
      })
    })
    .into_iter()
    .collect()
}

/// How `game` launches in `mode` on this host: as a native game, through
/// Steam when it has a Steam app id and directly otherwise. `None` where
/// launching is not supported.
fn context(state: &State, game: &str, mode: LaunchMode) -> Option<LaunchContext> {
  let host = state.host?;
  let store = match steam_app_id(&state.ecosystem, game) {
    Some(_) => StorePlatform::Steam,
    None => StorePlatform::Other,
  };

  Some(LaunchContext::new(host, store, Runtime::Native, mode))
}

fn steam_app_id(ecosystem: &Ecosystem, game: &str) -> Option<u32> {
  ecosystem
    .require_profile(game)
    .ok()
    .and_then(|profile| profile.steam_app_id())
}

/// `game`'s Steam app id when Steam must run it through the wrapper, which is
/// what makes a modded launch load packages.
fn steam_wrapped(state: &State, game: &str) -> Option<u32> {
  let context = context(state, game, LaunchMode::Vanilla)?;
  let plan = launch_plan_from(
    Path::new(""),
    &LaunchFacts::default(),
    &state.ecosystem,
    game,
    &context,
  )
  .ok()?;

  plan.steam_wrapper.and(steam_app_id(&state.ecosystem, game))
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
  /// read, no operation on it is running or waiting, no other launch is
  /// being prepared, no uninstall is being checked, and no profile change is
  /// being made.
  pub fn can_launch(&self) -> bool {
    let ready = self.profile.as_ref().and_then(Profile::installed).is_some();

    self.host.is_some()
      && ready
      && !self.is_launching()
      && !self.is_profile_busy()
      && !self.is_checking()
      && !self.is_changing_profile()
  }

  /// The mod loader the open game's packages load through, when the open
  /// profile does not hold it, so a modded launch would load none of them.
  ///
  /// Judged from the packages the profile records, which is how the app
  /// installs a loader. A launch also looks for loader files on disk.
  pub fn missing_loader(&self) -> Option<LoaderKind> {
    let profile = self.profile.as_ref()?;
    let installed = profile.installed()?;
    let context = context(self, &profile.game, LaunchMode::Modded)?;
    let mut facts = LaunchFacts::default();

    facts.recorded = installed.keys().cloned().collect();

    match prepare_plan_from(
      Path::new(""),
      &facts,
      &self.ecosystem,
      &profile.game,
      &context,
    ) {
      Err(Error::NoModLoader { loader }) => Some(loader),
      _ => None,
    }
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
  let launch = Launch {
    id: state.launches + 1,
    game: game.clone(),
    profile: profile.name.clone(),
    folder: folder.to_path_buf(),
    context,
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

/// Applies the result of launch `id`, saying so when it started the game,
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

  let launched = result.is_ok().then(|| Notice::Launched {
    game: attempt.game.clone(),
    mode: attempt.mode,
  });

  attempt.status = match result {
    Ok(()) => AttemptStatus::Started,
    Err(failure) => AttemptStatus::Failed(failure),
  };

  let mut effects: Vec<Effect> = launched.map(Effect::Notify).into_iter().collect();

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
pub(crate) fn steam_option_read(state: &mut State, read: SteamOptionRead) -> Vec<Effect> {
  let Some(steam) = state
    .launcher
    .as_mut()
    .filter(|launcher| launcher.game == read.game)
    .and_then(|launcher| launcher.steam.as_mut())
  else {
    return Vec::new();
  };

  steam.status = match read.current {
    Ok(Some(options)) if options.contains(&read.line) => OptionStatus::Set,
    Ok(_) => OptionStatus::NotSet,
    Err(reason) => OptionStatus::Unknown(reason),
  };
  steam.line = Some(read.line);

  Vec::new()
}

/// Reads Steam's launch options for the open game again, when it has a line
/// to set.
pub(crate) fn check_steam_option(state: &mut State) -> Vec<Effect> {
  let Some(launcher) = state.launcher.as_mut() else {
    return Vec::new();
  };
  let Some(steam) = launcher.steam.as_mut() else {
    return Vec::new();
  };
  let Some(app_id) = steam_app_id(&state.ecosystem, &launcher.game) else {
    return Vec::new();
  };

  steam.status = OptionStatus::Checking;

  vec![Effect::Run(Command::ReadSteamOption {
    game: launcher.game.clone(),
    app_id,
  })]
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::install::{DEFAULT_PROFILE, Installed};
  use crate::settings::{GameSettings, Settings};
  use crate::{Msg, update};
  use thunderstore_engine::ecosystem::Ecosystem;
  use thunderstore_engine::profile::launch::HostOs;

  const VALHEIM: u32 = 892970;
  const LINE: &str = r#""/data/valheim/steam-wrapper.sh" %command%"#;

  /// Valheim as a native BepInEx game on Steam, a BepInEx game with no Steam
  /// app id, and a game with no mod loader.
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
              "exeNames": ["valheim.x86_64"],
              "distributions": [{ "platform": "steam", "identifier": "892970" }],
              "packageIndex": "https://thunderstore.io/c/valheim/api/v1/package/"
            }]
          },
          "elsewhere": {
            "label": "elsewhere",
            "meta": { "displayName": "Elsewhere" },
            "r2modman": [{
              "gameInstanceType": "game",
              "packageLoader": "bepinex",
              "exeNames": ["elsewhere.x86_64"],
              "packageIndex": "https://thunderstore.io/c/elsewhere/api/v1/package/"
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
  /// yet read, and the effects of opening it.
  fn opened(game: &str) -> (State, Vec<Effect>) {
    let mut state = State::new(ecosystem());
    let settings = Settings {
      last_game: Some(game.to_string()),
      games: [(
        game.to_string(),
        GameSettings {
          folder: PathBuf::from("/games").join(game),
          profile: None,
        },
      )]
      .into(),
    };

    state.host = Some(HostOs::Linux);

    update(&mut state, Msg::Started);

    let effects = update(&mut state, Msg::SettingsLoaded(Ok(settings)));

    (state, effects)
  }

  fn read_steam_option(game: &str, app_id: u32) -> Effect {
    Effect::Run(Command::ReadSteamOption {
      game: game.to_string(),
      app_id,
    })
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
        status: OptionStatus::Checking,
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
      line: LINE.to_string(),
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

  fn shown(status: OptionStatus) -> Option<SteamOption> {
    Some(SteamOption {
      line: Some(LINE.to_string()),
      status,
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
        status: OptionStatus::Checking,
      })
    );
  }

  #[test]
  fn checking_again_rereads_steams_options_keeping_the_line() {
    let (mut state, _) = opened("valheim");

    update(&mut state, read("valheim", Ok(None)));

    let effects = update(&mut state, Msg::SteamOptionCheckPressed);

    assert_eq!(effects, vec![read_steam_option("valheim", VALHEIM)]);
    assert_eq!(steam_option(&state), shown(OptionStatus::Checking));
  }

  /// A Linux state with `game` open and its profile holding `packages`.
  fn ready(game: &str, packages: &[&str]) -> State {
    let (mut state, _) = opened(game);
    let packages: Vec<(&str, &str)> = packages.iter().map(|name| (*name, "1.0.0")).collect();
    let installed: Installed = crate::test_installed(&packages);

    update(
      &mut state,
      Msg::ProfileOpened {
        game: game.to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(installed),
      },
    );

    state
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
    assert_eq!(steam_option(&state), shown(OptionStatus::Checking));
  }

  #[test]
  fn a_failed_launch_is_not_announced() {
    let mut state = ready("valheim", &[]);

    update(&mut state, Msg::LaunchPressed(LaunchMode::Modded));

    let effects = update(
      &mut state,
      Msg::LaunchFinished {
        launch: 1,
        result: Err(LaunchFailure::NoModLoader(LoaderKind::BepInEx)),
      },
    );

    assert!(
      !effects
        .iter()
        .any(|effect| matches!(effect, Effect::Notify(_))),
      "{effects:?}"
    );
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
  fn a_profile_without_its_games_mod_loader_names_the_missing_loader() {
    let state = ready("valheim", &["Owner-Mod"]);

    assert_eq!(state.missing_loader(), Some(LoaderKind::BepInEx));
  }

  #[test]
  fn a_profile_holding_its_games_mod_loader_misses_nothing() {
    let state = ready("valheim", &["denikson-BepInExPack_Valheim", "Owner-Mod"]);

    assert_eq!(state.missing_loader(), None);
  }

  #[test]
  fn a_game_that_needs_no_mod_loader_misses_nothing() {
    let state = ready("plain", &[]);

    assert_eq!(state.missing_loader(), None);
  }

  #[test]
  fn a_profile_not_yet_read_misses_nothing() {
    let (state, _) = opened("valheim");

    assert_eq!(state.missing_loader(), None);
  }

  #[test]
  fn checking_again_for_a_game_without_a_wrapper_does_nothing() {
    let (mut state, _) = opened("plain");

    let effects = update(&mut state, Msg::SteamOptionCheckPressed);

    assert_eq!(effects, vec![]);
  }
}
