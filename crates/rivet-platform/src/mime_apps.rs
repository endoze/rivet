//! Which app Linux desktops open a MIME type or URL scheme with, as the
//! freedesktop mime-apps spec lays out in `mimeapps.list` files, and making
//! a program that app.

use crate::desktop::value;
use crate::url_scheme::{Change, Claim, Holder, change};
use std::io;
use std::path::{Path, PathBuf};

/// The folders desktops look in for `mimeapps.list` files and desktop
/// entries, and the desktops running, each lowercased.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folders {
  /// `$XDG_CONFIG_HOME`, where the user's own choices are written.
  pub config_home: PathBuf,
  /// `$XDG_CONFIG_DIRS`.
  pub config_dirs: Vec<PathBuf>,
  /// `$XDG_DATA_HOME`.
  pub data_home: PathBuf,
  /// `$XDG_DATA_DIRS`.
  pub data_dirs: Vec<PathBuf>,
  /// `$XDG_CURRENT_DESKTOP`'s names, lowercased, such as `kde`.
  pub desktops: Vec<String>,
}

impl Folders {
  /// The folders the environment names, or the spec's defaults where it
  /// names none. `None` when there is no home folder.
  pub fn from_env() -> Option<Self> {
    let config_dirs = std::env::var_os("XDG_CONFIG_DIRS")
      .filter(|value| !value.is_empty())
      .unwrap_or_else(|| "/etc/xdg".into());
    let desktops = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

    Some(Self {
      config_home: dirs::config_dir()?,
      config_dirs: std::env::split_paths(&config_dirs)
        .filter(|dir| dir.is_absolute())
        .collect(),
      data_home: dirs::data_dir()?,
      data_dirs: crate::desktop::data_dirs(),
      desktops: desktops
        .split(':')
        .filter(|name| !name.is_empty())
        .map(str::to_lowercase)
        .collect(),
    })
  }

  /// The `mimeapps.list` files, the one that wins first.
  fn lists(&self) -> Vec<PathBuf> {
    let config = std::iter::once(&self.config_home).chain(&self.config_dirs);
    let data = std::iter::once(&self.data_home)
      .chain(&self.data_dirs)
      .map(|dir| dir.join("applications"));

    config
      .cloned()
      .chain(data)
      .flat_map(|dir| {
        self
          .desktops
          .iter()
          .map(|desktop| format!("{desktop}-mimeapps.list"))
          .chain(["mimeapps.list".to_string()])
          .map(move |name| dir.join(name))
          .collect::<Vec<_>>()
      })
      .collect()
  }

  /// The folders desktop entries are installed in, the user's first.
  fn applications(&self) -> Vec<PathBuf> {
    std::iter::once(&self.data_home)
      .chain(&self.data_dirs)
      .map(|dir| dir.join("applications"))
      .collect()
  }

  /// The installed desktop entry named `name`, if there is one.
  fn entry(&self, name: &str) -> Option<PathBuf> {
    self
      .applications()
      .into_iter()
      .map(|dir| dir.join(name))
      .find(|path| path.is_file())
  }

  /// Every installed desktop entry, by file name, the user's first.
  fn entries(&self) -> Vec<(String, PathBuf)> {
    self
      .applications()
      .into_iter()
      .filter_map(|dir| std::fs::read_dir(dir).ok())
      .flat_map(|files| {
        let mut files: Vec<PathBuf> = files.flatten().map(|file| file.path()).collect();

        files.sort();

        files
      })
      .filter_map(|path| {
        let name = path.file_name()?.to_str()?.to_string();

        name.ends_with(".desktop").then_some((name, path))
      })
      .collect()
  }
}

/// Registers the desktop entry of the app `id`, its window's app ID, as the
/// default for `mime`, such as `x-scheme-handler/ror2mm`, as `claim` says.
/// Returns whether the app opens `mime` afterwards.
///
/// The app's entry is `{id}.desktop`, or else the copy an AppImage
/// integration tool made, which keeps `id` as its window class. Releasing
/// also removes the app's entries' association with `mime`, since they
/// list it among their types, and claiming again restores it.
///
/// # Errors
///
/// Why the user's `mimeapps.list` could not be read or written.
pub fn register(folders: &Folders, claim: Claim, mime: &str, id: &str) -> io::Result<bool> {
  let own = own_entries(folders, id);
  let holder = holder(folders, mime, &own);
  let change = change(claim, holder);
  let list = folders.config_home.join("mimeapps.list");

  match change {
    Change::Write => {
      let entry = own
        .first()
        .cloned()
        .unwrap_or_else(|| format!("{id}.desktop"));

      edit(&list, |text| claimed(text, mime, &own, &entry))?;
    }
    Change::Remove | Change::Keep if claim == Claim::Release => {
      edit(&list, |text| released(text, mime, &own))?;
    }
    Change::Remove | Change::Keep => {}
  }

  Ok(change.opens(holder))
}

/// The installed entries that are the app `id`'s, by file name: its own,
/// then any copy an AppImage integration tool made of it whose AppImage is
/// still there.
fn own_entries(folders: &Folders, id: &str) -> Vec<String> {
  let own = format!("{id}.desktop");
  let copies = folders.entries().into_iter().filter_map(|(name, path)| {
    let text = std::fs::read_to_string(path).ok()?;
    let copy = name != own
      && value(&text, "StartupWMClass") == Some(id)
      && value(&text, "TryExec").is_some_and(|file| Path::new(file).is_file());

    copy.then_some(name)
  });

  folders
    .entry(&own)
    .map(|_| own.clone())
    .into_iter()
    .chain(copies)
    .collect()
}

/// Which app opens `mime`, with `own` the app's entries: the first installed
/// default a `mimeapps.list` names, or else any other installed entry that
/// lists `mime` among its types.
fn holder(folders: &Folders, mime: &str, own: &[String]) -> Holder {
  let default = folders
    .lists()
    .into_iter()
    .filter_map(|list| std::fs::read_to_string(list).ok())
    .flat_map(|text| defaults(&text, mime))
    .find(|name| folders.entry(name).is_some());

  if let Some(name) = default {
    return if own.contains(&name) {
      Holder::This
    } else {
      Holder::Another
    };
  }

  let listed = folders.entries().into_iter().any(|(name, path)| {
    !own.contains(&name)
      && std::fs::read_to_string(path).is_ok_and(|text| {
        value(&text, "MimeType").is_some_and(|types| types.split(';').any(|kind| kind == mime))
      })
  });

  if listed {
    Holder::Another
  } else {
    Holder::Nobody
  }
}

const DEFAULTS: &str = "[Default Applications]";
const REMOVED: &str = "[Removed Associations]";

/// The entries a `mimeapps.list`'s defaults name for `mime`, in order.
fn defaults(text: &str, mime: &str) -> Vec<String> {
  let mut group = "";

  for line in text.lines() {
    let line = line.trim();

    if line.starts_with('[') {
      group = line;
    } else if group == DEFAULTS
      && let Some((key, names)) = line.split_once('=')
      && key.trim() == mime
    {
      return names
        .split(';')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    }
  }

  Vec::new()
}

/// `text`, a `mimeapps.list`, with its default for `mime` naming `entry`
/// alone and none of `own` or `entry` among the entries whose association
/// with `mime` it removes.
fn claimed(text: &str, mime: &str, own: &[String], entry: &str) -> String {
  let text = with_entries(text, REMOVED, mime, |names| {
    names
      .into_iter()
      .filter(|name| name != entry && !own.contains(name))
      .collect()
  });

  with_entries(&text, DEFAULTS, mime, |_| vec![entry.to_string()])
}

/// `text`, a `mimeapps.list`, with `own` taken out of its default for `mime`
/// and their association with `mime` removed.
fn released(text: &str, mime: &str, own: &[String]) -> String {
  let text = with_entries(text, DEFAULTS, mime, |names| {
    names
      .into_iter()
      .filter(|name| !own.contains(name))
      .collect()
  });

  with_entries(&text, REMOVED, mime, |mut names| {
    for name in own {
      if !names.contains(name) {
        names.push(name.clone());
      }
    }

    names
  })
}

/// `text`, a `mimeapps.list`, with the entries its `group` names for `mime`
/// replaced by what `edited` makes of them. A line left with no entries is
/// dropped, and its group too if nothing else is in it. Every other line is
/// kept.
fn with_entries(
  text: &str,
  group: &str,
  mime: &str,
  edited: impl Fn(Vec<String>) -> Vec<String>,
) -> String {
  let mut lines: Vec<String> = Vec::new();
  let mut current = String::new();
  let mut header = None;
  let mut end_of_group = None;
  let mut emptied = None;
  let mut found = false;

  for line in text.lines() {
    let trimmed = line.trim();

    if trimmed.starts_with('[') {
      current = trimmed.to_string();

      if current == group {
        header = Some(lines.len());
      }
    } else if current == group
      && let Some((key, names)) = trimmed.split_once('=')
      && key.trim() == mime
    {
      let names = edited(
        names
          .split(';')
          .map(str::trim)
          .filter(|name| !name.is_empty())
          .map(str::to_string)
          .collect(),
      );

      found = true;

      if names.is_empty() {
        emptied = header;
      } else {
        lines.push(format!("{mime}={};", names.join(";")));
        end_of_group = Some(lines.len());
      }

      continue;
    }

    lines.push(line.to_string());

    if current == group && !trimmed.is_empty() {
      end_of_group = Some(lines.len());
    }
  }

  if let Some(at) = emptied {
    drop_if_empty(&mut lines, at);
  }

  let names = if found {
    Vec::new()
  } else {
    edited(Vec::new())
  };

  if !names.is_empty() {
    let line = format!("{mime}={};", names.join(";"));

    match end_of_group {
      Some(at) => lines.insert(at, line),
      None => {
        if lines.last().is_some_and(|last| !last.trim().is_empty()) {
          lines.push(String::new());
        }

        lines.push(group.to_string());
        lines.push(line);
      }
    }
  }

  lines.into_iter().map(|line| line + "\n").collect()
}

/// Removes the group whose header is `lines[at]` when only blank lines
/// follow it, up to the next group, along with a blank line it leaves at
/// the end.
fn drop_if_empty(lines: &mut Vec<String>, at: usize) {
  let end = lines[at + 1..]
    .iter()
    .position(|line| line.trim().starts_with('['))
    .map_or(lines.len(), |next| at + 1 + next);

  if lines[at + 1..end]
    .iter()
    .any(|line| !line.trim().is_empty())
  {
    return;
  }

  let last = end == lines.len();

  lines.drain(at..end);

  while last && lines.last().is_some_and(|line| line.trim().is_empty()) {
    lines.pop();
  }
}

/// Rewrites the file at `list` as `edited` makes its text, a missing file
/// reading as empty, unless that changes nothing. A symlinked file is
/// written where it points. The new text is written aside and renamed into
/// place, so a crash leaves the old one whole.
fn edit(list: &Path, edited: impl Fn(&str) -> String) -> io::Result<()> {
  let list = std::fs::canonicalize(list).unwrap_or_else(|_| list.to_path_buf());
  let text = match std::fs::read_to_string(&list) {
    Ok(text) => text,
    Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
    Err(error) => return Err(error),
  };
  let new = edited(&text);

  if new == text {
    return Ok(());
  }

  if let Some(folder) = list.parent() {
    std::fs::create_dir_all(folder)?;
  }

  let partial = list.with_extension("list.new");

  std::fs::write(&partial, new)?;

  std::fs::rename(&partial, &list)
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  const MIME: &str = "x-scheme-handler/ror2mm";
  const ID: &str = "org.example.App";

  /// Folders under one temporary folder, with KDE running.
  fn folders(root: &TempDir) -> Folders {
    Folders {
      config_home: root.path().join("config"),
      config_dirs: vec![root.path().join("etc/xdg")],
      data_home: root.path().join("data"),
      data_dirs: vec![root.path().join("usr/share")],
      desktops: vec!["kde".to_string()],
    }
  }

  fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
  }

  /// Installs a desktop entry named `name` under `data`'s applications.
  fn entry(data: &Path, name: &str, extra: &str) {
    write(
      &data.join("applications").join(name),
      &format!("[Desktop Entry]\nType=Application\nName=X\n{extra}"),
    );
  }

  fn user_list(folders: &Folders) -> String {
    std::fs::read_to_string(folders.config_home.join("mimeapps.list")).unwrap_or_default()
  }

  #[test]
  fn an_unclaimed_scheme_is_claimed_in_the_users_list() {
    let root = TempDir::new().unwrap();
    let folders = folders(&root);

    entry(&folders.data_home, "org.example.App.desktop", "");

    assert!(register(&folders, Claim::IfUnclaimed, MIME, ID).unwrap());
    assert_eq!(
      user_list(&folders),
      "[Default Applications]\nx-scheme-handler/ror2mm=org.example.App.desktop;\n"
    );
  }

  #[test]
  fn claiming_keeps_every_other_line_of_the_users_list() {
    let root = TempDir::new().unwrap();
    let folders = folders(&root);

    entry(&folders.data_home, "org.example.App.desktop", "");
    write(
      &folders.config_home.join("mimeapps.list"),
      "[Added Associations]\ntext/plain=editor.desktop;\n\n\
       [Default Applications]\ntext/html=browser.desktop;\nx-scheme-handler/ror2mm=gone.desktop;\n",
    );

    assert!(register(&folders, Claim::Always, MIME, ID).unwrap());
    assert_eq!(
      user_list(&folders),
      "[Added Associations]\ntext/plain=editor.desktop;\n\n\
       [Default Applications]\ntext/html=browser.desktop;\nx-scheme-handler/ror2mm=org.example.App.desktop;\n"
    );
  }

  #[test]
  fn a_scheme_another_installed_app_opens_is_left_to_it_unless_always_claimed() {
    let root = TempDir::new().unwrap();
    let folders = folders(&root);
    let theirs = "[Default Applications]\nx-scheme-handler/ror2mm=r2modman.desktop;\n";

    entry(&folders.data_home, "org.example.App.desktop", "");
    entry(&folders.data_dirs[0], "r2modman.desktop", "");
    write(&folders.config_home.join("mimeapps.list"), theirs);

    assert!(!register(&folders, Claim::IfUnclaimed, MIME, ID).unwrap());
    assert_eq!(user_list(&folders), theirs);
    assert!(register(&folders, Claim::Always, MIME, ID).unwrap());
  }

  #[test]
  fn an_app_that_only_lists_the_scheme_in_its_entry_holds_it() {
    let root = TempDir::new().unwrap();
    let folders = folders(&root);

    entry(&folders.data_home, "org.example.App.desktop", "");
    entry(
      &folders.data_dirs[0],
      "r2modman.desktop",
      "MimeType=x-scheme-handler/ror2mm;\n",
    );

    assert!(!register(&folders, Claim::IfUnclaimed, MIME, ID).unwrap());
    assert_eq!(user_list(&folders), "");
  }

  #[test]
  fn the_apps_own_entry_listing_the_scheme_does_not_hold_it_over_another_app() {
    let root = TempDir::new().unwrap();
    let folders = folders(&root);

    entry(
      &folders.data_home,
      "org.example.App.desktop",
      "MimeType=x-scheme-handler/ror2mm;\n",
    );
    entry(
      &folders.data_dirs[0],
      "r2modman.desktop",
      "MimeType=x-scheme-handler/ror2mm;\n",
    );

    assert!(!register(&folders, Claim::IfUnclaimed, MIME, ID).unwrap());
    assert_eq!(user_list(&folders), "");
  }

  #[test]
  fn releasing_removes_the_apps_own_association_until_it_claims_again() {
    let root = TempDir::new().unwrap();
    let folders = folders(&root);

    entry(
      &folders.data_home,
      "org.example.App.desktop",
      "MimeType=x-scheme-handler/ror2mm;\n",
    );

    assert!(!register(&folders, Claim::Release, MIME, ID).unwrap());
    assert_eq!(
      user_list(&folders),
      "[Removed Associations]\nx-scheme-handler/ror2mm=org.example.App.desktop;\n"
    );
    assert!(register(&folders, Claim::Always, MIME, ID).unwrap());
    assert_eq!(
      user_list(&folders),
      "[Default Applications]\nx-scheme-handler/ror2mm=org.example.App.desktop;\n"
    );
  }

  #[test]
  fn a_default_naming_an_app_no_longer_installed_does_not_count() {
    let root = TempDir::new().unwrap();
    let folders = folders(&root);

    entry(&folders.data_home, "org.example.App.desktop", "");
    write(
      &folders.config_dirs[0].join("mimeapps.list"),
      "[Default Applications]\nx-scheme-handler/ror2mm=r2modman.desktop;\n",
    );

    assert!(register(&folders, Claim::IfUnclaimed, MIME, ID).unwrap());
  }

  #[test]
  fn a_desktop_specific_list_wins_over_the_users_list() {
    let root = TempDir::new().unwrap();
    let folders = folders(&root);

    entry(&folders.data_home, "org.example.App.desktop", "");
    entry(&folders.data_dirs[0], "r2modman.desktop", "");
    write(
      &folders.config_home.join("kde-mimeapps.list"),
      "[Default Applications]\nx-scheme-handler/ror2mm=r2modman.desktop;\n",
    );
    write(
      &folders.config_home.join("mimeapps.list"),
      "[Default Applications]\nx-scheme-handler/ror2mm=org.example.App.desktop;\n",
    );

    assert!(!register(&folders, Claim::IfUnclaimed, MIME, ID).unwrap());
  }

  #[test]
  fn an_appimage_integrated_entry_is_the_one_claimed() {
    let root = TempDir::new().unwrap();
    let folders = folders(&root);
    let appimage = root.path().join("App.AppImage");

    write(&appimage, "");
    entry(
      &folders.data_home,
      "appimagekit_0123-App.desktop",
      &format!(
        "TryExec={}\nStartupWMClass=org.example.App\nMimeType=x-scheme-handler/ror2mm;\n",
        appimage.display()
      ),
    );

    assert!(register(&folders, Claim::IfUnclaimed, MIME, ID).unwrap());
    assert_eq!(
      user_list(&folders),
      "[Default Applications]\nx-scheme-handler/ror2mm=appimagekit_0123-App.desktop;\n"
    );
  }

  #[test]
  fn releasing_removes_only_the_apps_own_default() {
    let root = TempDir::new().unwrap();
    let folders = folders(&root);

    entry(&folders.data_home, "org.example.App.desktop", "");
    entry(&folders.data_dirs[0], "r2modman.desktop", "");
    write(
      &folders.config_home.join("mimeapps.list"),
      "[Default Applications]\ntext/html=browser.desktop;\nx-scheme-handler/ror2mm=org.example.App.desktop;\n",
    );

    assert!(!register(&folders, Claim::Release, MIME, ID).unwrap());
    assert_eq!(
      user_list(&folders),
      "[Default Applications]\ntext/html=browser.desktop;\n\n\
       [Removed Associations]\nx-scheme-handler/ror2mm=org.example.App.desktop;\n"
    );

    write(
      &folders.config_home.join("mimeapps.list"),
      "[Default Applications]\nx-scheme-handler/ror2mm=r2modman.desktop;\n",
    );

    assert!(!register(&folders, Claim::Release, MIME, ID).unwrap());
    assert_eq!(
      user_list(&folders),
      "[Default Applications]\nx-scheme-handler/ror2mm=r2modman.desktop;\n\n\
       [Removed Associations]\nx-scheme-handler/ror2mm=org.example.App.desktop;\n"
    );
  }
}
