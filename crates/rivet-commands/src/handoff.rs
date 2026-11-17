//! One Rivet at a time: a start hands its request to the Rivet already
//! running and exits, or owns the endpoint later starts hand theirs to.

use futures::channel::mpsc::UnboundedSender;
use rivet_core::Msg;
use rivet_core::handoff::Request;
use std::time::{Duration, Instant};

pub use rivet_platform::instance::{Endpoint, Owner, Serving};

/// The argument the updater starts a new build with, telling it to
/// [`take_over`] from the Rivet it replaces.
pub const REPLACING: &str = "--replacing";

/// How long a start keeps trying while the endpoint is owned but nothing
/// answers on it, as when the owner is still starting or is closing.
const SETTLE_WAIT: Duration = Duration::from_secs(5);

/// How long a new build put in place by an update waits for the old Rivet
/// to close.
const REPLACE_WAIT: Duration = Duration::from_secs(60);

/// How often a start waiting on the endpoint tries again.
const POLL: Duration = Duration::from_millis(50);

/// What a start of Rivet does about the Rivet that may already be running.
#[derive(Debug)]
pub enum Start {
  /// No Rivet was running. This one owns the endpoint and keeps running.
  First(Owner),
  /// A running Rivet took the request. This one exits.
  HandedOff,
  /// The endpoint could be neither reached nor claimed. This one runs
  /// without it.
  Alone,
}

/// Hands `request` to the Rivet that owns `endpoint`, or claims the
/// endpoint when none does.
pub fn start(endpoint: &Endpoint, request: &Request) -> Start {
  let message = serde_json::to_vec(request).unwrap_or_default();
  let started = Instant::now();

  loop {
    if endpoint.send(&message).is_ok() {
      return Start::HandedOff;
    }

    match endpoint.claim() {
      Ok(Some(owner)) => return Start::First(owner),
      Ok(None) if started.elapsed() < SETTLE_WAIT => std::thread::sleep(POLL),
      _ => return Start::Alone,
    }
  }
}

/// Waits for the Rivet that owns `endpoint` to release it, then claims it:
/// the start of a new build the updater put in place of that Rivet, which
/// closes once the new one has run for a while. Never hands over, since the
/// updater counts a new build that exits early as one that did not start.
/// After [`REPLACE_WAIT`] it gives up and runs [`Start::Alone`].
pub fn take_over(endpoint: &Endpoint) -> Start {
  let started = Instant::now();

  loop {
    match endpoint.claim() {
      Ok(Some(owner)) => return Start::First(owner),
      Ok(None) if started.elapsed() < REPLACE_WAIT => std::thread::sleep(POLL),
      _ => return Start::Alone,
    }
  }
}

/// Listens on the endpoint `owner` claimed until the [`Serving`] it returns
/// is dropped, sending each request a later start hands over to `msgs` as
/// [`Msg::HandedOff`]. Anything that is not a request is ignored.
pub fn serve(owner: Owner, msgs: UnboundedSender<Msg>) -> Serving {
  owner.serve(move |message| {
    if let Ok(request) = serde_json::from_slice::<Request>(&message) {
      msgs.unbounded_send(Msg::HandedOff(request)).ok();
    }
  })
}
