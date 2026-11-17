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
  /// The kinds of file the program opens.
  pub file_types: &'a [FileType<'a>],
}

/// A kind of file, known to desktops by its MIME type.
#[derive(Clone, Copy, Debug)]
pub struct FileType<'a> {
  /// The MIME type, such as `application/x-r2z`.
  pub mime: &'a str,
  /// What file managers call it.
  pub comment: &'a str,
  /// The extension its file names end in, without the dot.
  pub extension: &'a str,
  /// The type its contents read as, such as `application/zip`. Declaring it
  /// lets the file name decide the type when the contents match too.
  pub sub_class_of: &'a str,
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
///   file_types: &[],
/// };
///
/// assert!(entry(&launcher).unwrap().contains("Exec=\"/opt/app\" %u\n"));
/// ```
pub fn entry(launcher: &Launcher) -> io::Result<String> {
  let executable = launcher.executable.to_str().ok_or_else(|| {
    io::Error::new(
      io::ErrorKind::InvalidData,
      "the executable's path is not UTF-8",
    )
  })?;
  let mime_types: String = launcher
    .file_types
    .iter()
    .map(|file_type| format!("{};", file_type.mime))
    .collect();

  Ok(format!(
    "[Desktop Entry]\n\
     Type=Application\n\
     Name={name}\n\
     Comment={comment}\n\
     TryExec={try_exec}\n\
     Exec={exec} %u\n\
     Icon={id}\n\
     Terminal=false\n\
     Categories={categories}\n\
     MimeType={mime_types}\n\
     StartupWMClass={id}\n",
    id = launcher.id,
    name = launcher.name,
    comment = launcher.comment,
    try_exec = string_value(executable),
    exec = quoted(executable),
    categories = launcher.categories,
  ))
}

/// The shared-mime-info package defining `launcher`'s file types, which
/// goes in `mime/packages` under a data folder. Each field is written as
/// given, so none may hold XML markup.
pub fn mime_package(launcher: &Launcher) -> String {
  let types: String = launcher
    .file_types
    .iter()
    .map(|file_type| {
      format!(
        "  <mime-type type=\"{mime}\">\n    \
         <comment>{comment}</comment>\n    \
         <sub-class-of type=\"{sub_class_of}\"/>\n    \
         <glob pattern=\"*.{extension}\"/>\n  \
         </mime-type>\n",
        mime = file_type.mime,
        comment = file_type.comment,
        sub_class_of = file_type.sub_class_of,
        extension = file_type.extension,
      )
    })
    .collect();

  format!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
     <mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n\
     {types}\
     </mime-info>\n"
  )
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

/// `text` as a desktop entry string value, such as `TryExec`. Only the
/// spec's string escapes apply, not the `Exec` quoting rules.
fn string_value(text: &str) -> String {
  let mut value = String::new();

  for c in text.chars() {
    match c {
      '\\' => value.push_str("\\\\"),
      '\n' => value.push_str("\\n"),
      '\t' => value.push_str("\\t"),
      '\r' => value.push_str("\\r"),
      c => value.push(c),
    }
  }

  value
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

/// Writes `launcher`'s desktop entry, icons and [`mime_package`] under
/// `data_home`, as returned by [`data_home`]. Files that already hold the
/// same contents are left alone, so this can run on every start. Returns
/// whether it wrote or removed anything, after which the desktop's caches
/// need a [`refresh`].
///
/// Writes nothing when a package installed an entry with the same ID in one
/// of `data_dirs`, as returned by [`data_dirs`], so the package's entry is
/// the one the menu shows. When an AppImage integration tool listed the
/// program under a name of its own, the entry this wrote before is removed
/// and only the mime package is written, so the menu doesn't show it twice.
///
/// # Errors
///
/// Why a file could not be written or removed. Files written before it stay
/// in place.
pub fn install(data_home: &Path, data_dirs: &[PathBuf], launcher: &Launcher) -> io::Result<bool> {
  let entry_name = format!("{}.desktop", launcher.id);
  let packaged = data_dirs
    .iter()
    .filter(|dir| *dir != data_home)
    .any(|dir| dir.join("applications").join(&entry_name).exists());

  if packaged {
    return Ok(false);
  }

  let applications = data_home.join("applications");
  let entry_file = applications.join(&entry_name);
  let package = data_home
    .join("mime/packages")
    .join(format!("{}.xml", launcher.id));
  let mut changed = write_if_changed(&package, mime_package(launcher).as_bytes())?;

  if integrated(&applications, &entry_name, launcher.id) {
    return Ok(remove_if_present(&entry_file)? || changed);
  }

  for (size, png) in launcher.icons {
    let icon = data_home
      .join("icons/hicolor")
      .join(format!("{size}x{size}"))
      .join("apps")
      .join(format!("{}.png", launcher.id));

    changed |= write_if_changed(&icon, png)?;
  }

  changed |= write_if_changed(&entry_file, entry(launcher)?.as_bytes())?;

  Ok(changed)
}

/// Updates the desktop's caches of the mime types and desktop entries under
/// `data_home`, so it sees what [`install`] wrote. A missing tool or a
/// failure is ignored: the caches are only rebuilt later.
pub fn refresh(data_home: &Path) {
  run(
    "update-mime-database",
    &[data_home.join("mime").as_os_str()],
  );
  run(
    "update-desktop-database",
    &[data_home.join("applications").as_os_str()],
  );
}

/// Makes the desktop entry `id` the default app for each of `file_types`
/// that has none, through `xdg-mime`. A missing tool or a failure is
/// ignored.
pub fn claim_defaults(id: &str, file_types: &[FileType]) {
  let entry_name = format!("{id}.desktop");

  for file_type in file_types {
    let current = std::process::Command::new("xdg-mime")
      .args(["query", "default", file_type.mime])
      .stderr(std::process::Stdio::null())
      .output();
    let unset = current.is_ok_and(|output| {
      output.status.success() && String::from_utf8_lossy(&output.stdout).trim().is_empty()
    });

    if unset {
      run(
        "xdg-mime",
        &[
          "default".as_ref(),
          entry_name.as_ref(),
          file_type.mime.as_ref(),
        ],
      );
    }
  }
}

/// Runs `program` with `arguments` to its end, ignoring its output and
/// whether it ran.
fn run(program: &str, arguments: &[&std::ffi::OsStr]) {
  std::process::Command::new(program)
    .args(arguments)
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .status()
    .ok();
}

/// Whether an AppImage integration tool, such as AppImageLauncher or Gear
/// Lever, listed the app in `applications`. Each copies the AppImage's own
/// entry under a name of its choosing, keeping its `StartupWMClass`, and sets
/// `TryExec` to the AppImage. An entry whose AppImage is gone is hidden by
/// desktops, so it doesn't count.
fn integrated(applications: &Path, own: &str, id: &str) -> bool {
  let Ok(files) = std::fs::read_dir(applications) else {
    return false;
  };

  files
    .flatten()
    .map(|file| file.path())
    .filter(|path| {
      path
        .extension()
        .is_some_and(|extension| extension == "desktop")
        && path.file_name().is_some_and(|name| name != own)
    })
    .filter_map(|path| std::fs::read_to_string(path).ok())
    .any(|text| {
      value(&text, "StartupWMClass") == Some(id)
        && value(&text, "TryExec").is_some_and(|file| Path::new(file).is_file())
    })
}

/// The value of the first `key=value` line for `key` in a desktop entry.
fn value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
  text
    .lines()
    .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
    .map(str::trim)
}

/// Removes the file at `path`, if there is one. Returns whether there was.
fn remove_if_present(path: &Path) -> io::Result<bool> {
  match std::fs::remove_file(path) {
    Ok(()) => Ok(true),
    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
    Err(error) => Err(error),
  }
}

/// Writes `contents` to `path`, unless it holds them already. Returns
/// whether it wrote.
fn write_if_changed(path: &Path, contents: &[u8]) -> io::Result<bool> {
  if std::fs::read(path).is_ok_and(|current| current == contents) {
    return Ok(false);
  }

  if let Some(folder) = path.parent() {
    std::fs::create_dir_all(folder)?;
  }

  std::fs::write(path, contents)?;

  Ok(true)
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
      file_types: &[FileType {
        mime: "application/x-app-save",
        comment: "App save",
        extension: "appsave",
        sub_class_of: "application/zip",
      }],
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
       TryExec=/opt/app\n\
       Exec=\"/opt/app\" %u\n\
       Icon=org.example.App\n\
       Terminal=false\n\
       Categories=Game;\n\
       MimeType=application/x-app-save;\n\
       StartupWMClass=org.example.App\n"
    );
  }

  #[test]
  fn the_mime_package_matches_each_file_type_by_name_over_its_contents() {
    let package = mime_package(&launcher(Path::new("/opt/app")));

    assert_eq!(
      package,
      "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
       <mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n\
       \x20 <mime-type type=\"application/x-app-save\">\n\
       \x20   <comment>App save</comment>\n\
       \x20   <sub-class-of type=\"application/zip\"/>\n\
       \x20   <glob pattern=\"*.appsave\"/>\n\
       \x20 </mime-type>\n\
       </mime-info>\n"
    );
  }

  #[test]
  fn install_writes_the_mime_package_and_says_when_it_changed_anything() {
    let home = tempfile::tempdir().unwrap();
    let launcher = launcher(Path::new("/opt/app"));

    assert!(install(home.path(), &[], &launcher).unwrap());
    assert_eq!(
      std::fs::read_to_string(home.path().join("mime/packages/org.example.App.xml")).unwrap(),
      mime_package(&launcher)
    );
    assert!(!install(home.path(), &[], &launcher).unwrap());
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
  fn try_exec_names_the_executable_as_a_plain_path() {
    let entry = entry(&launcher(Path::new("/my apps/app"))).unwrap();

    assert!(entry.contains("\nTryExec=/my apps/app\n"), "{entry}");
  }

  #[test]
  fn try_exec_escapes_only_what_a_string_value_reserves() {
    let entry = entry(&launcher(Path::new("/a\\b\nc\td\re\"$`%"))).unwrap();

    assert!(
      entry.contains("\nTryExec=/a\\\\b\\nc\\td\\re\"$`%\n"),
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

    assert!(entry.contains("Exec=\"/new/app\" %u\n"), "{entry}");
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

  /// Writes `name` under `home`'s launchers, as an AppImage integration tool
  /// copies the AppImage's entry: same window class, renamed, and pointing
  /// at the AppImage at `appimage`.
  fn integrated(home: &Path, name: &str, appimage: &Path) {
    let path = home.join("applications").join(name);

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
      &path,
      format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=App\n\
         Exec={appimage} %U\n\
         TryExec={appimage}\n\
         Icon=appimagekit_0123_org.example.App\n\
         StartupWMClass=org.example.App\n\
         X-AppImage-Version=1.0.0\n",
        appimage = appimage.display()
      ),
    )
    .unwrap();
  }

  #[test]
  fn install_leaves_the_menu_to_an_appimage_integration_tool_entry() {
    let names = [
      "appimagekit_5d41402abc4b2a76b9719d911017c592-App.desktop",
      "app.desktop",
    ];

    for name in names {
      let home = tempfile::tempdir().unwrap();
      let appimage = home.path().join("App.AppImage");
      let own = home.path().join("applications/org.example.App.desktop");

      std::fs::write(&appimage, "").unwrap();
      install(home.path(), &[], &launcher(&appimage)).unwrap();
      integrated(home.path(), name, &appimage);
      install(home.path(), &[], &launcher(&appimage)).unwrap();

      assert!(
        !own.exists(),
        "{name}: Rivet's own entry was kept beside it"
      );
      assert!(home.path().join("applications").join(name).exists());
    }
  }

  #[test]
  fn install_ignores_an_integration_tool_entry_whose_appimage_is_gone() {
    let home = tempfile::tempdir().unwrap();

    integrated(
      home.path(),
      "app.desktop",
      &home.path().join("Deleted.AppImage"),
    );
    install(home.path(), &[], &launcher(Path::new("/opt/app"))).unwrap();

    assert!(
      home
        .path()
        .join("applications/org.example.App.desktop")
        .exists()
    );
  }

  #[test]
  fn install_ignores_other_apps_entries() {
    let home = tempfile::tempdir().unwrap();
    let other = home.path().join("applications/other.desktop");
    let existing = home.path().join("Other.AppImage");

    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    std::fs::write(&existing, "").unwrap();
    std::fs::write(
      &other,
      format!(
        "[Desktop Entry]\nTryExec={}\nStartupWMClass=org.example.AppTwo\n",
        existing.display()
      ),
    )
    .unwrap();
    install(home.path(), &[], &launcher(Path::new("/opt/app"))).unwrap();

    assert!(
      home
        .path()
        .join("applications/org.example.App.desktop")
        .exists()
    );
  }

  #[test]
  fn install_keeps_updating_its_own_entry_when_data_home_is_a_data_dir() {
    let home = tempfile::tempdir().unwrap();
    let data_dirs = [home.path().to_path_buf()];

    install(home.path(), &data_dirs, &launcher(Path::new("/old/app"))).unwrap();
    install(home.path(), &data_dirs, &launcher(Path::new("/new/app"))).unwrap();

    let entry =
      std::fs::read_to_string(home.path().join("applications/org.example.App.desktop")).unwrap();

    assert!(entry.contains("Exec=\"/new/app\" %u\n"), "{entry}");
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
