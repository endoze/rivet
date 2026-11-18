//! Opening a kind of file with a program on Windows, through the user's own
//! file classes under `HKCU\Software\Classes`.

use std::path::Path;

/// A kind of file a program opens.
#[derive(Clone, Copy, Debug)]
pub struct Association<'a> {
  /// The extension, with its dot, such as `.r2z`.
  pub extension: &'a str,
  /// The class the extension maps to, such as `Rivet.r2z`.
  pub prog_id: &'a str,
  /// What Explorer calls the kind of file.
  pub description: &'a str,
  /// The program that opens the file. Its embedded icon is the file's.
  pub executable: &'a Path,
}

/// One string value under `HKEY_CURRENT_USER`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Value {
  /// The key's path, such as `Software\Classes\.r2z`.
  pub key: String,
  /// The value's name, empty for the key's default value.
  pub name: String,
  pub data: String,
}

/// The values that make `association`'s program open its files.
pub fn values(association: &Association) -> Vec<Value> {
  let extension = format!(r"Software\Classes\{}", association.extension);
  let class = format!(r"Software\Classes\{}", association.prog_id);
  let value = |key: &str, name: &str, data: String| Value {
    key: key.to_string(),
    name: name.to_string(),
    data,
  };

  vec![
    value(&extension, "", association.prog_id.to_string()),
    value(
      &format!(r"{extension}\OpenWithProgids"),
      association.prog_id,
      String::new(),
    ),
    value(&class, "", association.description.to_string()),
    value(
      &format!(r"{class}\DefaultIcon"),
      "",
      format!("{},0", association.executable.display()),
    ),
    value(
      &format!(r"{class}\shell\open\command"),
      "",
      command(association),
    ),
  ]
}

/// The command line Explorer runs to open a file, with the file as `%1`.
fn command(association: &Association) -> String {
  format!("\"{}\" \"%1\"", association.executable.display())
}

/// Whether an open command of `current`, or `None` when there is none,
/// must be rewritten for `association`'s program.
pub fn is_stale(current: Option<&str>, association: &Association) -> bool {
  current != Some(command(association).as_str())
}

/// Writes `association`'s [`values`], unless its open command already
/// starts the program, so this can run on every start.
///
/// # Errors
///
/// Why a value could not be written. Values written before it stay.
#[cfg(windows)]
pub fn install(association: &Association) -> std::io::Result<()> {
  use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
  use windows_registry::CURRENT_USER;

  let open = format!(
    r"Software\Classes\{}\shell\open\command",
    association.prog_id
  );
  let current = CURRENT_USER
    .open(&open)
    .and_then(|key| key.get_string(""))
    .ok();

  if !is_stale(current.as_deref(), association) {
    return Ok(());
  }

  for value in values(association) {
    CURRENT_USER
      .create(&value.key)?
      .set_string(&value.name, &value.data)?;
  }

  // Explorer caches file icons and handlers until told they changed.
  unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn association(executable: &Path) -> Association<'_> {
    Association {
      extension: ".appsave",
      prog_id: "App.appsave",
      description: "App save",
      executable,
    }
  }

  fn value(key: &str, name: &str, data: &str) -> Value {
    Value {
      key: key.to_string(),
      name: name.to_string(),
      data: data.to_string(),
    }
  }

  #[test]
  fn the_extension_opens_with_the_program_and_shows_its_icon() {
    let executable = Path::new(r"C:\Games\My App\app.exe");

    assert_eq!(
      values(&association(executable)),
      vec![
        value(r"Software\Classes\.appsave", "", "App.appsave"),
        value(
          r"Software\Classes\.appsave\OpenWithProgids",
          "App.appsave",
          ""
        ),
        value(r"Software\Classes\App.appsave", "", "App save"),
        value(
          r"Software\Classes\App.appsave\DefaultIcon",
          "",
          r"C:\Games\My App\app.exe,0"
        ),
        value(
          r"Software\Classes\App.appsave\shell\open\command",
          "",
          r#""C:\Games\My App\app.exe" "%1""#
        ),
      ]
    );
  }

  #[test]
  fn a_missing_open_command_is_written() {
    assert!(is_stale(None, &association(Path::new(r"C:\Games\app.exe"))));
  }

  #[test]
  fn an_open_command_for_the_running_program_is_left_alone() {
    assert!(!is_stale(
      Some(r#""C:\Games\app.exe" "%1""#),
      &association(Path::new(r"C:\Games\app.exe"))
    ));
  }

  #[test]
  fn an_open_command_for_where_the_program_was_is_rewritten() {
    assert!(is_stale(
      Some(r#""C:\Downloads\app.exe" "%1""#),
      &association(Path::new(r"C:\Games\app.exe"))
    ));
  }
}
