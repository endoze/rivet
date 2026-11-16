//! Where the app keeps its files in each platform's standard locations.

use std::path::PathBuf;

/// The folder the app's files sit under in each standard location.
const APP_FOLDER: &str = "rivet";

/// The settings file in the platform's config location, such as
/// `~/.config/rivet/settings.toml` on Linux. `None` when the platform
/// reports no config location, as when there is no home folder.
pub fn settings_file() -> Option<PathBuf> {
  dirs::config_dir().map(|config| config.join(APP_FOLDER).join("settings.toml"))
}

/// The folder for data the app can fetch again, such as package indexes, in
/// the platform's cache location: `~/.cache/rivet` on Linux. `None` when the
/// platform reports no cache location.
pub fn cache_folder() -> Option<PathBuf> {
  dirs::cache_dir().map(|cache| cache.join(APP_FOLDER))
}

/// The folder for profiles and the packages installed into them, in the
/// platform's data location: `~/.local/share/rivet` on Linux. `None` when the
/// platform reports no data location.
pub fn data_folder() -> Option<PathBuf> {
  dirs::data_dir().map(|data| data.join(APP_FOLDER))
}

/// The folder a save dialog opens in: the user's documents folder, or the
/// home folder when the platform reports none. `None` when there is neither.
pub fn save_folder() -> Option<PathBuf> {
  dirs::document_dir().or_else(dirs::home_dir)
}
