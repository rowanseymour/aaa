#!/bin/sh
# Rebuilds the README's screenshots in docs/screenshots/: those from the game
# each set up by a script of console commands, and the editor's with a window
# open on an object. Name some to rebuild only those.
#
#   docs/screenshots.sh "/path/to/Axis and Allies 2004" [name...]
#
# Needs ImageMagick 7 (`magick`).

set -eu

if [ $# -lt 1 ]; then
    echo "usage: $0 <game dir> [name...]" >&2
    exit 2
fi
game=$1
shift
only=" $* "
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cargo build --release -p aaa-game -p aaa-editor --manifest-path "$root/Cargo.toml"
bin=$root/target/release

out=$root/docs/screenshots
mkdir -p "$out"

W=1000
H=562

wanted() {
    [ "$only" = "  " ] || case $only in *" $1 "*) true ;; *) false ;; esac
}

fit() {
    magick "$work/$1-raw.png" -resize "${W}x$H^" -gravity center -extent "${W}x$H" \
        -strip -quality 85 "$out/$1.jpg"
    echo "wrote docs/screenshots/$1.jpg"
}

# game <name> <map> [seconds], with the script on stdin. Regiment ids are the
# ones `create` gives on that map, which are the same every run. Effects only
# play as frames are drawn, not through `advance`, so seconds plays on that
# long before the shot.
game() {
    cat >"$work/$1.txt"
    wanted "$1" || return 0
    "$bin/aaa-game" "$game" "$2" --exec "$work/$1.txt" --shot-after "${3:-0}" --screenshot "$work/$1-raw.png"
    fit "$1"
}

# menus <name>, with the script on stdin, which starts from the first menu.
menus() {
    cat >"$work/$1.txt"
    wanted "$1" || return 0
    "$bin/aaa-game" "$game" --exec "$work/$1.txt" --screenshot "$work/$1-raw.png"
    fit "$1"
}

# editor <name> <map> <camera> <ids to inspect>
editor() {
    wanted "$1" || return 0
    "$bin/aaa-editor" "$game" "$2" --camera "$3" --inspect "$4" --screenshot "$work/$1-raw.png"
    fit "$1"
}

game stalingrad allies03_stalingrad <<'EOF'
select 2551
destroyselected
advance 1
select 2762
commission regiment_rus_tank_light
advance 4
lookat 30 423
zoom 0.55
EOF

game normandy allies06_normandy <<'EOF'
select 68870 68880
lookat 336 230
zoom 0.6
advance 3
EOF

game el-alamein allies01_el_alamein 2.65 <<'EOF'
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

game guadalcanal allies02_guadalcanal <<'EOF'
lookat 46 276
zoom 0.5
advance 3
EOF

# Each side's nation, then the list of the first side's generals.
menus lobby <<'EOF'
click menu.new-game
click menu.custom
click lobby.side3.nation; click lobby.side3.nation.nation-ger
click lobby.side4.nation; click lobby.side4.nation.nation-jpn
click lobby.side1.nation; click lobby.side1.nation.nation-usa
click lobby.side2.nation; click lobby.side2.nation.nation-gbr
click lobby.side3.general
EOF

editor berlin-editor '(2v2)Berlin' 0.235,0.26,70,0.785,0.75 12193
