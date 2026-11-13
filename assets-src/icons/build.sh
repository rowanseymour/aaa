#!/bin/sh
# Rebuilds the interface's icons from their SVGs: white on clear, for the
# game to tint, and large enough to stay sharp once the interface is scaled
# up.
#
#   assets-src/icons/build.sh
#
# Needs ImageMagick 7 (`magick`).

set -eu

src=$(cd "$(dirname "$0")" && pwd)
out=$(cd "$src/../.." && pwd)/assets/icons
size=128

for svg in "$src"/*.svg; do
    name=$(basename "$svg" .svg)
    # Drawn at four times the size and scaled down, for smooth edges.
    magick -background none -density $((96 * size * 4 / 64)) "$svg" \
        -filter Lanczos -resize ${size}x$size -strip "$out/$name.png"
done
