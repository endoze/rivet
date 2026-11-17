#!/usr/bin/env bash
# Renders the app icons in crates/rivet-app/assets/icon from the SVGs in this
# folder. Needs ImageMagick with SVG support. Run from the repo root.
set -euo pipefail

out=crates/rivet-app/assets/icon
mkdir -p "$out"

# Below 256 px the word on the square logo is unreadable, so the small sizes
# use the single-letter icon.
for size in 16 24 32 48 64 128; do
  magick -background none -density 384 assets/icon.svg -resize "${size}x${size}" "$out/rivet-$size.png"
done

magick -background none -density 192 assets/logo-square.svg -resize 256x256 "$out/rivet-256.png"

magick "$out"/rivet-{16,24,32,48,256}.png "$out/rivet.ico"
