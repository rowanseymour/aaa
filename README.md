![Matches in aaa-game and a map in aaa-editor](docs/banner.jpg)

A work-in-progress [Rust](https://rust-lang.org)+[Bevy](https://bevy.org) re-implementation of [Axis & Allies (2004)](https://en.wikipedia.org/wiki/Axis_%26_Allies_(2004_video_game)), the
real-time strategy game.

> [!IMPORTANT]
> You need your own copy of the game. This project contains no game code or data.

> [!NOTE]
> This is not a decompilation of the game: the code is written from scratch, clean-room style.
> The original game's behavior is written up as specs, and the code here is written only from
> those specs, the data files and the game itself, played.

## Running

You need [Rust](https://rustup.rs), and on Linux the system packages listed in the
[editor's README](crates/editor/README.md). `aaa.sh` builds and runs the game or the
map editor, given the folder your copy of the game is installed in:

```sh
./aaa.sh game --data="/path/to/Axis and Allies"
./aaa.sh editor --data="/path/to/Axis and Allies"
```

Set `AAA_INSTALL` to that folder to leave `--data` off. Run `./aaa.sh` with no
arguments for usage.

## Development

The code is a workspace of crates in [`crates/`](crates), from readers for the game's
file formats up to the game itself.

```sh
cargo build --release
cargo test
```

## Legal

Not affiliated with or endorsed by TimeGate Studios, Atari or Hasbro. Axis &
Allies is a trademark of Hasbro, used here only to describe which game's files
these tools read.

Copyright © 2026 Rowan Seymour. Licensed under the [GNU GPL v3 or later](LICENSE).
