# aaa-data

The game's data as a whole, independent of how it is shown: opening the
installed `Data.rwd`, finding files in it, and building definitions (terrain,
object views, ...) from the parsed `.tgi` files. The format crates
([`tgck`](../tgck), [`tgi`](../tgi)) read single files; this crate knows how
the game ties them together.

The blocks it reads have typed forms in its `defs` module, which grow as more
of the data is put to use.

It has no Bevy dependency, so the viewer and anything else built on the game's
data can share it.

Licensed under the [MIT license](../../LICENSE).
