//! Runs [`Command`]s on a Tokio thread of their own and answers each with a
//! [`Msg`], so the UI thread never waits on disk or network.

mod portability;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use rivet_core::browse::{Fetch, Index};
use rivet_core::install::{
  FailedPackage, Install, Installed, InstalledPackage, Outcome, Progress, Stage,
};
use rivet_core::launch::{Launch, LaunchFailure, SteamOptionRead};
use rivet_core::manage::{SetEnabled, Uninstall};
use rivet_core::picker::NotFound;
use rivet_core::readme;
use rivet_core::settings::Settings;
use rivet_core::switcher::ProfileChange;
use rivet_core::updates::Update;
use rivet_core::{Command, Msg};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use thunderstore_engine::client::{Refreshed, ThunderstoreClient};
use thunderstore_engine::ecosystem::Ecosystem;
use thunderstore_engine::profile::modlist::{self, ProfileMod};
use thunderstore_engine::profile::target::InstallTarget;
use thunderstore_engine::profile::{self, launch, layout};
use thunderstore_engine::progress::{
  self as engine, CancelToken, OperationId, ProgressEvent, StageUpdate,
};

/// Where commands read and write the app's files, and the Thunderstore host
/// they fetch from.
#[derive(Debug, Clone)]
pub struct Config {
  /// The settings TOML file. Its folder is created on the first save.
  pub settings: PathBuf,
  /// The folder package indexes and downloaded archives are cached in.
  pub cache: PathBuf,
  /// The folder profiles and the extracted packages they are installed from
  /// are kept in. The engine records absolute paths into the extracted
  /// packages, so they cannot live in `cache`.
  pub data: PathBuf,
  /// The Thunderstore host, such as [`THUNDERSTORE`].
  pub thunderstore: String,
  /// The game definitions installs are routed by.
  pub ecosystem: Ecosystem,
  /// The Steam install whose launch options are read when the user chose
  /// none, or `None` when none was found.
  pub steam: Option<PathBuf>,
  /// Every Steam install searched for a game's folder, in order, when the
  /// user chose none. Empty when none was found.
  pub steam_installs: Vec<PathBuf>,
  /// Starts a prepared game, such as [`spawn_detached`].
  pub spawn: fn(std::process::Command) -> std::io::Result<()>,
  /// The cancel flags operations stop on.
  pub cancels: Cancels,
}

/// The cancel flags of operations, by operation id. Clones share the flags.
#[derive(Debug, Clone, Default)]
pub struct Cancels(Arc<Mutex<HashMap<u64, CancelToken>>>);

impl Cancels {
  /// Cancels operation `id`, whether or not it has started.
  fn cancel(&self, id: u64) {
    self.flags().entry(id).or_default().cancel();
  }

  /// The flag operation `id` stops on, until [`Cancels::forget`].
  fn token(&self, id: u64) -> CancelToken {
    self.flags().entry(id).or_default().clone()
  }

  fn forget(&self, id: u64) {
    self.flags().remove(&id);
  }

  fn flags(&self) -> std::sync::MutexGuard<'_, HashMap<u64, CancelToken>> {
    self.0.lock().unwrap_or_else(PoisonError::into_inner)
  }
}

/// Starts `command` without waiting for it to exit.
///
/// # Errors
///
/// Why the process could not be started.
pub fn spawn_detached(mut command: std::process::Command) -> std::io::Result<()> {
  let mut child = command.spawn()?;

  // Waited on aside so the exited process is reaped rather than left a zombie.
  std::thread::spawn(move || child.wait());

  Ok(())
}

/// The public Thunderstore host.
pub const THUNDERSTORE: &str = "https://thunderstore.io";

/// The `User-Agent` sent with every request the app makes.
pub const USER_AGENT: &str = concat!("rivet/", env!("CARGO_PKG_VERSION"));

/// Runs one command to completion and returns the message that answers it, or
/// `None` for a command nothing answers. Messages it sends before the answer,
/// such as an operation's progress, go to `progress`.
pub async fn run(
  command: Command,
  config: &Config,
  progress: &UnboundedSender<Msg>,
) -> Option<Msg> {
  let answer = match command {
    Command::Cancel(operation) => {
      config.cancels.cancel(operation);

      return None;
    }
    Command::LoadSettings => Msg::SettingsLoaded(load_settings(&config.settings).await),
    Command::SaveSettings(settings) => {
      Msg::SettingsSaved(save_settings(&config.settings, &settings).await)
    }
    Command::FindInSteam {
      game,
      app_id,
      steam,
    } => {
      let installs = steam.map_or_else(|| config.steam_installs.clone(), |steam| vec![steam]);

      Msg::SteamSearched {
        game,
        folder: find_in_steam(installs, app_id).await,
      }
    }
    Command::ListFolder(folder) => {
      let entries = list_folder(&folder).await;

      Msg::FolderListed { folder, entries }
    }
    Command::ReadCachedIndex(community) => {
      let index = match client(config, &community) {
        Ok(client) => client.cached_index().await,
        Err(error) => Err(error),
      };

      Msg::CachedIndexRead {
        community,
        index: index
          .map(|index| index.map(Index::from))
          .map_err(|error| error.to_string()),
      }
    }
    Command::RefreshIndex { refresh, fetch } => {
      let index = refresh_index(config, &refresh.community, fetch).await;

      Msg::IndexRefreshed {
        refresh,
        index: index.map_err(|error| error.to_string()),
      }
    }
    Command::OpenProfile { game, profile } => {
      let installed = open_profile(config, &game, &profile).await;

      Msg::ProfileOpened {
        game,
        profile,
        installed,
      }
    }
    Command::Install(install) => {
      let operation = install.operation;
      let outcome = install_package(config, install, progress.clone()).await;

      config.cancels.forget(operation);

      Msg::OperationFinished {
        operation,
        outcome: outcome.map_err(|error| error.to_string()),
      }
    }
    Command::Update(update) => {
      let operation = update.operation;
      let outcome = update_packages(config, update, progress.clone()).await;

      config.cancels.forget(operation);

      Msg::OperationFinished {
        operation,
        outcome: outcome.map_err(|error| error.to_string()),
      }
    }
    Command::CheckUninstall {
      game,
      profile,
      packages,
    } => {
      let untracked = check_uninstall(config, &game, &profile, &packages).await;

      Msg::UninstallChecked {
        game,
        profile,
        packages,
        untracked,
      }
    }
    Command::Uninstall(uninstall) => {
      let operation = uninstall.operation;
      let outcome = uninstall_packages(config, uninstall, progress.clone()).await;

      config.cancels.forget(operation);

      Msg::OperationFinished { operation, outcome }
    }
    Command::SetEnabled(set) => {
      let operation = set.operation;
      let outcome = set_enabled(config, set, progress.clone()).await;

      config.cancels.forget(operation);

      Msg::OperationFinished { operation, outcome }
    }
    Command::Launch(launch) => Msg::LaunchFinished {
      launch: launch.id,
      result: start(config, launch).await,
    },
    Command::ListGameFolder { game, folder } => Msg::GameFolderListed {
      entries: list_folder(&folder).await,
      game,
    },
    Command::ReadSteamOption {
      game,
      app_id,
      context,
      steam,
    } => {
      let script = launch::wrapper_path(&config.data, &game);
      let plan = launch::launch_plan_from(
        Path::new(""),
        &launch::LaunchFacts::default(),
        &config.ecosystem,
        &game,
        &context,
      )
      .ok();
      let line = plan
        .as_ref()
        .and_then(|plan| plan.steam_launch_options(&script));
      let wrapper = plan
        .is_some_and(|plan| plan.steam_wrapper.is_some())
        .then_some(script);
      let steam = steam.or_else(|| config.steam.clone());
      let current = match line {
        Some(_) => read_steam_option(steam, wrapper, app_id).await,
        None => Ok(None),
      };

      Msg::SteamOptionRead(SteamOptionRead {
        game,
        line,
        current,
      })
    }
    Command::ListProfiles(game) => {
      let names = list_profiles(config, &game).await;

      Msg::ProfilesListed { game, names }
    }
    Command::ChangeProfile { game, change } => {
      let result = change_profile(config, &game, &change).await;

      Msg::ProfileChanged {
        game,
        change,
        result,
      }
    }
    Command::FetchReadme(request) => Msg::ReadmeFetched {
      request: request.id,
      readme: fetch_readme(config, &request).await,
    },
    Command::Export(export) => Msg::ProfileExported {
      result: portability::export(config, &export).await,
      export,
    },
    Command::ReadShared { community, source } => Msg::SharedRead {
      shared: portability::read_shared(config, &community, &source).await,
      source,
    },
    Command::Import(import) => {
      let operation = import.operation;
      let outcome = portability::import(config, import, progress.clone()).await;

      config.cancels.forget(operation);

      Msg::OperationFinished { operation, outcome }
    }
  };

  Some(answer)
}

async fn fetch_readme(
  config: &Config,
  request: &readme::Request,
) -> Result<Option<String>, String> {
  let package = &request.package;
  let client = client(config, &request.community).map_err(|error| error.to_string())?;

  client
    .get_readme(&package.owner, &package.name, &package.version)
    .await
    .map_err(|error| error.to_string())
}

async fn list_profiles(config: &Config, game: &str) -> Result<Vec<String>, String> {
  let (data, game) = (config.data.clone(), game.to_string());

  tokio::task::spawn_blocking(move || layout::list(&data, &game))
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

/// Makes `change` to `game`'s profiles with the engine.
async fn change_profile(config: &Config, game: &str, change: &ProfileChange) -> Result<(), String> {
  let (data, game, change) = (config.data.clone(), game.to_string(), change.clone());

  tokio::task::spawn_blocking(move || match change {
    ProfileChange::Create(name) => layout::create(&data, &game, &name).map(drop),
    ProfileChange::Rename { from, to } => layout::rename(&data, &game, &from, &to),
    ProfileChange::Duplicate { from, to } => layout::duplicate(&data, &game, &from, &to).map(drop),
    ProfileChange::Delete(name) => layout::delete(&data, &game, &name),
  })
  .await
  .map_err(|error| error.to_string())?
  .map_err(|error| error.to_string())
}

/// Prepares `launch` with the engine, then starts the game.
async fn start(config: &Config, launch: Launch) -> Result<(), LaunchFailure> {
  let (data, ecosystem, spawn) = (config.data.clone(), config.ecosystem.clone(), config.spawn);

  tokio::task::spawn_blocking(move || {
    let target = InstallTarget::resolve(data, launch.folder, &launch.game, Some(&launch.profile))
      .map_err(launch_failure)?;
    let prepared =
      launch::prepare_launch(&target, &ecosystem, &launch.context).map_err(launch_failure)?;
    let program = prepared
      .command
      .get_program()
      .to_string_lossy()
      .into_owned();

    spawn(prepared.command)
      .map_err(|error| LaunchFailure::Other(format!("{program} could not be started: {error}")))
  })
  .await
  .map_err(|error| LaunchFailure::Other(error.to_string()))?
}

fn launch_failure(error: thunderstore_engine::Error) -> LaunchFailure {
  match error {
    thunderstore_engine::Error::NoModLoader { loader } => LaunchFailure::NoModLoader(loader),
    thunderstore_engine::Error::MissingGameDir { path } => LaunchFailure::MissingGameFolder(path),
    other => LaunchFailure::Other(other.to_string()),
  }
}

/// The folder the game sold on Steam as `app_id` is installed in, searching
/// every library of each of `installs`.
async fn find_in_steam(installs: Vec<PathBuf>, app_id: u32) -> Result<PathBuf, NotFound> {
  if installs.is_empty() {
    return Err(NotFound::NoSteam);
  }

  tokio::task::spawn_blocking(move || rivet_platform::steam::game_folder(&installs, app_id))
    .await
    .map_err(|error| NotFound::Unreadable(error.to_string()))?
    .map_err(NotFound::Unreadable)?
    .ok_or(NotFound::NotInstalled)
}

/// Writes the game's Steam wrapper at `wrapper`, when the line names one, so
/// the line works once pasted, then reads the launch options for `app_id` of
/// the Steam install at `steam`.
async fn read_steam_option(
  steam: Option<PathBuf>,
  wrapper: Option<PathBuf>,
  app_id: u32,
) -> Result<Option<String>, String> {
  tokio::task::spawn_blocking(move || {
    if let Some(script) = wrapper {
      launch::write_wrapper_script(&script).map_err(|error| error.to_string())?;
    }

    let steam = steam.ok_or_else(|| "No Steam install was found".to_string())?;

    rivet_platform::steam::launch_options(&steam, app_id)
  })
  .await
  .map_err(|error| error.to_string())?
}

/// Reads `game`'s `name` profile, creating it first if it does not exist.
async fn open_profile(config: &Config, game: &str, name: &str) -> Result<Installed, String> {
  let (data, game, name) = (config.data.clone(), game.to_string(), name.to_string());

  tokio::task::spawn_blocking(move || {
    let folder = layout::profile_dir(&data, &game, &name);

    if !folder.is_dir() {
      layout::create(&data, &game, &name)?;
    }

    modlist::read(&folder).map(|recorded| installed(&recorded))
  })
  .await
  .map_err(|error| error.to_string())?
  .map_err(|error| error.to_string())
}

/// Installs `install.package` and everything it needs, enabling it if it was
/// disabled.
async fn install_package(
  config: &Config,
  install: Install,
  progress: UnboundedSender<Msg>,
) -> thunderstore_engine::Result<Outcome> {
  let requested = [install.package.clone()];
  let batch = Batch {
    operation: install.operation,
    game: &install.game,
    community: &install.community,
    profile: &install.profile,
    index: &install.index,
    desired: &requested,
    explicit: &requested,
  };

  install_batch(config, batch, progress).await
}

/// Updates `update.packages` to their latest versions with what they need,
/// leaving disabled ones disabled.
async fn update_packages(
  config: &Config,
  update: Update,
  progress: UnboundedSender<Msg>,
) -> thunderstore_engine::Result<Outcome> {
  let batch = Batch {
    operation: update.operation,
    game: &update.game,
    community: &update.community,
    profile: &update.profile,
    index: &update.index,
    desired: &update.packages,
    explicit: &[],
  };

  install_batch(config, batch, progress).await
}

/// Packages to install into a game's profile from an index.
struct Batch<'a> {
  operation: u64,
  game: &'a str,
  community: &'a str,
  profile: &'a str,
  index: &'a Index,
  /// Every package to install, each with what it needs.
  desired: &'a [String],
  /// The packages the user asked to have active. Any other package keeps its
  /// disabled state.
  explicit: &'a [String],
}

/// Installs `batch`, sending its progress to `progress` and stopping when the
/// operation is cancelled. An error means nothing was installed. When the
/// profile's record cannot be read back after the install, the outcome names
/// what the install reported installing, with the read failure as its
/// profile.
async fn install_batch(
  config: &Config,
  batch: Batch<'_>,
  progress: UnboundedSender<Msg>,
) -> thunderstore_engine::Result<Outcome> {
  let folder = layout::profile_dir(&config.data, batch.game, batch.profile);
  let before = modlist::read(&folder)?;
  let client = client(config, batch.community)?;
  let operation = operation(config, batch.operation, progress);
  let outcome = profile::install_batch_with(
    &folder,
    &config.data,
    &config.ecosystem,
    &batch.index.0,
    &client,
    batch.game,
    &profile::plan_install_batch_from(&before, batch.desired, batch.explicit),
    modlist::now_millis(),
    &operation,
  )
  .await?;
  let before = installed(&before);
  let (changed, after) = match modlist::read(&folder) {
    Ok(after) => {
      let after = installed(&after);
      let changed = after
        .iter()
        .filter(|(name, package)| {
          before
            .get(*name)
            .is_none_or(|earlier| earlier.version != package.version)
        })
        .map(|(name, _)| name.clone())
        .collect();

      (changed, Ok(after))
    }
    Err(error) => (outcome.succeeded, Err(error.to_string())),
  };

  Ok(Outcome {
    changed,
    failed: outcome
      .failed
      .into_iter()
      .map(|(package, error)| FailedPackage {
        package,
        reason: error.to_string(),
      })
      .collect(),
    cancelled: outcome.cancelled,
    profile: after,
  })
}

/// The folders no package records in `game`'s `name` profile, which
/// uninstalling `packages` would offer to delete, or why it would be refused.
async fn check_uninstall(
  config: &Config,
  game: &str,
  name: &str,
  packages: &[String],
) -> Result<Vec<PathBuf>, String> {
  let folder = layout::profile_dir(&config.data, game, name);
  let (ecosystem, game, packages) = (
    config.ecosystem.clone(),
    game.to_string(),
    packages.to_vec(),
  );

  tokio::task::spawn_blocking(move || {
    profile::plan_uninstall_batch(&folder, &ecosystem, &game, Some(&packages))
      .map(|batch| batch.untracked)
      .map_err(|error| error.to_string())
  })
  .await
  .map_err(|error| error.to_string())?
}

/// Removes `uninstall.packages`, and of the folders no package records only
/// those in `uninstall.delete` that are still unrecorded, sending its progress
/// to `progress` and stopping when the operation is cancelled.
async fn uninstall_packages(
  config: &Config,
  uninstall: Uninstall,
  progress: UnboundedSender<Msg>,
) -> Result<Outcome, String> {
  let folder = layout::profile_dir(&config.data, &uninstall.game, &uninstall.profile);
  let ecosystem = config.ecosystem.clone();
  let operation = operation(config, uninstall.operation, progress);

  tokio::task::spawn_blocking(move || {
    let mut batch = profile::plan_uninstall_batch(
      &folder,
      &ecosystem,
      &uninstall.game,
      Some(&uninstall.packages),
    )?;

    batch
      .untracked
      .retain(|untracked| uninstall.delete.contains(untracked));

    if !batch.untracked.is_empty() {
      batch.confirm_untracked();
    }

    let outcome =
      profile::uninstall_batch_with(&folder, &ecosystem, &uninstall.game, &batch, &operation)?;

    Ok(batch_outcome(&folder, outcome))
  })
  .await
  .map_err(|error| error.to_string())?
  .map_err(|error: thunderstore_engine::Error| error.to_string())
}

/// Enables or disables `set.packages`, sending its progress to `progress` and
/// stopping when the operation is cancelled. Mod loaders are left as they are.
async fn set_enabled(
  config: &Config,
  set: SetEnabled,
  progress: UnboundedSender<Msg>,
) -> Result<Outcome, String> {
  let folder = layout::profile_dir(&config.data, &set.game, &set.profile);
  let ecosystem = config.ecosystem.clone();
  let operation = operation(config, set.operation, progress);

  tokio::task::spawn_blocking(move || {
    let batch =
      profile::plan_set_enabled_batch(&folder, &ecosystem, Some(&set.packages), set.enabled)?;
    let outcome =
      profile::set_enabled_batch_with(&folder, &ecosystem, &set.game, &batch, &operation)?;

    Ok(batch_outcome(&folder, outcome))
  })
  .await
  .map_err(|error| error.to_string())?
  .map_err(|error: thunderstore_engine::Error| error.to_string())
}

/// What a batch did to the profile in `folder`, with the profile read back.
fn batch_outcome(folder: &Path, outcome: profile::BatchOutcome) -> Outcome {
  let mut changed = outcome.succeeded;

  changed.sort();

  Outcome {
    changed,
    failed: outcome
      .failed
      .into_iter()
      .map(|(package, error)| FailedPackage {
        package,
        reason: error.to_string(),
      })
      .collect(),
    cancelled: outcome.cancelled,
    profile: modlist::read(folder)
      .map(|recorded| installed(&recorded))
      .map_err(|error| error.to_string()),
  }
}

/// The engine's handle on operation `id`: it stops on the operation's cancel
/// flag and sends its progress to `progress`.
fn operation(config: &Config, id: u64, progress: UnboundedSender<Msg>) -> engine::Operation {
  engine::Operation::new(OperationId(id))
    .with_cancel(config.cancels.token(id))
    .with_progress(move |event: ProgressEvent| {
      if let Some(update) = progress_of(event) {
        progress
          .unbounded_send(Msg::OperationProgress {
            operation: id,
            progress: update,
          })
          .ok();
      }
    })
}

/// The progress an engine event reports, or `None` for the end of a stage,
/// which the start of the next one replaces.
fn progress_of(event: ProgressEvent) -> Option<Progress> {
  let stage = match event.stage {
    engine::Stage::Downloading => Stage::Downloading,
    engine::Stage::Extracting => Stage::Extracting,
    engine::Stage::Installing => Stage::Installing,
    engine::Stage::Uninstalling => Stage::Uninstalling,
    engine::Stage::Enabling => Stage::Enabling,
    engine::Stage::Disabling => Stage::Disabling,
    _ => return None,
  };
  let (done, total) = match event.update {
    StageUpdate::Started { total } => (0, total),
    StageUpdate::Advanced { done, total } => (done, total),
    _ => return None,
  };

  Some(Progress {
    package: event.package,
    stage,
    done,
    total,
  })
}

fn installed(recorded: &[ProfileMod]) -> Installed {
  recorded
    .iter()
    .map(|entry| {
      let package = InstalledPackage {
        author: entry.author_name.clone(),
        version: entry.version_number.to_string(),
        enabled: entry.enabled,
        dependencies: entry
          .dependencies
          .iter()
          .map(|dependency| {
            dependency
              .rsplit_once('-')
              .map_or(dependency.as_str(), |(full_name, _version)| full_name)
              .to_string()
          })
          .collect(),
        installed_at: entry.installed_at_time,
      };

      (entry.name.clone(), package)
    })
    .collect()
}

/// Fetches `community`'s index as `fetch` asks, with `None` when the cache is
/// current.
async fn refresh_index(
  config: &Config,
  community: &str,
  fetch: Fetch,
) -> thunderstore_engine::Result<Option<Index>> {
  let client = client(config, community)?;

  match fetch {
    Fetch::Always => Ok(Some(client.refresh_index().await?.into())),
    Fetch::IfStale => match client.refresh_if_stale().await? {
      Refreshed::Updated(index) => Ok(Some(index.into())),
      Refreshed::Unchanged => Ok(None),
    },
  }
}

/// A Thunderstore client for one community. Building one reads and sends
/// nothing.
fn client(config: &Config, community: &str) -> thunderstore_engine::Result<ThunderstoreClient> {
  ThunderstoreClient::builder()
    .base_url(&config.thunderstore)
    .community(community)
    .cache_dir(&config.cache)
    .user_agent(USER_AGENT)
    .build()
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
/// Commands sent on the returned sender run concurrently on that runtime, but
/// settings saves run one at a time in the order sent, so the last one sent
/// is the one left on disk. Answers arrive on the returned receiver. The
/// runtime stops once the sender is dropped.
///
/// # Panics
///
/// Panics if the runtime or the thread that owns it cannot be started.
pub fn spawn(config: Config) -> (UnboundedSender<Command>, UnboundedReceiver<Msg>) {
  let (commands, mut pending) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (saves, mut saving) = unbounded::<Command>();
  let config = Arc::new(config);
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .thread_name("commands")
    .enable_all()
    .build()
    .expect("tokio runtime");

  std::thread::Builder::new()
    .name("commands".into())
    .spawn(move || {
      runtime.block_on(async move {
        let saver = {
          let answers = answers.clone();
          let config = config.clone();

          tokio::spawn(async move {
            while let Some(save) = saving.next().await {
              if let Some(answer) = run(save, &config, &answers).await {
                answers.unbounded_send(answer).ok();
              }
            }
          })
        };

        while let Some(command) = pending.next().await {
          if let Command::SaveSettings(_) = command {
            saves.unbounded_send(command).ok();

            continue;
          }

          let answers = answers.clone();
          let config = config.clone();

          tokio::spawn(async move {
            if let Some(answer) = run(command, &config, &answers).await {
              answers.unbounded_send(answer).ok();
            }
          });
        }

        drop(saves);
        saver.await.ok();
      });
    })
    .expect("commands thread");

  (commands, inbox)
}

#[cfg(test)]
mod tests {
  use super::*;
  use rivet_core::browse::{Fetch, Refresh};
  use rivet_core::install::{DEFAULT_PROFILE, Install, Installed, Outcome, Stage};
  use rivet_core::launch::{Launch, LaunchFailure, SteamOptionRead};
  use rivet_core::picker::NotFound;
  use rivet_core::settings::GameSettings;
  use tempfile::TempDir;
  use thunderstore_engine::ecosystem::LoaderKind;
  use thunderstore_engine::models::PackageIndex;
  use thunderstore_engine::profile::launch::{
    HostOs, LaunchContext, LaunchMode, Runtime, StorePlatform,
  };
  use thunderstore_engine::test_support::{MockThunderstore, index_json, readme, readme_path};

  /// A config whose files sit in `dir`, against the public Thunderstore.
  pub(crate) fn config_in(dir: &TempDir) -> Config {
    Config {
      settings: dir.path().join("config").join("settings.toml"),
      cache: dir.path().join("cache"),
      data: dir.path().join("data"),
      thunderstore: THUNDERSTORE.to_string(),
      ecosystem: Ecosystem::bundled(),
      steam: None,
      steam_installs: Vec::new(),
      spawn: |_| panic!("a test tried to start a game"),
      cancels: Cancels::default(),
    }
  }

  /// A mock Thunderstore serving its package index, and its Last-Modified to a
  /// HEAD, as the valheim community's,
  /// and answering `404` for the `gone` community.
  pub(crate) fn mock_thunderstore() -> MockThunderstore {
    let mut mock = MockThunderstore::start();
    let url = mock.url();
    let server = mock.server_mut();

    server
      .mock("GET", "/c/valheim/api/v1/package/")
      .with_status(200)
      .with_header("Content-Type", "application/json")
      .with_header("Last-Modified", "Wed, 21 Feb 2024 15:30:45 GMT")
      .with_body(index_json(&url))
      .create();
    server
      .mock("HEAD", "/c/valheim/api/v1/package/")
      .with_status(200)
      .with_header("Last-Modified", "Wed, 21 Feb 2024 15:30:45 GMT")
      .create();
    server
      .mock("GET", "/c/gone/api/v1/package/")
      .with_status(404)
      .create();

    mock
  }

  pub(crate) fn config_for(dir: &TempDir, mock: &MockThunderstore) -> Config {
    Config {
      thunderstore: mock.url(),
      ..config_in(dir)
    }
  }

  pub(crate) fn run_now(command: Command, config: &Config) -> Msg {
    run_reporting(command, config).1
  }

  /// Runs `command`, returning the messages it sent along the way and its
  /// answer.
  pub(crate) fn run_reporting(command: Command, config: &Config) -> (Vec<Msg>, Msg) {
    let (along_the_way, answer) = run_any(command, config);

    (along_the_way, answer.expect("the command is answered"))
  }

  /// Runs `command`, returning the messages it sent along the way and its
  /// answer, if it has one.
  pub(crate) fn run_any(command: Command, config: &Config) -> (Vec<Msg>, Option<Msg>) {
    let (progress, mut sent) = unbounded();
    let answer = tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .unwrap()
      .block_on(run(command, config, &progress));

    drop(progress);

    let along_the_way = futures::executor::block_on_stream(&mut sent).collect();

    (along_the_way, answer)
  }

  fn package_count(index: &Index) -> usize {
    index.0.len()
  }

  #[test]
  fn reading_the_cache_before_any_fetch_finds_nothing() {
    let dir = TempDir::new().unwrap();
    let mock = mock_thunderstore();

    let answer = run_now(
      Command::ReadCachedIndex("valheim".to_string()),
      &config_for(&dir, &mock),
    );

    assert_eq!(
      answer,
      Msg::CachedIndexRead {
        community: "valheim".to_string(),
        index: Ok(None),
      }
    );
  }

  #[test]
  fn a_refresh_fetches_the_index_and_caches_it_for_the_next_start() {
    let dir = TempDir::new().unwrap();
    let mock = mock_thunderstore();
    let config = config_for(&dir, &mock);

    let fetched = run_now(refresh("valheim", Fetch::Always), &config);

    drop(mock);

    let cached = run_now(Command::ReadCachedIndex("valheim".to_string()), &config);

    let Msg::IndexRefreshed {
      refresh,
      index: Ok(Some(fetched)),
    } = fetched
    else {
      panic!("unexpected answer {fetched:?}");
    };
    let Msg::CachedIndexRead {
      index: Ok(Some(cached)),
      ..
    } = cached
    else {
      panic!("unexpected answer {cached:?}");
    };

    assert_eq!(refresh, refresh_of("valheim"));
    assert_eq!(package_count(&fetched), 5);
    assert_eq!(package_count(&cached), 5);
  }

  #[test]
  fn a_failed_refresh_answers_with_why() {
    let dir = TempDir::new().unwrap();
    let mock = mock_thunderstore();

    let answer = run_now(refresh("gone", Fetch::Always), &config_for(&dir, &mock));

    assert!(
      matches!(&answer, Msg::IndexRefreshed { refresh, index: Err(_) } if *refresh == refresh_of("gone")),
      "{answer:?}"
    );
  }

  fn fetch_readme(owner: &str, name: &str, version: &str) -> Command {
    Command::FetchReadme(readme::Request {
      id: 7,
      community: "valheim".to_string(),
      package: readme::Package {
        owner: owner.to_string(),
        name: name.to_string(),
        version: version.to_string(),
      },
    })
  }

  #[test]
  fn a_readme_is_fetched_for_the_version_asked_for() {
    let dir = TempDir::new().unwrap();
    let mock = MockThunderstore::start();

    let answer = run_now(
      fetch_readme("Owner", "ModD", "1.0.0"),
      &config_for(&dir, &mock),
    );

    assert_eq!(
      answer,
      Msg::ReadmeFetched {
        request: 7,
        readme: Ok(Some(readme("ModD", "1.0.0"))),
      }
    );
  }

  #[test]
  fn a_package_without_a_readme_answers_with_none() {
    let dir = TempDir::new().unwrap();
    let mock = MockThunderstore::start();

    let answer = run_now(
      fetch_readme("Owner", "ModB", "1.0.0"),
      &config_for(&dir, &mock),
    );

    assert_eq!(
      answer,
      Msg::ReadmeFetched {
        request: 7,
        readme: Ok(None),
      }
    );
  }

  #[test]
  fn a_readme_that_cannot_be_fetched_answers_with_why() {
    let dir = TempDir::new().unwrap();
    let mut mock = MockThunderstore::start();

    mock
      .server_mut()
      .mock("GET", readme_path("Owner", "Locked", "1.0.0").as_str())
      .with_status(403)
      .create();

    let answer = run_now(
      fetch_readme("Owner", "Locked", "1.0.0"),
      &config_for(&dir, &mock),
    );

    assert!(
      matches!(
        &answer,
        Msg::ReadmeFetched {
          request: 7,
          readme: Err(_)
        }
      ),
      "{answer:?}"
    );
  }

  #[test]
  fn a_staleness_check_downloads_only_when_the_cache_is_out_of_date() {
    let dir = TempDir::new().unwrap();
    let mock = mock_thunderstore();
    let config = config_for(&dir, &mock);

    let first = run_now(refresh("valheim", Fetch::IfStale), &config);
    let second = run_now(refresh("valheim", Fetch::IfStale), &config);

    assert!(
      matches!(&first, Msg::IndexRefreshed { index: Ok(Some(index)), .. } if package_count(index) == 5),
      "{first:?}"
    );
    assert_eq!(
      second,
      Msg::IndexRefreshed {
        refresh: refresh_of("valheim"),
        index: Ok(None),
      }
    );
  }

  #[test]
  fn a_staleness_check_that_cannot_reach_thunderstore_answers_with_why() {
    let dir = TempDir::new().unwrap();
    let mock = mock_thunderstore();
    let config = config_for(&dir, &mock);

    run_now(refresh("valheim", Fetch::Always), &config);
    drop(mock);

    let answer = run_now(refresh("valheim", Fetch::IfStale), &config);

    assert!(
      matches!(&answer, Msg::IndexRefreshed { index: Err(_), .. }),
      "{answer:?}"
    );
  }

  fn refresh_of(community: &str) -> Refresh {
    Refresh {
      community: community.to_string(),
      id: 7,
    }
  }

  fn refresh(community: &str, fetch: Fetch) -> Command {
    Command::RefreshIndex {
      refresh: refresh_of(community),
      fetch,
    }
  }

  fn valheim_settings() -> Settings {
    Settings {
      last_game: Some("valheim".to_string()),
      games: [("valheim".to_string(), GameSettings::new("/games/Valheim"))].into(),
      ..Settings::default()
    }
  }

  fn open_profile(game: &str) -> Command {
    Command::OpenProfile {
      game: game.to_string(),
      profile: DEFAULT_PROFILE.to_string(),
    }
  }

  /// Each package of `profile` as its `Owner-Name`, version, and whether it
  /// is enabled.
  pub(crate) fn packages(profile: &Installed) -> Vec<(String, String, bool)> {
    profile
      .iter()
      .map(|(name, package)| (name.clone(), package.version.clone(), package.enabled))
      .collect()
  }

  fn enabled(name: &str) -> (String, String, bool) {
    (name.to_string(), "1.0.0".to_string(), true)
  }

  fn disabled(name: &str) -> (String, String, bool) {
    (name.to_string(), "1.0.0".to_string(), false)
  }

  pub(crate) fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
  }

  /// The mock's package index, plus `Owner-ModE`, which needs `Owner-ModA` and
  /// whose archive the mock answers with `404`.
  pub(crate) fn mock_index(mock: &mut MockThunderstore) -> Index {
    let url = mock.url();
    let mut packages: Vec<serde_json::Value> = serde_json::from_str(&index_json(&url)).unwrap();
    let mut broken = packages
      .iter()
      .find(|package| package["full_name"] == "Owner-ModC")
      .unwrap()
      .clone();

    broken["name"] = "ModE".into();
    broken["full_name"] = "Owner-ModE".into();
    broken["versions"][0]["name"] = "ModE".into();
    broken["versions"][0]["full_name"] = "Owner-ModE".into();
    broken["versions"][0]["download_url"] = format!("{url}/dl/ModE.zip").into();
    packages.push(broken);
    mock
      .server_mut()
      .mock("GET", "/dl/ModE.zip")
      .with_status(404)
      .create();

    let packages: Vec<thunderstore_engine::models::Package> =
      serde_json::from_value(serde_json::Value::Array(packages)).unwrap();

    PackageIndex::from(packages).into()
  }

  fn install(package: &str, index: &Index) -> Command {
    Command::Install(Install {
      operation: 3,
      game: "valheim".to_string(),
      community: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      package: package.to_string(),
      index: index.clone(),
    })
  }

  pub(crate) fn default_profile(config: &Config) -> PathBuf {
    config
      .data
      .join("valheim")
      .join("profiles")
      .join(DEFAULT_PROFILE)
  }

  #[test]
  fn opening_a_game_without_a_default_profile_creates_it() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    let answer = run_now(open_profile("valheim"), &config);

    assert_eq!(
      answer,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(Installed::new()),
      }
    );
    assert!(default_profile(&config).is_dir());
  }

  #[test]
  fn installing_a_package_brings_its_dependencies_into_the_profile() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let index = mock_index(&mut mock);

    run_now(open_profile("valheim"), &config);

    let (progress, answer) = run_reporting(install("Owner-ModC", &index), &config);
    let reopened = run_now(open_profile("valheim"), &config);

    let Msg::OperationFinished {
      operation: 3,
      outcome: Ok(outcome),
    } = answer
    else {
      panic!("unexpected answer {answer:?}");
    };
    let Msg::ProfileOpened {
      installed: Ok(profile),
      ..
    } = reopened
    else {
      panic!("unexpected answer {reopened:?}");
    };

    assert_eq!(outcome.changed, names(&["Owner-ModA", "Owner-ModC"]));
    assert_eq!(outcome.failed, vec![]);
    assert_eq!(outcome.profile.as_ref(), Ok(&profile));
    assert_eq!(
      packages(&profile),
      [enabled("Owner-ModA"), enabled("Owner-ModC")]
    );
    assert_eq!(profile["Owner-ModC"].author, "Owner");
    assert_eq!(profile["Owner-ModC"].dependencies, names(&["Owner-ModA"]));
    assert!(
      default_profile(&config)
        .join("BepInEx/plugins/Owner-ModC/ModC.dll")
        .is_file()
    );

    let stages: Vec<(String, Stage)> = progress
      .into_iter()
      .map(|msg| match msg {
        Msg::OperationProgress {
          operation: 3,
          progress,
        } => (progress.package, progress.stage),
        other => panic!("unexpected message {other:?}"),
      })
      .collect();

    for stage in [Stage::Downloading, Stage::Extracting, Stage::Installing] {
      assert!(
        stages.contains(&("Owner-ModC".to_string(), stage)),
        "{stage:?} missing from {stages:?}"
      );
    }
  }

  #[test]
  fn a_cancelled_install_installs_nothing_and_names_what_it_left() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let index = mock_index(&mut mock);

    run_now(open_profile("valheim"), &config);

    assert_eq!(run_any(Command::Cancel(3), &config), (vec![], None));

    let answer = run_now(install("Owner-ModC", &index), &config);

    let Msg::OperationFinished {
      operation: 3,
      outcome: Ok(outcome),
    } = answer
    else {
      panic!("unexpected answer {answer:?}");
    };

    assert_eq!(outcome.cancelled, names(&["Owner-ModC"]));
    assert_eq!(outcome.changed, Vec::<String>::new());
    assert_eq!(outcome.failed, vec![]);
    assert_eq!(outcome.profile, Ok(Installed::new()));
  }

  #[test]
  fn a_cancel_stops_only_the_run_of_the_operation_it_was_sent_for() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let index = mock_index(&mut mock);

    run_now(open_profile("valheim"), &config);
    run_any(Command::Cancel(3), &config);
    run_now(install("Owner-ModC", &index), &config);

    let answer = run_now(install("Owner-ModC", &index), &config);

    let Msg::OperationFinished {
      outcome: Ok(outcome),
      ..
    } = answer
    else {
      panic!("unexpected answer {answer:?}");
    };

    assert_eq!(outcome.cancelled, Vec::<String>::new());
    assert_eq!(outcome.changed, names(&["Owner-ModA", "Owner-ModC"]));
  }

  #[test]
  fn a_failed_download_names_the_package_and_leaves_the_profile_as_it_was() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let index = mock_index(&mut mock);

    run_now(open_profile("valheim"), &config);
    run_now(install("Owner-ModB", &index), &config);

    let answer = run_now(install("Owner-ModE", &index), &config);

    let Msg::OperationFinished {
      operation: 3,
      outcome: Ok(outcome),
    } = answer
    else {
      panic!("unexpected answer {answer:?}");
    };

    assert_eq!(outcome.changed, Vec::<String>::new());
    assert_eq!(
      outcome.profile.as_ref().map(packages),
      Ok(vec![enabled("Owner-ModB")])
    );
    assert_eq!(
      outcome
        .failed
        .iter()
        .map(|failure| failure.package.as_str())
        .collect::<Vec<_>>(),
      ["Owner-ModE"]
    );
    assert!(
      outcome.failed[0].reason.contains("ModE"),
      "{:?}",
      outcome.failed[0].reason
    );
  }

  fn change(change: ProfileChange) -> Command {
    Command::ChangeProfile {
      game: "valheim".to_string(),
      change,
    }
  }

  fn changed(change: ProfileChange, result: Result<(), String>) -> Msg {
    Msg::ProfileChanged {
      game: "valheim".to_string(),
      change,
      result,
    }
  }

  fn list(config: &Config) -> Msg {
    run_now(Command::ListProfiles("valheim".to_string()), config)
  }

  fn listed(profiles: &[&str]) -> Msg {
    Msg::ProfilesListed {
      game: "valheim".to_string(),
      names: Ok(names(profiles)),
    }
  }

  pub(crate) fn profile_folder(config: &Config, name: &str) -> PathBuf {
    config.data.join("valheim").join("profiles").join(name)
  }

  #[test]
  fn a_created_profile_is_listed_with_the_others() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    run_now(open_profile("valheim"), &config);

    let created = ProfileChange::Create("Modded".to_string());
    let answer = run_now(change(created.clone()), &config);

    assert_eq!(answer, changed(created, Ok(())));
    assert_eq!(list(&config), listed(&["Default", "Modded"]));
  }

  #[test]
  fn listing_a_game_with_no_profiles_finds_none() {
    let dir = TempDir::new().unwrap();

    assert_eq!(list(&config_in(&dir)), listed(&[]));
  }

  #[test]
  fn an_invalid_name_creates_nothing_and_says_why() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    run_now(open_profile("valheim"), &config);

    let answer = run_now(change(ProfileChange::Create("CON".to_string())), &config);

    assert!(
      matches!(&answer, Msg::ProfileChanged { result: Err(reason), .. } if reason.contains("CON")),
      "{answer:?}"
    );
    assert_eq!(list(&config), listed(&["Default"]));
  }

  #[test]
  fn renaming_a_profile_moves_its_files_to_the_new_name() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    run_now(change(ProfileChange::Create("Modded".to_string())), &config);
    std::fs::write(profile_folder(&config, "Modded").join("notes.txt"), "kept").unwrap();

    let renamed = ProfileChange::Rename {
      from: "Modded".to_string(),
      to: "Heavy".to_string(),
    };
    let answer = run_now(change(renamed.clone()), &config);

    assert_eq!(answer, changed(renamed, Ok(())));
    assert_eq!(list(&config), listed(&["Heavy"]));
    assert_eq!(
      std::fs::read_to_string(profile_folder(&config, "Heavy").join("notes.txt")).unwrap(),
      "kept"
    );
  }

  #[test]
  fn renaming_onto_an_existing_profile_is_refused_and_changes_nothing() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    run_now(open_profile("valheim"), &config);
    run_now(change(ProfileChange::Create("Modded".to_string())), &config);

    let answer = run_now(
      change(ProfileChange::Rename {
        from: "Modded".to_string(),
        to: DEFAULT_PROFILE.to_string(),
      }),
      &config,
    );

    assert!(
      matches!(&answer, Msg::ProfileChanged { result: Err(_), .. }),
      "{answer:?}"
    );
    assert_eq!(list(&config), listed(&["Default", "Modded"]));
  }

  #[test]
  fn a_duplicated_profile_holds_the_same_packages() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let index = mock_index(&mut mock);

    run_now(open_profile("valheim"), &config);
    run_now(install("Owner-ModC", &index), &config);

    let duplicated = ProfileChange::Duplicate {
      from: DEFAULT_PROFILE.to_string(),
      to: "Copy".to_string(),
    };
    let answer = run_now(change(duplicated.clone()), &config);
    let copy = run_now(
      Command::OpenProfile {
        game: "valheim".to_string(),
        profile: "Copy".to_string(),
      },
      &config,
    );

    let Msg::ProfileOpened {
      profile: opened,
      installed: Ok(copied),
      ..
    } = copy
    else {
      panic!("unexpected answer {copy:?}");
    };

    assert_eq!(answer, changed(duplicated, Ok(())));
    assert_eq!(list(&config), listed(&["Copy", "Default"]));
    assert_eq!(opened, "Copy");
    assert_eq!(
      packages(&copied),
      [enabled("Owner-ModA"), enabled("Owner-ModC")]
    );
    assert!(
      profile_folder(&config, "Copy")
        .join("BepInEx/plugins/Owner-ModC/ModC.dll")
        .is_file()
    );
  }

  #[test]
  fn deleting_a_profile_removes_its_folder() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    run_now(open_profile("valheim"), &config);
    run_now(change(ProfileChange::Create("Modded".to_string())), &config);

    let deleted = ProfileChange::Delete("Modded".to_string());
    let answer = run_now(change(deleted.clone()), &config);

    assert_eq!(answer, changed(deleted, Ok(())));
    assert_eq!(list(&config), listed(&["Default"]));
    assert!(!profile_folder(&config, "Modded").exists());
  }

  #[test]
  fn deleting_a_missing_profile_says_why() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    let answer = run_now(change(ProfileChange::Delete("Gone".to_string())), &config);

    assert!(
      matches!(&answer, Msg::ProfileChanged { result: Err(reason), .. } if reason.contains("Gone")),
      "{answer:?}"
    );
  }

  #[test]
  fn the_selected_profile_is_remembered_after_a_restart() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);
    let mut settings = valheim_settings();

    if let Some(valheim) = settings.games.get_mut("valheim") {
      valheim.profile = Some("Modded".to_string());
    }

    run_now(Command::SaveSettings(settings.clone()), &config);

    let loaded = run_now(Command::LoadSettings, &config);

    assert_eq!(loaded, Msg::SettingsLoaded(Ok(settings)));
    assert!(
      std::fs::read_to_string(&config.settings)
        .unwrap()
        .contains("profile = \"Modded\"")
    );
  }

  /// A config against a mock Thunderstore whose Default profile holds
  /// `Owner-ModC` and the `Owner-ModA` it needs.
  pub(crate) fn with_mod_c(dir: &TempDir, mock: &mut MockThunderstore) -> Config {
    let config = config_for(dir, mock);
    let index = mock_index(mock);

    run_now(open_profile("valheim"), &config);
    run_now(install("Owner-ModC", &index), &config);

    config
  }

  pub(crate) fn set_enabled(packages: &[&str], enabled: bool) -> Command {
    Command::SetEnabled(SetEnabled {
      operation: 5,
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      packages: names(packages),
      enabled,
    })
  }

  fn check_uninstall(packages: &[&str]) -> Command {
    Command::CheckUninstall {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      packages: names(packages),
    }
  }

  fn uninstall(packages: &[&str], delete: Vec<PathBuf>) -> Command {
    Command::Uninstall(Uninstall {
      operation: 5,
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      packages: names(packages),
      delete,
    })
  }

  /// The outcome of operation 5.
  fn finished(answer: Msg) -> Outcome {
    match answer {
      Msg::OperationFinished {
        operation: 5,
        outcome: Ok(outcome),
      } => outcome,
      other => panic!("unexpected answer {other:?}"),
    }
  }

  #[test]
  fn disabling_then_enabling_a_package_records_each_state() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);

    let off = finished(run_now(set_enabled(&["Owner-ModC"], false), &config));
    let on = finished(run_now(set_enabled(&["Owner-ModC"], true), &config));

    assert_eq!(off.changed, names(&["Owner-ModC"]));
    assert_eq!(off.failed, vec![]);
    assert_eq!(
      off.profile.as_ref().map(packages),
      Ok(vec![enabled("Owner-ModA"), disabled("Owner-ModC")])
    );
    assert_eq!(on.changed, names(&["Owner-ModC"]));
    assert_eq!(
      on.profile.as_ref().map(packages),
      Ok(vec![enabled("Owner-ModA"), enabled("Owner-ModC")])
    );
    assert!(
      default_profile(&config)
        .join("BepInEx/plugins/Owner-ModC/ModC.dll")
        .is_file()
    );
  }

  /// The mock's package index as it was before `Owner-ModD` 2.0.0 was
  /// published.
  fn index_before_mod_d_2(mock: &MockThunderstore) -> Index {
    let mut packages: Vec<serde_json::Value> =
      serde_json::from_str(&index_json(&mock.url())).unwrap();
    let mod_d = packages
      .iter_mut()
      .find(|package| package["full_name"] == "Owner-ModD")
      .unwrap();

    mod_d["versions"]
      .as_array_mut()
      .unwrap()
      .retain(|version| version["version_number"] == "1.0.0");

    let packages: Vec<thunderstore_engine::models::Package> =
      serde_json::from_value(serde_json::Value::Array(packages)).unwrap();

    PackageIndex::from(packages).into()
  }

  fn update(packages: &[&str], index: &Index) -> Command {
    Command::Update(Update {
      operation: 5,
      game: "valheim".to_string(),
      community: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      packages: names(packages),
      index: index.clone(),
    })
  }

  #[test]
  fn an_update_moves_a_package_to_its_latest_version_and_keeps_it_disabled() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let index = mock_index(&mut mock);

    run_now(open_profile("valheim"), &config);
    run_now(install("Owner-ModD", &index_before_mod_d_2(&mock)), &config);
    run_now(set_enabled(&["Owner-ModD"], false), &config);

    let (progress, answer) = run_reporting(update(&["Owner-ModD"], &index), &config);
    let outcome = finished(answer);

    assert_eq!(outcome.changed, names(&["Owner-ModD"]));
    assert_eq!(outcome.failed, vec![]);
    assert_eq!(
      outcome.profile.as_ref().map(packages),
      Ok(vec![("Owner-ModD".to_string(), "2.0.0".to_string(), false)])
    );
    assert!(stages_of(progress).contains(&("Owner-ModD".to_string(), Stage::Downloading)));
  }

  #[test]
  fn an_uninstall_or_toggle_drops_a_cancel_sent_for_it() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);

    run_any(Command::Cancel(5), &config);
    run_now(set_enabled(&["Owner-ModC"], false), &config);

    assert!(!config.cancels.flags().contains_key(&5));

    run_any(Command::Cancel(5), &config);
    run_now(uninstall(&["Owner-ModC"], vec![]), &config);

    assert!(!config.cancels.flags().contains_key(&5));
  }

  /// The package and stage of each progress message operation 5 sent.
  fn stages_of(progress: Vec<Msg>) -> Vec<(String, Stage)> {
    progress
      .into_iter()
      .map(|msg| match msg {
        Msg::OperationProgress {
          operation: 5,
          progress,
        } => (progress.package, progress.stage),
        other => panic!("unexpected message {other:?}"),
      })
      .collect()
  }

  #[test]
  fn an_uninstall_or_toggle_reports_each_package_as_it_changes() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);
    let both = ["Owner-ModA", "Owner-ModC"];

    let (disabling, _) = run_reporting(set_enabled(&both, false), &config);
    let (enabling, _) = run_reporting(set_enabled(&both, true), &config);
    let (uninstalling, _) = run_reporting(uninstall(&["Owner-ModC"], vec![]), &config);

    assert_eq!(
      stages_of(disabling),
      [
        ("Owner-ModA".to_string(), Stage::Disabling),
        ("Owner-ModC".to_string(), Stage::Disabling),
      ]
    );
    assert_eq!(
      stages_of(enabling),
      [
        ("Owner-ModA".to_string(), Stage::Enabling),
        ("Owner-ModC".to_string(), Stage::Enabling),
      ]
    );
    assert_eq!(
      stages_of(uninstalling),
      [("Owner-ModC".to_string(), Stage::Uninstalling)]
    );
  }

  #[test]
  fn a_cancelled_toggle_changes_nothing_and_names_what_it_left() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);

    run_any(Command::Cancel(5), &config);

    let outcome = finished(run_now(
      set_enabled(&["Owner-ModA", "Owner-ModC"], false),
      &config,
    ));

    assert_eq!(outcome.cancelled, names(&["Owner-ModA", "Owner-ModC"]));
    assert_eq!(outcome.changed, Vec::<String>::new());
    assert_eq!(outcome.failed, vec![]);
    assert_eq!(
      outcome.profile.as_ref().map(packages),
      Ok(vec![enabled("Owner-ModA"), enabled("Owner-ModC")])
    );
  }

  #[test]
  fn a_cancelled_uninstall_keeps_the_packages_and_folders_it_did_not_reach() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);
    let stray = default_profile(&config).join("BepInEx/plugins/Someone-Stray");

    std::fs::create_dir_all(&stray).unwrap();

    let Msg::UninstallChecked {
      untracked: Ok(untracked),
      ..
    } = run_now(check_uninstall(&["Owner-ModC"]), &config)
    else {
      panic!("the check failed");
    };
    let kept = untracked[0].display().to_string();

    run_any(Command::Cancel(5), &config);

    let outcome = finished(run_now(uninstall(&["Owner-ModC"], untracked), &config));

    assert_eq!(outcome.cancelled, ["Owner-ModC".to_string(), kept]);
    assert_eq!(outcome.changed, Vec::<String>::new());
    assert!(stray.is_dir());
    assert!(
      default_profile(&config)
        .join("BepInEx/plugins/Owner-ModC/ModC.dll")
        .is_file()
    );
  }

  #[test]
  fn uninstalling_a_package_leaves_its_dependency() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);

    let outcome = finished(run_now(uninstall(&["Owner-ModC"], vec![]), &config));

    assert_eq!(outcome.changed, names(&["Owner-ModC"]));
    assert_eq!(outcome.failed, vec![]);
    assert_eq!(
      outcome.profile.as_ref().map(packages),
      Ok(vec![enabled("Owner-ModA")])
    );
    assert!(
      !default_profile(&config)
        .join("BepInEx/plugins/Owner-ModC")
        .exists()
    );
    assert!(
      default_profile(&config)
        .join("BepInEx/plugins/Owner-ModA/ModA.dll")
        .is_file()
    );
  }

  #[test]
  fn checking_an_uninstall_lists_untracked_folders_and_changes_nothing() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);
    let stray = default_profile(&config).join("BepInEx/plugins/Someone-Stray");

    std::fs::create_dir_all(&stray).unwrap();

    let answer = run_now(check_uninstall(&["Owner-ModC"]), &config);

    let Msg::UninstallChecked {
      packages: checked,
      untracked: Ok(untracked),
      ..
    } = answer
    else {
      panic!("unexpected answer {answer:?}");
    };

    assert_eq!(checked, names(&["Owner-ModC"]));
    assert_eq!(untracked.len(), 1, "{untracked:?}");
    assert!(untracked[0].ends_with("Someone-Stray"), "{untracked:?}");
    assert!(stray.is_dir());
    assert!(
      default_profile(&config)
        .join("BepInEx/plugins/Owner-ModC/ModC.dll")
        .is_file()
    );
  }

  #[test]
  fn an_uninstall_deletes_untracked_folders_only_when_the_user_agreed() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);
    let stray = default_profile(&config).join("BepInEx/plugins/Someone-Stray");

    std::fs::create_dir_all(&stray).unwrap();

    let Msg::UninstallChecked {
      untracked: Ok(untracked),
      ..
    } = run_now(check_uninstall(&["Owner-ModC"]), &config)
    else {
      panic!("the check failed");
    };

    finished(run_now(uninstall(&["Owner-ModC"], vec![]), &config));

    assert!(stray.is_dir());

    finished(run_now(uninstall(&["Owner-ModA"], untracked), &config));

    assert!(!stray.exists());
  }

  #[test]
  fn checking_the_uninstall_of_a_package_not_installed_says_why() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);

    let answer = run_now(check_uninstall(&["Owner-ModB"]), &config);

    assert!(
      matches!(&answer, Msg::UninstallChecked { untracked: Err(reason), .. } if reason.contains("Owner-ModB")),
      "{answer:?}"
    );
  }

  #[test]
  fn config_files_survive_uninstalling_and_reinstalling() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);
    let index = mock_index(&mut mock);
    let settings = default_profile(&config).join("BepInEx/config/Owner.ModC.cfg");

    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, "volume = 3").unwrap();
    finished(run_now(uninstall(&["Owner-ModC"], vec![]), &config));
    run_now(install("Owner-ModC", &index), &config);

    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "volume = 3");
  }

  const VALHEIM: u32 = 892970;

  /// A Steam install in `dir` whose user set `options` as Valheim's launch
  /// options.
  fn steam_with(dir: &TempDir, options: &str) -> PathBuf {
    let root = dir.path().join("Steam");
    let config = root.join("userdata/14502031/config");

    std::fs::create_dir_all(root.join("config")).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
      root.join("config/loginusers.vdf"),
      r#""users" { "76561197974767759" { "MostRecent" "1" } }"#,
    )
    .unwrap();
    std::fs::write(
      config.join("localconfig.vdf"),
      format!(
        r#""UserLocalConfigStore" {{ "Software" {{ "Valve" {{ "Steam" {{ "apps" {{ "{VALHEIM}" {{ "LaunchOptions" "{}" }} }} }} }} }} }}"#,
        options.replace('\\', "\\\\").replace('"', "\\\"")
      ),
    )
    .unwrap();

    root
  }

  fn find_valheim() -> Command {
    Command::FindInSteam {
      game: "valheim".to_string(),
      app_id: VALHEIM,
      steam: None,
    }
  }

  #[test]
  fn finding_a_game_searches_only_the_steam_folder_the_user_chose() {
    let dir = TempDir::new().unwrap();
    let chosen = dir.path().join("Chosen");
    let found = dir.path().join("Found");

    for (install, folder) in [(&chosen, "Chosen Valheim"), (&found, "Found Valheim")] {
      std::fs::create_dir_all(install.join("steamapps")).unwrap();
      std::fs::write(
        install.join(format!("steamapps/appmanifest_{VALHEIM}.acf")),
        format!(r#""AppState" {{ "installdir" "{folder}" }}"#),
      )
      .unwrap();
    }

    let config = Config {
      steam_installs: vec![found],
      ..config_in(&dir)
    };
    let find = Command::FindInSteam {
      game: "valheim".to_string(),
      app_id: VALHEIM,
      steam: Some(chosen.clone()),
    };

    assert_eq!(
      run_now(find, &config),
      steam_searched(Ok(chosen.join("steamapps/common/Chosen Valheim")))
    );
  }

  fn steam_searched(folder: Result<PathBuf, NotFound>) -> Msg {
    Msg::SteamSearched {
      game: "valheim".to_string(),
      folder,
    }
  }

  #[test]
  fn finding_a_game_in_steam_names_its_folder() {
    let dir = TempDir::new().unwrap();
    let install = dir.path().join("Steam");
    let steamapps = install.join("steamapps");

    std::fs::create_dir_all(&steamapps).unwrap();
    std::fs::write(
      steamapps.join(format!("appmanifest_{VALHEIM}.acf")),
      r#""AppState" { "installdir" "Valheim" }"#,
    )
    .unwrap();

    let config = Config {
      steam_installs: vec![install],
      ..config_in(&dir)
    };

    assert_eq!(
      run_now(find_valheim(), &config),
      steam_searched(Ok(steamapps.join("common/Valheim")))
    );
  }

  #[test]
  fn finding_a_game_steam_has_not_installed_says_so() {
    let dir = TempDir::new().unwrap();
    let install = dir.path().join("Steam");

    std::fs::create_dir_all(install.join("steamapps")).unwrap();

    let config = Config {
      steam_installs: vec![install],
      ..config_in(&dir)
    };

    assert_eq!(
      run_now(find_valheim(), &config),
      steam_searched(Err(NotFound::NotInstalled))
    );
  }

  #[test]
  fn finding_a_game_without_steam_says_so() {
    let dir = TempDir::new().unwrap();

    assert_eq!(
      run_now(find_valheim(), &config_in(&dir)),
      steam_searched(Err(NotFound::NoSteam))
    );
  }

  #[test]
  fn finding_a_game_in_unreadable_steam_files_says_why() {
    let dir = TempDir::new().unwrap();
    let install = dir.path().join("Steam");

    std::fs::create_dir_all(install.join("steamapps")).unwrap();
    std::fs::write(install.join("steamapps/libraryfolders.vdf"), "{").unwrap();

    let config = Config {
      steam_installs: vec![install],
      ..config_in(&dir)
    };
    let answer = run_now(find_valheim(), &config);

    assert!(
      matches!(
        &answer,
        Msg::SteamSearched { folder: Err(NotFound::Unreadable(reason)), .. }
          if reason.contains("libraryfolders.vdf")
      ),
      "{answer:?}"
    );
  }

  fn read_steam_option() -> Command {
    read_steam_option_under(Runtime::Native)
  }

  fn read_steam_option_under(runtime: Runtime) -> Command {
    read_steam_option_from(runtime, None)
  }

  /// As [`read_steam_option_under`], reading the Steam install at `steam`
  /// when given.
  fn read_steam_option_from(runtime: Runtime, steam: Option<PathBuf>) -> Command {
    Command::ReadSteamOption {
      game: "valheim".to_string(),
      app_id: VALHEIM,
      context: LaunchContext::new(
        HostOs::Linux,
        StorePlatform::Steam,
        runtime,
        LaunchMode::Vanilla,
      ),
      steam,
    }
  }

  #[test]
  fn the_steam_option_is_read_from_the_steam_folder_the_user_chose() {
    let dir = TempDir::new().unwrap();
    let chosen = TempDir::new().unwrap();
    let config = Config {
      steam: Some(steam_with(&dir, "found")),
      ..config_in(&dir)
    };
    let read = read_steam_option_from(Runtime::Proton, Some(steam_with(&chosen, "chosen")));

    let Msg::SteamOptionRead(read) = run_now(read, &config) else {
      panic!("a Steam option read answers with what it read");
    };

    assert_eq!(read.current, Ok(Some("chosen".to_string())));
  }

  #[test]
  fn reading_the_steam_option_under_proton_gives_the_dll_override_and_writes_no_wrapper() {
    let dir = TempDir::new().unwrap();
    let line = r#"WINEDLLOVERRIDES="winhttp=n,b" %command%"#;
    let config = Config {
      steam: Some(steam_with(&dir, line)),
      ..config_in(&dir)
    };

    let answer = run_now(read_steam_option_under(Runtime::Proton), &config);

    assert_eq!(
      answer,
      Msg::SteamOptionRead(SteamOptionRead {
        game: "valheim".to_string(),
        line: Some(line.to_string()),
        current: Ok(Some(line.to_string())),
      })
    );
    assert!(!config.data.join("valheim/steam-wrapper.sh").exists());
  }

  #[test]
  fn listing_the_game_folder_names_its_entries() {
    let dir = TempDir::new().unwrap();

    std::fs::write(dir.path().join("valheim.exe"), "game").unwrap();

    let answer = run_now(
      Command::ListGameFolder {
        game: "valheim".to_string(),
        folder: dir.path().to_path_buf(),
      },
      &config_in(&dir),
    );

    assert_eq!(
      answer,
      Msg::GameFolderListed {
        game: "valheim".to_string(),
        entries: Ok(vec!["valheim.exe".to_string()]),
      }
    );
  }

  #[test]
  fn reading_the_steam_option_writes_the_wrapper_and_reads_steams_options() {
    let dir = TempDir::new().unwrap();
    let script = dir.path().join("data/valheim/steam-wrapper.sh");
    let line = format!("\"{}\" %command%", forward_slashed(&script));
    let config = Config {
      steam: Some(steam_with(&dir, &format!("gamemoderun {line}"))),
      ..config_in(&dir)
    };

    let answer = run_now(read_steam_option(), &config);

    assert_eq!(
      answer,
      Msg::SteamOptionRead(SteamOptionRead {
        game: "valheim".to_string(),
        line: Some(line.clone()),
        current: Ok(Some(format!("gamemoderun {line}"))),
      })
    );
    assert!(script.is_file());
  }

  #[test]
  fn reading_the_steam_option_without_steam_still_gives_the_line() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    let answer = run_now(read_steam_option(), &config);

    let Msg::SteamOptionRead(read) = answer else {
      panic!("unexpected answer {answer:?}");
    };

    assert!(
      read
        .line
        .as_deref()
        .is_some_and(|line| line.contains("steam-wrapper.sh")),
      "{:?}",
      read.line
    );
    assert!(read.current.is_err(), "{:?}", read.current);
  }

  /// Every command [`record_spawn`] was asked to start, as its program and
  /// arguments.
  static SPAWNED: std::sync::Mutex<Vec<Vec<String>>> = std::sync::Mutex::new(Vec::new());

  /// Records `command` instead of starting it, so tests never start a game.
  fn record_spawn(command: std::process::Command) -> std::io::Result<()> {
    let argv = std::iter::once(command.get_program())
      .chain(command.get_args())
      .map(|arg| arg.to_string_lossy().into_owned())
      .collect();

    SPAWNED.lock().unwrap().push(argv);

    Ok(())
  }

  /// The commands started for a launch from `profile`.
  fn spawned_from(profile: &Path) -> Vec<Vec<String>> {
    let profile = forward_slashed(profile);

    SPAWNED
      .lock()
      .unwrap()
      .iter()
      .filter(|argv| {
        argv
          .iter()
          .any(|arg| forward_slashed(Path::new(arg)).contains(&profile))
      })
      .cloned()
      .collect()
  }

  /// `path` with forward slashes, the form the engine hands paths to Steam
  /// and the Linux wrapper in, whichever host the tests run on.
  fn forward_slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
  }

  /// A config in `dir` that records launches, with Valheim installed in a
  /// `game` folder there, and its Default profile created.
  fn launchable(dir: &TempDir) -> Config {
    let config = Config {
      spawn: record_spawn,
      ..config_in(dir)
    };
    let game = dir.path().join("game");
    let exe = config
      .ecosystem
      .require_profile("valheim")
      .unwrap()
      .exe_names
      .iter()
      .find(|name| name.ends_with(".x86_64"))
      .unwrap()
      .clone();

    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(game.join(exe), "game").unwrap();
    run_now(open_profile("valheim"), &config);

    config
  }

  /// Puts BepInEx and its Linux start script into the Default profile.
  fn with_bepinex(config: &Config) {
    let profile = default_profile(config);

    std::fs::create_dir_all(profile.join("BepInEx/core")).unwrap();
    std::fs::write(profile.join("BepInEx/core/BepInEx.Preloader.dll"), "x").unwrap();
    std::fs::write(profile.join("run_bepinex.sh"), "#!/bin/sh\n").unwrap();
  }

  fn launch_from(dir: &TempDir, mode: LaunchMode) -> Command {
    launch_under(dir, mode, Runtime::Native)
  }

  fn launch_under(dir: &TempDir, mode: LaunchMode, runtime: Runtime) -> Command {
    Command::Launch(Launch {
      id: 4,
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      folder: dir.path().join("game"),
      context: LaunchContext::new(HostOs::Linux, StorePlatform::Steam, runtime, mode),
    })
  }

  #[test]
  fn a_modded_proton_launch_starts_the_game_through_steam_with_wine_paths() {
    let dir = TempDir::new().unwrap();
    let config = launchable(&dir);

    with_bepinex(&config);

    let answer = run_now(
      launch_under(&dir, LaunchMode::Modded, Runtime::Proton),
      &config,
    );
    let spawned = spawned_from(&default_profile(&config));

    assert_eq!(
      answer,
      Msg::LaunchFinished {
        launch: 4,
        result: Ok(()),
      }
    );
    assert_eq!(spawned.len(), 1, "{spawned:?}");
    assert!(
      spawned[0]
        .windows(2)
        .any(|pair| pair == ["-applaunch", "892970"]),
      "{spawned:?}"
    );
    assert!(
      spawned[0]
        .iter()
        .any(|arg| arg.starts_with("Z:") && arg.ends_with("BepInEx.Preloader.dll")),
      "{spawned:?}"
    );
    assert!(!config.data.join("valheim/steam-wrapper.sh").exists());
  }

  #[test]
  fn a_modded_launch_starts_the_game_through_steam_with_the_profiles_loader() {
    let dir = TempDir::new().unwrap();
    let config = launchable(&dir);

    with_bepinex(&config);

    let answer = run_now(launch_from(&dir, LaunchMode::Modded), &config);
    let spawned = spawned_from(&default_profile(&config));

    assert_eq!(
      answer,
      Msg::LaunchFinished {
        launch: 4,
        result: Ok(()),
      }
    );
    assert_eq!(spawned.len(), 1, "{spawned:?}");
    assert!(
      spawned[0]
        .windows(2)
        .any(|pair| pair == ["-applaunch", "892970"]),
      "{spawned:?}"
    );
    assert!(
      spawned[0]
        .iter()
        .any(|arg| arg.ends_with("BepInEx.Preloader.dll")),
      "{spawned:?}"
    );
    assert!(config.data.join("valheim/steam-wrapper.sh").is_file());
  }

  #[test]
  fn launch_arguments_reach_steam_after_the_loaders_own() {
    let dir = TempDir::new().unwrap();
    let config = launchable(&dir);
    let arguments = vec!["-console".to_string(), "My Logs".to_string()];
    let Command::Launch(launch) = launch_from(&dir, LaunchMode::Modded) else {
      unreachable!("launch_from builds a launch");
    };
    let launch = Launch {
      context: launch.context.with_extra_args(arguments),
      ..launch
    };

    with_bepinex(&config);
    run_now(Command::Launch(launch), &config);

    let spawned = spawned_from(&default_profile(&config));
    let argv = &spawned[0];
    let position = |wanted: &str| argv.iter().position(|arg| arg.ends_with(wanted));

    assert_eq!(spawned.len(), 1, "{spawned:?}");
    assert!(
      argv.windows(2).any(|pair| pair == ["-applaunch", "892970"]),
      "{argv:?}"
    );
    assert!(
      argv.windows(2).any(|pair| pair == ["-console", "My Logs"]),
      "{argv:?}"
    );
    assert!(
      matches!(
        (position("BepInEx.Preloader.dll"), position("-console")),
        (Some(loader), Some(user)) if loader < user
      ),
      "{argv:?}"
    );
  }

  #[test]
  fn a_vanilla_launch_starts_the_game_without_the_loader_and_leaves_the_profile_alone() {
    let dir = TempDir::new().unwrap();
    let config = launchable(&dir);

    with_bepinex(&config);

    let before = run_now(open_profile("valheim"), &config);
    let answer = run_now(launch_from(&dir, LaunchMode::Vanilla), &config);
    let after = run_now(open_profile("valheim"), &config);
    let spawned = spawned_from(&default_profile(&config));

    assert_eq!(
      answer,
      Msg::LaunchFinished {
        launch: 4,
        result: Ok(()),
      }
    );
    assert_eq!(before, after);
    assert_eq!(spawned.len(), 1, "{spawned:?}");
    assert!(
      !spawned[0]
        .iter()
        .any(|arg| arg.ends_with("BepInEx.Preloader.dll")),
      "{spawned:?}"
    );
  }

  #[test]
  fn a_modded_launch_from_a_profile_with_no_mod_loader_names_the_loader() {
    let dir = TempDir::new().unwrap();
    let config = launchable(&dir);

    let answer = run_now(launch_from(&dir, LaunchMode::Modded), &config);

    assert_eq!(
      answer,
      Msg::LaunchFinished {
        launch: 4,
        result: Err(LaunchFailure::NoModLoader(LoaderKind::BepInEx)),
      }
    );
    assert_eq!(
      spawned_from(&default_profile(&config)),
      Vec::<Vec<String>>::new()
    );
  }

  #[test]
  fn a_launch_into_a_missing_game_folder_names_the_folder() {
    let dir = TempDir::new().unwrap();
    let config = launchable(&dir);

    std::fs::remove_dir_all(dir.path().join("game")).unwrap();

    let answer = run_now(launch_from(&dir, LaunchMode::Vanilla), &config);

    assert_eq!(
      answer,
      Msg::LaunchFinished {
        launch: 4,
        result: Err(LaunchFailure::MissingGameFolder(dir.path().join("game"))),
      }
    );
  }

  #[test]
  fn a_game_that_cannot_be_started_says_why() {
    let dir = TempDir::new().unwrap();
    let config = Config {
      spawn: |_| Err(std::io::Error::other("no such program")),
      ..launchable(&dir)
    };

    let answer = run_now(launch_from(&dir, LaunchMode::Vanilla), &config);

    assert!(
      matches!(
        &answer,
        Msg::LaunchFinished { launch: 4, result: Err(LaunchFailure::Other(reason)) }
          if reason.contains("no such program")
      ),
      "{answer:?}"
    );
  }

  #[test]
  fn loading_without_a_settings_file_gives_the_defaults() {
    let dir = TempDir::new().unwrap();

    let answer = run_now(Command::LoadSettings, &config_in(&dir));

    assert_eq!(answer, Msg::SettingsLoaded(Ok(Settings::default())));
  }

  #[test]
  fn saved_settings_load_back_after_a_restart() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    let saved = run_now(Command::SaveSettings(valheim_settings()), &config);
    let loaded = run_now(Command::LoadSettings, &config);

    assert_eq!(saved, Msg::SettingsSaved(Ok(())));
    assert_eq!(loaded, Msg::SettingsLoaded(Ok(valheim_settings())));
  }

  #[test]
  fn the_steam_folder_and_launch_arguments_load_back_after_a_restart() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);
    let mut settings = valheim_settings();

    settings.steam_folder = Some(PathBuf::from("/mnt/games/Steam"));

    if let Some(valheim) = settings.games.get_mut("valheim") {
      valheim.launch_arguments = r#"-console -log "My Logs""#.to_string();
    }

    run_now(Command::SaveSettings(settings.clone()), &config);

    assert_eq!(
      run_now(Command::LoadSettings, &config),
      Msg::SettingsLoaded(Ok(settings))
    );
  }

  #[test]
  fn settings_saved_in_quick_succession_keep_the_last_one_sent() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);
    let file = config.settings.clone();
    let (commands, inbox) = spawn(config);
    let saves = 100;
    let with_arguments = |n: usize| {
      let mut settings = valheim_settings();

      if let Some(valheim) = settings.games.get_mut("valheim") {
        valheim.launch_arguments = format!("-save {n}");
      }

      settings
    };

    for n in 0..saves {
      commands
        .unbounded_send(Command::SaveSettings(with_arguments(n)))
        .unwrap();
    }

    let answers: Vec<Msg> = futures::executor::block_on(inbox.take(saves).collect());
    let text = std::fs::read_to_string(&file).unwrap();

    assert!(
      answers
        .iter()
        .all(|answer| *answer == Msg::SettingsSaved(Ok(())))
    );
    assert_eq!(
      toml::from_str::<Settings>(&text).unwrap(),
      with_arguments(saves - 1)
    );
  }

  #[test]
  fn settings_are_stored_as_toml() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    run_now(Command::SaveSettings(valheim_settings()), &config);

    assert_eq!(
      std::fs::read_to_string(&config.settings).unwrap(),
      "last_game = \"valheim\"\n\n[games.valheim]\nfolder = \"/games/Valheim\"\n"
    );
  }

  #[test]
  fn a_malformed_settings_file_is_reported() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);

    std::fs::create_dir_all(config.settings.parent().unwrap()).unwrap();
    std::fs::write(&config.settings, "last_game = [").unwrap();

    let answer = run_now(Command::LoadSettings, &config);

    assert!(matches!(answer, Msg::SettingsLoaded(Err(_))), "{answer:?}");
  }

  #[test]
  fn listing_a_folder_answers_with_its_entry_names() {
    let dir = TempDir::new().unwrap();

    std::fs::write(dir.path().join("valheim.x86_64"), "").unwrap();
    std::fs::create_dir(dir.path().join("valheim_Data")).unwrap();

    let answer = run_now(
      Command::ListFolder(dir.path().to_path_buf()),
      &config_in(&dir),
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

    let answer = run_now(Command::ListFolder(missing.clone()), &config_in(&dir));

    assert!(
      matches!(&answer, Msg::FolderListed { folder, entries: Err(_) } if *folder == missing),
      "{answer:?}"
    );
  }

  #[test]
  fn a_command_sent_to_the_commands_thread_is_answered() {
    let dir = TempDir::new().unwrap();
    let (commands, mut inbox) = spawn(config_in(&dir));

    commands.unbounded_send(Command::LoadSettings).unwrap();

    let answer = futures::executor::block_on(inbox.next());

    assert_eq!(answer, Some(Msg::SettingsLoaded(Ok(Settings::default()))));
  }

  #[test]
  fn the_inbox_closes_when_the_commands_sender_is_dropped() {
    let dir = TempDir::new().unwrap();
    let (commands, mut inbox) = spawn(config_in(&dir));

    drop(commands);

    let answer = futures::executor::block_on(inbox.next());

    assert_eq!(answer, None);
  }
}
