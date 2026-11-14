//! Reading a Steam install: where its games are installed, and the launch
//! options the signed-in user set for a game. Nothing here writes to Steam's
//! files.

use std::path::{Path, PathBuf};

/// The launch options Steam's most recent user set for `app_id`, read from
/// the Steam install at `root`. `None` when that user set none.
///
/// # Errors
///
/// A description of the file that could not be read or parsed, or of the
/// signed-in user that could not be found.
pub fn launch_options(root: &Path, app_id: u32) -> Result<Option<String>, String> {
  let users = read_vdf(&root.join("config").join("loginusers.vdf"))?;
  let account = most_recent_account(&users)
    .ok_or_else(|| "Steam has no signed-in user in loginusers.vdf".to_string())?;
  let config = read_vdf(
    &root
      .join("userdata")
      .join(account.to_string())
      .join("config")
      .join("localconfig.vdf"),
  )?;
  let app_id = app_id.to_string();
  let options = lookup(
    &config,
    &[
      "UserLocalConfigStore",
      "Software",
      "Valve",
      "Steam",
      "apps",
      &app_id,
      "LaunchOptions",
    ],
  )
  .and_then(Value::text)
  .map(str::to_string);

  Ok(options)
}

/// Where Steam installs live under the user's home folder on Linux, native
/// first, then Flatpak.
const CANDIDATES: [&str; 6] = [
  ".local/share/Steam",
  ".steam/steam",
  ".steam/root",
  ".steam",
  ".var/app/com.valvesoftware.Steam/.local/share/Steam",
  ".var/app/com.valvesoftware.Steam/.steam/steam",
];

/// The first Steam install found under the user's home folder, or `None`.
/// Covers the native and Flatpak installs on Linux.
pub fn root() -> Option<PathBuf> {
  let home = dirs::home_dir()?;

  CANDIDATES
    .iter()
    .map(|candidate| home.join(candidate))
    .find(|candidate| candidate.join("config").join("loginusers.vdf").is_file())
}

/// Every Steam install under the user's home folder, native before Flatpak,
/// each listed once however many links lead to it. Empty when there is none.
pub fn installs() -> Vec<PathBuf> {
  dirs::home_dir()
    .map(|home| installs_in(&home))
    .unwrap_or_default()
}

fn installs_in(home: &Path) -> Vec<PathBuf> {
  let mut found: Vec<PathBuf> = Vec::new();

  for candidate in CANDIDATES.iter().map(|candidate| home.join(candidate)) {
    if candidate.join("steamapps").is_dir() && !found.iter().any(|seen| same(seen, &candidate)) {
      found.push(candidate);
    }
  }

  found
}

/// The Steam libraries of the install at `install`: the install itself, then
/// every other folder its `steamapps/libraryfolders.vdf` lists, such as one on
/// another drive.
///
/// The install comes first because a Flatpak install lists its own folder by
/// its path inside the sandbox, which is not where it is on the host.
///
/// # Errors
///
/// A description of the library file that could not be read or parsed. A
/// missing file is not an error: the install is then the only library.
pub fn libraries(install: &Path) -> Result<Vec<PathBuf>, String> {
  let file = install.join("steamapps").join("libraryfolders.vdf");
  let mut found = vec![install.to_path_buf()];

  if !file.exists() {
    return Ok(found);
  }

  let folders = read_vdf(&file)?;
  let listed = folders
    .first()
    .and_then(|(_, value)| value.entries())
    .unwrap_or_default()
    .iter()
    .filter(|(key, _)| key.parse::<u32>().is_ok())
    .filter_map(|(_, value)| match value {
      Value::Text(path) => Some(path.as_str()),
      Value::Entries(entries) => lookup(entries, &["path"]).and_then(Value::text),
    })
    .map(PathBuf::from);

  for library in listed {
    if !found.iter().any(|seen| same(seen, &library)) {
      found.push(library);
    }
  }

  Ok(found)
}

/// The folder `app_id` is installed in, searching every library of each of
/// `installs` in turn. `None` when no library has it.
///
/// # Errors
///
/// When the game is found nowhere and an install's library file or a
/// manifest for the game could not be read, a description of the first such
/// file.
pub fn game_folder(installs: &[PathBuf], app_id: u32) -> Result<Option<PathBuf>, String> {
  let mut failure = None;

  for install in installs {
    let libraries = match libraries(install) {
      Ok(libraries) => libraries,
      Err(error) => {
        failure.get_or_insert(error);

        continue;
      }
    };

    for library in libraries {
      let steamapps = library.join("steamapps");
      let manifest = steamapps.join(format!("appmanifest_{app_id}.acf"));

      if !manifest.is_file() {
        continue;
      }

      let dir = read_vdf(&manifest).and_then(|entries| {
        lookup(&entries, &["AppState", "installdir"])
          .and_then(Value::text)
          .map(str::to_string)
          .ok_or_else(|| format!("{}: names no install folder", manifest.display()))
      });

      match dir {
        Ok(dir) => return Ok(Some(steamapps.join("common").join(dir))),
        Err(error) => {
          failure.get_or_insert(error);
        }
      }
    }
  }

  failure.map_or(Ok(None), Err)
}

/// Whether `a` and `b` name the same folder, following links.
fn same(a: &Path, b: &Path) -> bool {
  if a == b {
    return true;
  }

  match (a.canonicalize(), b.canonicalize()) {
    (Ok(a), Ok(b)) => a == b,
    _ => false,
  }
}

/// The 32-bit account id of the user marked most recent, or of the one who
/// signed in last when none is marked.
fn most_recent_account(users: &[(String, Value)]) -> Option<u32> {
  let users = lookup(users, &["users"])?.entries()?;
  fn field<'a>(user: &'a Value, key: &str) -> Option<&'a str> {
    lookup(user.entries()?, &[key]).and_then(Value::text)
  }

  let marked = users
    .iter()
    .find(|(_, user)| field(user, "MostRecent") == Some("1"));
  let latest = || {
    users.iter().max_by_key(|(_, user)| {
      field(user, "Timestamp")
        .and_then(|stamp| stamp.parse::<u64>().ok())
        .unwrap_or(0)
    })
  };
  let (id, _) = marked.or_else(latest)?;
  let id: u64 = id.parse().ok()?;

  Some((id & 0xFFFF_FFFF) as u32)
}

/// A value in Valve's text KeyValues format: a string, or nested pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
  Text(String),
  Entries(Vec<(String, Value)>),
}

impl Value {
  fn text(&self) -> Option<&str> {
    match self {
      Value::Text(text) => Some(text),
      Value::Entries(_) => None,
    }
  }

  fn entries(&self) -> Option<&[(String, Value)]> {
    match self {
      Value::Entries(entries) => Some(entries),
      Value::Text(_) => None,
    }
  }
}

/// Follows `path` through nested entries, matching keys without regard to
/// case, as Steam itself does.
fn lookup<'a>(entries: &'a [(String, Value)], path: &[&str]) -> Option<&'a Value> {
  let (first, rest) = path.split_first()?;
  let value = entries
    .iter()
    .find(|(key, _)| key.eq_ignore_ascii_case(first))
    .map(|(_, value)| value)?;

  if rest.is_empty() {
    return Some(value);
  }

  lookup(value.entries()?, rest)
}

fn read_vdf(file: &Path) -> Result<Vec<(String, Value)>, String> {
  let text =
    std::fs::read_to_string(file).map_err(|error| format!("{}: {error}", file.display()))?;

  parse(&text).map_err(|error| format!("{}: {error}", file.display()))
}

/// Parses KeyValues text into its top-level pairs.
fn parse(text: &str) -> Result<Vec<(String, Value)>, String> {
  let mut tokens = Tokens(text.chars().peekable());
  let entries = pairs(&mut tokens)?;

  match tokens.next()? {
    None => Ok(entries),
    Some(Token::Close) => Err("an unmatched closing brace".to_string()),
    Some(_) => Err("unexpected text after the last entry".to_string()),
  }
}

/// Pairs up to a closing brace or the end of the text, leaving that token
/// unread.
fn pairs(tokens: &mut Tokens<'_>) -> Result<Vec<(String, Value)>, String> {
  let mut entries = Vec::new();

  while let Some(Token::Text(_)) = tokens.peek()? {
    let Some(Token::Text(key)) = tokens.next()? else {
      unreachable!("peeked a key");
    };
    let value = match tokens.next()? {
      Some(Token::Text(text)) => Value::Text(text),
      Some(Token::Open) => {
        let nested = pairs(tokens)?;

        if tokens.next()? != Some(Token::Close) {
          return Err(format!("the entries under {key:?} are never closed"));
        }

        Value::Entries(nested)
      }
      _ => return Err(format!("{key:?} has no value")),
    };

    entries.push((key, value));
  }

  Ok(entries)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
  Text(String),
  Open,
  Close,
}

struct Tokens<'a>(std::iter::Peekable<std::str::Chars<'a>>);

impl Tokens<'_> {
  fn peek(&mut self) -> Result<Option<Token>, String> {
    let saved = self.0.clone();
    let token = self.next();

    self.0 = saved;

    token
  }

  fn next(&mut self) -> Result<Option<Token>, String> {
    loop {
      match self.0.peek() {
        Some(c) if c.is_whitespace() => {
          self.0.next();
        }
        Some('/') => {
          for c in self.0.by_ref() {
            if c == '\n' {
              break;
            }
          }
        }
        _ => break,
      }
    }

    let Some(c) = self.0.next() else {
      return Ok(None);
    };

    match c {
      '{' => Ok(Some(Token::Open)),
      '}' => Ok(Some(Token::Close)),
      '"' => self.quoted().map(|text| Some(Token::Text(text))),
      c => {
        let mut text = c.to_string();

        while let Some(&c) = self.0.peek() {
          if c.is_whitespace() || matches!(c, '{' | '}' | '"') {
            break;
          }

          text.push(c);
          self.0.next();
        }

        Ok(Some(Token::Text(text)))
      }
    }
  }

  fn quoted(&mut self) -> Result<String, String> {
    let mut text = String::new();

    while let Some(c) = self.0.next() {
      match c {
        '"' => return Ok(text),
        '\\' => match self.0.next() {
          Some('n') => text.push('\n'),
          Some('t') => text.push('\t'),
          Some(other) => text.push(other),
          None => break,
        },
        c => text.push(c),
      }
    }

    Err("a quoted string is never closed".to_string())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  const LOGIN_USERS: &str = r#""users"
{
	"76561197960265729"
	{
		"AccountName"		"old"
		"MostRecent"		"0"
		"Timestamp"		"1600000000"
	}
	"76561197974767759"
	{
		"AccountName"		"me"
		"mostrecent"		"1"
		"Timestamp"		"1700000000"
	}
}
"#;

  /// A Steam install at `dir` whose most recent user, account 14502031, has
  /// `local_config` as their `localconfig.vdf`.
  fn steam_with(dir: &TempDir, local_config: &str) -> PathBuf {
    let root = dir.path().join("Steam");
    let config = root.join("userdata/14502031/config");

    std::fs::create_dir_all(root.join("config")).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(root.join("config/loginusers.vdf"), LOGIN_USERS).unwrap();
    std::fs::write(config.join("localconfig.vdf"), local_config).unwrap();

    root
  }

  fn local_config(apps: &str) -> String {
    format!(
      r#""UserLocalConfigStore"
{{
	// Steam's own comment
	"Software"
	{{
		"Valve"
		{{
			"Steam"
			{{
				"apps"
				{{
{apps}
				}}
			}}
		}}
	}}
}}
"#
    )
  }

  #[test]
  fn the_most_recent_users_launch_options_for_the_game_are_read() {
    let dir = TempDir::new().unwrap();
    let root = steam_with(
      &dir,
      &local_config(
        r#"					"892970"
					{
						"LaunchOptions"		"\"/home/me/rivet/steam-wrapper.sh\" %command%"
					}
					"1"
					{
						"LaunchOptions"		"other"
					}"#,
      ),
    );

    assert_eq!(
      launch_options(&root, 892970),
      Ok(Some(
        r#""/home/me/rivet/steam-wrapper.sh" %command%"#.to_string()
      ))
    );
  }

  #[test]
  fn a_game_without_launch_options_has_none() {
    let dir = TempDir::new().unwrap();
    let root = steam_with(
      &dir,
      &local_config(
        r#"					"892970"
					{
						"playtime"		"5"
					}"#,
      ),
    );

    assert_eq!(launch_options(&root, 892970), Ok(None));
    assert_eq!(launch_options(&root, 4), Ok(None));
  }

  #[test]
  fn keys_match_whatever_their_case() {
    let dir = TempDir::new().unwrap();
    let root = steam_with(
      &dir,
      r#""UserLocalConfigStore" { "software" { "valve" { "steam" { "Apps" { "892970" { "launchoptions" "-console" } } } } } }"#,
    );

    assert_eq!(
      launch_options(&root, 892970),
      Ok(Some("-console".to_string()))
    );
  }

  #[test]
  fn escaped_backslashes_and_quotes_are_unescaped() {
    let dir = TempDir::new().unwrap();
    let root = steam_with(
      &dir,
      &local_config(r#"					"892970" { "LaunchOptions" "\"C:\\Games\\a b.sh\" %command%" }"#),
    );

    assert_eq!(
      launch_options(&root, 892970),
      Ok(Some(r#""C:\Games\a b.sh" %command%"#.to_string()))
    );
  }

  #[test]
  fn a_steam_folder_with_no_users_says_why() {
    let dir = TempDir::new().unwrap();

    let error = launch_options(dir.path(), 892970).unwrap_err();

    assert!(error.contains("loginusers.vdf"), "{error}");
  }

  /// A Steam install at `dir/name` whose `libraryfolders.vdf` is `folders`.
  fn steam_install(dir: &TempDir, name: &str, folders: &str) -> PathBuf {
    let root = dir.path().join(name);

    std::fs::create_dir_all(root.join("steamapps")).unwrap();
    std::fs::write(root.join("steamapps/libraryfolders.vdf"), folders).unwrap();

    root
  }

  /// Marks `app_id` as installed in `library` under `steamapps/common/<dir>`.
  fn install_app(library: &Path, app_id: u32, dir: &str) {
    let steamapps = library.join("steamapps");

    std::fs::create_dir_all(steamapps.join("common").join(dir)).unwrap();
    std::fs::write(
      steamapps.join(format!("appmanifest_{app_id}.acf")),
      format!(
        r#""AppState"
{{
	"appid"		"{app_id}"
	"name"		"A Game"
	"StateFlags"		"4"
	"installdir"		"{dir}"
}}
"#
      ),
    )
    .unwrap();
  }

  fn library_folders(paths: &[&Path]) -> String {
    let entries: String = paths
      .iter()
      .enumerate()
      .map(|(index, path)| {
        format!(
          r#"	"{index}"
	{{
		"path"		"{}"
		"label"		""
		"apps"
		{{
		}}
	}}
"#,
          path.display()
        )
      })
      .collect();

    format!("\"libraryfolders\"\n{{\n{entries}}}\n")
  }

  #[test]
  fn libraries_are_the_install_then_every_listed_folder() {
    let dir = TempDir::new().unwrap();
    let second = dir.path().join("mnt/games/SteamLibrary");
    let root = dir.path().join("Steam");
    let root = steam_install(&dir, "Steam", &library_folders(&[&root, &second]));

    assert_eq!(libraries(&root), Ok(vec![root.clone(), second]));
  }

  #[test]
  fn libraries_are_read_from_the_old_format_too() {
    let dir = TempDir::new().unwrap();
    let root = steam_install(
      &dir,
      "Steam",
      r#""LibraryFolders"
{
	"TimeNextStatsReport"		"1600000000"
	"ContentStatsID"		"-123"
	"1"		"/mnt/games/SteamLibrary"
}
"#,
    );

    assert_eq!(
      libraries(&root),
      Ok(vec![root.clone(), PathBuf::from("/mnt/games/SteamLibrary")])
    );
  }

  #[test]
  fn an_install_without_a_library_file_is_its_only_library() {
    let dir = TempDir::new().unwrap();

    assert_eq!(libraries(dir.path()), Ok(vec![dir.path().to_path_buf()]));
  }

  #[test]
  fn an_unreadable_library_file_says_why() {
    let dir = TempDir::new().unwrap();
    let root = steam_install(&dir, "Steam", r#""libraryfolders" {"#);

    let error = libraries(&root).unwrap_err();

    assert!(error.contains("libraryfolders.vdf"), "{error}");
  }

  #[test]
  fn a_game_in_the_default_library_is_found() {
    let dir = TempDir::new().unwrap();
    let root = steam_install(&dir, "Steam", &library_folders(&[]));

    install_app(&root, 892970, "Valheim");

    assert_eq!(
      game_folder(std::slice::from_ref(&root), 892970),
      Ok(Some(root.join("steamapps/common/Valheim")))
    );
  }

  #[test]
  fn a_game_in_a_second_library_on_another_drive_is_found() {
    let dir = TempDir::new().unwrap();
    let second = dir.path().join("mnt/games/SteamLibrary");
    let root = steam_install(&dir, "Steam", &library_folders(&[&second]));

    install_app(&second, 892970, "Valheim");

    assert_eq!(
      game_folder(&[root], 892970),
      Ok(Some(second.join("steamapps/common/Valheim")))
    );
  }

  #[test]
  fn a_game_in_a_later_steam_install_is_found() {
    let dir = TempDir::new().unwrap();
    let native = steam_install(&dir, "native", &library_folders(&[]));
    let flatpak = steam_install(&dir, "flatpak", &library_folders(&[]));

    install_app(&flatpak, 892970, "Valheim");

    assert_eq!(
      game_folder(&[native, flatpak.clone()], 892970),
      Ok(Some(flatpak.join("steamapps/common/Valheim")))
    );
  }

  #[test]
  fn a_game_that_is_not_installed_is_not_found() {
    let dir = TempDir::new().unwrap();
    let root = steam_install(&dir, "Steam", &library_folders(&[]));

    install_app(&root, 1, "Other");

    assert_eq!(game_folder(&[root], 892970), Ok(None));
  }

  #[test]
  fn a_broken_install_does_not_hide_a_game_in_another() {
    let dir = TempDir::new().unwrap();
    let broken = steam_install(&dir, "broken", r#""libraryfolders" {"#);
    let flatpak = steam_install(&dir, "flatpak", &library_folders(&[]));

    install_app(&flatpak, 892970, "Valheim");

    assert_eq!(
      game_folder(&[broken.clone(), flatpak.clone()], 892970),
      Ok(Some(flatpak.join("steamapps/common/Valheim")))
    );

    let error = game_folder(&[broken], 892970).unwrap_err();

    assert!(error.contains("libraryfolders.vdf"), "{error}");
  }

  #[cfg(unix)]
  #[test]
  fn native_and_flatpak_steam_are_both_found_under_home() {
    let home = TempDir::new().unwrap();
    let native = home.path().join(".local/share/Steam");
    let flatpak = home
      .path()
      .join(".var/app/com.valvesoftware.Steam/.local/share/Steam");

    std::fs::create_dir_all(native.join("steamapps")).unwrap();
    std::fs::create_dir_all(flatpak.join("steamapps")).unwrap();
    std::fs::create_dir_all(home.path().join(".steam")).unwrap();
    std::os::unix::fs::symlink(&native, home.path().join(".steam/steam")).unwrap();

    assert_eq!(installs_in(home.path()), vec![native, flatpak]);
  }

  #[test]
  fn a_home_without_steam_has_no_installs() {
    let home = TempDir::new().unwrap();

    assert_eq!(installs_in(home.path()), Vec::<PathBuf>::new());
  }

  #[test]
  fn an_unterminated_file_says_why() {
    let dir = TempDir::new().unwrap();
    let root = steam_with(&dir, r#""UserLocalConfigStore" { "Software" {"#);

    let error = launch_options(&root, 892970).unwrap_err();

    assert!(error.contains("localconfig.vdf"), "{error}");
  }
}
