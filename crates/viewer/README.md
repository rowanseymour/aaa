# aaa-viewer

A 3D viewer for Axis & Allies (2004) maps, built with [Bevy](https://bevy.org).
It reads maps, terrain definitions and textures straight from the game's
installed `Data.rwd`; nothing needs extracting.

```sh
cargo run --release -p aaa-viewer -- "/path/to/Axis and Allies 2004"                  # list maps
cargo run --release -p aaa-viewer -- "/path/to/Axis and Allies 2004" "(2v2)Berlin"    # view one
```

A map can be named by file name without extension or by its full archive path
(`Maps/Campaign/allies10_berlin.RMP`). `--screenshot out.png` renders a frame to
a file and exits.

## Controls

| Input | Action |
|---|---|
| WASD, arrow keys, middle drag | Pan |
| Q / E, right drag | Rotate |
| R / F | Tilt |
| Mouse wheel | Zoom |

## What it shows

- The height map, with each tile textured by its terrain variation and soft
  edges between terrain types from the game's transition masks
- Water surfaces, with the bed carved out below them
- Placed objects as simple markers: cones for trees, boxes for buildings and
  rocks, coloured boxes for units, red pillars for start locations

Not yet: soft water edges, models, doodads, lighting from the map's sun
settings. Some developer test maps refer to terrain types the game no longer
defines; those tiles show as magenta.

## Building on Linux

Bevy needs a C toolchain plus the ALSA, udev, Vulkan, X11/Wayland and xkbcommon
development packages. On Arch, for example:

```sh
pacman -S base-devel alsa-lib systemd-libs vulkan-icd-loader libxkbcommon wayland libx11 libxcursor libxrandr libxi
```
