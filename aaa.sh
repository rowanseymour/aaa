#!/bin/bash
# Builds and runs aaa-game or aaa-editor from whichever checkout or worktree
# it's run in.
set -e
root=$(git rev-parse --show-toplevel)
data=$AAA_INSTALL

usage() {
    echo "usage: $0 game|editor [map] [--data=dir] [app options...]" >&2
    echo "  --data=dir  path to your legally owned copy of the game (default: \$AAA_INSTALL)" >&2
    exit 2
}

case $1 in
game | editor) app=$1 ;;
*) usage ;;
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
[[ -n $data ]] || usage
data=$(cd "$data" && pwd)

cd "$root"
cargo build --release -p "aaa-$app"
exec "target/release/aaa-$app" "$data" "${args[@]}"
