#!/bin/sh
# Rebuilds the interface's icons from their SVGs: white on clear, for the
# game to tint, at about three times the size they're drawn: sharp once the
# interface is scaled up, without so many texels they alias at its smallest.
#
#   assets-src/icons/build.sh
#
# Needs ImageMagick 7 (`magick`).

set -eu

src=$(cd "$(dirname "$0")" && pwd)
out=$(cd "$src/../.." && pwd)/assets/icons
size=64

for svg in "$src"/*.svg; do
    name=$(basename "$svg" .svg)
    # Drawn at four times the size and scaled down, for smooth edges, with
    # ImageMagick's own renderer so the result doesn't hang on which SVG
    # library it was built with.
    magick -background none -density $((96 * size * 4 / 64)) "msvg:$svg" \
        -filter Lanczos -resize ${size}x$size -strip "$out/$name.png"
done
