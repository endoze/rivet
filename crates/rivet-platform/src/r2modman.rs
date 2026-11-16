//! Finding the profiles r2modman keeps on this computer. Nothing here writes
//! to r2modman's files.

use std::path::{Path, PathBuf};
use thunderstore_engine::profile::modlist;

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

/// The profile folders r2modman holds for a game it files under each of
/// `game_folders`, such as those `GameDef::r2modman_folders` names, in each of
/// `data_folders`: by data folder, then game folder, then name. A profile
/// counts once it records a mod list that is not empty. Folders that cannot
/// be read hold none.
pub fn profiles(data_folders: &[PathBuf], game_folders: &[String]) -> Vec<PathBuf> {
  data_folders
    .iter()
    .flat_map(|data| {
      game_folders
        .iter()
        .flat_map(move |game| profiles_in(&data.join(game).join("profiles")))
    })
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
    .filter(|profile| has_mods(profile))
    .collect();

  found.sort();

  found
}

/// Whether the r2modman profile in `profile` records a mod list that holds a
/// mod.
fn has_mods(profile: &Path) -> bool {
  // A list that cannot be read still counts, so importing it tells the user
  // why rather than the profile silently going missing.
  profile.join("mods.yml").is_file() && modlist::read(profile).map_or(true, |mods| !mods.is_empty())
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  /// A `mods.yml` entry as r2modman writes it, trimmed to the keys it needs.
  const ONE_MOD: &str = "- name: Author-Mod\n  versionNumber: {major: 1, minor: 0, patch: 0}\n";

  /// Makes `profile` in r2modman's `data` folder for `game`, with `mods` as
  /// its mod list when given.
  fn profile(data: &Path, game: &str, profile: &str, mods: Option<&str>) -> PathBuf {
    let folder = data.join(game).join("profiles").join(profile);

    std::fs::create_dir_all(&folder).unwrap();

    if let Some(mods) = mods {
      std::fs::write(folder.join("mods.yml"), mods).unwrap();
    }

    folder
  }

  fn folders(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
  }

  #[test]
  fn a_games_profiles_are_found_in_each_data_folder_by_name() {
    let dir = TempDir::new().unwrap();
    let (native, flatpak) = (dir.path().join("native"), dir.path().join("flatpak"));
    let modded = profile(&native, "Valheim", "Modded", Some(ONE_MOD));
    let default = profile(&native, "Valheim", "Default", Some(ONE_MOD));
    let sandboxed = profile(&flatpak, "Valheim", "Default", Some(ONE_MOD));

    profile(&native, "Valheim", "Unlisted", None);
    profile(&native, "RiskOfRain2", "Other", Some(ONE_MOD));
    std::fs::write(native.join("Valheim/profiles/stray.txt"), "").unwrap();

    let found = profiles(
      &[native, dir.path().join("missing"), flatpak],
      &folders(&["Valheim"]),
    );

    assert_eq!(found, [default, modded, sandboxed]);
  }

  #[test]
  fn profiles_are_found_in_every_folder_the_game_is_filed_under() {
    let dir = TempDir::new().unwrap();
    let data = dir.path().join("data");
    let classic = profile(&data, "Outward", "Classic", Some(ONE_MOD));
    let definitive = profile(&data, "OutwardDe", "Definitive", Some(ONE_MOD));

    let found = profiles(&[data], &folders(&["Outward", "OutwardDe"]));

    assert_eq!(found, [classic, definitive]);
  }

  #[test]
  fn the_folder_r2modman_builds_an_update_in_is_not_a_profile() {
    let dir = TempDir::new().unwrap();
    let data = dir.path().join("data");

    profile(&data, "Valheim", "_profile_update", Some(ONE_MOD));

    assert_eq!(
      profiles(&[data], &folders(&["Valheim"])),
      Vec::<PathBuf>::new()
    );
  }

  #[test]
  fn a_profile_with_no_mods_is_not_found() {
    let dir = TempDir::new().unwrap();
    let data = dir.path().join("data");

    profile(&data, "Valheim", "Fresh", Some("[]"));

    assert_eq!(
      profiles(&[data], &folders(&["Valheim"])),
      Vec::<PathBuf>::new()
    );
  }

  #[test]
  fn a_profile_whose_mod_list_cannot_be_read_is_still_found_so_reading_it_can_say_why() {
    let dir = TempDir::new().unwrap();
    let data = dir.path().join("data");
    let damaged = profile(&data, "Valheim", "Damaged", Some("- name: [unclosed"));

    assert_eq!(profiles(&[data], &folders(&["Valheim"])), [damaged]);
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
