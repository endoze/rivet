use futures::channel::mpsc::unbounded;
use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
use rivet_app::Shell;
use rivet_core::handoff::Request;
use rivet_core::{Command, Msg, State};
use thunderstore_engine::ecosystem::Ecosystem;

#[gpui_kit::test]
fn a_later_launch_brings_the_window_to_the_front(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, _sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| {
        Shell::new(
          State::new(Ecosystem::bundled()),
          commands,
          inbox,
          window,
          cx,
        )
      })
    })
    .expect("open test window")
  });
  let active = |cx: &mut TestAppContext| {
    cx.update_window(window, |_, window, _| window.is_window_active())
      .unwrap()
  };

  cx.run_until_parked();

  assert!(!active(cx), "the window started at the front");

  answers
    .unbounded_send(Msg::HandedOff(Request::default()))
    .unwrap();
  cx.run_until_parked();

  assert!(active(cx), "the window did not come to the front");
}
