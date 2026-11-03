# File formats

The byte layouts of Axis & Allies (2004)'s data files, as
[Kaitai Struct](https://kaitai.io) specs. A spec compiles to a parser, so it
can be checked against real files; these parse every map and archive that
ships with the game, to the last byte. The `doc` of each field gives its
meaning, or says it's unknown.

## The files

The game keeps its files in **TGCK containers**: a header, named section
payloads back to back, and a table at the end that locates each payload and
says whether it's zlib-compressed.

- **`.rwd` archives** hold the game's files: textures, models, sounds,
  videos and data. A `Files` section holds their data and a `Footer`
  section their directory.
- **`.rmp` maps** hold a saved world: terrain, heights, water, placed
  objects, players and scripting, a section each.

## Sections and versions

Section payloads are packed little-endian, with no alignment. Strings are a
`u2` length in UTF-16 code units, then UTF-16LE.

Most sections, and most objects within them, start with their own version,
and fields come and go with it; the specs gate them with `if:`. A map's
GWORLD section starts with the **world version**, the version of the game
state it was saved from, which decides the layout of several other sections
(KINGDOMS, MISC_DATA, UPDATABLES, ...) and of some objects.

The placed objects, *things*, are records whose class decides their layout:
an actor's (a unit, building or herd), an area's or a camera's. The
`_INDEX` section beside a thing list gives each record's size. An actor's
record is a set of components, each with its own layout, chosen by the
bits of the actor's component mask.

## Specs

| Spec | Format | Undecoded |
|---|---|---|
| [`tgck.ksy`](tgck.ksy) | The container, with each payload as bytes | — |
| [`rwd.ksy`](rwd.ksy) | `.rwd` archives | A few `Header` fields, which the game ignores |
| [`rmp.ksy`](rmp.ksy) | `.rmp` maps, a type per section | See below |

What `rmp.ksy` keeps as bytes, or reads without naming:

| Section | Undecoded |
|---|---|
| TERRAIN | The AI's cached analysis of the map, after the tiles |
| DYNAMIC_THINGS | Things other than actors, areas and cameras. Actors with a Projectile, Wall, Settlement, Aircraft, Flare or MetaGame component, or an aircraft's mind. Many component fields are read to their end but unnamed |
| KINGDOMS | Two fields of an ability and one of a saved group |
| MAP_HEADER | One byte's meaning |
| DENIZEN_GROUPS | A group's layout; no shipped map has any |

Every other section is decoded: MAP, GWORLD, RULES, HEIGHT_MAP,
MOUNTAIN_HEIGHT_MAP, IDMGR, UPDATABLES, RESTRICTION_GROUPS, TEAMS,
OBJECTIVES, TRIGGERS, SUN, MISC_DATA, MAPSTRINGS and
USED_REQUIRED_OBJECTS.

## Using the specs

Compile them into a parser for your language, for example Python:

```sh
kaitai-struct-compiler -t python docs/formats/*.ksy
```

or load them with a file into the [Kaitai Web IDE](https://ide.kaitai.io)
to browse it. `rwd.ksy` and `rmp.ksy` import `tgck.ksy`.

The [`tgck`](../../crates/tgck) crate reads and writes the same layouts in
Rust.
