# Scenery

How trees, hedges, fences, wire and tall grass react to what happens around
them: knocked over, trampled, regrowing, rustling. What a building does to
the scenery under it is in [objects](objects.md#scenery-under-a-building).

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Reacting scenery

- Scenery that reacts is a `DYNAMIC_DOODAD` template carrying one or both
  of two components: a `BREAKABLE` one (trees, hedges, fences, walls,
  barbed wire: things that can be knocked down) and a `DISTURBABLE` one
  (tall grass, dry grass, bamboo, the ambient audio doodads: things that
  stir when someone passes). Plain doodads don't react at all. [code, data]
- Every shipped tree takes `TreeDynamicDoodadComponent`: `break_impulse`
  100, `toppled = true`, `topple_duration` 1, `bounce_duration` 0.75,
  `bounce_percentage` 5, `regrow = true`, `regrow_dead_duration` 10,
  `regrow_sink_duration` 4, `regrow_grow_duration` 30, and the
  `dynamic_tree_fall` sound. Hedges, fences, walls and wire are
  `toppled = false` and `regrow = false`, with a `trampled_aspect_ids`
  model and a crush effect; `break_impulse` is 100, or 30 for the plain
  `barbed_wire`. [data]

## Knocking scenery down

Breakable scenery reacts to an **impulse**, a strength compared with its
`break_impulse`. Two things give one; nothing else does.

- **A vehicle driving.** Each time a unit's centre crosses into another
  tile, every breakable doodad whose soft shape's bounding box reaches into
  the tile it enters takes the unit's `collision_impulse` (its
  `MoverComponent`), at full strength whatever the distance. For scenery
  the soft shape is the default circle of radius 0.5 about its position,
  so that's the tile it stands in and sometimes a neighbour. [code]
  - Only in play, not in the editor. [code]
  - Stock values: tanks and self-propelled guns 8000; half-tracks, trucks
    and HQ trucks 5000; jeeps 2000; infantry, the Goliath, aircraft and
    ships 0. A unit with 0 knocks nothing down. So every vehicle but the
    Goliath fells the trees and tramples the hedges it drives into, and
    infantry pass through untouched. [data]
- **A blast.** An ability with an `area` above 0 and an `impact_impulse`
  above 0, where it strikes, pushes every breakable doodad whose position
  is within `impulse_area` of the blast's centre (`impulse_area` 0, as in
  all shipped data, means the ability's `area`). A doodad at distance `d`
  from the centre, with radius `r`, takes `impact_impulse × (1 − d² / r²)`.
  [code]
  - In the shipped data that's the special operations (carpet bomb, atomic
    bomb, kamikaze, V-weapons, 10000), the self-propelled gun's shell (area
    2, 16000), battleship fire (area 4, 10000), the Russian rocket truck
    (6000), and the suicide attacks and mines (banzai charges, the Goliath,
    minefields and beach mines, 2000). Weapons with an `impact_impulse` but
    no `area` (tank guns, rockets, the vehicle machine gun) fell nothing.
    [data]
- **Buildings never give an impulse**: they don't drive, and placing one
  removes the scenery under it instead (see
  [objects](objects.md#scenery-under-a-building)). [code]
- A doodad reacts only while standing (or regrowing, below), and only to a
  single impulse at least its `break_impulse`; weaker ones do nothing and
  don't add up. [code]
- On reacting it plays its `Action` effects at once (the tree's fall sound,
  the hedge's crush effect and sound). [code, data]
- `toppled = false`: it turns into its `trampled_aspect_ids` model and stays
  that way for the rest of the game, whatever `regrow` says. [code]
- `toppled = true`: it falls over over `topple_duration` seconds, away
  from where the impulse came from (the driving unit's position or the
  blast's centre, along the ground); with the source exactly on it, it
  falls in one fixed direction for the whole map. It then bounces over
  `bounce_duration` seconds, by `bounce_percentage`, and lies there.
  [code] The `ToppledAction` effects play when it hits the ground. [guess]
  - With `regrow = false` it stays down. [code]
  - With `regrow = true` it lies `regrow_dead_duration` seconds, sinks into
    the ground over `regrow_sink_duration` seconds, then grows back in
    place from nothing to full size over `regrow_grow_duration` seconds,
    and stands as before. So a stock tree is back 45.75 seconds after it's
    hit. [code, data]
  - A regrowing tree can be knocked down again, at whatever size it has
    reached. [code]
- Knocked-down, trampled and regrowing scenery is saved with the game and
  comes back in the same state and timing. A map opened in the editor shows
  all of it standing. [code]
- Scenery is never `blocking`, so knocking it down changes nothing about
  where units can go. [code, data]

## Rustling

- A disturbable doodad checks every `update_time` seconds (1 in all
  shipped data) for a unit that is moving within `affect_radius` metres of
  it. Units standing still and buildings never stir it. [code]
- When it finds one and its quiet time has run out, it plays its `Action`
  effects (tall grass: the `rustle` animation; the ambient audio doodads:
  a `speaker` animation). [code, data]
- Each check that finds a moving unit, whether or not it played, sets a new
  quiet time: a random time between `recharge_time_min` and
  `recharge_time_max` from then. So a unit lingering in it keeps it from
  playing again until the unit has gone (or a check falls after the quiet
  time). Stock grass is 0.5 to 0.5 against a 1-second check, so it rustles
  at every check while a unit moves through. [code, data]

## Swaying

- Trees' models sway in the wind (`wind_sway` in their `[View]` node's
  `[Swivel]`), which is only for show. [data]
- The boreal and arctic trees also set `impact_sway`; what sets that sway
  off is open. [data]

## Open questions

- Is "within `affect_radius`" measured to the moving unit's centre or to
  its shape? (analysis, 2026-11-10)
- What sets off a tree's `impact_sway`: blasts nearby, units driving close,
  or both, and how far does it reach? (analysis, 2026-11-10)
- Do the `ToppledAction` effects play as the felled tree reaches the
  ground? No shipped tree has any. (analysis, 2026-11-10)
