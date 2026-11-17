#!/usr/bin/env bash
# Unpacks the builds dist packed in target/distrib into the bare files Rivet's
# updater downloads, and packs the Linux one into an AppImage, each with a
# sha256sum checksum beside it. dist runs this in its global build, after
# every platform's archive is fetched. The AppImage's runtime is static, so
# it needs no newer glibc than the binary, which was built in Debian 12.
set -euo pipefail

distrib=target/distrib
out=target/bare
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

mkdir -p "$out"

unpack() {
  local archive="$distrib/$1" binary="$2" asset="$3"

  if [ ! -f "$archive" ]; then
    echo "error: $archive is missing, so $asset can't be attached" >&2
    exit 1
  fi

  mkdir -p "$work/$asset"

  case "$archive" in
    *.zip) unzip -q "$archive" -d "$work/$asset" ;;
    *) tar -xf "$archive" -C "$work/$asset" ;;
  esac

  local found
  found="$(find "$work/$asset" -type f -name "$binary" | head -n 1)"

  if [ -z "$found" ]; then
    echo "error: $archive holds no $binary" >&2
    exit 1
  fi

  cp "$found" "$out/$asset"
  chmod +x "$out/$asset"
  (cd "$out" && sha256sum "$asset" > "$asset.sha256")
}

unpack rivet-app-x86_64-unknown-linux-gnu.tar.xz rivet rivet-linux-x86_64
unpack rivet-app-x86_64-pc-windows-msvc.zip rivet.exe rivet-windows-x86_64.exe

bash .github/appimage.sh "$out/rivet-linux-x86_64" "$out/rivet-linux-x86_64.AppImage"
(cd "$out" && sha256sum rivet-linux-x86_64.AppImage > rivet-linux-x86_64.AppImage.sha256)
