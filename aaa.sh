#!/bin/bash
# Builds and runs aaa-game or aaa-editor from whichever checkout or worktree
# it's run in.
set -e
root=$(git rev-parse --show-toplevel)
common=$(git rev-parse --path-format=absolute --git-common-dir)
# Defaults to the install in an aaa-private checkout beside the main one.
data=$(dirname "$common")/../aaa-private/game

case $1 in
game | editor) app=$1 ;;
*)
    echo "usage: $0 game|editor [map] [--data=dir] [app options...]" >&2
    echo "  --data=dir  path to your legally owned copy of the game" >&2
    exit 2
    ;;
esac
shift

args=()
while (($#)); do
    case $1 in
    --data=*) data=${1#--data=} ;;
    --data) data=$2; shift ;;
    *) args+=("$1") ;;
    esac
    shift
done
data=$(cd "$data" && pwd)

cd "$root"
cargo build --release -p "aaa-$app"
exec "target/release/aaa-$app" "$data" "${args[@]}"
