# aaa-data

The game's data as a whole, independent of how it is shown: opening the
installed archives, finding files in them, and building definitions (terrain,
object views, sounds, ...) from the parsed `.tgi` files. The format crates
([`tgck`](../tgck), [`tgi`](../tgi)) read single files; this crate knows how
the game ties them together.

The blocks it reads have typed forms in its `defs` module, which grow as more
of the data is put to use.

To see how much of the data those definitions read, and which keys they skip:

```sh
aaa-data coverage "/path/to/Axis and Allies 2004"          # by top-level block kind
aaa-data coverage "/path/to/Axis and Allies 2004" Thing    # each key of one kind
```

It has no Bevy dependency, so rendering, simulation and anything else built on
the game's data can share it.

Licensed under the [MIT license](../../LICENSE).
