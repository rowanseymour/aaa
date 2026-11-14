#!/bin/sh
# Rebuilds the README's screenshots in docs/screenshots/: those from the game
# each set up by a script of console commands, and the editor's with a window
# open on an object. Name some to rebuild only those.
#
#   tools/screenshots.sh "/path/to/Axis and Allies 2004" [name...]
#
# With --compare, shoots the same scenes several times each from this
# checkout and from a git ref, and says which differ by more than shots from
# one build differ among themselves: for checking that a change draws what it
# drew before. The shots, and for each scene that differs an image of where,
# go in target/screenshots-compare/. TAKES sets how many shots each build
# takes of a scene (default 2); more catch rarer flickers.
#
#   tools/screenshots.sh --compare main "/path/to/Axis and Allies 2004" [name...]
#
# Needs ImageMagick 7 (`magick`).

set -eu

usage() {
    echo "usage: $0 [--compare <ref>] <game dir> [name...]" >&2
    exit 2
}

ref=
if [ "${1:-}" = --compare ]; then
    [ $# -ge 2 ] || usage
    ref=$2
    shift 2
fi
[ $# -ge 1 ] || usage
game=$1
shift
only=" $* "
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
cleanup() {
    if [ -n "$ref" ] && [ -d "$work/base" ]; then
        git -C "$root" worktree remove --force "$work/base"
    fi
    rm -rf "$work"
}
trap cleanup EXIT

W=1000
H=562

wanted() {
    [ "$only" = "  " ] || case $only in *" $1 "*) true ;; *) false ;; esac
}

# taken <name>: the scene's frame is in $work/<name>.png. For the README it's
# cropped into docs/screenshots/; when comparing it's kept as this take.
taken() {
    if [ -z "$ref" ]; then
        magick "$work/$1.png" -resize "${W}x$H^" -gravity center -extent "${W}x$H" \
            -strip -quality 85 "$root/docs/screenshots/$1.jpg"
        echo "wrote docs/screenshots/$1.jpg"
    else
        mkdir -p "$out/$1"
        mv "$work/$1.png" "$out/$1/$side-$take.png"
    fi
}

# game <name> <map> [seconds], with the script on stdin. Regiment ids are the
# ones `create` gives on that map, which are the same every run. Effects only
# play as frames are drawn, not through `advance`, so seconds plays on that
# long before the shot.
game() {
    cat >"$work/$1.txt"
    wanted "$1" || return 0
    "$bin/aaa-game" "$game" "$2" --exec "$work/$1.txt" --shot-after "${3:-0}" --screenshot "$work/$1.png"
    taken "$1"
}

# menus <name>, with the script on stdin, which starts from the first menu.
menus() {
    cat >"$work/$1.txt"
    wanted "$1" || return 0
    "$bin/aaa-game" "$game" --exec "$work/$1.txt" --screenshot "$work/$1.png"
    taken "$1"
}

# editor <name> <map> <camera> <ids to inspect>
editor() {
    wanted "$1" || return 0
    "$bin/aaa-editor" "$game" "$2" --camera "$3" --inspect "$4" --screenshot "$work/$1.png"
    taken "$1"
}

scenes() {
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
}

build() {
    cargo build --release -p aaa-game -p aaa-editor --manifest-path "$1/Cargo.toml"
}

if [ -z "$ref" ]; then
    build "$root"
    bin=$root/target/release
    mkdir -p "$root/docs/screenshots"
    scenes
    exit 0
fi

# The pixels two frames differ in, ignoring small changes of colour.
differ() {
    magick compare -metric AE -fuzz 2% "$1" "$2" null: 2>&1 | cut -d' ' -f1
}

takes=${TAKES:-2}
out=$root/target/screenshots-compare
rm -rf "$out"
mkdir -p "$out"
build "$root"
git -C "$root" worktree add --quiet --detach "$work/base" "$ref"
# The ref builds into a target of its own, kept so later comparisons only
# rebuild what changed.
CARGO_TARGET_DIR=$root/target/compare-base build "$work/base"
for side in new base; do
    case $side in
    new) bin=$root/target/release ;;
    base) bin=$root/target/compare-base/release ;;
    esac
    take=1
    while [ "$take" -le "$takes" ]; do
        log=$out/$side-$take.log
        echo "shooting $side, take $take of $takes"
        scenes >"$log" 2>&1 || {
            echo "a scene failed from $side; see $log" >&2
            exit 1
        }
        take=$((take + 1))
    done
done

# A scene matches if some shot from each build is as close as two shots from
# the same build can be.
printf '%-16s %10s %10s\n' scene noise differs
status=0
for dir in "$out"/*/; do
    name=$(basename "$dir")
    noise=0
    closest=
    for a in "$dir"*.png; do
        after=false
        for b in "$dir"*.png; do
            if $after; then
                n=$(differ "$a" "$b")
                case $(basename "$a")/$(basename "$b") in
                base-*/new-*)
                    if [ -z "$closest" ] || awk "BEGIN { exit !($n < $closest) }"; then
                        closest=$n
                        pair_a=$a
                        pair_b=$b
                    fi
                    ;;
                *) noise=$(awk "BEGIN { print ($n > $noise) ? $n : $noise }") ;;
                esac
            fi
            [ "$b" != "$a" ] || after=true
        done
    done
    if awk "BEGIN { exit !($closest <= $noise) }"; then
        verdict=same
    else
        verdict="DIFFERENT: see target/screenshots-compare/$name/diff.png"
        magick compare -fuzz 2% "$pair_a" "$pair_b" -compose src "$dir/diff.png" 2>/dev/null || true
        status=1
    fi
    printf '%-16s %10s %10s  %s\n' "$name" "$noise" "$closest" "$verdict"
done
exit $status
