//! The files bundled into the binary for gpui to load by path: the component
//! library's default icons plus the few Lucide icons Rivet draws beyond them.

use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

gpui_kit::assets::icon_assets!(Extra, [X, Gamepad2, List, LayoutGrid]);

/// Rivet's asset source. Draw an extra icon with its `gpui_kit::assets::IconName`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl AssetSource for Assets {
  fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
    match Extra.load(path)? {
      Some(icon) => Ok(Some(icon)),
      None => gpui_kit::assets::Assets.load(path),
    }
  }

  fn list(&self, path: &str) -> Result<Vec<SharedString>> {
    let mut paths = gpui_kit::assets::Assets.list(path)?;

    paths.extend(Extra.list(path)?);

    Ok(paths)
  }
}
