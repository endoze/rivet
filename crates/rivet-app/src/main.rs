use gpui_kit::{AppContext as _, WindowOptions};
use reqwest_client::ReqwestClient;
use rivet_app::Shell;
use rivet_core::State;
use std::sync::Arc;
use thunderstore_engine::ecosystem::Ecosystem;

fn main() {
  let config = rivet_commands::Config {
    settings: rivet_platform::paths::settings_file()
      .expect("the platform reports no config location"),
    cache: rivet_platform::paths::cache_folder().expect("the platform reports no cache location"),
    data: rivet_platform::paths::data_folder().expect("the platform reports no data location"),
    thunderstore: rivet_commands::THUNDERSTORE.to_string(),
    ecosystem: Ecosystem::bundled(),
    steam: rivet_platform::steam::root(),
    steam_installs: rivet_platform::steam::installs(),
    spawn: rivet_commands::spawn_detached,
    cancels: rivet_commands::Cancels::default(),
  };

  gpui_kit::application().run(move |cx| {
    gpui_kit::init(cx);

    let images =
      ReqwestClient::user_agent(rivet_commands::USER_AGENT).expect("the image HTTP client");

    cx.set_http_client(Arc::new(images));

    let state = State::new(config.ecosystem.clone());
    let (commands, inbox) = rivet_commands::spawn(config);

    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(state, commands, inbox, window, cx))
    })
    .expect("failed to open window");
  });
}
