use rivet_platform::executable::{backup_of, replace, restore, staged_of};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError};
use tempfile::TempDir;

// Linux refuses to start a program while any process holds it open for
// writing, and a test starting a program in one thread can inherit a file
// another is writing. One test at a time keeps that from happening.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
  SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Where the stand-in for Rivet sits in `dir`, named as the real one is.
fn stand_in(dir: &TempDir) -> PathBuf {
  dir
    .path()
    .join(if cfg!(windows) { "rivet.exe" } else { "rivet" })
}

/// What `program` prints, run with nothing on its input.
fn output_of(program: &Path) -> String {
  let output = Command::new(program)
    .stdin(Stdio::null())
    .output()
    .unwrap_or_else(|error| panic!("{} did not start: {error}", program.display()));

  assert!(output.status.success(), "{} failed", program.display());

  String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Starts `program` waiting on its input, as a running Rivet holds its own
/// file.
fn start(program: &Path) -> Child {
  Command::new(program)
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .spawn()
    .unwrap()
}

#[cfg(unix)]
mod builds {
  use std::os::unix::fs::PermissionsExt as _;
  use std::path::Path;

  /// A program that waits for a line, then prints `old`.
  pub fn old(path: &Path) {
    std::fs::write(path, "#!/bin/sh\nread line\necho old\n").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
  }

  /// A program that prints `new`, downloaded without its executable bit.
  pub fn new(path: &Path) {
    std::fs::write(path, "#!/bin/sh\necho new\n").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
  }

  pub fn old_output() -> String {
    "old".to_string()
  }

  pub fn new_output() -> String {
    "new".to_string()
  }

  pub fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o111 == 0o111
  }
}

#[cfg(windows)]
mod builds {
  use std::path::{Path, PathBuf};

  fn system(program: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap())
      .join("System32")
      .join(program)
  }

  /// A copy of `cmd.exe`, which waits on its input.
  pub fn old(path: &Path) {
    std::fs::copy(system("cmd.exe"), path).unwrap();
  }

  /// A copy of `hostname.exe`, which prints the computer's name.
  pub fn new(path: &Path) {
    std::fs::copy(system("hostname.exe"), path).unwrap();
  }

  pub fn old_output() -> String {
    String::new()
  }

  pub fn new_output() -> String {
    super::output_of(&system("hostname.exe"))
  }

  pub fn is_executable(path: &Path) -> bool {
    path.is_file()
  }
}

#[test]
fn an_update_puts_the_new_build_in_place_of_a_running_one_and_keeps_the_old_aside() {
  let _serial = serial();
  let dir = TempDir::new().unwrap();
  let executable = stand_in(&dir);
  let staged = staged_of(&executable);
  let backup = backup_of(&executable);

  builds::old(&executable);

  let mut running = start(&executable);

  builds::new(&staged);

  replace(&executable, &staged, &backup).unwrap();

  assert_eq!(output_of(&executable), builds::new_output());
  assert!(
    builds::is_executable(&executable),
    "the new build lost its executable permission"
  );
  assert!(!staged.exists(), "the downloaded build was left beside it");
  assert!(backup.is_file(), "the old build was not kept aside");

  running.kill().unwrap();
  running.wait().unwrap();

  std::fs::remove_file(&backup).expect("the old build is deleted once it stops");
}

#[test]
fn a_new_build_that_does_not_start_is_swapped_back_for_the_old_one() {
  let _serial = serial();
  let dir = TempDir::new().unwrap();
  let executable = stand_in(&dir);
  let staged = staged_of(&executable);
  let backup = backup_of(&executable);

  builds::old(&executable);
  builds::new(&staged);

  replace(&executable, &staged, &backup).unwrap();
  restore(&executable, &backup).unwrap();

  if cfg!(unix) {
    assert_eq!(output_of(&executable), builds::old_output());
  }

  assert!(!backup.exists(), "the old build is still aside");
  assert!(builds::is_executable(&executable));
}

#[test]
fn a_stale_backup_from_an_earlier_update_is_replaced() {
  let _serial = serial();
  let dir = TempDir::new().unwrap();
  let executable = stand_in(&dir);
  let staged = staged_of(&executable);
  let backup = backup_of(&executable);

  builds::old(&executable);
  builds::new(&staged);
  std::fs::write(&backup, "stale").unwrap();

  replace(&executable, &staged, &backup).unwrap();

  assert_ne!(std::fs::read(&backup).unwrap(), b"stale");
}

#[test]
fn the_backup_and_the_download_sit_beside_the_program() {
  let executable = Path::new("/opt/rivet/rivet");

  assert_eq!(backup_of(executable), Path::new("/opt/rivet/rivet.old"));
  assert_eq!(staged_of(executable), Path::new("/opt/rivet/rivet.new"));
}
