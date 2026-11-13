#!/bin/sh
# Rebuilds the logo, the app icons and the README banner from the emblem
# artwork and the game's stencil font.
#
#   assets-src/logo/build.sh
#
# Needs ImageMagick 7 (`magick`) and python3.

set -eu

src=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$src/../.." && pwd)
out=$root/assets/logo
font=$root/assets/fonts/BigShouldersStencilDisplay-800.ttf
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Lettering, with a soft drop shadow, centred on the emblem.
magick -background none -font "$font" -pointsize 235 -kerning 10 -fill '#e6e0c8' label:AAA -trim +repage "$work/text.png"
magick "$work/text.png" -alpha extract -background black -alpha shape -channel A -evaluate multiply 0.35 +channel "$work/shadow.png"
magick "$src/emblem.jpg" \( "$work/shadow.png" \) -gravity center -geometry +0+5 -composite \
    \( "$work/text.png" \) -gravity center -composite -strip "$out/logo.png"

# Large icons are the disc inside the outer ring; small ones only the inner
# disc, so the lettering stays readable.
magick -size 1024x1024 xc:black -fill white -draw "circle 513,513 513,61" -blur 0x1 "$work/mask.png"
magick "$out/logo.png" "$work/mask.png" -alpha off -compose CopyOpacity -composite -crop 906x906+60+60 +repage "$work/outer.png"
magick -size 1024x1024 xc:black -fill white -draw "circle 512,512 512,282" -blur 0x1 "$work/mask-small.png"
magick "$out/logo.png" "$work/mask-small.png" -alpha off -compose CopyOpacity -composite -crop 462x462+281+281 +repage "$work/inner.png"

magick "$work/outer.png" -filter Lanczos -resize 512x512 -strip "$out/icon.png"
for s in 256 64; do
    magick "$work/outer.png" -filter Lanczos -resize ${s}x$s -strip "$out/icon-$s.png"
done
for s in 48 32 16; do
    magick "$work/inner.png" -filter Lanczos -resize ${s}x$s -strip "$work/icon-$s.png"
done
cp "$work/icon-32.png" "$work/icon-16.png" "$out/"
magick "$out/icon-256.png" "$out/icon-64.png" "$work/icon-48.png" "$out/icon-32.png" "$out/icon-16.png" "$out/icon.ico"

python3 "$src/banner.py" "$out/logo.png" "$root/docs/banner.jpg" "$work" 4608
echo "wrote assets/logo and docs/banner.jpg"
