//! Opening links of a URL scheme, such as `ror2mm:`, with a program: when to
//! claim the scheme from another app, and on Windows the user's own class
//! for it under `HKCU\Software\Classes`.

use crate::file_types::Value;
use std::path::Path;

/// When to make a program the one that opens a scheme's links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Claim {
  /// Only when no other app opens them.
  IfUnclaimed,
  /// Whichever app opens them now.
  Always,
  /// Never, and remove the program's own registration if it opens them.
  Release,
}

/// Which app opens a scheme's links now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
  /// None does, or the one registered is gone.
  Nobody,
  /// The program registering.
  This,
  /// Another app.
  Another,
}

/// What registering for a scheme changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
  /// Write the program's registration.
  Write,
  /// Remove the program's registration.
  Remove,
  /// Leave the registration as it is.
  Keep,
}

/// What `claim` changes while `holder` opens the scheme's links.
pub fn change(claim: Claim, holder: Holder) -> Change {
  match (claim, holder) {
    (Claim::IfUnclaimed, Holder::Another) => Change::Keep,
    (Claim::IfUnclaimed | Claim::Always, _) => Change::Write,
    (Claim::Release, Holder::This) => Change::Remove,
    (Claim::Release, _) => Change::Keep,
  }
}

impl Change {
  /// Whether the program opens the scheme's links once this change is made,
  /// when `holder` opened them before.
  pub fn opens(self, holder: Holder) -> bool {
    match self {
      Change::Write => true,
      Change::Remove => false,
      Change::Keep => holder == Holder::This,
    }
  }
}

/// A program that opens a scheme's links on Windows.
#[derive(Clone, Copy, Debug)]
pub struct Scheme<'a> {
  /// The scheme, without its colon, such as `ror2mm`.
  pub scheme: &'a str,
  /// The program that opens the links, as its first argument.
  pub executable: &'a Path,
}

/// The values under `HKEY_CURRENT_USER` that make `scheme`'s program open
/// its links.
pub fn values(scheme: &Scheme) -> Vec<Value> {
  let class = format!(r"Software\Classes\{}", scheme.scheme);
  let value = |key: String, name: &str, data: String| Value {
    key,
    name: name.to_string(),
    data,
  };

  vec![
    value(class.clone(), "", format!("URL:{}", scheme.scheme)),
    value(class.clone(), "URL Protocol", String::new()),
    value(
      format!(r"{class}\DefaultIcon"),
      "",
      format!("{},0", scheme.executable.display()),
    ),
    value(format!(r"{class}\shell\open\command"), "", command(scheme)),
  ]
}

/// The command line Windows runs to open a link, with the link as `%1`.
fn command(scheme: &Scheme) -> String {
  format!("\"{}\" \"%1\"", scheme.executable.display())
}

/// Which app the open command `current` starts, or `None` when there is
/// none, compared with `scheme`'s program. A command with a quoted program
/// `exists` says is gone counts as none. An unquoted one counts as
/// another app's, since a path with spaces can't be told from its
/// arguments.
pub fn holder(current: Option<&str>, scheme: &Scheme, exists: impl Fn(&Path) -> bool) -> Holder {
  let Some(current) = current else {
    return Holder::Nobody;
  };

  if current == command(scheme) {
    return Holder::This;
  }

  let present = match current.strip_prefix('"') {
    Some(quoted) => quoted
      .split('"')
      .next()
      .is_some_and(|program| !program.is_empty() && exists(Path::new(program))),
    None => !current.trim().is_empty(),
  };

  if present {
    Holder::Another
  } else {
    Holder::Nobody
  }
}

/// Registers `scheme`'s program for its links as `claim` says, in the
/// user's own classes. Returns whether the program opens them afterwards.
///
/// # Errors
///
/// Why a value could not be written or the class removed.
#[cfg(windows)]
pub fn register(claim: Claim, scheme: &Scheme) -> std::io::Result<bool> {
  use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
  use windows_registry::{CLASSES_ROOT, CURRENT_USER};

  let open = format!(r"{}\shell\open\command", scheme.scheme);
  let current = CLASSES_ROOT
    .open(&open)
    .and_then(|key| key.get_string(""))
    .ok();
  let holder = holder(current.as_deref(), scheme, Path::exists);
  let change = change(claim, holder);

  match change {
    Change::Write if current.as_deref() != Some(command(scheme).as_str()) => {
      for value in values(scheme) {
        CURRENT_USER
          .create(&value.key)?
          .set_string(&value.name, &value.data)?;
      }
    }
    Change::Remove => {
      CURRENT_USER.remove_tree(format!(r"Software\Classes\{}", scheme.scheme))?;
    }
    Change::Write | Change::Keep => return Ok(change.opens(holder)),
  }

  // Explorer caches handlers until told they changed.
  unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };

  Ok(change.opens(holder))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_program_claims_a_scheme_when_unclaimed_or_always_and_releases_only_its_own() {
    let cases = [
      (Claim::IfUnclaimed, Holder::Nobody, Change::Write, true),
      (Claim::IfUnclaimed, Holder::This, Change::Write, true),
      (Claim::IfUnclaimed, Holder::Another, Change::Keep, false),
      (Claim::Always, Holder::Nobody, Change::Write, true),
      (Claim::Always, Holder::This, Change::Write, true),
      (Claim::Always, Holder::Another, Change::Write, true),
      (Claim::Release, Holder::Nobody, Change::Keep, false),
      (Claim::Release, Holder::This, Change::Remove, false),
      (Claim::Release, Holder::Another, Change::Keep, false),
    ];

    for (claim, holder, expected, opens) in cases {
      let made = change(claim, holder);

      assert_eq!(made, expected, "{claim:?} with {holder:?}");
      assert_eq!(made.opens(holder), opens, "{claim:?} with {holder:?}");
    }
  }

  fn value(key: &str, name: &str, data: &str) -> Value {
    Value {
      key: key.to_string(),
      name: name.to_string(),
      data: data.to_string(),
    }
  }

  fn rivet() -> Scheme<'static> {
    Scheme {
      scheme: "ror2mm",
      executable: Path::new(r"C:\Games\Rivet\rivet.exe"),
    }
  }

  #[test]
  fn the_scheme_is_a_url_protocol_opened_with_the_link_as_one_argument() {
    assert_eq!(
      values(&rivet()),
      vec![
        value(r"Software\Classes\ror2mm", "", "URL:ror2mm"),
        value(r"Software\Classes\ror2mm", "URL Protocol", ""),
        value(
          r"Software\Classes\ror2mm\DefaultIcon",
          "",
          r"C:\Games\Rivet\rivet.exe,0"
        ),
        value(
          r"Software\Classes\ror2mm\shell\open\command",
          "",
          r#""C:\Games\Rivet\rivet.exe" "%1""#
        ),
      ]
    );
  }

  #[test]
  fn the_holder_is_read_from_the_program_the_open_command_starts() {
    let exists = |path: &Path| path != Path::new(r"C:\Gone\r2modman.exe");
    let cases = [
      (None, Holder::Nobody),
      (Some(r#""C:\Games\Rivet\rivet.exe" "%1""#), Holder::This),
      (
        Some(r#""C:\Users\me\AppData\Local\Programs\r2modman\r2modman.exe" "%1""#),
        Holder::Another,
      ),
      (Some(r#""C:\Gone\r2modman.exe" "%1""#), Holder::Nobody),
      (Some(r"C:\Tools\opener.exe %1"), Holder::Another),
      (Some(""), Holder::Nobody),
    ];

    for (current, expected) in cases {
      assert_eq!(holder(current, &rivet(), exists), expected, "{current:?}");
    }
  }

  #[test]
  fn an_unquoted_open_command_holds_the_scheme_however_it_splits() {
    let exists = |path: &Path| path == Path::new(r"C:\Program Files\X\x.exe");

    for current in [r"C:\Program Files\X\x.exe %1", r"C:\Gone\r2modman.exe %1"] {
      assert_eq!(
        holder(Some(current), &rivet(), exists),
        Holder::Another,
        "{current:?}"
      );
    }
  }
}
