//! Finding the profiles r2modman keeps on this computer. Nothing here writes
//! to r2modman's files.

use std::path::{Path, PathBuf};

/// The folder r2modman keeps its data in, inside the config location it
/// runs with.
const DATA_FOLDER: &str = "r2modmanPlus-local";

/// The config location of r2modman's Flatpak, under the user's home folder.
const FLATPAK_CONFIG: &str = ".var/app/io.github.ebkr.r2modman/config";

/// Every folder r2modman keeps its data in by default that exists on this
/// computer: the one in the platform's config location, such as
/// `~/.config/r2modmanPlus-local` on Linux, then the Flatpak's. A data
/// folder the user moved elsewhere in r2modman is not found.
pub fn data_folders() -> Vec<PathBuf> {
  data_folders_under(dirs::config_dir().as_deref(), dirs::home_dir().as_deref())
    .into_iter()
    .filter(|folder| folder.is_dir())
    .collect()
}

fn data_folders_under(config: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
  let flatpak = home.map(|home| home.join(FLATPAK_CONFIG));

  config
    .map(Path::to_path_buf)
    .into_iter()
    .chain(flatpak)
    .map(|config| config.join(DATA_FOLDER))
    .collect()
}

/// The profile folders r2modman holds for the game it files under
/// `game_folder` in each of `data_folders`, in that order and by name within
/// each. A profile counts once it records a mod list. Folders that cannot be
/// read hold none.
pub fn profiles(data_folders: &[PathBuf], game_folder: &str) -> Vec<PathBuf> {
  data_folders
    .iter()
    .flat_map(|data| profiles_in(&data.join(game_folder).join("profiles")))
    .collect()
}

/// The profiles in r2modman's profiles folder `folder`, by name. Names that
/// start with `_` are r2modman's scratch folders, such as the one it builds
/// a profile update in.
fn profiles_in(folder: &Path) -> Vec<PathBuf> {
  let Ok(entries) = std::fs::read_dir(folder) else {
    return Vec::new();
  };
  let mut found: Vec<PathBuf> = entries
    .flatten()
    .filter(|entry| !entry.file_name().to_string_lossy().starts_with('_'))
    .map(|entry| entry.path())
    .filter(|profile| profile.join("mods.yml").is_file())
    .collect();

  found.sort();

  found
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  /// Makes `profile` in r2modman's `data` folder for `game`, recording a mod
  /// list when `listed`.
  fn profile(data: &Path, game: &str, profile: &str, listed: bool) -> PathBuf {
    let folder = data.join(game).join("profiles").join(profile);

    std::fs::create_dir_all(&folder).unwrap();

    if listed {
      std::fs::write(folder.join("mods.yml"), "[]").unwrap();
    }

    folder
  }

  #[test]
  fn a_games_profiles_are_found_in_each_data_folder_by_name() {
    let dir = TempDir::new().unwrap();
    let (native, flatpak) = (dir.path().join("native"), dir.path().join("flatpak"));
    let modded = profile(&native, "Valheim", "Modded", true);
    let default = profile(&native, "Valheim", "Default", true);
    let sandboxed = profile(&flatpak, "Valheim", "Default", true);

    profile(&native, "Valheim", "Empty", false);
    profile(&native, "Valheim", "_profile_update", true);
    profile(&native, "RiskOfRain2", "Other", true);
    std::fs::write(native.join("Valheim/profiles/stray.txt"), "").unwrap();

    let found = profiles(&[native, dir.path().join("missing"), flatpak], "Valheim");

    assert_eq!(found, [default, modded, sandboxed]);
  }

  #[test]
  fn r2modman_keeps_its_data_in_the_config_folder_or_its_flatpak_sandbox() {
    let folders = data_folders_under(
      Some(Path::new("/home/me/.config")),
      Some(Path::new("/home/me")),
    );

    assert_eq!(
      folders,
      [
        PathBuf::from("/home/me/.config/r2modmanPlus-local"),
        PathBuf::from("/home/me/.var/app/io.github.ebkr.r2modman/config/r2modmanPlus-local"),
      ]
    );
  }

  #[test]
  fn without_a_config_or_home_folder_there_is_nowhere_to_look() {
    assert_eq!(data_folders_under(None, None), Vec::<PathBuf>::new());
    assert_eq!(
      data_folders_under(None, Some(Path::new("/home/me"))),
      [PathBuf::from(
        "/home/me/.var/app/io.github.ebkr.r2modman/config/r2modmanPlus-local"
      )]
    );
  }
}
