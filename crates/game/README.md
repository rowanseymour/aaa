# aaa-game

Plays Axis & Allies (2004) matches. It starts one straight away on the map
it's given, drawn by [`aaa-render`](../render), as a way to try the rules in
[`aaa-sim`](../sim) as they arrive.

```sh
cargo run --release -p aaa-game -- "/path/to/Axis and Allies 2004" "(2v2)Berlin"
```

It has the same system requirements on Linux as [`aaa-editor`](../editor).

Licensed under the [MIT license](../../LICENSE).
