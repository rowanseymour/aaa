# nif

A Rust library and command-line tool for reading the `.nif` model and `.kf`
animation files of **Axis & Allies (2004)**.

You need your own copy of the game. This project contains no game code or data.

## Command line

```sh
nif dump TemperateTree01.nif
```

Run `nif` with no arguments for usage.

Extract models from the game's archive first with `tgck extract Data.rwd out/ .nif`
(or `.kf` for animations).

## Library

```rust
use nif::{Nif, Object};

let model = Nif::parse(&std::fs::read("TemperateTree01.nif")?)?;
for placed in model.shapes() {
    if let Some(Object::Geometry(g)) = model.get(placed.shape.data) {
        let p = placed.transform.apply(g.positions[0]);
        // ...
    }
}
```

The format is documented in the crate docs.

## Legal

Not affiliated with or endorsed by TimeGate Studios, Atari or Hasbro. Axis &
Allies is a trademark of Hasbro, used here only to describe which game's files
this tool reads.

Licensed under the [GNU GPL v3 or later](../../LICENSE).
