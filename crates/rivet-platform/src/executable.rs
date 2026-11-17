//! Finding the file the running program was started from, and putting a new
//! build in place of it the way each platform allows, with the old build
//! kept aside until the new one has started.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// The file the running program was started from: the one an update
/// replaces and a launcher starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Running {
  /// A bare executable.
  Bare(PathBuf),
  /// An AppImage. The program itself runs from a temporary mount of it,
  /// which is gone once it exits.
  AppImage(PathBuf),
}

impl Running {
  /// The file on disk, whichever kind it is.
  pub fn file(&self) -> &Path {
    match self {
      Self::Bare(file) | Self::AppImage(file) => file,
    }
  }

  pub fn is_appimage(&self) -> bool {
    matches!(self, Self::AppImage(_))
  }
}

/// The file this program was started from, or `None` when the platform
/// can't say. It is the AppImage when the program runs from inside one, as
/// its runtime reports in `$APPIMAGE` and `$APPDIR`.
pub fn running() -> Option<Running> {
  let appdir = std::env::var_os("APPDIR").map(|dir| {
    std::fs::canonicalize(&dir)
      .map(OsString::from)
      .unwrap_or(dir)
  });

  running_in(
    std::env::current_exe().ok(),
    std::env::var_os("APPIMAGE"),
    appdir,
  )
}

/// `executable` is trusted to be in an AppImage only when it sits inside
/// `appdir`, since a program started by another AppImage inherits that
/// one's variables.
fn running_in(
  executable: Option<PathBuf>,
  appimage: Option<OsString>,
  appdir: Option<OsString>,
) -> Option<Running> {
  let executable = executable?;
  let appimage = appimage
    .map(PathBuf::from)
    .filter(|file| file.is_absolute());
  let inside = appdir
    .map(PathBuf::from)
    .is_some_and(|dir| dir.is_absolute() && executable.starts_with(dir));

  match appimage {
    Some(appimage) if inside => Some(Running::AppImage(appimage)),
    _ => Some(Running::Bare(executable)),
  }
}

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

#[cfg(test)]
mod tests {
  use super::*;

  #[cfg(unix)]
  const MOUNT: &str = "/tmp/.mount_rivetAb12";

  #[cfg(unix)]
  fn inside_mount() -> Option<PathBuf> {
    Some(PathBuf::from(MOUNT).join("usr/bin/rivet"))
  }

  #[test]
  fn a_bare_build_is_the_executable_itself() {
    let running = running_in(Some(PathBuf::from("/home/me/rivet")), None, None);

    assert_eq!(
      running,
      Some(Running::Bare(PathBuf::from("/home/me/rivet")))
    );
  }

  #[test]
  #[cfg(unix)]
  fn an_appimage_build_is_the_appimage_file_not_the_program_in_its_mount() {
    let running = running_in(
      inside_mount(),
      Some("/home/me/Apps/rivet-linux-x86_64.AppImage".into()),
      Some(MOUNT.into()),
    );

    assert_eq!(
      running,
      Some(Running::AppImage(PathBuf::from(
        "/home/me/Apps/rivet-linux-x86_64.AppImage"
      )))
    );
  }

  #[test]
  #[cfg(unix)]
  fn appimage_variables_inherited_from_another_appimage_are_ignored() {
    let running = running_in(
      Some(PathBuf::from("/home/me/rivet")),
      Some("/home/me/Apps/Other.AppImage".into()),
      Some("/tmp/.mount_OtherXy34".into()),
    );

    assert_eq!(
      running,
      Some(Running::Bare(PathBuf::from("/home/me/rivet")))
    );
  }

  #[test]
  #[cfg(unix)]
  fn an_empty_or_relative_appimage_path_is_ignored() {
    for appimage in ["", "rivet.AppImage"] {
      let running = running_in(inside_mount(), Some(appimage.into()), Some(MOUNT.into()));

      assert_eq!(running, inside_mount().map(Running::Bare), "{appimage:?}");
    }
  }

  #[test]
  fn the_file_is_where_updates_and_launchers_point() {
    let bare = Running::Bare(PathBuf::from("/home/me/rivet"));
    let appimage = Running::AppImage(PathBuf::from("/home/me/rivet.AppImage"));

    assert_eq!(bare.file(), Path::new("/home/me/rivet"));
    assert_eq!(appimage.file(), Path::new("/home/me/rivet.AppImage"));
    assert!(!bare.is_appimage());
    assert!(appimage.is_appimage());
  }
}
