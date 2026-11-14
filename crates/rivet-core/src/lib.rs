//! The pure core of the app: all state, every message that can change it, and
//! the effects the shell must run. Nothing here does IO.

pub mod browse;
pub mod install;
pub mod launch;
pub mod manage;
pub mod picker;
pub mod queue;
pub mod readme;
pub mod settings;
pub mod switcher;

use browse::{Browse, Fetch, Filters, Index, Refresh, SortKey};
use install::{Confirmation, Install, Installed, Outcome, Profile, Progress};
use launch::{Attempt, Launch, LaunchFailure, Launcher, SteamOptionRead};
use manage::{Folders, Manage, Scope, SetEnabled, Sort, Tab, Uninstall};
use picker::{Fallback, FolderRefusal, NotFound, Picker, Refusal, Step};
use queue::{CloseChoice, Closing, Queue};
use readme::Readme;
use settings::{GameSettings, Settings};
use std::path::PathBuf;
use switcher::{Naming, ProfileChange, Switcher};
use thunderstore_engine::ecosystem::Ecosystem;
use thunderstore_engine::profile::launch::{HostOs, LaunchMode};

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
  /// The open game's package list. `None` outside a game.
  pub browse: Option<Browse>,
  /// How many index fetches have been started, so each gets its own id.
  pub refreshes: u64,
  /// The open game's profile. `None` outside a game.
  pub profile: Option<Profile>,
  /// An install waiting for the user to confirm it.
  pub confirmation: Option<Confirmation>,
  /// Every operation asked for and not dismissed, on every game and profile.
  /// It outlives leaving the game and profile each belongs to.
  pub queue: Queue,
  /// Closing the window while operations are running or waiting.
  pub closing: Option<Closing>,
  /// The operating system games launch on, or `None` where launching is not
  /// supported.
  pub host: Option<HostOs>,
  /// Launching the open game. `None` outside a game.
  pub launcher: Option<Launcher>,
  /// The latest launch, kept after it ends so its result can be shown. One
  /// still being prepared outlives leaving the game it belongs to.
  pub launch: Option<Attempt>,
  /// How many launches have been started, so each gets its own id.
  pub launches: u64,
  /// The open game's profiles. `None` outside a game.
  pub switcher: Option<Switcher>,
  /// The tab shown in a game.
  pub tab: Tab,
  /// The Installed tab.
  pub manage: Manage,
  /// The readme of the package selected in the shown tab. `None` when no
  /// package is selected.
  pub readme: Option<Readme>,
  /// How many readme fetches have been started, so each gets its own id.
  pub readmes: u64,
}

impl State {
  /// A state that has not loaded its settings yet, launching games on the
  /// operating system it was built for.
  pub fn new(ecosystem: Ecosystem) -> Self {
    Self {
      host: HostOs::detect(),
      launcher: None,
      launch: None,
      launches: 0,
      switcher: None,
      ecosystem,
      settings: Settings::default(),
      screen: Screen::Starting,
      picker: Picker::default(),
      settings_error: None,
      browse: None,
      refreshes: 0,
      profile: None,
      confirmation: None,
      queue: Queue::default(),
      closing: None,
      tab: Tab::default(),
      manage: Manage::default(),
      readme: None,
      readmes: 0,
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
  /// Steam's libraries were searched for a game: the folder it is installed
  /// in, or why none was found.
  SteamSearched {
    game: String,
    folder: Result<PathBuf, NotFound>,
  },
  /// The folder dialog closed, with `None` if it was cancelled.
  FolderChosen(Option<PathBuf>),
  /// A folder was listed: the names of its entries, or why it could not be.
  FolderListed {
    folder: PathBuf,
    entries: Result<Vec<String>, String>,
  },
  SwitchGamePressed,
  /// A community's cached package index was read: `None` when nothing is
  /// cached, or why the cache could not be read.
  CachedIndexRead {
    community: String,
    index: Result<Option<Index>, String>,
  },
  /// A [`Refresh`] finished: the fetched package index, `None` when the
  /// cached one is current, or why it could not be fetched.
  IndexRefreshed {
    refresh: Refresh,
    index: Result<Option<Index>, String>,
  },
  RefreshPressed,
  /// The package list's search text changed.
  PackageSearchChanged(String),
  /// A sort of the package list was pressed. Pressing the current one
  /// reverses it, except best match.
  SortPressed(SortKey),
  /// A category of the package list was chosen, or unchosen if it was.
  CategoryToggled(String),
  /// NSFW packages were turned on or off in the package list.
  ShowNsfw(bool),
  /// Deprecated packages were turned on or off in the package list.
  ShowDeprecated(bool),
  /// The package list's search and filters were reset.
  FiltersCleared,
  /// A game's profile was opened: its packages, or why it could not be
  /// created or read.
  ProfileOpened {
    game: String,
    profile: String,
    installed: Result<Installed, String>,
  },
  /// Install was pressed on a package, by `Owner-Name`.
  InstallPressed(String),
  InstallConfirmed,
  InstallDismissed,
  /// An [`Install`] moved on, by operation id.
  InstallProgress {
    operation: u64,
    progress: Progress,
  },
  /// An operation ended, by its id: what it did, or why it could not run at
  /// all.
  OperationFinished {
    operation: u64,
    outcome: Result<Outcome, String>,
  },
  /// Cancel was pressed on an operation, by its id.
  OperationCancelPressed(u64),
  /// Retry was pressed on an operation that failed or was cancelled, by its
  /// id.
  RetryPressed(u64),
  /// An operation that ended was removed from the queue, by its id.
  OperationDismissed(u64),
  /// Every operation that ended was removed from the queue.
  QueueCleared,
  /// The user asked to close the window.
  CloseRequested,
  /// The user answered how to close the window while work is in progress.
  CloseChosen(CloseChoice),
  /// The user chose to keep the window open.
  CloseDismissed,
  TabChosen(Tab),
  /// The Installed tab's search text changed.
  InstalledSearchChanged(String),
  InstalledSortChosen(Sort),
  /// Enable was pressed on an installed package, by `Owner-Name`.
  EnablePressed(String),
  /// Disable was pressed on an installed package, by `Owner-Name`.
  DisablePressed(String),
  /// Uninstall was pressed on an installed package, by `Owner-Name`.
  UninstallPressed(String),
  EnableAllPressed,
  DisableAllPressed,
  /// The user answered whether a change takes the package's dependants too.
  DependantsChosen(Scope),
  /// The user answered whether an uninstall deletes the folders no package
  /// records.
  FoldersChosen(Folders),
  /// The question or refusal of the Installed tab was closed.
  PendingDismissed,
  /// An uninstall was checked: the folders no package records, which it
  /// would offer to delete, or why it cannot go ahead.
  UninstallChecked {
    game: String,
    profile: String,
    packages: Vec<String>,
    untracked: Result<Vec<PathBuf>, String>,
  },
  /// A package was selected in the shown tab, to read its readme.
  PackageSelected(readme::Package),
  /// A [`readme::Request`] ended, by its id: the readme's markdown, `None`
  /// when the package has none, or why it could not be fetched.
  ReadmeFetched {
    request: u64,
    readme: Result<Option<String>, String>,
  },
  /// A link in a readme was clicked, by its target.
  LinkClicked(String),
  /// A launch button was pressed.
  LaunchPressed(LaunchMode),
  /// A [`Launch`] ended, by its id: the game started, or why it did not.
  LaunchFinished {
    launch: u64,
    result: Result<(), LaunchFailure>,
  },
  SteamOptionRead(SteamOptionRead),
  /// Asked to check Steam's launch options again.
  SteamOptionCheckPressed,
  /// A game's profiles were listed by name, or why they could not be.
  ProfilesListed {
    game: String,
    names: Result<Vec<String>, String>,
  },
  /// A profile in the switcher was picked, by name.
  ProfilePicked(String),
  /// A button that asks for a profile name was pressed.
  ProfileNamePressed(Naming),
  /// A profile name was entered.
  ProfileNameSubmitted(String),
  ProfileNameDismissed,
  /// Delete was pressed on the selected profile.
  DeleteProfilePressed,
  DeleteProfileConfirmed,
  DeleteProfileDismissed,
  /// A [`ProfileChange`] to a game's profiles ended: made, or why it was not.
  ProfileChanged {
    game: String,
    change: ProfileChange,
    result: Result<(), String>,
  },
}

/// Work the shell must do outside the pure core. Each effect is answered with
/// a [`Msg`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
  /// Ask the user for a folder. Needs the window, so the shell runs it.
  /// Answered with [`Msg::FolderChosen`].
  ChooseFolder,
  /// Put this text in the profile name field. Needs the window, so the shell
  /// runs it. Not answered.
  FillProfileName(String),
  /// Open this web address in the user's browser. Needs the platform's
  /// opener, which the shell holds. Not answered.
  OpenUrl(String),
  /// Close the window. Needs the window, so the shell runs it. Not answered.
  CloseWindow,
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
  /// Search every Steam library on the computer for the game sold on Steam
  /// as `app_id`. Answered with [`Msg::SteamSearched`].
  FindInSteam { game: String, app_id: u32 },
  /// Answered with [`Msg::FolderListed`].
  ListFolder(PathBuf),
  /// Read a community's cached package index without touching the network.
  /// Answered with [`Msg::CachedIndexRead`].
  ReadCachedIndex(String),
  /// Fetch a community's package index and cache it. Answered with
  /// [`Msg::IndexRefreshed`].
  RefreshIndex { refresh: Refresh, fetch: Fetch },
  /// Read a game's profile, creating it first if it does not exist. Answered
  /// with [`Msg::ProfileOpened`].
  OpenProfile { game: String, profile: String },
  /// Answered with any number of [`Msg::InstallProgress`], then
  /// [`Msg::OperationFinished`].
  Install(Install),
  /// Stop the operation with this id after the package it is on. Not
  /// answered: the operation's own answer says what it did.
  Cancel(u64),
  /// Check what uninstalling `packages` from a game's profile would delete,
  /// changing nothing. Answered with [`Msg::UninstallChecked`].
  CheckUninstall {
    game: String,
    profile: String,
    packages: Vec<String>,
  },
  /// Answered with [`Msg::OperationFinished`].
  Uninstall(Uninstall),
  /// Answered with [`Msg::OperationFinished`].
  SetEnabled(SetEnabled),
  /// Prepare and start the game. Answered with [`Msg::LaunchFinished`].
  Launch(Launch),
  /// Write the game's Steam wrapper and read Steam's launch options for
  /// `app_id`, never changing them. Answered with [`Msg::SteamOptionRead`].
  ReadSteamOption { game: String, app_id: u32 },
  /// List a game's profiles. Answered with [`Msg::ProfilesListed`].
  ListProfiles(String),
  /// Answered with [`Msg::ProfileChanged`].
  ChangeProfile { game: String, change: ProfileChange },
  /// Answered with [`Msg::ReadmeFetched`].
  FetchReadme(readme::Request),
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

      match state.settings.last_game.clone() {
        Some(game) if offered(state, &game) && state.settings.folder(&game).is_some() => {
          show_game(state, game)
        }
        _ => {
          state.screen = Screen::Picker;

          Vec::new()
        }
      }
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
    Msg::SteamSearched { game, folder } => steam_searched(state, game, folder),
    Msg::FolderChosen(folder) => folder_chosen(state, folder),
    Msg::FolderListed { folder, entries } => folder_listed(state, folder, entries),
    Msg::SwitchGamePressed => {
      state.screen = Screen::Picker;
      state.picker.step = Step::Idle;
      state.picker.refusal = None;
      state.picker.fallback = None;
      state.browse = None;
      state.profile = None;
      state.confirmation = None;
      state.launcher = None;
      state.switcher = None;
      state.manage.pending = None;
      state.readme = None;

      if !state.is_launching() {
        state.launch = None;
      }

      Vec::new()
    }
    Msg::SettingsSaved(result) => {
      state.settings_error = result.err().map(SettingsError::Save);

      Vec::new()
    }
    Msg::CachedIndexRead { community, index } => browse::cache_read(state, community, index),
    Msg::IndexRefreshed { refresh, index } => browse::refreshed(state, refresh, index),
    Msg::RefreshPressed => browse::refresh_pressed(state),
    Msg::PackageSearchChanged(search) => {
      browse::filter(state, |filters| filters.set_search(search))
    }
    Msg::SortPressed(key) => browse::filter(state, |filters| filters.sort_by(key)),
    Msg::CategoryToggled(category) => {
      browse::filter(state, |filters| filters.toggle_category(category))
    }
    Msg::ShowNsfw(show) => browse::filter(state, |filters| filters.show_nsfw = show),
    Msg::ShowDeprecated(show) => browse::filter(state, |filters| filters.show_deprecated = show),
    Msg::FiltersCleared => browse::filter(state, Filters::clear),
    Msg::ProfileOpened {
      game,
      profile,
      installed,
    } => install::profile_opened(state, game, profile, installed),
    Msg::InstallPressed(package) => install::install_pressed(state, package),
    Msg::InstallConfirmed => install::install_confirmed(state),
    Msg::InstallDismissed => {
      state.confirmation = None;

      Vec::new()
    }
    Msg::InstallProgress {
      operation,
      progress,
    } => queue::progressed(state, operation, progress),
    Msg::OperationFinished { operation, outcome } => queue::finished(state, operation, outcome),
    Msg::OperationCancelPressed(operation) => queue::cancel_pressed(state, operation),
    Msg::RetryPressed(operation) => queue::retry_pressed(state, operation),
    Msg::OperationDismissed(operation) => queue::dismissed(state, operation),
    Msg::QueueCleared => queue::cleared(state),
    Msg::CloseRequested => queue::close_requested(state),
    Msg::CloseChosen(choice) => queue::close_chosen(state, choice),
    Msg::CloseDismissed => queue::close_dismissed(state),
    Msg::TabChosen(tab) => {
      if state.tab != tab {
        state.readme = None;
      }

      state.tab = tab;

      Vec::new()
    }
    Msg::InstalledSearchChanged(query) => {
      state.manage.query = query;

      Vec::new()
    }
    Msg::InstalledSortChosen(sort) => {
      state.manage.sort = sort;

      Vec::new()
    }
    Msg::EnablePressed(package) => manage::enable_pressed(state, package),
    Msg::DisablePressed(package) => manage::disable_pressed(state, package),
    Msg::UninstallPressed(package) => manage::uninstall_pressed(state, package),
    Msg::EnableAllPressed => manage::set_all(state, true),
    Msg::DisableAllPressed => manage::set_all(state, false),
    Msg::DependantsChosen(scope) => manage::dependants_chosen(state, scope),
    Msg::FoldersChosen(folders) => manage::folders_chosen(state, folders),
    Msg::PendingDismissed => {
      state.manage.pending = None;

      Vec::new()
    }
    Msg::UninstallChecked {
      game,
      profile,
      packages,
      untracked,
    } => manage::uninstall_checked(state, game, profile, packages, untracked),
    Msg::PackageSelected(package) => readme::selected(state, package),
    Msg::ReadmeFetched { request, readme } => readme::fetched(state, request, readme),
    Msg::LinkClicked(url) => readme::link_clicked(url),
    Msg::LaunchPressed(mode) => launch::launch_pressed(state, mode),
    Msg::LaunchFinished { launch, result } => launch::launch_finished(state, launch, result),
    Msg::SteamOptionRead(read) => launch::steam_option_read(state, read),
    Msg::SteamOptionCheckPressed => launch::check_steam_option(state),
    Msg::ProfilesListed { game, names } => switcher::listed(state, game, names),
    Msg::ProfilePicked(name) => switcher::picked(state, name),
    Msg::ProfileNamePressed(naming) => switcher::name_pressed(state, naming),
    Msg::ProfileNameSubmitted(name) => switcher::name_submitted(state, name),
    Msg::ProfileNameDismissed => switcher::name_dismissed(state),
    Msg::DeleteProfilePressed => switcher::delete_pressed(state),
    Msg::DeleteProfileConfirmed => switcher::delete_confirmed(state),
    Msg::DeleteProfileDismissed => switcher::delete_dismissed(state),
    Msg::ProfileChanged {
      game,
      change,
      result,
    } => switcher::changed(state, game, change, result),
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
  state.picker.fallback = None;

  if state.settings.folder(&game).is_some() {
    state.picker.step = Step::Idle;

    return open_game(state, game);
  }

  let app_id = state.ecosystem.game(&game).and_then(picker::steam_app_id);

  match app_id {
    Some(app_id) => {
      state.picker.step = Step::Locating(game.clone());

      vec![Effect::Run(Command::FindInSteam { game, app_id })]
    }
    None => choose_by_hand(state, game, NotFound::NotOnSteam),
  }
}

fn steam_searched(
  state: &mut State,
  game: String,
  folder: Result<PathBuf, NotFound>,
) -> Vec<Effect> {
  if state.picker.step != Step::Locating(game.clone()) {
    return Vec::new();
  }

  match folder {
    Ok(folder) => {
      state.picker.step = Step::Checking {
        game,
        folder: folder.clone(),
        from_steam: true,
      };

      vec![Effect::Run(Command::ListFolder(folder))]
    }
    Err(reason) => choose_by_hand(state, game, reason),
  }
}

/// Opens the folder dialog for `game`, recording why Steam could not name
/// its folder.
fn choose_by_hand(state: &mut State, game: String, reason: NotFound) -> Vec<Effect> {
  state.picker.fallback = Some(Fallback {
    game: game.clone(),
    reason,
  });
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
        from_steam: false,
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
    from_steam,
  } = &state.picker.step
  else {
    return Vec::new();
  };

  if *checking != folder {
    return Vec::new();
  }

  let (game, from_steam) = (game.clone(), *from_steam);

  state.picker.step = Step::Idle;

  let Some(definition) = state.ecosystem.game(&game) else {
    return Vec::new();
  };
  let verdict = entries
    .map_err(FolderRefusal::Unreadable)
    .and_then(|entries| picker::check_folder(definition, &entries));

  match verdict {
    Ok(()) => {
      state.settings.games.insert(
        game.clone(),
        GameSettings {
          folder,
          profile: None,
        },
      );

      open_game(state, game)
    }
    Err(reason) => {
      state.picker.refusal = Some(Refusal {
        game: game.clone(),
        folder,
        reason,
      });

      if !from_steam {
        return Vec::new();
      }

      state.picker.step = Step::Choosing(game);

      vec![Effect::ChooseFolder]
    }
  }
}

/// Shows `game` and saves it as the game to reopen on the next start.
fn open_game(state: &mut State, game: String) -> Vec<Effect> {
  state.settings.last_game = Some(game.clone());

  let mut effects = vec![Effect::Run(Command::SaveSettings(state.settings.clone()))];

  effects.extend(show_game(state, game));

  effects
}

/// Shows `game` and starts loading its package list and the profile last
/// selected for it.
fn show_game(state: &mut State, game: String) -> Vec<Effect> {
  let community = state
    .ecosystem
    .game(&game)
    .and_then(|definition| definition.community())
    .map(str::to_string);
  let selected = state.settings.profile(&game).to_string();
  let profile = install::open_profile(state, &game, &selected);
  let launcher = launch::open(state, &game);

  switcher::open(state, &game);

  state.screen = Screen::Game(game);
  state.browse = community.clone().map(Browse::new);

  community
    .map(|community| Effect::Run(Command::ReadCachedIndex(community)))
    .into_iter()
    .chain(profile)
    .chain(launcher)
    .collect()
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
            {
              "gameInstanceType": "game",
              "exeNames": ["valheim.exe", "valheim.x86_64"],
              "packageIndex": "https://thunderstore.io/c/valheim/api/v1/package/"
            },
            { "gameInstanceType": "server", "exeNames": ["valheim_server.exe"] }
          ]
        },
        "riskofrain2": {
          "label": "riskofrain2",
          "meta": { "displayName": "Risk of Rain 2" },
          "r2modman": [{
            "gameInstanceType": "game",
            "exeNames": ["Risk of Rain 2.exe"],
            "packageIndex": "https://thunderstore.io/c/riskofrain2/api/v1/package/"
          }]
        },
        "lethal-company": {
          "label": "lethal-company",
          "meta": { "displayName": "Lethal Company" },
          "r2modman": [{
            "gameInstanceType": "game",
            "exeNames": ["Lethal Company.exe"],
            "distributions": [{ "platform": "steam", "identifier": "1966720" }],
            "packageIndex": "https://thunderstore.io/c/lethal-company/api/v1/package/"
          }]
        },
        "no-install-definition": {
          "label": "no-install-definition",
          "meta": { "displayName": "A Game Without An Install Definition" },
          "r2modman": null
        },
        "no-package-index": {
          "label": "no-package-index",
          "meta": { "displayName": "A Game Without A Package Index" },
          "r2modman": [{ "gameInstanceType": "game", "exeNames": ["game.exe"] }]
        }
      },
      "modloaderPackages": [
        { "packageId": "denikson-BepInExPack_Valheim", "rootFolder": "BepInExPack_Valheim", "loader": "bepinex" }
      ]
    }"#,
  )
  .expect("test ecosystem parses")
}

/// A profile holding each `(Owner-Name, version)`, enabled and needing
/// nothing.
#[cfg(test)]
pub(crate) fn test_installed(packages: &[(&str, &str)]) -> Installed {
  packages
    .iter()
    .map(|(full_name, version)| {
      let author = full_name.split_once('-').map_or("", |(author, _)| author);

      (
        full_name.to_string(),
        install::InstalledPackage {
          author: author.to_string(),
          version: version.to_string(),
          enabled: true,
          dependencies: Vec::new(),
          installed_at: 0,
        },
      )
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use picker::{Fallback, NotFound};
  use std::path::Path;

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
          profile: None,
        },
      )]
      .into(),
    }
  }

  fn read_cache(community: &str) -> Effect {
    Effect::Run(Command::ReadCachedIndex(community.to_string()))
  }

  fn open_profile(game: &str) -> Effect {
    Effect::Run(Command::OpenProfile {
      game: game.to_string(),
      profile: install::DEFAULT_PROFILE.to_string(),
    })
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
    assert_eq!(
      effects,
      vec![read_cache("valheim"), open_profile("valheim")]
    );
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
    assert_eq!(
      state.picker.fallback,
      Some(Fallback {
        game: "valheim".to_string(),
        reason: NotFound::NotOnSteam,
      })
    );
  }

  const LETHAL: &str = "lethal-company";

  fn steam_searched(folder: Result<&str, NotFound>) -> Msg {
    Msg::SteamSearched {
      game: LETHAL.to_string(),
      folder: folder.map(PathBuf::from),
    }
  }

  #[test]
  fn picking_a_steam_game_without_a_folder_looks_for_it_in_steam() {
    let mut state = on_picker();

    let effects = update(&mut state, Msg::GamePicked(LETHAL.to_string()));

    assert_eq!(state.picker.step, Step::Locating(LETHAL.to_string()));
    assert_eq!(
      effects,
      vec![Effect::Run(Command::FindInSteam {
        game: LETHAL.to_string(),
        app_id: 1966720,
      })]
    );
  }

  #[test]
  fn a_folder_steam_finds_is_checked_and_opened_with_no_user_action() {
    let mut state = on_picker();
    let folder = "/steam/steamapps/common/Lethal Company";

    update(&mut state, Msg::GamePicked(LETHAL.to_string()));

    let effects = update(&mut state, steam_searched(Ok(folder)));

    assert_eq!(
      effects,
      vec![Effect::Run(Command::ListFolder(PathBuf::from(folder)))]
    );

    update(&mut state, listed(folder, &["Lethal Company.exe"]));

    assert_eq!(state.screen, Screen::Game(LETHAL.to_string()));
    assert_eq!(state.settings.folder(LETHAL), Some(Path::new(folder)));
  }

  #[test]
  fn a_game_steam_does_not_have_falls_back_to_choosing_its_folder_saying_why() {
    let mut state = on_picker();

    update(&mut state, Msg::GamePicked(LETHAL.to_string()));

    let effects = update(&mut state, steam_searched(Err(NotFound::NotInstalled)));

    assert_eq!(state.picker.step, Step::Choosing(LETHAL.to_string()));
    assert_eq!(effects, vec![Effect::ChooseFolder]);
    assert_eq!(
      state.picker.fallback,
      Some(Fallback {
        game: LETHAL.to_string(),
        reason: NotFound::NotInstalled,
      })
    );
  }

  #[test]
  fn a_folder_steam_finds_without_the_game_falls_back_to_choosing_its_folder() {
    let mut state = on_picker();
    let folder = "/steam/steamapps/common/Lethal Company";

    update(&mut state, Msg::GamePicked(LETHAL.to_string()));
    update(&mut state, steam_searched(Ok(folder)));

    let effects = update(&mut state, listed(folder, &[]));

    assert_eq!(state.screen, Screen::Picker);
    assert_eq!(state.picker.step, Step::Choosing(LETHAL.to_string()));
    assert_eq!(effects, vec![Effect::ChooseFolder]);
    assert!(state.picker.refusal.is_some());
  }

  #[test]
  fn a_steam_search_for_a_game_no_longer_picked_is_ignored() {
    let mut state = on_picker();

    update(&mut state, Msg::GamePicked(LETHAL.to_string()));
    update(&mut state, Msg::GamePicked("valheim".to_string()));

    let effects = update(&mut state, steam_searched(Ok("/steam/Lethal Company")));

    assert_eq!(state.picker.step, Step::Choosing("valheim".to_string()));
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn a_saved_folder_is_used_instead_of_searching_steam() {
    let mut state = started();

    update(
      &mut state,
      Msg::SettingsLoaded(Ok(settings_with(LETHAL, "/games/lethal"))),
    );
    update(&mut state, Msg::SwitchGamePressed);

    let effects = update(&mut state, Msg::GamePicked(LETHAL.to_string()));

    assert_eq!(state.screen, Screen::Game(LETHAL.to_string()));
    assert!(
      !effects
        .iter()
        .any(|effect| matches!(effect, Effect::Run(Command::FindInSteam { .. })))
    );
  }

  #[test]
  fn switching_game_forgets_why_a_folder_was_asked_for() {
    let mut state = on_picker();

    update(&mut state, Msg::GamePicked("valheim".to_string()));
    update(&mut state, Msg::FolderChosen(None));
    update(&mut state, Msg::SwitchGamePressed);

    assert_eq!(state.picker.fallback, None);
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
        profile: None,
      },
    );
    update(&mut state, Msg::SettingsLoaded(Ok(settings)));
    update(&mut state, Msg::SwitchGamePressed);

    let effects = update(&mut state, Msg::GamePicked("valheim".to_string()));

    assert_eq!(state.screen, Screen::Game("valheim".to_string()));
    assert_eq!(state.settings.last_game.as_deref(), Some("valheim"));
    assert_eq!(
      effects,
      vec![
        Effect::Run(Command::SaveSettings(state.settings.clone())),
        read_cache("valheim"),
        open_profile("valheim"),
      ]
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
        from_steam: false,
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
      vec![
        Effect::Run(Command::SaveSettings(settings_with(
          "valheim",
          "/games/valheim"
        ))),
        read_cache("valheim"),
        open_profile("valheim"),
      ]
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
