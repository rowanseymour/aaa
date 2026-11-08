# nif

A Rust library and command-line tool for reading the `.nif` model files of
**Axis & Allies (2004)**: scene nodes, triangle meshes, textures (external
files or stored in the model) and render properties such as alpha and
double-sidedness, skins binding meshes to bones, keyframe and alpha
animations, and particle systems with their emitters, modifiers and
colliders. It also reads the `.kf`
animation sequences, which share the format and animate a model's nodes by
name.

You need your own copy of the game. This project contains no game code or data.

## Command line

```sh
nif check Doodads/Temperate/*/*.nif    # report files that fail or stop early
nif dump  TemperateTree01.nif          # print the objects in one model
```

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

Types the reader does not know end a read early; what came before them is
kept. Particle systems in the older NetImmerse format are read only to step
over them. The layouts are
documented in [`src/lib.rs`](src/lib.rs) and [`src/objects.rs`](src/objects.rs).

## Legal

Not affiliated with or endorsed by TimeGate Studios, Atari or Hasbro. Axis &
Allies is a trademark of Hasbro, used here only to describe which game's files
this tool reads.

Licensed under the [GNU GPL v3 or later](../../LICENSE).
