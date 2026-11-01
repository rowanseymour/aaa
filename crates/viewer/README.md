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

The camera starts in the game's own fixed corner-on view. Tab frees it to
rotate and tilt, and switches back.

| Input | Action |
|---|---|
| WASD, arrow keys, middle drag | Pan |
| Mouse wheel | Zoom |
| Tab | Toggle game / free camera |
| Q / E, right drag | Rotate (free camera) |
| R / F | Tilt (free camera) |

## What it shows

- The height map, with each tile textured by its terrain variation and soft
  edges between terrain types from the game's transition masks
- Water surfaces, with the bed carved out below them and coloured by depth
  from the biome's water settings
- Placed objects with the game's models: trees, buildings, roads, fences and
  the like, at each object's own size and variation
- Simple markers where a model can't be read yet: boxes for skinned models
  such as tall grass, coloured boxes for units, red pillars for start
  locations

Not yet: animated water textures, skinned and animated models, particle effects,
lighting from the map's sun settings. Some developer test maps refer to terrain types the game no longer
defines; those tiles show as magenta.

## Building on Linux

Bevy needs a C toolchain plus the ALSA, udev, Vulkan, X11/Wayland and xkbcommon
development packages. On Arch, for example:

```sh
pacman -S base-devel alsa-lib systemd-libs vulkan-icd-loader libxkbcommon wayland libx11 libxcursor libxrandr libxi
```
