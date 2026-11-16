#!/bin/bash
# Builds and runs aaa-game or tgeditor from whichever checkout or worktree
# it's run in.
set -e
root=$(git rev-parse --show-toplevel)
data=$AA_INSTALL

usage() {
    echo "usage: $0 [game|editor] [map] [--data=dir] [app options...]" >&2
    echo "  --data=dir  path to your legally owned copy of the game (default: \$AA_INSTALL)" >&2
    exit 2
}

app=aaa-game
case $1 in
game) shift ;;
editor) app=tgeditor; shift ;;
-h | --help) usage ;;
esac

args=()
while (($#)); do
    case $1 in
    --data=*) data=${1#--data=} ;;
    --data) data=$2; shift ;;
    *) args+=("$1") ;;
    esac
    shift
done
[[ -n $data ]] || usage
data=$(cd "$data" && pwd)

cd "$root"
cargo build --release -p "$app"
exec "target/release/$app" "$data" "${args[@]}"
