use app::Shell;
use futures::channel::mpsc::unbounded;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
use state::{Effect, Msg, Ping};

#[gpui_kit::test]
fn clicking_ping_sends_the_effect_and_the_answer_updates_the_view(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (effects, mut sent) = unbounded::<Effect>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, shell) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
      cx.new(|cx| Shell::new(effects, inbox, cx))
    })
    .expect("open test window")
  });

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("ping", cx);
  })
  .unwrap();

  assert_eq!(sent.try_recv().ok(), Some(Effect::Ping));
  assert_eq!(
    shell.read_with(cx, |shell, _| shell.state().ping),
    Ping::Waiting
  );

  answers.unbounded_send(Msg::PongReceived).unwrap();
  cx.run_until_parked();

  assert_eq!(
    shell.read_with(cx, |shell, _| shell.state().ping),
    Ping::Answered
  );
}
