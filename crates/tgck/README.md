# tgck

A Rust library and command-line tool for reading the data files of
**Axis & Allies (2004)**, the real-time strategy game. The game stores its
files in a container format with the magic bytes `TGCK`:

- **`.rwd` archives**: textures, models, sounds, videos and game data
- **`.rmp` maps**: terrain, heights, water and placed objects

You need your own copy of the game. This project contains no game code or data.

## Command line

```sh
tgck list     Data.rwd [filter]          # list files (filter: case-insensitive substring)
tgck stats    Data.rwd Music.rwd ...     # file counts and sizes by extension
tgck extract  Data.rwd out/ [filter]     # extract files
tgck cat      Data.rwd Units/USA/usa_battleship.tgi
tgck sections Maps/Kursk.RMP             # list the sections of any TGCK file
tgck unpack   Maps/Kursk.RMP out/        # write each section, decompressed, as <name>.bin
tgck map      Maps/*.RMP                 # summarise maps
tgck check    Maps/*.RMP                 # rewrite maps from what's decoded and compare
```

```text
$ tgck map "Maps/(3v3)Saint_lo.RMP"
Maps/(3v3)Saint_lo.RMP: "Normandy Breakout"
  terrain 257x257 vertices (512 world units), heights 0.74..10.00, water 34.8%
  terrain v7, biome temperate_grassland, 43 of 410 types used: temperate_ocean01 11876, ...
  editor 0.3.1.0 by Dev AA Dev, weather none
  7288 objects, 52 templates: temperate_trees 2689, temperate_lake_ripples 1618, ...
  start locations: (299,349) (180,361) (134,239) (124,78) (410,45) (491,321)
```

Maps are inside `Data.rwd`; extract them first with `tgck extract Data.rwd out/ Maps/`.

## Library

```rust
use tgck::{Archive, Map};

let mut archive = Archive::open("Data.rwd")?;
let entry = archive.find("Units/USA/usa_battleship.tgi").unwrap().clone();
let text = archive.read(&entry)?;

let mut map = Map::open("Kursk.RMP")?;
let heights = map.height_map()?; // f32 grid plus a per-vertex water layer
let terrain = map.terrain()?;    // biome, terrain names, per-tile terrain and water body
let things = map.things()?;      // placed objects: template, id, position, facing
let sun = map.sun()?;            // light direction and colours
let kingdoms = map.kingdoms()?;  // players: name, colour, team, nation, commander, resources
let header = map.header()?;      // game options, sides, editor details
let goals = map.objectives()?;   // objectives and whether each is primary
let triggers = map.triggers()?;  // scripting: each trigger's events, conditions and effects
```

Each decoded section can be written back (`to_bytes`, `map::write_things`),
and `container::write` puts a file back together.

## Format support

| Data | Status |
|---|---|
| Container: sections, zlib compression | Done |
| `.rwd` archives | Done; tested against all 11 archives in the game |
| Map `HEIGHT_MAP`, `MOUNTAIN_HEIGHT_MAP` | Done |
| Map `TERRAIN` | Done for versions 6 and 7; the AI analysis at the end is kept as raw bytes |
| Map `DYNAMIC_THINGS` | The fields every object has (template, id, name, position, facing, scale, variation), areas, cameras, and actors: owner, components and the members of herds and regiments, with some components' state typed (construction, body, mover, properties) and the rest read to its exact end but kept as bytes; other classes' records are kept as raw bytes |
| Map `MAP_HEADER` | Game options, sides, size, biome, weather and editor details; a few fields' meanings are unknown |
| Map `OBJECTIVES` | Done |
| Map `TRIGGERS` | Done |
| Map `TEAMS` | Done |
| Map `KINGDOMS` | Name, colour, relations, resources, team, commander, nation, technologies and the strategic AI's state, much of it as unnamed values |
| Map `SUN` | Direction and colours; the rest is kept as raw bytes |
| Map `MAPSTRINGS` | Returned as text |
| Other map sections (`RULES`, ...) | Not decoded; available as raw bytes through `tgck unpack` |

All 80 maps that ship with the game parse fully, and every decoded section
writes back byte for byte. Whole files differ only in their zlib streams, as
the game's zlib and `miniz_oxide` compress differently. The byte layouts are
documented at the top of [`src/container.rs`](src/container.rs),
[`src/archive.rs`](src/archive.rs) and [`src/map.rs`](src/map.rs).

This repository contains only original code.

## Building

```sh
cargo build --release
cargo test
```

Requires Rust 1.88 or newer. The only dependency is
[`miniz_oxide`](https://crates.io/crates/miniz_oxide), a pure-Rust zlib, so no C
toolchain is needed.

## Legal

Not affiliated with or endorsed by TimeGate Studios, Atari or Hasbro. Axis &
Allies is a trademark of Hasbro, used here only to describe which game's files
this tool reads.

Licensed under the [MIT license](../../LICENSE).
