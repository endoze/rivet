//! The gpui shell: draws [`State`], turns input into [`Msg`]s, and hands the
//! [`Effect`]s that come back to the commands thread.

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use gpui_kit::component::button::Button;
use gpui_kit::component::v_flex;
use gpui_kit::{Context, IntoElement, ParentElement as _, Render, Styled as _, Task, Window};
use state::{Effect, Msg, Ping, State};

/// The root view. It owns the state and is the only place `update` is called.
pub struct Shell {
  state: State,
  effects: UnboundedSender<Effect>,
  _inbox: Task<()>,
}

impl Shell {
  /// Builds the view. Effects go out on `effects`, and every message arriving
  /// on `inbox` is applied to the state.
  pub fn new(
    effects: UnboundedSender<Effect>,
    mut inbox: UnboundedReceiver<Msg>,
    cx: &mut Context<Self>,
  ) -> Self {
    let _inbox = cx.spawn(async move |this, cx| {
      while let Some(msg) = inbox.next().await {
        if this.update(cx, |this, cx| this.dispatch(msg, cx)).is_err() {
          break;
        }
      }
    });

    Self {
      state: State::default(),
      effects,
      _inbox,
    }
  }

  pub fn state(&self) -> &State {
    &self.state
  }

  fn dispatch(&mut self, msg: Msg, cx: &mut Context<Self>) {
    for effect in state::update(&mut self.state, msg) {
      self.effects.unbounded_send(effect).ok();
    }

    cx.notify();
  }
}

impl Render for Shell {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let status = match self.state.ping {
      Ping::Idle => "No ping sent",
      Ping::Waiting => "Waiting for the commands thread",
      Ping::Answered => "The commands thread answered",
    };

    v_flex()
      .size_full()
      .items_center()
      .justify_center()
      .gap_4()
      .child(
        Button::new("ping")
          .label("Ping")
          .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::PingPressed, cx))),
      )
      .child(status)
  }
}
