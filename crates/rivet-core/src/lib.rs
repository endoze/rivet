//! The pure core of the app: all state, every message that can change it, and
//! the effects the shell must run. Nothing here does IO.

pub mod browse;
pub mod details;
pub mod install;
pub mod launch;
pub mod loader_files;
pub mod manage;
pub mod notice;
pub mod picker;
pub mod pins;
pub mod portability;
pub mod queue;
pub mod r2modman;
pub mod readme;
pub mod settings;
pub mod settings_screen;
pub mod switcher;
pub mod updates;

use browse::{Browse, Fetch, Filters, Index, Refresh, SortKey};
use install::{Confirmation, Install, Installed, Outcome, Profile, Progress};
use launch::{Attempt, Launch, LaunchFailure, Launcher, SteamOptionRead};
use loader_files::{Copied, FileRemoval, LoaderFile};
use manage::{Folders, Manage, Scope, SetEnabled, Sort, Tab, Uninstall};
use notice::Notice;
use picker::{Cover, Fallback, FolderRefusal, NotFound, Picker, Refusal, Step};
use portability::{Export, Exported, Exporting, Importing, ReadFailure, Shared, Source};
use queue::{CloseChoice, Closing, Queue};
use r2modman::R2modman;
use readme::Readme;
use settings::{Appearance, GameSettings, PickerLayout, RuntimeChoice, Settings, ThemeFamily};
use settings_screen::{SettingsScreen, Target};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use switcher::{Naming, ProfileChange, Switcher};
use thunderstore_engine::ecosystem::Ecosystem;
use thunderstore_engine::profile::launch::{HostOs, LaunchContext, LaunchMode};
use updates::Update;

/// Everything the app knows.
#[derive(Debug)]
pub struct State {
  /// The game definitions.
  pub ecosystem: Ecosystem,
  pub settings: Settings,
  pub screen: Screen,
  pub picker: Picker,
  /// The settings screen, while it is shown.
  pub settings_screen: SettingsScreen,
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
  /// Closing the window while operations are running or waiting, or settings
  /// saves are unanswered.
  pub closing: Option<Closing>,
  /// How many settings saves are sent and not yet answered. The window
  /// closes only once each has been written.
  pub saves: usize,
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
  /// The game whose loader files are being removed. It outlives leaving the
  /// settings screen and the game.
  pub loader_removal: Option<String>,
  /// The open game's profiles. `None` outside a game.
  pub switcher: Option<Switcher>,
  /// The tab shown in a game.
  pub tab: Tab,
  /// The Installed tab.
  pub manage: Manage,
  /// The readme of the package the detail pane shows: the one selected in
  /// the shown tab, or else the first that tab lists. `None` when it lists
  /// none.
  pub readme: Option<Readme>,
  /// Whether the detail pane is shown beside the list. It stays as the user
  /// left it while the app runs.
  pub details_shown: bool,
  /// How many readme fetches have been started, so each gets its own id.
  pub readmes: u64,
  /// Exporting one of the open game's profiles. One running outlives leaving its game.
  pub exporting: Option<Exporting>,
  /// Importing a shared profile into the open game. `None` outside a game.
  pub importing: Option<Importing>,
  /// The open game's r2modman profiles and the offer to import them. `None`
  /// outside a game.
  pub r2modman: Option<R2modman>,
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
      loader_removal: None,
      switcher: None,
      ecosystem,
      settings: Settings::default(),
      screen: Screen::Starting,
      picker: Picker::default(),
      settings_screen: SettingsScreen::default(),
      settings_error: None,
      browse: None,
      refreshes: 0,
      profile: None,
      confirmation: None,
      queue: Queue::default(),
      closing: None,
      saves: 0,
      tab: Tab::default(),
      manage: Manage::default(),
      readme: None,
      details_shown: true,
      readmes: 0,
      exporting: None,
      importing: None,
      r2modman: None,
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
  /// The settings of one game, by community slug, which return to it.
  Settings(String),
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
  /// The picker's list or grid was chosen.
  PickerLayoutChosen(PickerLayout),
  /// Game covers now on disk, each by community slug. A
  /// [`Command::CacheCovers`] sends these as covers are found or downloaded.
  CoversCached(BTreeMap<String, PathBuf>),
  /// A [`Command::CacheCovers`] ended, every cover it could get reported.
  CoverCachingEnded,
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
  /// Every chosen category of the package list was unchosen.
  CategoriesCleared,
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
  /// A version was chosen for the install waiting to be confirmed, or `None`
  /// for the latest.
  VersionChosen(Option<String>),
  InstallConfirmed,
  InstallDismissed,
  /// A running operation moved on to a package or stage, by operation id.
  OperationProgress {
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
  /// Update was pressed on an installed package, by `Owner-Name`.
  UpdatePressed(String),
  UpdateAllPressed,
  /// Unpin was pressed on an installed package, by `Owner-Name`.
  UnpinPressed(String),
  /// A [`pins::Unpin`] ended: made, or why it was not.
  Unpinned {
    unpin: pins::Unpin,
    result: Result<(), String>,
  },
  /// A profile's pin file could not be read, with the reason naming the
  /// file. The profile opened with no package pinned. Sent before its
  /// [`Msg::ProfileOpened`].
  PinsUnread(String),
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
  /// The detail pane was opened, or closed.
  ShowDetails(bool),
  /// Typing in a search paused, by the id of the readme waiting for it.
  ReadmeDue(u64),
  /// A launch button was pressed.
  LaunchPressed(LaunchMode),
  /// A [`Launch`] ended, by its id: the game started, or why it did not.
  LaunchFinished {
    launch: u64,
    result: Result<(), LaunchFailure>,
  },
  /// A launch copied files into a game's folder. Sent before its
  /// [`Msg::LaunchFinished`], whether or not the game then started.
  LoaderFilesCopied(Copied),
  /// The open game's folder was listed: the names of its entries, or why it
  /// could not be.
  GameFolderListed {
    game: String,
    entries: Result<Vec<String>, String>,
  },
  SteamOptionRead(SteamOptionRead),
  /// Asked to check Steam's launch options again.
  SteamOptionCheckPressed,
  /// Copy was pressed beside the Steam launch option line.
  LaunchLineCopyPressed,
  /// The window came back into focus.
  WindowFocused,
  /// A game's profiles were listed by name, or why they could not be.
  ProfilesListed {
    game: String,
    names: Result<Vec<String>, String>,
  },
  /// How many packages each of a game's profiles holds, by name, leaving out
  /// any whose list of packages could not be read.
  ProfilesCounted {
    game: String,
    counts: BTreeMap<String, usize>,
  },
  /// A profile in the switcher was picked, by name.
  ProfilePicked(String),
  /// A button that asks for a profile name was pressed.
  ProfileNamePressed(Naming),
  /// A profile name was entered.
  ProfileNameSubmitted(String),
  ProfileNameDismissed,
  /// Delete was pressed on a profile, by name.
  DeleteProfilePressed(String),
  DeleteProfileConfirmed,
  DeleteProfileDismissed,
  /// A [`ProfileChange`] to a game's profiles ended: made, or why it was not.
  ProfileChanged {
    game: String,
    change: ProfileChange,
    result: Result<(), String>,
  },
  /// Settings was pressed in a game.
  SettingsPressed,
  /// The settings screen was left for its game.
  SettingsClosed,
  /// Change was pressed beside a folder on the settings screen.
  ChangeFolderPressed(Target),
  /// The launch arguments on the settings screen were edited.
  LaunchArgumentsChanged(String),
  /// How the open game runs on Linux was chosen on the settings screen.
  RuntimeChosen(RuntimeChoice),
  /// Light, dark, or the system's appearance was chosen on the settings
  /// screen.
  AppearanceChosen(Appearance),
  /// A family of themes was chosen on the settings screen.
  ThemeChosen(ThemeFamily),
  /// An interface size was chosen on the settings screen, as a percentage,
  /// or `None` to pick one for the display.
  InterfaceSizeChosen(Option<u16>),
  /// Remove was pressed beside the loader files on the settings screen.
  RemoveLoaderFilesPressed,
  /// A [`Command::RemoveLoaderFiles`] ended: what became of each file.
  LoaderFilesRemoved {
    game: String,
    files: Vec<(PathBuf, FileRemoval)>,
  },
  /// Share as code was pressed on a profile, by name.
  ExportCodePressed(String),
  /// Share as file was pressed on a profile, by name.
  ExportFilePressed(String),
  /// The save dialog closed, with `None` if it was cancelled.
  ExportFileChosen(Option<PathBuf>),
  /// Import from code was pressed.
  ImportCodePressed,
  /// A profile code was entered.
  ImportCodeSubmitted(String),
  /// Import from file was pressed.
  ImportFilePressed,
  /// The file dialog closed, with `None` if it was cancelled.
  ImportFileChosen(Option<PathBuf>),
  /// What `source` shares was read, or why it could not be.
  SharedRead {
    source: Source,
    shared: Result<Shared, ReadFailure>,
  },
  /// A name for the profile to import as was entered.
  ImportNameSubmitted(String),
  /// The import was closed before it started.
  ImportDismissed,
  /// An [`Export`] ended: where the profile went, or why it did not.
  ProfileExported {
    export: Export,
    result: Result<Exported, String>,
  },
  /// The profiles r2modman holds for a game were found. None are found when
  /// r2modman's data cannot be read.
  R2modmanFound {
    game: String,
    profiles: Vec<r2modman::Found>,
  },
  /// Import from r2modman was pressed in the Import a profile menu.
  R2modmanPressed,
  /// The offer to import r2modman's profiles was declined.
  R2modmanDeclined,
  /// A profile in the offer was chosen, or unchosen if it was, by folder.
  R2modmanToggled(PathBuf),
  /// The offer to import r2modman's profiles was accepted.
  R2modmanAccepted,
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
  /// Put this text in the launch arguments field. Needs the window, so the
  /// shell runs it. Not answered.
  FillLaunchArguments(String),
  /// Open this web address in the user's browser. Needs the platform's
  /// opener, which the shell holds. Not answered.
  OpenUrl(String),
  /// Close the window. Needs the window, so the shell runs it. Not answered.
  CloseWindow,
  /// Ask the user where to save an export, suggesting this file name. Needs
  /// the window, so the shell runs it. Answered with
  /// [`Msg::ExportFileChosen`].
  ChooseExportFile(String),
  /// Ask the user for a profile export to import. Needs the window, so the
  /// shell runs it. Answered with [`Msg::ImportFileChosen`].
  ChooseImportFile,
  /// Put this text on the clipboard. Needs the platform's clipboard, which
  /// the shell holds. Not answered.
  CopyToClipboard(String),
  /// Tell the user this in passing, over the window. Needs the window, so
  /// the shell runs it. Not answered.
  Notify(Notice),
  /// Send `msg` back after `delay`. The shell keeps the time, so the core
  /// stays free of clocks. Answered with `msg`.
  After { delay: Duration, msg: Box<Msg> },
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
  /// Search every Steam library for the game sold on Steam as `app_id`: those
  /// of the install at `steam` when the user chose one, or else of every
  /// install on the computer. Answered with [`Msg::SteamSearched`].
  FindInSteam {
    game: String,
    app_id: u32,
    steam: Option<PathBuf>,
  },
  /// Answered with [`Msg::FolderListed`].
  ListFolder(PathBuf),
  /// Download each cover not yet in the cache. Sends [`Msg::CoversCached`]
  /// with those already there, then again as each download lands; a cover
  /// that can't be fetched is left out. Answered with
  /// [`Msg::CoverCachingEnded`].
  CacheCovers(Vec<Cover>),
  /// Read a community's cached package index without touching the network.
  /// Answered with [`Msg::CachedIndexRead`].
  ReadCachedIndex(String),
  /// Fetch a community's package index and cache it. Answered with
  /// [`Msg::IndexRefreshed`].
  RefreshIndex { refresh: Refresh, fetch: Fetch },
  /// Read a game's profile, creating it first if it does not exist. Answered
  /// with [`Msg::ProfileOpened`].
  OpenProfile { game: String, profile: String },
  /// Answered with any number of [`Msg::OperationProgress`], then
  /// [`Msg::OperationFinished`].
  Install(Install),
  /// Answered with any number of [`Msg::OperationProgress`], then
  /// [`Msg::OperationFinished`].
  Update(Update),
  /// Remove a pin from a profile's pin file. Answered with
  /// [`Msg::Unpinned`].
  Unpin(pins::Unpin),
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
  /// Answered with any number of [`Msg::OperationProgress`], then
  /// [`Msg::OperationFinished`].
  Uninstall(Uninstall),
  /// Answered with any number of [`Msg::OperationProgress`], then
  /// [`Msg::OperationFinished`].
  SetEnabled(SetEnabled),
  /// Prepare and start the game. Answered with [`Msg::LaunchFinished`].
  Launch(Launch),
  /// List the open game's folder, to tell whether it runs under Proton.
  /// Answered with [`Msg::GameFolderListed`].
  ListGameFolder { game: String, folder: PathBuf },
  /// Work out the line Steam's launch options must hold for a launch in
  /// `context`, write the game's Steam wrapper when the line names it, and
  /// read Steam's launch options for `app_id`, never changing them. They are
  /// read from the install at `steam` when the user chose one, or else from
  /// the one found on the computer. Answered with [`Msg::SteamOptionRead`].
  ReadSteamOption {
    game: String,
    app_id: u32,
    context: LaunchContext,
    steam: Option<PathBuf>,
  },
  /// Delete `files` from `game`'s folder, leaving any changed since they
  /// were copied. Answered with [`Msg::LoaderFilesRemoved`].
  RemoveLoaderFiles {
    game: String,
    files: Vec<LoaderFile>,
  },
  /// List a game's profiles. Answered with [`Msg::ProfilesListed`].
  ListProfiles(String),
  /// Count the packages in each of `game`'s profiles `names`, changing
  /// nothing. Answered with [`Msg::ProfilesCounted`].
  CountProfiles { game: String, names: Vec<String> },
  /// Answered with [`Msg::ProfileChanged`].
  ChangeProfile { game: String, change: ProfileChange },
  /// Answered with [`Msg::ReadmeFetched`].
  FetchReadme(readme::Request),
  /// Export a profile as r2modman does. Answered with
  /// [`Msg::ProfileExported`].
  Export(Export),
  /// Read the profile a code or file shares, changing nothing. Answered with
  /// [`Msg::SharedRead`].
  ReadShared { community: String, source: Source },
  /// Answered with any number of [`Msg::OperationProgress`], then
  /// [`Msg::OperationFinished`].
  Import(portability::Import),
  /// Find the profiles r2modman holds for `game`, which it files under each
  /// of `folders`, changing nothing. Answered with [`Msg::R2modmanFound`].
  FindR2modmanProfiles { game: String, folders: Vec<String> },
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
  let follows = readme::follows(&msg);
  let mut effects = apply(state, msg);

  if let Some(when) = follows {
    effects.extend(readme::follow_list(state, when));
  }

  effects.extend(remember_tab(state));

  state.saves += effects
    .iter()
    .filter(|effect| matches!(effect, Effect::Run(Command::SaveSettings(_))))
    .count();

  effects
}

fn apply(state: &mut State, msg: Msg) -> Vec<Effect> {
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
        _ => picker::show(state),
      }
    }
    Msg::SettingsLoaded(Err(detail)) => {
      state.settings_error = Some(SettingsError::Load(detail));

      picker::show(state)
    }
    Msg::SearchChanged(query) => {
      state.picker.query = query;

      Vec::new()
    }
    Msg::PickerLayoutChosen(layout) => picker::layout_chosen(state, layout),
    Msg::GamePicked(game) => pick_game(state, game),
    Msg::SteamSearched { game, folder } => steam_searched(state, game, folder),
    Msg::FolderChosen(folder) => match state.screen {
      Screen::Settings(_) => settings_screen::folder_chosen(state, folder),
      _ => folder_chosen(state, folder),
    },
    Msg::FolderListed { folder, entries } => match state.screen {
      Screen::Settings(_) => settings_screen::folder_listed(state, folder, entries),
      _ => folder_listed(state, folder, entries),
    },
    Msg::SwitchGamePressed => {
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
      state.importing = None;
      state.r2modman = None;

      if matches!(state.exporting, Some(Exporting::ChoosingFile(_))) {
        state.exporting = None;
      }

      if !state.is_launching() {
        state.launch = None;
      }

      picker::show(state)
    }
    Msg::CoversCached(covers) => picker::covers_cached(state, covers),
    Msg::CoverCachingEnded => {
      state.picker.caching_covers = false;

      Vec::new()
    }
    Msg::SettingsSaved(result) => {
      state.settings_error = result.err().map(SettingsError::Save);
      state.saves = state.saves.saturating_sub(1);

      queue::close_when_done(state)
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
    Msg::CategoriesCleared => browse::filter(state, |filters| filters.categories.clear()),
    Msg::ShowNsfw(show) => browse::filter(state, |filters| filters.show_nsfw = show),
    Msg::ShowDeprecated(show) => browse::filter(state, |filters| filters.show_deprecated = show),
    Msg::FiltersCleared => browse::filter(state, Filters::clear),
    Msg::ProfileOpened {
      game,
      profile,
      installed,
    } => install::profile_opened(state, game, profile, installed),
    Msg::InstallPressed(package) => install::install_pressed(state, package),
    Msg::VersionChosen(version) => install::version_chosen(state, version),
    Msg::InstallConfirmed => install::install_confirmed(state),
    Msg::InstallDismissed => {
      state.confirmation = None;

      Vec::new()
    }
    Msg::OperationProgress {
      operation,
      progress,
    } => queue::progressed(state, operation, progress),
    Msg::OperationFinished { operation, outcome } => queue::finished(state, operation, outcome),
    Msg::OperationCancelPressed(operation) => queue::cancel_pressed(state, operation),
    Msg::CloseRequested => queue::close_requested(state),
    Msg::CloseChosen(choice) => queue::close_chosen(state, choice),
    Msg::CloseDismissed => queue::close_dismissed(state),
    Msg::TabChosen(tab) => {
      show_tab(state, tab);

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
    Msg::UpdatePressed(package) => updates::update_pressed(state, package),
    Msg::UpdateAllPressed => updates::update_all(state),
    Msg::UnpinPressed(package) => pins::unpin_pressed(state, package),
    Msg::Unpinned { unpin, result } => pins::unpinned(state, unpin, result),
    Msg::PinsUnread(reason) => vec![Effect::Notify(Notice::PinsUnread(reason))],
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
    Msg::ShowDetails(shown) => details::show(state, shown),
    Msg::ReadmeDue(id) => readme::due(state, id),
    Msg::LaunchPressed(mode) => launch::launch_pressed(state, mode),
    Msg::LaunchFinished { launch, result } => launch::launch_finished(state, launch, result),
    Msg::LoaderFilesCopied(copied) => loader_files::copied(state, copied),
    Msg::GameFolderListed { game, entries } => launch::game_folder_listed(state, game, entries),
    Msg::SteamOptionRead(read) => launch::steam_option_read(state, read),
    Msg::SteamOptionCheckPressed => launch::check_steam_option(state),
    Msg::LaunchLineCopyPressed => launch::copy_line(state),
    Msg::WindowFocused => launch::window_focused(state),
    Msg::ProfilesListed { game, names } => switcher::listed(state, game, names),
    Msg::ProfilesCounted { game, counts } => switcher::counts_read(state, &game, counts),
    Msg::ProfilePicked(name) => switcher::picked(state, name),
    Msg::ProfileNamePressed(naming) => switcher::name_pressed(state, naming),
    Msg::ProfileNameSubmitted(name) => switcher::name_submitted(state, name),
    Msg::ProfileNameDismissed => switcher::name_dismissed(state),
    Msg::DeleteProfilePressed(name) => switcher::delete_pressed(state, name),
    Msg::DeleteProfileConfirmed => switcher::delete_confirmed(state),
    Msg::DeleteProfileDismissed => switcher::delete_dismissed(state),
    Msg::ProfileChanged {
      game,
      change,
      result,
    } => switcher::changed(state, game, change, result),
    Msg::SettingsPressed => settings_screen::pressed(state),
    Msg::SettingsClosed => settings_screen::closed(state),
    Msg::ChangeFolderPressed(target) => settings_screen::change_pressed(state, target),
    Msg::LaunchArgumentsChanged(arguments) => settings_screen::arguments_changed(state, arguments),
    Msg::RuntimeChosen(choice) => settings_screen::runtime_chosen(state, choice),
    Msg::AppearanceChosen(appearance) => settings_screen::appearance_chosen(state, appearance),
    Msg::ThemeChosen(theme) => settings_screen::theme_chosen(state, theme),
    Msg::InterfaceSizeChosen(size) => settings_screen::interface_size_chosen(state, size),
    Msg::RemoveLoaderFilesPressed => loader_files::remove_pressed(state),
    Msg::LoaderFilesRemoved { game, files } => loader_files::removed(state, game, files),
    Msg::ExportCodePressed(name) => portability::export_code_pressed(state, name),
    Msg::ExportFilePressed(name) => portability::export_file_pressed(state, name),
    Msg::ExportFileChosen(path) => portability::export_file_chosen(state, path),
    Msg::ImportCodePressed => portability::import_code_pressed(state),
    Msg::ImportCodeSubmitted(code) => portability::import_code_submitted(state, code),
    Msg::ImportFilePressed => portability::import_file_pressed(state),
    Msg::ImportFileChosen(path) => portability::import_file_chosen(state, path),
    Msg::SharedRead {
      source: Source::R2modman(folder),
      shared,
    } => r2modman::read(state, folder, shared),
    Msg::SharedRead { source, shared } => portability::shared_read(state, source, shared),
    Msg::ImportNameSubmitted(name) => portability::import_name_submitted(state, name),
    Msg::ImportDismissed => {
      state.importing = None;

      Vec::new()
    }
    Msg::ProfileExported { export, result } => portability::exported(state, export, result),
    Msg::R2modmanFound { game, profiles } => r2modman::found(state, game, profiles),
    Msg::R2modmanPressed => r2modman::pressed(state),
    Msg::R2modmanDeclined => r2modman::declined(state),
    Msg::R2modmanToggled(folder) => r2modman::toggled(state, folder),
    Msg::R2modmanAccepted => r2modman::accepted(state),
  }
}

/// Records the tab shown in the open game as the one it opens on next, and
/// saves it, when that differs from the one recorded. Whatever changed the
/// tab, a click on a view or on a profile, the game reopens where it was
/// left.
fn remember_tab(state: &mut State) -> Option<Effect> {
  let Screen::Game(game) = &state.screen else {
    return None;
  };
  let settings = state.settings.games.get_mut(game)?;

  if settings.tab == state.tab {
    return None;
  }

  settings.tab = state.tab;

  Some(Effect::Run(Command::SaveSettings(state.settings.clone())))
}

/// Shows `tab`, dropping the readme of the tab left so the next one follows
/// the new list. Returns whether the tab changed.
pub(crate) fn show_tab(state: &mut State, tab: Tab) -> bool {
  let changed = state.tab != tab;

  if changed {
    state.readme = None;
  }

  state.tab = tab;

  changed
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

      vec![Effect::Run(Command::FindInSteam {
        game,
        app_id,
        steam: state.settings.steam_folder.clone(),
      })]
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
      state
        .settings
        .games
        .insert(game.clone(), GameSettings::new(folder));

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
  show_tab(state, state.settings.tab(&game));

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
              "settingsIdentifier": "Valheim",
              "internalFolderName": "Valheim",
              "packageIndex": "https://thunderstore.io/c/valheim/api/v1/package/"
            },
            {
              "gameInstanceType": "server",
              "exeNames": ["valheim_server.exe"],
              "settingsIdentifier": "ValheimServer",
              "internalFolderName": "Valheim"
            }
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
          pinned: false,
        },
      )
    })
    .collect()
}

/// A package published at each of `versions`, in order, the last most
/// recently, needing nothing.
#[cfg(test)]
pub(crate) fn test_listed(
  full_name: &str,
  versions: &[&str],
  deprecated: bool,
) -> serde_json::Value {
  let (owner, name) = full_name.split_once('-').unwrap();
  let versions: Vec<serde_json::Value> = versions
    .iter()
    .enumerate()
    .map(|(day, version)| {
      serde_json::json!({
        "version_number": version,
        "dependencies": [],
        "download_url": format!("https://example.com/{full_name}-{version}.zip"),
        "date_created": format!("2024-01-{:02}T00:00:00Z", day + 1),
      })
    })
    .collect();

  serde_json::json!({
    "name": name,
    "full_name": full_name,
    "owner": owner,
    "date_created": "2024-01-01T00:00:00Z",
    "date_updated": "2024-01-01T00:00:00Z",
    "is_deprecated": deprecated,
    "categories": [],
    "versions": versions,
  })
}

/// An index of `packages`, each as [`test_listed`] makes them.
#[cfg(test)]
pub(crate) fn test_index(packages: Vec<serde_json::Value>) -> Index {
  let packages: Vec<thunderstore_engine::models::Package> =
    serde_json::from_value(serde_json::Value::Array(packages)).unwrap();

  thunderstore_engine::models::PackageIndex::from(packages).into()
}

/// A package index with `Owner-ModA` and `Owner-ModB` at 1.0.0, and
/// `Owner-ModC` at 1.0.0 and 2.0.0.
#[cfg(test)]
pub(crate) fn test_shared_index() -> Index {
  test_index(vec![
    test_listed("Owner-ModA", &["1.0.0"], false),
    test_listed("Owner-ModB", &["1.0.0"], false),
    test_listed("Owner-ModC", &["1.0.0", "2.0.0"], false),
  ])
}

#[cfg(test)]
mod tests {
  use super::*;
  use picker::{Fallback, NotFound};
  use std::path::Path;

  /// A Linux state that has asked for its settings.
  fn started() -> State {
    let mut state = State::new(test_ecosystem());

    state.host = Some(HostOs::Linux);

    update(&mut state, Msg::Started);

    state
  }

  fn settings_with(game: &str, folder: &str) -> Settings {
    Settings {
      last_game: Some(game.to_string()),
      games: [(game.to_string(), GameSettings::new(folder))].into(),
      ..Settings::default()
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

  fn list_game_folder(game: &str, folder: &str) -> Effect {
    Effect::Run(Command::ListGameFolder {
      game: game.to_string(),
      folder: PathBuf::from(folder),
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
      vec![
        read_cache("valheim"),
        open_profile("valheim"),
        list_game_folder("valheim", "/games/valheim"),
      ]
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

  /// Valheim with a cover and Risk of Rain 2 without one.
  fn covered_ecosystem() -> Ecosystem {
    Ecosystem::from_json(
      r#"{
        "games": {
          "valheim": {
            "label": "valheim",
            "meta": { "displayName": "Valheim", "iconUrl": "valheim/valheim-cover.webp" },
            "r2modman": [{
              "gameInstanceType": "game",
              "exeNames": ["valheim.x86_64"],
              "packageIndex": "https://thunderstore.io/c/valheim/api/v1/package/"
            }]
          },
          "riskofrain2": {
            "label": "riskofrain2",
            "meta": { "displayName": "Risk of Rain 2" },
            "r2modman": [{
              "gameInstanceType": "game",
              "exeNames": ["Risk of Rain 2.exe"],
              "packageIndex": "https://thunderstore.io/c/riskofrain2/api/v1/package/"
            }]
          }
        }
      }"#,
    )
    .unwrap()
  }

  fn cache_valheim_cover() -> Effect {
    Effect::Run(Command::CacheCovers(vec![Cover {
      game: "valheim".to_string(),
      path: "valheim/valheim-cover.webp".to_string(),
    }]))
  }

  #[test]
  fn showing_the_picker_caches_the_covers_of_games_that_have_one() {
    let mut state = State::new(covered_ecosystem());

    update(&mut state, Msg::Started);

    let effects = update(&mut state, Msg::SettingsLoaded(Ok(Settings::default())));

    assert_eq!(effects, vec![cache_valheim_cover()]);
  }

  #[test]
  fn unreadable_settings_still_cache_the_covers() {
    let mut state = State::new(covered_ecosystem());

    update(&mut state, Msg::Started);

    let effects = update(&mut state, Msg::SettingsLoaded(Err("bad toml".to_string())));

    assert_eq!(effects, vec![cache_valheim_cover()]);
  }

  /// A Linux state on Valheim, with a cover for it, after its settings.
  fn in_covered_valheim() -> State {
    let mut state = State::new(covered_ecosystem());

    state.host = Some(HostOs::Linux);

    update(&mut state, Msg::Started);
    update(
      &mut state,
      Msg::SettingsLoaded(Ok(settings_with("valheim", "/games/valheim"))),
    );

    state
  }

  #[test]
  fn covers_are_not_asked_for_again_while_they_are_being_cached() {
    let mut state = in_covered_valheim();

    let first = update(&mut state, Msg::SwitchGamePressed);

    update(&mut state, Msg::GamePicked("valheim".to_string()));

    let again = update(&mut state, Msg::SwitchGamePressed);

    assert_eq!(state.screen, Screen::Picker);
    assert_eq!(first, vec![cache_valheim_cover()]);
    assert_eq!(again, vec![]);
  }

  #[test]
  fn covers_still_missing_once_caching_ends_are_asked_for_on_the_next_show() {
    let mut state = in_covered_valheim();

    update(&mut state, Msg::SwitchGamePressed);
    update(&mut state, Msg::CoverCachingEnded);

    let again = update(&mut state, Msg::SwitchGamePressed);

    assert_eq!(again, vec![cache_valheim_cover()]);
  }

  #[test]
  fn cached_covers_are_not_asked_for_again() {
    let mut state = in_covered_valheim();

    update(&mut state, Msg::SwitchGamePressed);
    update(
      &mut state,
      Msg::CoversCached([("valheim".to_string(), PathBuf::from("/cache/v.webp"))].into()),
    );
    update(&mut state, Msg::CoverCachingEnded);

    let again = update(&mut state, Msg::SwitchGamePressed);

    assert_eq!(again, vec![]);
  }

  #[test]
  fn cached_covers_are_kept_for_their_games() {
    let mut state = State::new(covered_ecosystem());
    let valheim = PathBuf::from("/cache/covers/valheim/valheim-cover.webp");

    update(
      &mut state,
      Msg::CoversCached([("valheim".to_string(), valheim.clone())].into()),
    );

    assert_eq!(state.picker.covers.get("valheim"), Some(&valheim));
  }

  #[test]
  fn choosing_a_picker_layout_saves_it() {
    let mut state = on_picker();

    let effects = update(&mut state, Msg::PickerLayoutChosen(PickerLayout::Grid));

    assert_eq!(state.settings.picker_layout, PickerLayout::Grid);
    assert_eq!(
      effects,
      vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
    );
  }

  #[test]
  fn choosing_a_picker_layout_over_unreadable_settings_keeps_the_file() {
    let mut state = started();

    update(&mut state, Msg::SettingsLoaded(Err("bad toml".to_string())));

    let effects = update(&mut state, Msg::PickerLayoutChosen(PickerLayout::Grid));

    assert_eq!(state.settings.picker_layout, PickerLayout::Grid);
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn choosing_the_picker_layout_already_shown_saves_nothing() {
    let mut state = on_picker();

    let effects = update(&mut state, Msg::PickerLayoutChosen(PickerLayout::List));

    assert_eq!(effects, vec![]);
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
  fn a_start_on_the_last_game_lands_on_its_profiles_mods() {
    let mut state = started();

    update(
      &mut state,
      Msg::SettingsLoaded(Ok(settings_with("valheim", "/games/valheim"))),
    );

    assert_eq!(state.tab, Tab::Installed);
  }

  #[test]
  fn picking_a_game_lands_on_its_profiles_mods() {
    let mut state = checking("valheim", "/games/valheim");

    update(&mut state, listed("/games/valheim", &["valheim.x86_64"]));

    assert_eq!(state.screen, Screen::Game("valheim".to_string()));
    assert_eq!(state.tab, Tab::Installed);
  }

  /// Settings with valheim and riskofrain2 set up, valheim open last.
  fn two_set_up() -> Settings {
    let mut settings = settings_with("valheim", "/games/valheim");

    settings.games.insert(
      "riskofrain2".to_string(),
      GameSettings::new("/games/riskofrain2"),
    );

    settings
  }

  #[test]
  fn switching_from_find_mods_to_a_game_with_no_saved_view_lands_on_its_profile() {
    let mut state = started();

    update(&mut state, Msg::SettingsLoaded(Ok(two_set_up())));
    update(&mut state, Msg::TabChosen(Tab::Browse));
    update(&mut state, Msg::SwitchGamePressed);
    update(&mut state, Msg::GamePicked("riskofrain2".to_string()));

    assert_eq!(state.screen, Screen::Game("riskofrain2".to_string()));
    assert_eq!(state.tab, Tab::Installed);
  }

  #[test]
  fn choosing_find_mods_saves_it_as_the_games_view() {
    let mut state = started();

    update(
      &mut state,
      Msg::SettingsLoaded(Ok(settings_with("valheim", "/games/valheim"))),
    );

    let effects = update(&mut state, Msg::TabChosen(Tab::Browse));

    assert_eq!(state.settings.games["valheim"].tab, Tab::Browse);
    assert!(
      effects.contains(&Effect::Run(Command::SaveSettings(state.settings.clone()))),
      "the view was not saved: {effects:?}"
    );
  }

  #[test]
  fn choosing_the_view_already_saved_saves_nothing() {
    let mut state = started();

    update(
      &mut state,
      Msg::SettingsLoaded(Ok(settings_with("valheim", "/games/valheim"))),
    );

    let effects = update(&mut state, Msg::TabChosen(Tab::Installed));

    assert!(
      !effects
        .iter()
        .any(|effect| matches!(effect, Effect::Run(Command::SaveSettings(_)))),
      "{effects:?}"
    );
  }

  #[test]
  fn clicking_a_profile_from_find_mods_saves_its_mods_as_the_view() {
    let mut state = started();

    update(
      &mut state,
      Msg::SettingsLoaded(Ok(settings_with("valheim", "/games/valheim"))),
    );
    update(
      &mut state,
      Msg::ProfilesListed {
        game: "valheim".to_string(),
        names: Ok(vec![install::DEFAULT_PROFILE.to_string()]),
      },
    );
    update(&mut state, Msg::TabChosen(Tab::Browse));
    update(
      &mut state,
      Msg::ProfilePicked(install::DEFAULT_PROFILE.to_string()),
    );

    assert_eq!(state.tab, Tab::Installed);
    assert_eq!(state.settings.games["valheim"].tab, Tab::Installed);
  }

  #[test]
  fn a_start_reopens_the_view_the_last_game_was_left_on() {
    let mut state = started();
    let mut settings = settings_with("valheim", "/games/valheim");

    settings.games.get_mut("valheim").unwrap().tab = Tab::Browse;

    update(&mut state, Msg::SettingsLoaded(Ok(settings)));

    assert_eq!(state.tab, Tab::Browse);
  }

  #[test]
  fn each_game_reopens_on_its_own_view() {
    let mut state = started();

    update(&mut state, Msg::SettingsLoaded(Ok(two_set_up())));
    update(&mut state, Msg::TabChosen(Tab::Browse));
    update(&mut state, Msg::SwitchGamePressed);
    update(&mut state, Msg::GamePicked("riskofrain2".to_string()));
    update(&mut state, Msg::SwitchGamePressed);
    update(&mut state, Msg::GamePicked("valheim".to_string()));

    assert_eq!(state.tab, Tab::Browse);
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
        steam: None,
      })]
    );
  }

  #[test]
  fn a_steam_folder_chosen_in_settings_is_the_one_searched() {
    let mut state = started();
    let settings = Settings {
      steam_folder: Some(PathBuf::from("/mnt/Steam")),
      ..Settings::default()
    };

    update(&mut state, Msg::SettingsLoaded(Ok(settings)));

    let effects = update(&mut state, Msg::GamePicked(LETHAL.to_string()));

    assert_eq!(
      effects,
      vec![Effect::Run(Command::FindInSteam {
        game: LETHAL.to_string(),
        app_id: 1966720,
        steam: Some(PathBuf::from("/mnt/Steam")),
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

    settings
      .games
      .insert("riskofrain2".to_string(), GameSettings::new("/games/ror2"));
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
        list_game_folder("valheim", "/games/valheim"),
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
        list_game_folder("valheim", "/games/valheim"),
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
