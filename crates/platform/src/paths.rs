//! Where the app keeps its files in each platform's standard locations.

use std::path::PathBuf;

/// The folder the app's files sit under in each standard location.
const APP_FOLDER: &str = "mod-manager";

/// The settings file in the platform's config location, such as
/// `~/.config/mod-manager/settings.toml` on Linux. `None` when the platform
/// reports no config location, as when there is no home folder.
pub fn settings_file() -> Option<PathBuf> {
  dirs::config_dir().map(|config| config.join(APP_FOLDER).join("settings.toml"))
}
