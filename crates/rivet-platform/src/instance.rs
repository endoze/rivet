//! One running copy of a program per user. The first copy to start owns an
//! endpoint, a local socket or a named pipe, and a later start sends it a
//! message instead of running beside it.

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericFilePath, Listener, ListenerOptions, Name, Stream};
use std::fmt;
use std::fs::{File, TryLockError};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The most of a message the owner reads. The rest is dropped.
const MESSAGE_LIMIT: u64 = 64 * 1024;

/// How long the owner pauses after failing to take a connection, so an
/// error that persists does not spin.
const ACCEPT_PAUSE: Duration = Duration::from_millis(50);

/// How long releasing an endpoint waits for its listening thread to stop,
/// and how often it wakes the thread meanwhile.
const STOP_WAIT: Duration = Duration::from_secs(1);
const STOP_POLL: Duration = Duration::from_millis(10);

/// Where the one running copy of a program can be reached.
#[derive(Debug, Clone)]
pub struct Endpoint {
  /// The file the owner keeps locked while it owns the endpoint.
  lock: PathBuf,
  /// The socket's path on Unix, or the named pipe's on Windows.
  socket: PathBuf,
}

impl Endpoint {
  /// The endpoint `name` in `folder`: the lock file `{name}.lock` there,
  /// which the owner holds, and beside it the socket `{name}.sock` on Unix,
  /// or on Windows a named pipe named after the folder.
  ///
  /// `folder` should belong to one user, so that another user's copy is
  /// never reached.
  ///
  /// ```
  /// use rivet_platform::instance::Endpoint;
  /// use std::path::Path;
  ///
  /// let endpoint = Endpoint::new(Path::new("/run/user/1000"), "rivet");
  /// ```
  pub fn new(folder: &Path, name: &str) -> Self {
    Self {
      lock: folder.join(format!("{name}.lock")),
      socket: socket_path(folder, name),
    }
  }

  /// Sends `message` to the program that owns the endpoint. On Windows,
  /// that program may then take focus from this one.
  ///
  /// # Errors
  ///
  /// No program owns the endpoint, or it did not take the message.
  pub fn send(&self, message: &[u8]) -> io::Result<()> {
    let mut stream = self.connect()?;

    #[cfg(windows)]
    allow_foreground(&stream);

    stream.write_all(message)?;
    stream.flush()
  }

  /// Claims the endpoint, or `None` when a running program owns it. An
  /// endpoint left behind by a program that crashed is taken over.
  ///
  /// # Errors
  ///
  /// Why the endpoint could not be made.
  pub fn claim(&self) -> io::Result<Option<Owner>> {
    if let Some(folder) = self.lock.parent() {
      std::fs::create_dir_all(folder)?;
    }

    let lock = File::options()
      .create(true)
      .truncate(false)
      .write(true)
      .open(&self.lock)?;

    match lock.try_lock() {
      Ok(()) => {}
      Err(TryLockError::WouldBlock) => return Ok(None),
      Err(TryLockError::Error(error)) => return Err(error),
    }

    #[cfg(not(windows))]
    remove_if_present(&self.socket)?;

    let listener = ListenerOptions::new().name(self.name()?).create_sync()?;

    Ok(Some(Owner {
      listener,
      _lock: lock,
      endpoint: self.clone(),
    }))
  }

  fn connect(&self) -> io::Result<Stream> {
    Stream::connect(self.name()?)
  }

  fn name(&self) -> io::Result<Name<'_>> {
    self.socket.as_path().to_fs_name::<GenericFilePath>()
  }
}

#[cfg(not(windows))]
fn socket_path(folder: &Path, name: &str) -> PathBuf {
  folder.join(format!("{name}.sock"))
}

/// A pipe named after the folder's path, since pipe names share one
/// namespace across the machine. A pipe name may hold no backslash.
#[cfg(windows)]
fn socket_path(folder: &Path, name: &str) -> PathBuf {
  let named = folder.join(name).to_string_lossy().replace('\\', "/");

  PathBuf::from(format!(r"\\.\pipe\{named}"))
}

#[cfg(not(windows))]
fn remove_if_present(path: &Path) -> io::Result<()> {
  match std::fs::remove_file(path) {
    Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
    _ => Ok(()),
  }
}

/// Lets the program at the other end of `stream` bring its window in front
/// of this one's, which Windows refuses a program in the background.
#[cfg(windows)]
fn allow_foreground(stream: &Stream) {
  use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

  if let Some(process) = stream.peer_creds().ok().and_then(|creds| creds.pid()) {
    // SAFETY: takes a process id by value and touches no memory of ours.
    unsafe { AllowSetForegroundWindow(process) };
  }
}

/// An endpoint this program claimed. Dropping it releases the endpoint.
pub struct Owner {
  // Dropped before the lock, which removes the socket file first: a program
  // that claims the endpoint once the lock is free makes a socket of its
  // own there, which removing ours afterwards would delete.
  listener: Listener,
  _lock: File,
  endpoint: Endpoint,
}

impl fmt::Debug for Owner {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Owner")
      .field("endpoint", &self.endpoint)
      .finish_non_exhaustive()
  }
}

impl Owner {
  /// Listens for messages on a thread of its own until the [`Serving`] it
  /// returns is dropped, handing each to `received`.
  pub fn serve(self, received: impl Fn(Vec<u8>) + Send + Sync + 'static) -> Serving {
    let stopped = Arc::new(AtomicBool::new(false));
    let stopping = stopped.clone();
    let endpoint = self.endpoint.clone();
    let received = Arc::new(received);
    let thread = std::thread::spawn(move || {
      // Takes the whole owner, lock and all, for as long as it listens. A
      // closure naming only `self.listener` would capture that field alone
      // and release the lock at once.
      let owner = self;

      for stream in owner.listener.incoming() {
        if stopping.load(Ordering::Acquire) {
          break;
        }

        let Ok(stream) = stream else {
          std::thread::sleep(ACCEPT_PAUSE);

          continue;
        };
        let received = received.clone();

        // Read apart, so a sender that never finishes holds up no other.
        std::thread::spawn(move || received(read(stream)));
      }
    });

    Serving {
      stopped,
      endpoint,
      thread: Some(thread),
    }
  }
}

/// What a sender wrote, up to [`MESSAGE_LIMIT`]. A Windows pipe reports its
/// sender closing as an error after the message, so errors end the message
/// rather than discard it.
fn read(stream: Stream) -> Vec<u8> {
  let mut message = Vec::new();

  stream.take(MESSAGE_LIMIT).read_to_end(&mut message).ok();

  message
}

/// An endpoint being listened on. Dropping it stops listening and releases
/// the endpoint.
pub struct Serving {
  stopped: Arc<AtomicBool>,
  endpoint: Endpoint,
  thread: Option<JoinHandle<()>>,
}

impl Drop for Serving {
  fn drop(&mut self) {
    self.stopped.store(true, Ordering::Release);

    let Some(thread) = self.thread.take() else {
      return;
    };
    let started = Instant::now();

    // The listening thread waits for a connection, so only one stops it. A
    // Windows pipe refuses one while it sets up for the next, so this keeps
    // trying.
    while !thread.is_finished() && started.elapsed() < STOP_WAIT {
      self.endpoint.connect().ok();
      std::thread::sleep(STOP_POLL);
    }

    if thread.is_finished() {
      thread.join().ok();
    }
  }
}
