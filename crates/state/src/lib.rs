//! The pure core of the app: all state, every message that can change it, and
//! the effects the shell must run. Nothing here does IO.

/// Whether the commands thread has answered the last ping.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Ping {
  #[default]
  Idle,
  Waiting,
  Answered,
}

/// Everything the app knows.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct State {
  pub ping: Ping,
}

/// Something that happened: user input, or the result of an [`Effect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
  PingPressed,
  PongReceived,
}

/// Work the shell must do outside the pure core. Each effect is answered with
/// a [`Msg`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
  Ping,
}

/// Applies one message to the state and returns the effects to run.
///
/// ```
/// use state::{Effect, Msg, Ping, State, update};
///
/// let mut state = State::default();
/// let effects = update(&mut state, Msg::PingPressed);
///
/// assert_eq!(state.ping, Ping::Waiting);
/// assert_eq!(effects, vec![Effect::Ping]);
/// ```
pub fn update(state: &mut State, msg: Msg) -> Vec<Effect> {
  match msg {
    Msg::PingPressed => {
      state.ping = Ping::Waiting;

      vec![Effect::Ping]
    }
    Msg::PongReceived => {
      state.ping = Ping::Answered;

      Vec::new()
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn pressing_ping_waits_and_asks_for_a_pong() {
    let mut state = State::default();

    let effects = update(&mut state, Msg::PingPressed);

    assert_eq!(state.ping, Ping::Waiting);
    assert_eq!(effects, vec![Effect::Ping]);
  }

  #[test]
  fn a_pong_answers_the_ping() {
    let mut state = State {
      ping: Ping::Waiting,
    };

    let effects = update(&mut state, Msg::PongReceived);

    assert_eq!(state.ping, Ping::Answered);
    assert_eq!(effects, vec![]);
  }
}
