use app::Shell;
use gpui_kit::{AppContext as _, WindowOptions};

fn main() {
  gpui_kit::application().run(|cx| {
    gpui_kit::init(cx);

    let (effects, inbox) = commands::spawn();

    gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
      cx.new(|cx| Shell::new(effects, inbox, cx))
    })
    .expect("failed to open window");
  });
}
