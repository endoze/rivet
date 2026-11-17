//! Listing a program in the Windows Start Menu, through a shortcut in the
//! user's own Start Menu folder.

use std::path::{Path, PathBuf};

/// A program to list in the Start Menu.
#[derive(Clone, Copy, Debug)]
pub struct Shortcut<'a> {
  /// The name Start shows, which also names the shortcut file.
  pub name: &'a str,
  /// What the program is, shown in the shortcut's tooltip.
  pub description: &'a str,
  /// The program the shortcut starts. Its embedded icon is the shortcut's.
  pub executable: &'a Path,
}

/// Where `shortcut`'s file goes under `app_data`, the user's roaming
/// `%APPDATA%` folder.
pub fn file(app_data: &Path, shortcut: &Shortcut) -> PathBuf {
  app_data
    .join("Microsoft")
    .join("Windows")
    .join("Start Menu")
    .join("Programs")
    .join(format!("{}.lnk", shortcut.name))
}

/// Whether a shortcut that starts `target`, or `None` when there is no
/// readable shortcut, must be rewritten to start `shortcut`'s executable.
pub fn is_stale(target: Option<&Path>, shortcut: &Shortcut) -> bool {
  target != Some(shortcut.executable)
}

/// The user's roaming `%APPDATA%` folder. `None` when Windows reports none.
#[cfg(windows)]
pub fn app_data() -> Option<PathBuf> {
  dirs::config_dir()
}

/// Writes `shortcut` under `app_data`, as returned by [`app_data`]. A
/// shortcut that already starts the executable is left alone, so this can
/// run on every start.
///
/// Initializes COM on the calling thread for the duration of the call.
///
/// # Errors
///
/// Why the shortcut could not be written, including when the calling thread
/// already uses COM in the multithreaded apartment.
#[cfg(windows)]
pub fn install(app_data: &Path, shortcut: &Shortcut) -> std::io::Result<()> {
  let file = file(app_data, shortcut);
  let _com = link::Apartment::enter()?;

  if !is_stale(link::target(&file).as_deref(), shortcut) {
    return Ok(());
  }

  if let Some(folder) = file.parent() {
    std::fs::create_dir_all(folder)?;
  }

  link::write(&file, shortcut)
}

/// Shell link files, through the Windows shell's COM objects.
#[cfg(windows)]
mod link {
  use super::Shortcut;
  use std::ffi::OsString;
  use std::io;
  use std::os::windows::ffi::OsStringExt;
  use std::path::{Path, PathBuf};
  use windows::Win32::Foundation::MAX_PATH;
  use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, IPersistFile, STGM_READ,
  };
  use windows::Win32::UI::Shell::{IShellLinkW, SLGP_RAWPATH, ShellLink};
  use windows::core::{HSTRING, Interface};

  /// COM initialized on this thread until dropped.
  pub struct Apartment(());

  impl Apartment {
    pub fn enter() -> io::Result<Self> {
      unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;

      Ok(Self(()))
    }
  }

  impl Drop for Apartment {
    fn drop(&mut self) {
      unsafe { CoUninitialize() };
    }
  }

  /// The path the shortcut at `file` starts, as written. `None` when there
  /// is no shortcut there or it cannot be read.
  pub fn target(file: &Path) -> Option<PathBuf> {
    let link: IShellLinkW =
      unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.ok()?;
    let mut path = [0u16; MAX_PATH as usize];

    unsafe {
      link
        .cast::<IPersistFile>()
        .ok()?
        .Load(&HSTRING::from(file), STGM_READ)
        .ok()?;
      link
        .GetPath(&mut path, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
        .ok()?;
    }

    let length = path
      .iter()
      .position(|&unit| unit == 0)
      .unwrap_or(path.len());

    Some(OsString::from_wide(&path[..length]).into())
  }

  /// Writes a shortcut to `shortcut`'s executable at `file`, with the icon
  /// the executable embeds.
  pub fn write(file: &Path, shortcut: &Shortcut) -> io::Result<()> {
    let executable = HSTRING::from(shortcut.executable);

    unsafe {
      let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;

      link.SetPath(&executable)?;
      link.SetIconLocation(&executable, 0)?;
      link.SetDescription(&HSTRING::from(shortcut.description))?;
      link
        .cast::<IPersistFile>()?
        .Save(&HSTRING::from(file), true)?;
    }

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn shortcut(executable: &Path) -> Shortcut<'_> {
    Shortcut {
      name: "App",
      description: "An app",
      executable,
    }
  }

  #[test]
  fn the_file_is_named_after_the_program_in_the_users_start_menu() {
    let app_data = Path::new(r"C:\Users\me\AppData\Roaming");

    assert_eq!(
      file(app_data, &shortcut(Path::new(r"C:\Games\app.exe"))),
      Path::new(r"C:\Users\me\AppData\Roaming")
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("App.lnk")
    );
  }

  #[test]
  fn a_missing_or_unreadable_shortcut_is_written() {
    assert!(is_stale(None, &shortcut(Path::new(r"C:\Games\app.exe"))));
  }

  #[test]
  fn a_shortcut_to_the_running_program_is_left_alone() {
    let current = Path::new(r"C:\Games\app.exe");

    assert!(!is_stale(
      Some(current),
      &shortcut(Path::new(r"C:\Games\app.exe"))
    ));
  }

  #[test]
  fn a_shortcut_to_where_the_program_was_is_rewritten() {
    let current = Path::new(r"C:\Downloads\app.exe");

    assert!(is_stale(
      Some(current),
      &shortcut(Path::new(r"C:\Games\app.exe"))
    ));
  }
}
