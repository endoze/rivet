//! Where the app keeps its files in each platform's standard locations.

use std::path::PathBuf;

/// The folder the app's files sit under in each standard location.
const APP_FOLDER: &str = "rivet";

/// The settings file in the platform's config location, such as
/// `~/.config/rivet/settings.toml` on Linux or
/// `%APPDATA%\rivet\settings.toml` on Windows. `None` when the platform
/// reports no config location, as when there is no home folder.
pub fn settings_file() -> Option<PathBuf> {
  dirs::config_dir().map(|config| config.join(APP_FOLDER).join("settings.toml"))
}

/// The folder for data the app can fetch again, such as package indexes, in
/// the platform's cache location: `~/.cache/rivet` on Linux,
/// `%LOCALAPPDATA%\rivet\cache` on Windows. `None` when the platform reports
/// no cache location.
pub fn cache_folder() -> Option<PathBuf> {
  dirs::cache_dir().map(|cache| apart(cache.join(APP_FOLDER), "cache"))
}

/// The folder for profiles and the packages installed into them, in the
/// platform's local data location: `~/.local/share/rivet` on Linux,
/// `%LOCALAPPDATA%\rivet\data` on Windows. `None` when the platform reports
/// no data location.
///
/// Local rather than roaming on Windows: profiles hold whole mods and the
/// engine records absolute paths into them, so they belong to one machine.
pub fn data_folder() -> Option<PathBuf> {
  dirs::data_local_dir().map(|data| apart(data.join(APP_FOLDER), "data"))
}

/// The folder for the endpoint a running Rivet is reached at, one only this
/// user can reach: `$XDG_RUNTIME_DIR` on Linux, or else the
/// [`data_folder`]. `None` when the platform reports neither.
pub fn endpoint_folder() -> Option<PathBuf> {
  dirs::runtime_dir().or_else(data_folder)
}

/// `folder`, or on Windows its subfolder `name`: Windows has one local
/// location for both cache and data, so each needs a folder of its own.
fn apart(folder: PathBuf, name: &str) -> PathBuf {
  if cfg!(windows) {
    return folder.join(name);
  }

  folder
}

/// The folder a save dialog opens in: the user's documents folder, or the
/// home folder when the platform reports none. `None` when there is neither.
pub fn save_folder() -> Option<PathBuf> {
  dirs::document_dir().or_else(dirs::home_dir)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_cache_and_the_data_never_share_a_folder() {
    let cache = cache_folder().unwrap();
    let data = data_folder().unwrap();

    assert!(
      !cache.starts_with(&data) && !data.starts_with(&cache),
      "{} and {} overlap",
      cache.display(),
      data.display()
    );
  }

  #[cfg(windows)]
  #[test]
  fn windows_keeps_settings_in_roaming_app_data_and_the_rest_in_local() {
    let roaming = PathBuf::from(std::env::var_os("APPDATA").unwrap());
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap());

    assert_eq!(
      settings_file(),
      Some(roaming.join("rivet").join("settings.toml"))
    );
    assert_eq!(cache_folder(), Some(local.join("rivet").join("cache")));
    assert_eq!(data_folder(), Some(local.join("rivet").join("data")));
  }
}
