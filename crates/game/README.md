# aaa-game

Plays Axis & Allies (2004) matches, drawn by [`aaa-render`](../render), as a
way to try the rules in [`aaa-sim`](../sim) as they arrive. It opens on a menu
of the game's maps, or starts straight away on a map it's given:

```sh
cargo run --release -p aaa-game -- "/path/to/Axis and Allies 2004" ["(2v2)Berlin"]
```

The key left of 1 opens a console of script commands in the game's own style,
such as `lookat x y` or `create <regiment template>`; `help` lists them, and
`--exec` runs a file of them once the match starts.

It has the same system requirements on Linux as [`aaa-editor`](../editor).

Licensed under the [MIT license](../../LICENSE).
