#!/bin/sh
# Rebuilds AA's logo, app icons and the README banner.
#
#   assets-src/aa/logo/build.sh
#
# Needs ImageMagick 7 (`magick`) and python3.

src=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$src/../../.." && pwd)
emblem=$src/emblem.jpg
text=AAA pointsize=235
font=$root/data/modern/Fonts/TrueType/BigShouldersStencilDisplay-800.ttf
outer=452 inner=230
logo=$root/data/modern_aa/Modern/logo.png
icons=$root/packaging/aa/icons
. "$root/assets-src/logo.sh"

python3 "$src/banner.py" "$logo" "$root/docs/banner.jpg" "$work" 4608
echo "wrote $logo, $icons and docs/banner.jpg"
