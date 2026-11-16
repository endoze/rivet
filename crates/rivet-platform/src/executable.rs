//! Putting a new build of a program in place of the running one, the way
//! each platform allows, with the old build kept aside until the new one has
//! started.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// Where the build `executable` replaced is kept until the new one has
/// started: beside it, with `.old` added to its name.
///
/// ```
/// use rivet_platform::executable::backup_of;
/// use std::path::Path;
///
/// assert_eq!(backup_of(Path::new("rivet.exe")), Path::new("rivet.exe.old"));
/// ```
pub fn backup_of(executable: &Path) -> PathBuf {
  suffixed(executable, ".old")
}

/// Where a new build of `executable` downloads to: beside it, with `.new`
/// added to its name, so moving it into place never crosses a filesystem.
///
/// ```
/// use rivet_platform::executable::staged_of;
/// use std::path::Path;
///
/// assert_eq!(staged_of(Path::new("rivet")), Path::new("rivet.new"));
/// ```
pub fn staged_of(executable: &Path) -> PathBuf {
  suffixed(executable, ".new")
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
  let mut name = path.file_name().map(OsString::from).unwrap_or_default();

  name.push(suffix);

  path.with_file_name(name)
}

/// Puts the build at `new` in place of `executable`, which may be running,
/// and keeps the build it replaces at `backup`, replacing any build left
/// there before.
///
/// On Windows the running program is renamed aside, as Windows allows, and
/// the new one moved into its place. Elsewhere the old build is linked or
/// copied aside, and the new one renamed over it in one atomic step, given
/// the old one's permissions so it stays executable.
///
/// # Errors
///
/// Why the build could not be put in place. `executable` is then left as it
/// was.
pub fn replace(executable: &Path, new: &Path, backup: &Path) -> io::Result<()> {
  remove_if_present(backup)?;

  swap(executable, new, backup)
}

#[cfg(windows)]
fn swap(executable: &Path, new: &Path, backup: &Path) -> io::Result<()> {
  std::fs::rename(executable, backup)?;

  if let Err(error) = std::fs::rename(new, executable) {
    std::fs::rename(backup, executable).ok();

    return Err(error);
  }

  Ok(())
}

#[cfg(not(windows))]
fn swap(executable: &Path, new: &Path, backup: &Path) -> io::Result<()> {
  let permissions = std::fs::metadata(executable)?.permissions();

  std::fs::set_permissions(new, permissions)?;

  if std::fs::hard_link(executable, backup).is_err() {
    std::fs::copy(executable, backup)?;
  }

  std::fs::rename(new, executable)
}

/// Puts the build [`replace`] kept at `backup` back in place of
/// `executable`, for a new build that did not start.
///
/// # Errors
///
/// Why the old build could not be put back.
pub fn restore(executable: &Path, backup: &Path) -> io::Result<()> {
  std::fs::rename(backup, executable)
}

fn remove_if_present(path: &Path) -> io::Result<()> {
  match std::fs::remove_file(path) {
    Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
    _ => Ok(()),
  }
}
