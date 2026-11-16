# tgeditor

A map editor for games on TimeGate's engine, such as Axis & Allies (2004) and
Kohan II, built with [Bevy](https://bevy.org). It views maps, drawn by
[`tgrender`](../tgrender), reading them straight from the game's installed
archives; nothing needs extracting.

```sh
cargo run --release -p tgeditor -- "/path/to/Axis and Allies 2004"                  # list maps
cargo run --release -p tgeditor -- "/path/to/Axis and Allies 2004" "(2v2)Berlin"    # view one
cargo run --release -p tgeditor -- "/path/to/Kohan II" "(1v1)Battleground"
```

A map can be named by file name without extension or by its full archive path
(`Maps/Campaign/allies10_berlin.RMP`). Run it without arguments for the options,
such as rendering a screenshot from a chosen camera position, which is how
[`tools/screenshots.sh`](../../tools/screenshots.sh) makes part of the README screenshots.

The window's help line lists the controls.

## Building on Linux

Bevy needs a C toolchain plus the ALSA, udev, Vulkan, X11/Wayland and xkbcommon
development packages. On Arch, for example:

```sh
pacman -S base-devel alsa-lib systemd-libs vulkan-icd-loader libxkbcommon wayland libx11 libxcursor libxrandr libxi
```
