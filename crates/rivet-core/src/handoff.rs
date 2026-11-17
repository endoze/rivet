//! A later launch of Rivet handing over to the one already running.

use serde::{Deserialize, Serialize};

/// What a later launch of Rivet asks the running one to do. For now that is
/// only to come to the front.
///
/// Fields a newer Rivet adds, such as a file or link it was started with,
/// are ignored by an older one, so each new field needs a default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
  /// The Wayland activation token the desktop gave the later launch, which
  /// lets the running Rivet take focus.
  pub activation_token: Option<String>,
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{Effect, Msg, State, update};
  use thunderstore_engine::ecosystem::Ecosystem;

  #[test]
  fn a_later_launch_brings_the_window_to_the_front() {
    let mut state = State::new(Ecosystem::bundled());

    let effects = update(&mut state, Msg::HandedOff(Request::default()));

    assert_eq!(effects, vec![Effect::FocusWindow]);
  }
}
