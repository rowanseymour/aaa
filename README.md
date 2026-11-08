![Matches in aaa-game and a map in aaa-editor](docs/banner.jpg)

Experiments with the data files of [Axis & Allies (2004)](https://en.wikipedia.org/wiki/Axis_%26_Allies_(2004_video_game)), the
real-time strategy game.

> [!IMPORTANT]
> You need your own copy of the game. This project contains no game code or data.

> [!NOTE]
> This is not a decompilation of the game - everything is written from scratch, clean-room style.
> Whoever studies the executable writes up its behavior as specs, and the code here is written
> only from those specs, the data files and what the running game shows.

## Crates

| Crate | What it does | Status |
|---|---|---|
| [`tgck`](crates/tgck) | Reads the game's `.rwd` archives and `.rmp` maps, as a library and a command-line tool | Done; a few map fields still unnamed |
| [`tgi`](crates/tgi) | Parses the game's `.tgi` text data files (units, templates, settings, localisation) | Done |
| [`nif`](crates/nif) | Reads the game's `.nif` models, as a library and a command-line tool | Done; particle systems skipped |
| [`aaa‑data`](crates/data) | The game's data as a whole: archive lookup and definitions built from its files, as a library and a command-line tool | Partial: definitions cover what rendering and the rules use so far |
| [`aaa‑map`](crates/map) | Maps as loaded: the ground, objects and players | Early: ground, placed objects, players and teams, regiments, buildings and cities, and where movers can go |
| [`aaa‑sim`](crates/sim) | The game's rules, played out with no rendering | Early: sides and how they stand toward each other; regiments take their owner's orders, move in formation around what blocks them, their vehicles speeding up, steering and towing trailers, fire on enemies in range until they're destroyed, and stop to fight enemies they meet unless told to run; buildings are captured, destroyed or disabled, and cities go to whoever holds them |
| [`aaa‑render`](crates/render) | Draws maps with Bevy | Ground, water, models in their owner's colour and their animations, units idling, walking, attacking and dying, hover marks, health bars and regiments' banners; no particles |
| [`aaa‑editor`](crates/editor) | Map editor built with Bevy | Views maps and inspects objects' records; no editing yet |
| [`aaa‑game`](crates/game) | Plays matches on a map picked from a menu | Play one side: select your regiments, send them somewhere or run them there and change their formation; they fight enemies in range and take cities by holding them, with their sounds as they answer orders, move, fire and die |

## Building

```sh
cargo build --release
cargo test
```

Requires Rust 1.88 or newer (1.97.1 for the Bevy crates). The `tgck`, `tgi`, `nif`, `aaa-data`, `aaa-editor` and `aaa-game` commands end up
in `target/release/`. The editor has extra system requirements on Linux; see
[its README](crates/editor/README.md).

`./aaa.sh` builds and runs the game or the editor; run it with no arguments for
usage.

## Legal

Not affiliated with or endorsed by TimeGate Studios, Atari or Hasbro. Axis &
Allies is a trademark of Hasbro, used here only to describe which game's files
these tools read.

Copyright © 2026 Rowan Seymour. Licensed under the [GNU GPL v3 or later](LICENSE).
