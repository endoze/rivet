#![windows_subsystem = "windows"]

use gpui_kit::component::Theme;
use gpui_kit::{AppContext as _, WindowOptions};
use reqwest_client::ReqwestClient;
use rivet_app::Shell;
use rivet_core::{Command, State};
use std::sync::Arc;
use thunderstore_engine::ecosystem::Ecosystem;

/// The app ID Rivet's window reports, which Linux desktops match to its
/// launcher and icon.
const APP_ID: &str = "io.github.endoze.rivet";

fn main() {
  #[cfg(target_os = "linux")]
  install_launcher();

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
      executable: std::env::current_exe().ok(),
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

      let state = State::new(config.ecosystem.clone());
      let (commands, inbox) = rivet_commands::spawn(config);

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

        cx.new(|cx| Shell::new(state, commands, inbox, window, cx))
      })
      .expect("failed to open window");
    });
}

/// Lists Rivet in the desktop's app menu, with its icon, pointing at this
/// executable. A failure costs only the menu entry, so it is ignored.
#[cfg(target_os = "linux")]
fn install_launcher() {
  use rivet_platform::desktop::{self, Launcher};

  // `cargo run` would otherwise point the menu at a development build.
  if cfg!(debug_assertions) {
    return;
  }

  let (Some(data_home), Ok(executable)) = (desktop::data_home(), std::env::current_exe()) else {
    return;
  };
  let launcher = Launcher {
    id: APP_ID,
    name: "Rivet",
    comment: "Mod manager for Thunderstore games",
    categories: "Game;",
    executable: &executable,
    icons: &[
      (16, include_bytes!("../assets/icon/rivet-16.png")),
      (24, include_bytes!("../assets/icon/rivet-24.png")),
      (32, include_bytes!("../assets/icon/rivet-32.png")),
      (48, include_bytes!("../assets/icon/rivet-48.png")),
      (64, include_bytes!("../assets/icon/rivet-64.png")),
      (128, include_bytes!("../assets/icon/rivet-128.png")),
      (256, include_bytes!("../assets/icon/rivet-256.png")),
    ],
  };

  desktop::install(&data_home, &desktop::data_dirs(), &launcher).ok();
}
