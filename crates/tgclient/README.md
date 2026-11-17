# tgclient

Plays the matches of games on TimeGate's engine, such as Axis & Allies (2004)
and Kohan II, drawn by [`tgrender`](../tgrender), as a way to try the rules in
[`tgsim`](../tgsim) as they arrive. It opens on menus leading to a custom
game's lobby, where you pick a map, how its sides team up and the side you
play, the computer taking the rest, or starts straight away on a map it's
given.

It's a library: each game's own program, such as [`aaa-game`](../aaa) or
[`k2-game`](../k2), runs it with what that game's data doesn't say, such as
its name and what its main menu stands over. Everything else comes from the
game's data.

The key left of 1 opens a console of script commands in the game's own style;
`help` lists them. Run a game's program without arguments for the options,
such as running a file of them or taking them from a socket.

It has the same system requirements on Linux as [`tgeditor`](../tgeditor).

Licensed under the [GNU GPL v3 or later](../../LICENSE).
