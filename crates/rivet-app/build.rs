//! Embeds Rivet's icon in Windows builds, where Explorer, the taskbar, and
//! the window's title bar all read it from the executable.

use std::path::PathBuf;

fn main() {
  let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
  let icon = manifest.join("assets/icon/rivet.ico");

  println!("cargo:rerun-if-changed=build.rs");
  println!("cargo:rerun-if-changed={}", icon.display());

  if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
    return;
  }

  let script = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("rivet.rc");
  let icon = icon.to_string_lossy().replace('\\', "/");

  // gpui loads the window's icon as resource 1.
  std::fs::write(&script, format!("1 ICON \"{icon}\"\n")).expect("writing rivet.rc");

  embed_resource::compile(&script, embed_resource::NONE)
    .manifest_optional()
    .unwrap();
}
