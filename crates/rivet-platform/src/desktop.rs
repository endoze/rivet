//! Listing a program in Linux desktops' app menus. Desktops find a program's
//! name and icon through a desktop entry file and the hicolor icon theme, and
//! match its windows to that entry by the window's app ID.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// A program to list in the app menu.
#[derive(Clone, Copy, Debug)]
pub struct Launcher<'a> {
  /// The app ID the program's windows report. It names the entry and icon
  /// files, so the desktop can match the windows to them.
  pub id: &'a str,
  /// The name the menu shows.
  pub name: &'a str,
  /// What the program is, shown beside the name in some menus.
  pub comment: &'a str,
  /// The desktop's menu categories, each followed by `;`, such as `Game;`.
  pub categories: &'a str,
  /// The program the entry starts.
  pub executable: &'a Path,
  /// PNG icons, each with its width in pixels.
  pub icons: &'a [(u32, &'a [u8])],
}

/// The text of `launcher`'s desktop entry file.
///
/// # Errors
///
/// `InvalidData` when the executable's path is not UTF-8, which a desktop
/// entry cannot hold.
///
/// ```
/// use rivet_platform::desktop::{Launcher, entry};
/// use std::path::Path;
///
/// let launcher = Launcher {
///   id: "org.example.App",
///   name: "App",
///   comment: "An app",
///   categories: "Utility;",
///   executable: Path::new("/opt/app"),
///   icons: &[],
/// };
///
/// assert!(entry(&launcher).unwrap().contains("Exec=\"/opt/app\"\n"));
/// ```
pub fn entry(launcher: &Launcher) -> io::Result<String> {
  let executable = launcher.executable.to_str().ok_or_else(|| {
    io::Error::new(
      io::ErrorKind::InvalidData,
      "the executable's path is not UTF-8",
    )
  })?;

  Ok(format!(
    "[Desktop Entry]\n\
     Type=Application\n\
     Name={name}\n\
     Comment={comment}\n\
     Exec={exec}\n\
     Icon={id}\n\
     Terminal=false\n\
     Categories={categories}\n\
     StartupWMClass={id}\n",
    id = launcher.id,
    name = launcher.name,
    comment = launcher.comment,
    exec = quoted(executable),
    categories = launcher.categories,
  ))
}

/// `path` as one argument of an `Exec` key. The desktop entry spec escapes
/// it twice: once as a quoted argument, then again as a string value.
fn quoted(path: &str) -> String {
  let mut argument = String::from("\"");

  for c in path.chars() {
    match c {
      '"' | '`' | '$' => argument.push_str(&format!("\\\\{c}")),
      '\\' => argument.push_str("\\\\\\\\"),
      '%' => argument.push_str("%%"),
      c => argument.push(c),
    }
  }

  argument.push('"');

  argument
}

/// The user's `$XDG_DATA_HOME`, usually `~/.local/share`, where desktops look
/// for the user's own launchers and icons. `None` when there is no home
/// folder.
pub fn data_home() -> Option<PathBuf> {
  dirs::data_dir()
}

/// The folders in `$XDG_DATA_DIRS`, where packages install launchers for
/// every user: `/usr/local/share` and `/usr/share` when it is unset or empty.
pub fn data_dirs() -> Vec<PathBuf> {
  data_dirs_in(std::env::var_os("XDG_DATA_DIRS"))
}

fn data_dirs_in(value: Option<OsString>) -> Vec<PathBuf> {
  let value = value
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| "/usr/local/share:/usr/share".into());

  std::env::split_paths(&value)
    .filter(|dir| dir.is_absolute())
    .collect()
}

/// Writes `launcher`'s desktop entry and icons under `data_home`, as returned
/// by [`data_home`]. Files that already hold the same contents are left
/// alone, so this can run on every start.
///
/// Writes nothing when a package installed an entry with the same ID in one
/// of `data_dirs`, as returned by [`data_dirs`], so the package's entry is
/// the one the menu shows.
///
/// # Errors
///
/// Why a file could not be written. Files written before it stay in place.
pub fn install(data_home: &Path, data_dirs: &[PathBuf], launcher: &Launcher) -> io::Result<()> {
  let entry_name = format!("{}.desktop", launcher.id);
  let packaged = data_dirs
    .iter()
    .filter(|dir| *dir != data_home)
    .any(|dir| dir.join("applications").join(&entry_name).exists());

  if packaged {
    return Ok(());
  }

  for (size, png) in launcher.icons {
    let icon = data_home
      .join("icons/hicolor")
      .join(format!("{size}x{size}"))
      .join("apps")
      .join(format!("{}.png", launcher.id));

    write_if_changed(&icon, png)?;
  }

  let entry_file = data_home.join("applications").join(entry_name);

  write_if_changed(&entry_file, entry(launcher)?.as_bytes())
}

fn write_if_changed(path: &Path, contents: &[u8]) -> io::Result<()> {
  if std::fs::read(path).is_ok_and(|current| current == contents) {
    return Ok(());
  }

  if let Some(folder) = path.parent() {
    std::fs::create_dir_all(folder)?;
  }

  std::fs::write(path, contents)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn launcher(executable: &Path) -> Launcher<'_> {
    Launcher {
      id: "org.example.App",
      name: "App",
      comment: "An app",
      categories: "Game;",
      executable,
      icons: &[(16, b"small"), (256, b"large")],
    }
  }

  #[test]
  fn the_entry_names_the_icon_and_window_class_by_the_app_id() {
    let entry = entry(&launcher(Path::new("/opt/app"))).unwrap();

    assert_eq!(
      entry,
      "[Desktop Entry]\n\
       Type=Application\n\
       Name=App\n\
       Comment=An app\n\
       Exec=\"/opt/app\"\n\
       Icon=org.example.App\n\
       Terminal=false\n\
       Categories=Game;\n\
       StartupWMClass=org.example.App\n"
    );
  }

  #[test]
  fn exec_escapes_the_characters_the_spec_reserves() {
    let entry = entry(&launcher(Path::new(r#"/my apps/"q"$`\100%"#))).unwrap();

    assert!(
      entry.contains(r#"Exec="/my apps/\\"q\\"\\$\\`\\\\100%%""#),
      "{entry}"
    );
  }

  #[test]
  fn install_writes_the_entry_and_each_icon_size() {
    let home = tempfile::tempdir().unwrap();
    let launcher = launcher(Path::new("/opt/app"));

    install(home.path(), &[], &launcher).unwrap();

    assert_eq!(
      std::fs::read(
        home
          .path()
          .join("icons/hicolor/16x16/apps/org.example.App.png")
      )
      .unwrap(),
      b"small"
    );
    assert_eq!(
      std::fs::read(
        home
          .path()
          .join("icons/hicolor/256x256/apps/org.example.App.png")
      )
      .unwrap(),
      b"large"
    );
    assert_eq!(
      std::fs::read_to_string(home.path().join("applications/org.example.App.desktop")).unwrap(),
      entry(&launcher).unwrap()
    );
  }

  #[test]
  fn install_points_the_entry_at_a_moved_executable() {
    let home = tempfile::tempdir().unwrap();

    install(home.path(), &[], &launcher(Path::new("/old/app"))).unwrap();
    install(home.path(), &[], &launcher(Path::new("/new/app"))).unwrap();

    let entry =
      std::fs::read_to_string(home.path().join("applications/org.example.App.desktop")).unwrap();

    assert!(entry.contains("Exec=\"/new/app\"\n"), "{entry}");
  }

  #[test]
  fn install_writes_nothing_when_a_package_installed_the_entry() {
    let home = tempfile::tempdir().unwrap();
    let system = tempfile::tempdir().unwrap();
    let packaged = system.path().join("applications/org.example.App.desktop");

    std::fs::create_dir_all(packaged.parent().unwrap()).unwrap();
    std::fs::write(&packaged, "[Desktop Entry]\n").unwrap();

    install(
      home.path(),
      &[system.path().to_path_buf()],
      &launcher(Path::new("/opt/app")),
    )
    .unwrap();

    assert!(!home.path().join("applications").exists());
    assert!(!home.path().join("icons").exists());
  }

  #[test]
  fn install_keeps_updating_its_own_entry_when_data_home_is_a_data_dir() {
    let home = tempfile::tempdir().unwrap();
    let data_dirs = [home.path().to_path_buf()];

    install(home.path(), &data_dirs, &launcher(Path::new("/old/app"))).unwrap();
    install(home.path(), &data_dirs, &launcher(Path::new("/new/app"))).unwrap();

    let entry =
      std::fs::read_to_string(home.path().join("applications/org.example.App.desktop")).unwrap();

    assert!(entry.contains("Exec=\"/new/app\"\n"), "{entry}");
  }

  #[test]
  fn data_dirs_default_to_the_spec_folders_when_unset_or_empty() {
    let defaults = [
      PathBuf::from("/usr/local/share"),
      PathBuf::from("/usr/share"),
    ];

    assert_eq!(data_dirs_in(None), defaults);
    assert_eq!(data_dirs_in(Some("".into())), defaults);
  }

  #[test]
  fn data_dirs_skip_relative_paths() {
    assert_eq!(
      data_dirs_in(Some("/a:relative:/b".into())),
      [PathBuf::from("/a"), PathBuf::from("/b")]
    );
  }
}
