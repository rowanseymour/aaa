#!/bin/sh
# Rebuilds docs/banner.jpg as a 2x2 grid of screenshots: three from the game,
# each set up by a script of console commands, and one from the editor with a
# window open on an object.
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

cargo build --release -p aaa-game -p aaa-editor --manifest-path "$root/Cargo.toml"
bin=$root/target/release

GAP=8
W=1000
H=562

fit() {
    magick "$work/$1-raw.png" -resize "${W}x$H^" -gravity center -extent "${W}x$H" "$work/$1.png"
}

# game <name> <map> [seconds], with the script on stdin. Regiment ids are the
# ones `create` gives on that map, which are the same every run. Effects only
# play as frames are drawn, not through `advance`, so seconds plays on that
# long before the shot.
game() {
    cat >"$work/$1.txt"
    "$bin/aaa-game" "$game" "$2" --exec "$work/$1.txt" --shot-after "${3:-0}" --screenshot "$work/$1-raw.png"
    fit "$1"
}

# editor <name> <map> <camera> <ids to inspect>
editor() {
    "$bin/aaa-editor" "$game" "$2" --camera "$3" --inspect "$4" --screenshot "$work/$1-raw.png"
    fit "$1"
}

game top-right allies03_stalingrad <<'EOF'
select 2551
destroyselected
advance 1
select 2762
commission regiment_rus_tank_light
advance 4
lookat 30 423
zoom 0.55
EOF

game top-left allies06_normandy <<'EOF'
select 68870 68880
lookat 336 230
zoom 0.6
advance 3
EOF

game bottom-left allies01_el_alamein 2.65 <<'EOF'
create regiment_gbr_tank_heavy 340 300
create regiment_gbr_tank_medium 348 294
placementkingdom kingdom_1
create regiment_ger_tank_heavy 330 280
create regiment_ger_halftrack_at 342 278
advance 4
select 10744
lookat 342 289
zoom 0.6
EOF

editor bottom-right '(2v2)Berlin' 0.235,0.26,70,0.785,0.75 12193

magick -size $((2 * W + GAP))x$((2 * H + GAP)) xc:white \
    "$work/top-left.png" -geometry +0+0 -composite \
    "$work/top-right.png" -geometry +$((W + GAP))+0 -composite \
    "$work/bottom-left.png" -geometry +0+$((H + GAP)) -composite \
    "$work/bottom-right.png" -geometry +$((W + GAP))+$((H + GAP)) -composite \
    -strip -quality 85 "$root/docs/banner.jpg"
echo "wrote docs/banner.jpg"
