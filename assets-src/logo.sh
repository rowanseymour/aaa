# Builds a game's logo and app icons from its emblem artwork. Sourced by each game's logo/build.sh, which sets:
#
#   root     the repository
#   emblem  the 1024x1024 artwork, its centre disc left empty for the lettering
#   text     the lettering, font its face and pointsize its size
#   logo     where the logo goes
#   icons    the folder the icons go in
#   outer    radius of the emblem's outer ring, and inner of its inner disc
#            with its rim, both about the centre
#
# Needs ImageMagick 7 (`magick`).

set -eu

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Lettering, with a soft drop shadow, centred on the emblem.
magick -background none -font "$font" -pointsize "$pointsize" -kerning 10 -fill '#e6e0c8' label:"$text" -trim +repage "$work/text.png"
magick "$work/text.png" -alpha extract -background black -alpha shape -channel A -evaluate multiply 0.35 +channel "$work/shadow.png"
magick "$emblem" \( "$work/shadow.png" \) -gravity center -geometry +0+5 -composite \
    \( "$work/text.png" \) -gravity center -composite -strip "$logo"

# Large icons are the disc inside the outer ring; small ones only the inner
# disc, so the lettering stays readable.
disc() {
    magick -size 1024x1024 xc:black -fill white -draw "circle 512,512 512,$((512 - $1))" -blur 0x1 "$work/mask.png"
    magick "$logo" "$work/mask.png" -alpha off -compose CopyOpacity -composite \
        -crop $(($1 * 2))x$(($1 * 2))+$((512 - $1))+$((512 - $1)) +repage "$2"
}
disc "$outer" "$work/outer.png"
disc "$inner" "$work/inner.png"

mkdir -p "$icons"
magick "$work/outer.png" -filter Lanczos -resize 512x512 -strip "$icons/icon.png"
for s in 256 64; do
    magick "$work/outer.png" -filter Lanczos -resize ${s}x$s -strip "$icons/icon-$s.png"
done
for s in 48 32 16; do
    magick "$work/inner.png" -filter Lanczos -resize ${s}x$s -strip "$work/icon-$s.png"
done
cp "$work/icon-32.png" "$work/icon-16.png" "$icons/"
magick "$icons/icon-256.png" "$icons/icon-64.png" "$work/icon-48.png" "$icons/icon-32.png" "$icons/icon-16.png" "$icons/icon.ico"
