# aaa-editor

A map editor for Axis & Allies (2004), built with [Bevy](https://bevy.org). For
now it views maps, drawn by [`aaa-render`](../render). It reads them straight
from the game's installed `Data.rwd`; nothing needs extracting.

```sh
cargo run --release -p aaa-editor -- "/path/to/Axis and Allies 2004"                  # list maps
cargo run --release -p aaa-editor -- "/path/to/Axis and Allies 2004" "(2v2)Berlin"    # view one
```

A map can be named by file name without extension or by its full archive path
(`Maps/Campaign/allies10_berlin.RMP`). Run it without arguments for the options,
such as rendering a screenshot from a chosen camera position; the README banner
is made that way by [`docs/banner.sh`](../../docs/banner.sh).

## Controls

The camera starts in the game's own fixed corner-on view, but zooms out far
enough to see the whole map. Tab frees it to rotate and tilt, and switches
back.

| Input | Action |
|---|---|
| WASD, arrow keys, middle drag | Pan |
| Mouse wheel | Zoom |
| Tab | Toggle game / free camera |
| Q / E, right drag | Rotate (free camera) |
| R / F | Tilt (free camera) |
| M | Show / hide placeholder markers, as screenshots leave them out |

## Building on Linux

Bevy needs a C toolchain plus the ALSA, udev, Vulkan, X11/Wayland and xkbcommon
development packages. On Arch, for example:

```sh
pacman -S base-devel alsa-lib systemd-libs vulkan-icd-loader libxkbcommon wayland libx11 libxcursor libxrandr libxi
```
