use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use futures::executor::block_on;
use rivet_commands::handoff::{self, Endpoint, Serving, Start};
use rivet_core::Msg;
use rivet_core::handoff::Request;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;

/// Starts a Rivet with the endpoint in `dir`, which must be the first, and
/// serves it, passing each request it is handed to the receiver.
fn first(dir: &TempDir) -> (Serving, UnboundedReceiver<Msg>) {
  let Start::First(owner) = handoff::start(&endpoint(dir), &Request::default()) else {
    panic!("the first start did not own the endpoint");
  };
  let (sender, received) = unbounded();

  (handoff::serve(owner, sender), received)
}

fn endpoint(dir: &TempDir) -> Endpoint {
  Endpoint::new(dir.path(), "rivet")
}

#[test]
fn a_second_start_hands_its_request_to_the_first() {
  let dir = TempDir::new().unwrap();
  let (_serving, mut received) = first(&dir);
  let request = Request {
    activation_token: Some("token".to_string()),
    open: Some("/home/me/Shared.r2z".to_string()),
  };

  let second = handoff::start(&endpoint(&dir), &request);

  assert!(matches!(second, Start::HandedOff), "{second:?}");
  assert_eq!(block_on(received.next()), Some(Msg::HandedOff(request)));
}

fn arguments(arguments: &[&str]) -> Vec<OsString> {
  arguments.iter().map(OsString::from).collect()
}

#[test]
fn a_start_sends_the_file_it_was_started_with_as_an_absolute_path() {
  let working_folder = Path::new("/home/me/Downloads");

  let request = handoff::request(arguments(&["Shared.r2z"]), working_folder, None);

  assert_eq!(
    request.open.map(PathBuf::from),
    Some(working_folder.join("Shared.r2z"))
  );
}

#[test]
fn a_start_sends_a_uri_as_given() {
  let link = "ror2mm://v1/install/thunderstore.io/Owner/Mod/1.0.0/";

  let request = handoff::request(arguments(&[link]), Path::new("/home/me"), None);

  assert_eq!(request.open.as_deref(), Some(link));
}

#[test]
fn a_start_the_updater_made_opens_nothing() {
  let request = handoff::request(
    arguments(&[handoff::REPLACING]),
    Path::new("/home/me"),
    Some("token".to_string()),
  );

  assert_eq!(
    request,
    Request {
      activation_token: Some("token".to_string()),
      open: None,
    }
  );
}

/// Leaves in `dir` what a Rivet that crashed leaves: its lock file, no
/// longer locked, and on Unix its socket file with nothing listening.
fn crashed(dir: &TempDir) {
  std::fs::write(dir.path().join("rivet.lock"), "").unwrap();

  #[cfg(unix)]
  drop(std::os::unix::net::UnixListener::bind(dir.path().join("rivet.sock")).unwrap());
}

#[test]
fn a_start_takes_over_the_endpoint_a_crashed_rivet_left() {
  let dir = TempDir::new().unwrap();

  crashed(&dir);

  let (_serving, mut received) = first(&dir);
  let second = handoff::start(&endpoint(&dir), &Request::default());

  assert!(matches!(second, Start::HandedOff), "{second:?}");
  assert_eq!(
    block_on(received.next()),
    Some(Msg::HandedOff(Request::default()))
  );
}

/// The updater counts a new build that exits within its first seconds as one
/// that did not start, so one replacing the running Rivet must not hand over
/// and exit: it waits for the old Rivet to close, then takes its place.
#[test]
fn a_start_replacing_the_running_rivet_waits_for_it_to_close_then_takes_over() {
  let dir = TempDir::new().unwrap();
  let (serving, mut received) = first(&dir);
  let replacing = {
    let endpoint = endpoint(&dir);

    std::thread::spawn(move || handoff::take_over(&endpoint))
  };

  std::thread::sleep(Duration::from_millis(500));

  assert!(
    !replacing.is_finished(),
    "the new build stopped while the old one ran"
  );
  assert_eq!(received.try_recv().ok(), None);

  drop(serving);

  let Start::First(owner) = replacing.join().unwrap() else {
    panic!("the new build did not take the endpoint over");
  };
  let (sender, mut received) = unbounded();
  let _serving = handoff::serve(owner, sender);
  let later = handoff::start(&endpoint(&dir), &Request::default());

  assert!(matches!(later, Start::HandedOff), "{later:?}");
  assert_eq!(
    block_on(received.next()),
    Some(Msg::HandedOff(Request::default()))
  );
}

#[test]
fn a_rivet_with_its_endpoint_elsewhere_is_not_reached() {
  let mine = TempDir::new().unwrap();
  let theirs = TempDir::new().unwrap();
  let (_serving, mut received) = first(&theirs);

  let (_mine, _) = first(&mine);

  assert_eq!(received.try_recv().ok(), None);
}
