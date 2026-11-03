#!/bin/sh
# Rebuilds docs/banner.jpg from editor screenshots: one large tile on the left
# and a 2x2 grid of small ones on the right.
#
#   docs/banner.sh "/path/to/Axis and Allies 2004"
#
# Needs ImageMagick 7 (`magick`).

set -eu

if [ $# -ne 1 ]; then
    echo "usage: $0 <game dir>" >&2
    exit 2
fi
game=$1
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cargo build --release -p aaa-editor --manifest-path "$root/Cargo.toml"
editor=$root/target/release/aaa-editor

GAP=8
BIG_W=1000
H=600
SMALL_W=392
SMALL_H=$(((H - GAP) / 2))

# tile <name> <map> <camera> <width> <height>
tile() {
    "$editor" "$game" "$2" --camera "$3" --screenshot "$work/$1-raw.png"
    magick "$work/$1-raw.png" -resize "$4x$5^" -gravity center -extent "$4x$5" "$work/$1.png"
}

tile big '(2v2)Berlin' 0.25,0.25,110,0.785,0.7 $BIG_W $H
tile top-left allies05_monte_cassino 0.25,0.45,110,0.785,0.7 $SMALL_W $SMALL_H
tile top-right allies09_battle_of_the_bulge 0.3,0.7,110,0.785,0.7 $SMALL_W $SMALL_H
tile bottom-left axis03_suez_canal 0.7,0.15,120,0.785,0.7 $SMALL_W $SMALL_H
tile bottom-right axis09_burma 0.3,0.3,110,0.785,0.75 $SMALL_W $SMALL_H

left=$((BIG_W + GAP))
right=$((left + SMALL_W + GAP))
magick -size $((right + SMALL_W))x$H xc:white \
    "$work/big.png" -geometry +0+0 -composite \
    "$work/top-left.png" -geometry +$left+0 -composite \
    "$work/top-right.png" -geometry +$right+0 -composite \
    "$work/bottom-left.png" -geometry +$left+$((SMALL_H + GAP)) -composite \
    "$work/bottom-right.png" -geometry +$right+$((SMALL_H + GAP)) -composite \
    -strip -quality 85 "$root/docs/banner.jpg"
echo "wrote docs/banner.jpg"
