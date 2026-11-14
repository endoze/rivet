use gpui_kit::{AppContext as _, WindowOptions};
use rivet_app::Shell;
use rivet_core::State;
use thunderstore_engine::ecosystem::Ecosystem;

fn main() {
  let settings =
    rivet_platform::paths::settings_file().expect("the platform reports no config location");

  gpui_kit::application().run(move |cx| {
    gpui_kit::init(cx);

    let (commands, inbox) = rivet_commands::spawn(rivet_commands::Paths { settings });
    let state = State::new(Ecosystem::bundled());

    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(state, commands, inbox, window, cx))
    })
    .expect("failed to open window");
  });
}
