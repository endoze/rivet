//! Runs [`Effect`]s on a Tokio thread of their own and answers each with a
//! [`Msg`], so the UI thread never waits on disk or network.

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use state::{Effect, Msg};

/// Runs one effect to completion and returns the message that answers it.
pub async fn run(effect: Effect) -> Msg {
  match effect {
    Effect::Ping => Msg::PongReceived,
  }
}

/// Starts a Tokio runtime for effects, on threads named `commands`.
///
/// Effects sent on the returned sender run concurrently on that runtime, and
/// their answers arrive on the returned receiver. The runtime stops once the
/// sender is dropped.
///
/// # Panics
///
/// Panics if the runtime or the thread that owns it cannot be started.
pub fn spawn() -> (UnboundedSender<Effect>, UnboundedReceiver<Msg>) {
  let (effects, mut pending) = unbounded::<Effect>();
  let (answers, inbox) = unbounded::<Msg>();
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .thread_name("commands")
    .build()
    .expect("tokio runtime");

  std::thread::Builder::new()
    .name("commands".into())
    .spawn(move || {
      runtime.block_on(async move {
        while let Some(effect) = pending.next().await {
          let answers = answers.clone();

          tokio::spawn(async move {
            answers.unbounded_send(run(effect).await).ok();
          });
        }
      });
    })
    .expect("commands thread");

  (effects, inbox)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_ping_sent_to_the_commands_thread_comes_back_as_a_pong() {
    let (effects, mut inbox) = spawn();

    effects.unbounded_send(Effect::Ping).unwrap();

    let answer = futures::executor::block_on(inbox.next());

    assert_eq!(answer, Some(Msg::PongReceived));
  }

  #[test]
  fn the_inbox_closes_when_the_effects_sender_is_dropped() {
    let (effects, mut inbox) = spawn();

    drop(effects);

    let answer = futures::executor::block_on(inbox.next());

    assert_eq!(answer, None);
  }
}
