# tgi

A Rust parser for `.tgi` files, the text format Axis & Allies (2004) uses for
units, buildings, abilities, templates, map settings, localisation and
configuration.

```text
/* block comment */
[Thing Template=BaseTank]          ; header: words plus key=value attributes
{
    IDS = test_tank                ; key = value, to end of line
    name = "#test_tank_name"       ; surrounding quotes are stripped
    [Body]                         ; a header without braces owns the fields
    health = 200                   ; after it, up to the next header or '}'
    defense = 1
}
```

Schema files declare typed fields (`fixed health`, `flag destructible = true`)
and nested block slots (`[optional OnDeath template=Ability]`); these parse into
the same tree.

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

Files are decoded as UTF-8 (with or without BOM), falling back to Windows-1252.
Malformed input (missing `]`, unbalanced braces, stray text) produces warnings
instead of errors, so every file the game ships loads.

This crate builds the syntax tree only; resolving `Template=` / `Inherit=`
inheritance is left to the caller.

## Command line

```sh
tgi check *.tgi     # parse and report warnings
tgi dump file.tgi   # print the tree in normalised form
```

Tested against all 1,842 `.tgi` files in the game: every file parses, the 14
warnings all point at genuine mistakes in the data, and every file round-trips
through `dump` unchanged.

Licensed under the [MIT license](../../LICENSE).
