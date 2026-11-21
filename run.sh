#!/bin/bash
# Builds and runs one of the programs from whichever checkout or worktree it's run in.
set -e
root=$(git rev-parse --show-toplevel)

usage() {
    echo "usage: $0 aa|k2|server|editor [map] [--data=dir] [app options...]" >&2
    echo "  aa          play AA" >&2
    echo "  k2          play K2" >&2
    echo "  server      host or play a match with no window (tgserver)" >&2
    echo "  editor      view maps (tgeditor)" >&2
    echo "  --data=dir  path to your legally owned copy of the game" >&2
    echo "              (default: \$K2_INSTALL for k2, else \$AA_INSTALL)" >&2
    exit 2
}

install=AA_INSTALL
case $1 in
aa) app=aa ;;
k2) app=k2; install=K2_INSTALL ;;
server) app=tgserver ;;
editor) app=tgeditor ;;
*) usage ;;
esac
data=${!install}
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
if [[ -z $data ]]; then
    echo "$0: no game folder: set $install or give --data" >&2
    exit 2
fi
# A ~ left unexpanded, as an environment variable can hold it.
data=${data/#\~/$HOME}
data=$(cd "$data" && pwd)

cd "$root"
cargo build --release -p "$app"
exec "target/release/$app" "$data" "${args[@]}"
