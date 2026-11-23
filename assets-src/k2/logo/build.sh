#!/bin/sh
# Rebuilds K2's logo and app icons.
#
#   assets-src/k2/logo/build.sh
#
# Needs ImageMagick 7 (`magick`).

src=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$src/../../.." && pwd)
emblem=$src/emblem.jpg
text=K2 pointsize=220
font=$root/data/k2/Fonts/TrueType/CinzelDecorative-700.ttf
outer=446 inner=225
logo=$root/data/k2/Modern/logo.png
icons=$root/packaging/k2/icons
. "$root/assets-src/logo.sh"

echo "wrote $logo and $icons"
