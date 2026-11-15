![aaa](docs/banner.jpg)

[![CI](https://github.com/rowanseymour/aaa/actions/workflows/ci.yml/badge.svg)](https://github.com/rowanseymour/aaa/actions/workflows/ci.yml)
[![License: GPL v3+](https://img.shields.io/badge/license-GPL%20v3%2B-blue.svg)](LICENSE)
[![Bevy 0.20](https://img.shields.io/badge/bevy-0.20-232326.svg?logo=bevy&logoColor=white)](https://bevy.org)

An open source re-implementation of [Axis & Allies (2004)](https://en.wikipedia.org/wiki/Axis_%26_Allies_(2004_video_game)),
the real-time strategy game, written in [Rust](https://rust-lang.org) with [Bevy](https://bevy.org).
It's a work in progress, playable in parts. Name is pronounced _aaaaahh_.

> [!IMPORTANT]
> You need your own copy of the original game. This project contains none of its code or data.

> [!NOTE]
> This is a clean-room reimplementation rather than a recompilation: it reproduces the
> original game's behavior, not its code. That behavior is written up as specs, and the code
> here is written from those specs, the data files and the game itself, played.

<p>
<a href="docs/screenshots/main-menu.jpg"><img src="docs/screenshots/main-menu.jpg" width="32%" alt="The main menu"></a>
<a href="docs/screenshots/stalingrad.jpg"><img src="docs/screenshots/stalingrad.jpg" width="32%" alt="A match on the Stalingrad map"></a>
<a href="docs/screenshots/el-alamein.jpg"><img src="docs/screenshots/el-alamein.jpg" width="32%" alt="Tanks fighting on the El Alamein map"></a>
<a href="docs/screenshots/guadalcanal.jpg"><img src="docs/screenshots/guadalcanal.jpg" width="32%" alt="A match on the Guadalcanal map"></a>
<a href="docs/screenshots/normandy.jpg"><img src="docs/screenshots/normandy.jpg" width="32%" alt="A match on the Normandy map"></a>
<a href="docs/screenshots/pearl-harbor.jpg"><img src="docs/screenshots/pearl-harbor.jpg" width="32%" alt="Battleships and a carrier in Pearl Harbor"></a>
<a href="docs/screenshots/okinawa.jpg"><img src="docs/screenshots/okinawa.jpg" width="32%" alt="A town on the Okinawa map"></a>
<a href="docs/screenshots/lobby.jpg"><img src="docs/screenshots/lobby.jpg" width="32%" alt="A custom game's lobby, with each side's nation chosen"></a>
<a href="docs/screenshots/berlin-editor.jpg"><img src="docs/screenshots/berlin-editor.jpg" width="32%" alt="The Berlin map in aaa-editor, with a window open on an object"></a>
</p>

## Goals

- Play like the original: the same rules giving the same results. Any where it looks or
  controls differently on purpose, that's listed in [divergences](docs/divergences.md).
- Playable on modern systems, Linux and macOS included.
- Stay driven by the data files, so the original's maps and mods keep working and new ones
  are easier to make. The engine stays generic enough that, with
  a little work, it could also run [Kohan II](https://en.wikipedia.org/wiki/Kohan_II:_Kings_of_War), which shares its file formats.
- Document every [file format](docs/formats) the game uses.

## Status

| Feature | Status |
|---------------------|---------|
| Data files          | ✅ Maps and models read in full |
| Rendering           | ✅ Ground, units, animations, effects |
| UI                  | 🟡 Menus, lobby and in-match interface for custom games, and basic options |
| Movement and combat | 🟡 Regiments move, fight and take buildings and cities |
| Economy and supply  | ✅ Income, upkeep, supply, building, recruiting and research |
| Campaign mode       | ⬜ Not started |
| Multiplayer         | 🟡 Basic architecture and beginnings of a standalone server |
| Map editor          | 🟡 Basic viewing of maps and placed objects |
| Loading and saving  | ⬜ Not started |
| AI                  | ⬜ Not started |

## Running

You need Rust, and on Linux the system packages listed in the
[editor's README](crates/editor/README.md). `aaa.sh` builds and runs the game or the
map editor, given the folder your copy of the game is installed in:

```sh
./aaa.sh --data="/path/to/Axis and Allies"
./aaa.sh editor --data="/path/to/Axis and Allies"
```

Set `AAA_INSTALL` to that folder to leave `--data` off. Run `./aaa.sh --help` for
usage.

## Development

The code is a workspace of crates in [`crates/`](crates), from readers for the game's
file formats up to the game itself.

```sh
cargo build --release
cargo test
```

## Contributing

Bug reports, ideas and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md),
in particular for what the clean-room approach asks of contributions.

## Legal

Not affiliated with or endorsed by TimeGate Studios, Atari or Hasbro. Axis &
Allies is a trademark of Hasbro, used here only to describe which game's files
these tools read.

Copyright © 2026 Rowan Seymour and contributors. Licensed under the [GNU GPL v3 or later](LICENSE).
