#![windows_subsystem = "windows"]

use futures::channel::mpsc::unbounded;
use futures::stream::select;
use gpui_kit::component::Theme;
use gpui_kit::{AppContext as _, WindowOptions};
use reqwest_client::ReqwestClient;
use rivet_app::Shell;
use rivet_commands::handoff::{self, Endpoint, Start};
use rivet_core::handoff::Request;
use rivet_core::self_update::platform_asset;
use rivet_core::{Command, Msg, State};
use rivet_platform::executable::Running;
use std::sync::Arc;
use thunderstore_engine::ecosystem::Ecosystem;

/// The app ID Rivet's window reports, which Linux desktops match to its
/// launcher and icon.
const APP_ID: &str = "io.github.endoze.rivet";

/// What file managers call a `.r2z` file.
const PROFILE_EXPORT: &str = "Mod profile export";

fn main() {
  let request = handoff::request(
    std::env::args_os().skip(1),
    &std::env::current_dir().unwrap_or_default(),
    std::env::var("XDG_ACTIVATION_TOKEN").ok(),
  );
  let owner = match hand_off(&request) {
    Some(Start::HandedOff) => return,
    Some(Start::First(owner)) => Some(owner),
    Some(Start::Alone) | None => None,
  };

  let running = rivet_platform::executable::running();

  #[cfg(target_os = "linux")]
  if let Some(running) = &running {
    install_launcher(running.file());
  }

  #[cfg(windows)]
  {
    install_shortcut();
    open_profile_exports();
  }

  let asset = platform_asset(running.as_ref().is_some_and(Running::is_appimage));
  let config = rivet_commands::Config {
    settings: rivet_platform::paths::settings_file()
      .expect("the platform reports no config location"),
    cache: rivet_platform::paths::cache_folder().expect("the platform reports no cache location"),
    data: rivet_platform::paths::data_folder().expect("the platform reports no data location"),
    thunderstore: rivet_commands::THUNDERSTORE.to_string(),
    covers: rivet_commands::COVERS.to_string(),
    ecosystem: Ecosystem::bundled(),
    steam: rivet_platform::steam::root(),
    steam_installs: rivet_platform::steam::installs(),
    r2modman: rivet_platform::r2modman::data_folders(),
    spawn: rivet_commands::spawn_detached,
    cancels: rivet_commands::Cancels::default(),
    updater: rivet_commands::Updater {
      executable: running.as_ref().map(|running| running.file().to_path_buf()),
      ..rivet_commands::Updater::default()
    },
  };

  gpui_kit::application()
    .with_assets(rivet_app::assets::Assets)
    .run(move |cx| {
      gpui_kit::init(cx);
      rivet_app::theme::init(cx);

      let images =
        ReqwestClient::user_agent(rivet_commands::USER_AGENT).expect("the image HTTP client");

      cx.set_http_client(Arc::new(images));

      let mut state = State::new(config.ecosystem.clone());

      state.self_update.asset = asset.map(str::to_string);

      let (commands, inbox) = rivet_commands::spawn(config);
      let (handed_over, handed) = unbounded();

      handed_over.unbounded_send(Msg::Launched(request)).ok();

      let mut serving = owner.map(|owner| handoff::serve(owner, handed_over));

      // Releasing the endpoint as the window closes, rather than as the
      // process ends, lets a new build the updater started take over sooner.
      cx.on_window_closed(move |_, _| drop(serving.take()))
        .detach();

      commands
        .unbounded_send(Command::SettleRivetUpdate {
          running: rivet_core::self_update::RUNNING.to_string(),
        })
        .ok();

      let options = WindowOptions {
        app_id: Some(APP_ID.to_string()),
        ..WindowOptions::default()
      };

      gpui_kit::open_window(options, cx, |window, cx| {
        Theme::sync_system_appearance(Some(window), cx);

        cx.new(|cx| Shell::new(state, commands, select(inbox, handed), window, cx))
      })
      .expect("failed to open window");
    });
}

/// Hands `request` to the Rivet already running, or claims the endpoint
/// later launches hand theirs to. A launch the updater started waits for
/// the Rivet it replaces to close instead. `None` when the platform reports
/// no folder for the endpoint.
fn hand_off(request: &Request) -> Option<Start> {
  let endpoint = Endpoint::new(&rivet_platform::paths::endpoint_folder()?, APP_ID);

  if std::env::args_os().any(|argument| argument == handoff::REPLACING) {
    return Some(handoff::take_over(&endpoint));
  }

  Some(handoff::start(&endpoint, request))
}

/// Lists Rivet in the desktop's app menu, with its icon, pointing at
/// `executable`, as the app that opens `.r2z` files. A failure costs only
/// the menu entry or the file type, so it is ignored.
#[cfg(target_os = "linux")]
fn install_launcher(executable: &std::path::Path) {
  use rivet_platform::desktop::{self, FileType, Launcher};

  const FILE_TYPES: &[FileType] = &[FileType {
    mime: "application/x-r2z",
    comment: PROFILE_EXPORT,
    extension: "r2z",
    sub_class_of: "application/zip",
  }];

  // `cargo run` would otherwise point the menu at a development build.
  if cfg!(debug_assertions) {
    return;
  }

  let Some(data_home) = desktop::data_home() else {
    return;
  };
  let launcher = Launcher {
    id: APP_ID,
    name: "Rivet",
    comment: "Mod manager for Thunderstore games",
    categories: "Game;",
    executable,
    icons: &[
      (16, include_bytes!("../assets/icon/rivet-16.png")),
      (24, include_bytes!("../assets/icon/rivet-24.png")),
      (32, include_bytes!("../assets/icon/rivet-32.png")),
      (48, include_bytes!("../assets/icon/rivet-48.png")),
      (64, include_bytes!("../assets/icon/rivet-64.png")),
      (128, include_bytes!("../assets/icon/rivet-128.png")),
      (256, include_bytes!("../assets/icon/rivet-256.png")),
    ],
    file_types: FILE_TYPES,
  };

  if desktop::install(&data_home, &desktop::data_dirs(), &launcher).unwrap_or(false) {
    // The desktop's tools can take a second, which the window need not wait for.
    std::thread::spawn(move || {
      desktop::refresh(&data_home);
      desktop::claim_defaults(APP_ID, FILE_TYPES);
    });
  }
}

/// Lists Rivet in the Start Menu, with its icon, pointing at this
/// executable. A failure costs only the shortcut, so it is ignored.
#[cfg(windows)]
fn install_shortcut() {
  use rivet_platform::start_menu::{self, Shortcut};

  // `cargo run` would otherwise point the Start Menu at a development build.
  if cfg!(debug_assertions) {
    return;
  }

  let (Some(app_data), Ok(executable)) = (start_menu::app_data(), std::env::current_exe()) else {
    return;
  };
  let shortcut = Shortcut {
    name: "Rivet",
    description: "Mod manager for Thunderstore games",
    executable: &executable,
  };

  start_menu::install(&app_data, &shortcut).ok();
}

/// Makes this executable the program that opens `.r2z` files. A failure
/// costs only that, so it is ignored.
#[cfg(windows)]
fn open_profile_exports() {
  use rivet_platform::file_types::{self, Association};

  // `cargo run` would otherwise open the files with a development build.
  if cfg!(debug_assertions) {
    return;
  }

  let Ok(executable) = std::env::current_exe() else {
    return;
  };
  let association = Association {
    extension: ".r2z",
    prog_id: "Rivet.r2z",
    description: PROFILE_EXPORT,
    executable: &executable,
  };

  file_types::install(&association).ok();
}
