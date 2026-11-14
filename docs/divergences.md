# Intentional divergences

Where this project deliberately looks or behaves differently from the original game. Anything
not listed here that differs is a bug.

Rules stay the original's: a divergence here may change how the game is shown or
controlled, never what a match plays out to.

When adding one, say what differs and why, and link the code that does it rather than
restating its numbers.

## Interface

The interface is our own: it offers the same screens, information and controls as the
original's, laid out much the same, but drawn in its own style rather than with the
original's bitmaps and window chrome. Differences in look alone aren't listed here;
differences in what you can see or do are.

## View

- **Widescreen.** The original always renders a 4:3 view, stretched to fill wider
  screens. We keep the window's true aspect, with the same vertical view as the
  original, so a wider window sees more of the map to each side rather than stretching
  it. At the same zoom this reads as being zoomed out further. See
  [camera.rs](../crates/render/src/camera.rs).

## New options

Options the original doesn't offer, all in the Video tab's Basic section:

- **Display:** windowed, borderless or fullscreen.
- **Anti-aliasing:** how the map's edges are smoothed.
- **Interface Scale:** draws the interface larger or smaller, for high-resolution
  screens.

## Multiplayer

Matches between machines work as the original's do: every machine plays its own copy
of the match, and a host decides what the match is given and when. How the machines
talk is our own, so this project and the original can't join each other's matches.

- **Protocol.** The messages are ours, not the original's. See
  [protocol.rs](../crates/server/src/protocol.rs).
- **Transport.** Machines connect over QUIC, which is encrypted, rather than the
  original's networking. See [quic.rs](../crates/server/src/quic.rs).
- **A match played alone has a host too.** It's a client of a host in the same
  program, so playing alone goes the way playing with others does. See
  [orders.rs](../crates/game/src/orders.rs).
- **Films.** A recorded match is kept in a format of our own, and the original's
  films aren't read. See [film.rs](../crates/server/src/film.rs).
- **A server with no window.** `aaa-server` hosts a match without playing in it;
  the original's host is always one of the players.

## Tools

- **Console.** A command console, also reachable over a local port, for testing and
  scripting; the original has none. See [its commands](console.md).
- **Fog of war override.** `--fog` sets how much the fog hides, for testing.
