![aaa](docs/banner.svg)

Rust tools for working with the data files of **[Axis & Allies (2004)](https://en.wikipedia.org/wiki/Axis_%26_Allies_(2004_video_game))**, the
real-time strategy game.

You need your own copy of the game. This project contains no game code or data.

## Crates

| Crate | What it does |
|---|---|
| [`tgck`](crates/tgck) | Reads the game's `.rwd` archives and `.rmp` maps, as a library and a command-line tool |
| [`tgi`](crates/tgi) | Parses the game's `.tgi` text data files (units, templates, settings, localisation) |
| [`nif`](crates/nif) | Reads the game's `.nif` models, as a library and a command-line tool |
| [`aaa-viewer`](crates/viewer) | 3D map viewer built with Bevy |

## Building

```sh
cargo build --release
cargo test
```

Requires Rust 1.88 or newer (1.97.1 for the viewer). The `tgck`, `tgi`, `nif` and `aaa-viewer` commands end up
in `target/release/`. The viewer has extra system requirements on Linux; see
[its README](crates/viewer/README.md).

## Legal

Not affiliated with or endorsed by TimeGate Studios, Atari or Hasbro. Axis &
Allies is a trademark of Hasbro, used here only to describe which game's files
these tools read.

Licensed under the [MIT license](LICENSE).
