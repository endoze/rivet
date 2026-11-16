#![windows_subsystem = "windows"]

use gpui_kit::component::Theme;
use gpui_kit::{AppContext as _, WindowOptions};
use reqwest_client::ReqwestClient;
use rivet_app::Shell;
use rivet_core::{Command, State};
use std::sync::Arc;
use thunderstore_engine::ecosystem::Ecosystem;

fn main() {
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

      gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
        Theme::sync_system_appearance(Some(window), cx);

        cx.new(|cx| Shell::new(state, commands, inbox, window, cx))
      })
      .expect("failed to open window");
    });
}
