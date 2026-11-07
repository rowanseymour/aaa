# aaa-editor

A map editor for Axis & Allies (2004), built with [Bevy](https://bevy.org). It
views maps, drawn by [`aaa-render`](../render), reading them straight from the
game's installed `Data.rwd`; nothing needs extracting.

```sh
cargo run --release -p aaa-editor -- "/path/to/Axis and Allies 2004"                  # list maps
cargo run --release -p aaa-editor -- "/path/to/Axis and Allies 2004" "(2v2)Berlin"    # view one
```

A map can be named by file name without extension or by its full archive path
(`Maps/Campaign/allies10_berlin.RMP`). Run it without arguments for the options,
such as rendering a screenshot from a chosen camera position; the README banner
is made that way by [`docs/banner.sh`](../../docs/banner.sh).

The window's help line lists the controls.

## Building on Linux

Bevy needs a C toolchain plus the ALSA, udev, Vulkan, X11/Wayland and xkbcommon
development packages. On Arch, for example:

```sh
pacman -S base-devel alsa-lib systemd-libs vulkan-icd-loader libxkbcommon wayland libx11 libxcursor libxrandr libxi
```
