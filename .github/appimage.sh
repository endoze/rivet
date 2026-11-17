#!/usr/bin/env bash
# Packs a Linux build of Rivet into an AppImage: the binary, its desktop entry
# and its icons, and nothing else, so the system's graphics drivers and
# Vulkan loader are used as with the bare binary.
#
# Usage: .github/appimage.sh <rivet binary> <AppImage to write>
#
# Run from the repo root. The AppImage uses the static type 2 runtime, which
# needs no libfuse2 on the system. appimagetool and the runtime are pinned by
# version and SHA-256.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <rivet binary> <AppImage to write>" >&2
  exit 2
fi

binary="$1"
out="$2"
id=io.github.endoze.rivet
icons=crates/rivet-app/assets/icon

appimagetool_version=1.9.1
appimagetool_sha256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
runtime_version=20251108
runtime_sha256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fetch() {
  local url="$1" sha256="$2" file="$3"

  curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$file" "$url"
  echo "$sha256  $file" | sha256sum --check --quiet -
}

fetch "https://github.com/AppImage/appimagetool/releases/download/$appimagetool_version/appimagetool-x86_64.AppImage" \
  "$appimagetool_sha256" "$work/appimagetool"
fetch "https://github.com/AppImage/type2-runtime/releases/download/$runtime_version/runtime-x86_64" \
  "$runtime_sha256" "$work/runtime"
chmod +x "$work/appimagetool"

appdir="$work/Rivet.AppDir"

install -Dm755 "$binary" "$appdir/usr/bin/rivet"
ln -s usr/bin/rivet "$appdir/AppRun"

# Matches the entry and mime package Rivet writes for itself
# (rivet_platform::desktop::entry and mime_package), except that Exec names
# the binary inside the AppImage. Integration tools such as AppImageLauncher
# and Gear Lever copy the entry and point Exec at the file. AppImageLauncher
# installs the mime package too; Rivet writes its own either way.
install -d "$appdir/usr/share/applications" "$appdir/usr/share/mime/packages"
cat > "$appdir/usr/share/applications/$id.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Rivet
Comment=Mod manager for Thunderstore games
Exec=rivet %u
Icon=$id
Terminal=false
Categories=Game;
MimeType=application/x-r2z;x-scheme-handler/ror2mm;
StartupWMClass=$id
EOF
cat > "$appdir/usr/share/mime/packages/$id.xml" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-r2z">
    <comment>Mod profile export</comment>
    <sub-class-of type="application/zip"/>
    <glob pattern="*.r2z"/>
  </mime-type>
</mime-info>
EOF
ln -s "usr/share/applications/$id.desktop" "$appdir/$id.desktop"

for size in 16 24 32 48 64 128 256; do
  install -Dm644 "$icons/rivet-$size.png" "$appdir/usr/share/icons/hicolor/${size}x${size}/apps/$id.png"
done

ln -s "usr/share/icons/hicolor/256x256/apps/$id.png" "$appdir/$id.png"
ln -s "$id.png" "$appdir/.DirIcon"

# appimagetool is itself an AppImage. Extracting it rather than mounting it
# needs no FUSE, which containers and CI runners may lack.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$work/appimagetool" --no-appstream \
  --runtime-file "$work/runtime" "$appdir" "$out"
