# aaa

Rust tools for working with the data files of **Axis & Allies (2004)**, the
real-time strategy game.

You need your own copy of the game. This project contains no game code or data.

## Crates

| Crate | What it does |
|---|---|
| [`tgck`](crates/tgck) | Reads the game's `.rwd` archives and `.rmp` maps, as a library and a command-line tool |
| [`tgi`](crates/tgi) | Parses the game's `.tgi` text data files (units, templates, settings, localisation) |

## Building

```sh
cargo build --release
cargo test
```

Requires Rust 1.88 or newer. The `tgck` and `tgi` commands end up in `target/release/`.

## Legal

Not affiliated with or endorsed by TimeGate Studios, Atari or Hasbro. Axis &
Allies is a trademark of Hasbro, used here only to describe which game's files
these tools read.

Licensed under the [MIT license](LICENSE).
