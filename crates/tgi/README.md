# tgi

A Rust parser for `.tgi` files, the text format Axis & Allies (2004) uses for
units, buildings, abilities, templates, map settings, localisation and
configuration. The format is described in the [crate docs](src/lib.rs).

## Usage

```rust
let parsed = tgi::parse_bytes(&std::fs::read("usa_engineer.tgi")?);
for w in &parsed.warnings {
    eprintln!("line {}: {}", w.line, w.message);
}
let thing = parsed.root.blocks_of("Thing").next().unwrap();
let base = thing.attr("template");                  // case-insensitive
let health = thing.blocks_of("BodyComponent").next()
    .and_then(|b| b.value("health"));
```

The tree is left as written; `Templates` resolves `Template=` / `Inherit=`
inheritance on request:

```rust
let templates = tgi::Templates::new(files.iter().map(|p| &p.root));
let resolved = templates.resolve(thing);           // thing with its parents merged in
```

With the `serde` feature, a block reads into any `Deserialize` type (see the
`de` module for how keys match):

```rust
#[derive(serde::Deserialize)]
struct Thing { ids: String, body_component: Body }
#[derive(serde::Deserialize)]
struct Body { health: u32, defense: Option<u32> }

let t: Thing = tgi::from_block(thing)?;             // as written
let t: Thing = templates.deserialize(thing)?;       // with inheritance resolved
```

## Command line

The `tgi` binary reports a file's warnings or prints its parsed tree; run it
without arguments for usage.

Licensed under the [GNU GPL v3 or later](../../LICENSE).
