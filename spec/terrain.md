# Terrain

How the ground's height at any point comes from a map's HEIGHT_MAP and
MOUNTAIN_HEIGHT_MAP and its tiles' terrains, on land, under water and beside
both, and which things sit on that height.

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Grids

- Both height maps are grids of vertices `2^HeightmapResolutionBits` world
  units apart (stock 1, so 2 units). Vertex `(i, j)` is at world
  `(2i, 2j)`; a map `N` tiles across has `N + 1` vertices a side. [code]
- Tiles are `2^MapGTileResolutionBits` units square (stock 1, so 2 units,
  the same as a height cell). The tile under a point is
  `(floor(x / 2), floor(y / 2))`, clamped to the map. [code]
- A map records both resolutions it was made with, and the game refuses a
  map whose resolutions differ from its settings. [code]

## Sampling a height map

The same sampling is used for both maps.

- Inside a cell the height is bilinear between the cell's four vertices.
  [code]
- On a grid line (a coordinate within 0.0001 of a multiple of 2) the height
  is the plain mean of the two vertices on that line either side of the
  point, wherever along the edge the point lies, rather than a linear blend.
  On a vertex it's that vertex. [code] In shipped maps, 3 of the 4 stored
  objects where this differs from a linear blend sit at the mean. [data]
- Vertex indices are clamped to the grid, so beyond the last row or column
  the height is held at the edge vertex. [code]

## Which terrain the mountain map applies to

- Every terrain has a `type`: `LAND` (template `Terrain`), `MOUNTAIN`
  (`MountainTerrain`) or `WATER` (`WaterTerrain`). [code]
- The mountain map counts at a point when the tile under it, or any of its
  eight neighbouring tiles, has type `MOUNTAIN` or `WATER`. Neighbours off
  the map are ignored. So a strip of `LAND` one tile wide around every
  mountain and every body of water also gets it. [code]
- Elsewhere the mountain map is ignored, whatever it holds there. [code]

## Ground height

- Ground = `height + mountain + noise` where the mountain map counts, else
  just `height`. Both maps are sampled at the point as above. [code]
- `noise` is added only when the point's own tile is `MOUNTAIN` (not for a
  neighbour, not for `WATER`). It's one value per tile, the same across the
  whole tile, so the ground steps at the edges of mountain tiles:
  - `k = row * tiles_per_side + column` of the tile;
    `r = ((k * 4723 + 6323) mod 6997) mod 1000`;
    `noise = noise_min + (noise_max - noise_min) * r / 1000`. [code]
  - `noise_min` and `noise_max` are the mountain terrain's fields
    (`MountainTerrain` defaults 1 and 2; shipped mountains set their own,
    e.g. `chaparral_mountain` 0.25 and 0.65). [code]
  - It's fixed by tile position, not random per game or per load. [code]
- Shipped maps bear this out: of objects stored on dry land beside mountain
  tiles, 97% sit at this ground against 85% for the height map alone; on
  mountain tiles 84% match with the noise and under 3% without it. [data]

## Water

- The water surface is drawn at `height - 0.5`, from the height map alone.
  [code]
- Under water the ground is the bed: `height + mountain` as above, since
  water tiles always get the mountain map (and no noise). The mountain map
  is normally negative there. [code]
- Things that float or keep above water use `max(ground, height - 0.5)`:
  the bed where it's above the surface, else the surface. This applies
  wherever they are, not only on water tiles. [code]

## What sits on it

- Things are snapped to the ground when placed: templates flagged
  `water_based` use the float-or-ground height above, `flying` things keep
  their own height, everything else uses the ground. Units and static
  objects (doodads, buildings) use the same rule. [code]
- This includes every thing a map holds as it loads (see
  [objects](objects.md)), so a map edited outside the game gets the
  ground's heights back. [code] The water effects (`waves`,
  `temperate_lake_ripples`, `tropical_lilypads`) are `water_based`, so they
  sit at float-or-ground even on land. [code] Stored heights that match
  neither rule are stale and get overwritten. [guess]
- Units falling or dying land on the ground. [code] Aircraft measure their
  height against the float-or-ground height, so they treat the water surface
  as the floor. [guess]
- Things flagged `conform_to_terrain` tilt to the ground's shape, sampling
  this same ground height. [guess]
- The heights stored in shipped maps were produced by this rule: across the
  80 shipped maps 95% of stored objects match ground or float-or-ground
  within 0.05. [data]

## Terrain properties

A terrain can list properties (`[Property] property_IDS`), defined in
`Properties/properties_terrains.tgi`. Units standing on it gain them.

- Each tile has one terrain, and that alone decides: there's no blending
  with neighbours and no wider area. A transition terrain (the `*trans`
  files) is a terrain of its own, with whatever properties its own file
  lists; stock city, forest, sand and snow transitions list the same pair
  as the terrain they lead into. [code, data]
- The terrain is the one the map stores for the tile, exactly. Nothing
  is worked out again when a map loads: not from neighbours, not from
  `base_priority` (which only orders how terrain textures overlap when
  drawn), not from what stands on the tile. [code]
- Trees have no part in it. On a stored map every tree is a stored
  scenery object, put there by the map's maker, and the game adds none as
  it loads. A terrain's `doodad_frequency` only scatters scenery when
  terrain is painted in the editor or a random map is generated. So
  forest is wherever tiles hold a forest terrain (`*forestdeciduous*`,
  `*forestconiferous*` and the transitions into them, which list the
  forest properties too), trees or none, and trees standing on grass
  tiles give nothing. Forest ground shows as its own darker texture, not
  by its trees. [code, data]
  - `(2v2)Berlin` has such ground beside the start at (392, 264): the
    Corps HQ's tile (196, 132) is `temperate_grassdry03`, and tiles
    199 to 206 of the same row are `temperate_forestdeciduous01` with no
    tree on them, part of a bare forest block some 15 tiles wide running
    north from there. [data]
- A regiment has a terrain of its own: that of the tile under the
  regiment's position, which follows its leader
  ([movement](movement.md)). [code]
- A member's terrain is its regiment's while it is with the regiment (it
  is the leader, or stands inside the regiment's control zone), and that
  of the tile under its own centre while it straggles outside. A thing
  in no regiment uses the tile under its centre. [code]
- A thing's terrain is looked at again the moment its centre crosses into
  another tile, when a member joins or leaves the body of its regiment,
  and when an HQ finishes packing. When a regiment's terrain changes,
  every member's is looked at again too. There's no delay either way: the
  old terrain's properties go and the new one's arrive in the same step.
  Nothing happens while the terrain stays the same, even if the tile
  changes. [code]
- These never have their terrain looked at, and keep whatever terrain
  properties they held: [code]
  - buildings and anything else that can't move (no regiment of its own,
    no `speed_max` above 0);
  - aircraft and anything else flying, and units falling from a drop;
  - a mobile building while it is deployed, packing or unpacking.
- Gaining a property needs every one of its `required_properties`.
  [code] Each terrain property names its own, `infantry` or `vehicle`,
  which replaces the `mobile` of their template `TerrainProperty` (a
  field a definition sets wins, [data](data.md#templates)). `infantry`
  and `vehicle` come from the unit's own property list (trucks, packed
  HQs included, are vehicles). [code, data]
- `mobile` isn't in any unit's file. The game gives it to every thing
  that can move (the complement of the first bullet above: regiments,
  and units with `speed_max` above 0) and has a `PropertiesComponent`,
  as the thing is made, whether it's on the map at the start or made in
  play (a recruit, a truck made by packing). It isn't handed from a
  regiment to its members (`pass_down_members = false`); each gets its
  own. No terrain property needs it. [code, data]
- A thing's terrain is first looked at as it's made, before it has the
  properties it's made with, so a unit can't qualify for a terrain
  property then. This holds for things on the map at the start too:
  every actor in the shipped maps is stored with no properties held, so
  each starts from its template's like a thing made in play. [code,
  data] Since nothing happens while the terrain stays the same, a unit
  outside a regiment, on the map or made in play, gets no terrain
  property until its terrain changes: one on forest has none until it
  reaches another terrain, and gains forest's when it comes back. [code]
- A regiment on the map at the start has its units stored within it,
  and they're made, and have joined it, before its terrain is first
  looked at. So it qualifies by its units, gains the terrain's
  properties for the kinds it holds, and hands them to those units
  then. [code, data]
- A regiment made in play is made before its units, so its every place
  is empty when its terrain is first looked at. An empty place counts by
  the unit template that fills it, so the regiment gains the terrain's
  properties for the kinds of unit it's made of: a tank regiment made on
  forest holds the vehicle Forest property from the start, and an HQ
  regiment its truck's. Its units then get them from it as they join
  (below). [code]
- A regiment gains a terrain property when any one of its members meets
  the requirements (an empty place by its template), or it does itself.
  It then hands the property to each member that meets them, and to each
  unit that joins it later (terrain properties leave `pass_down_members`
  at its default, true). So on forest a mixed regiment holds both the
  infantry and the vehicle property, its foot soldiers hold the first and
  its vehicles the second. [code]
- A member therefore holds, of the properties it qualifies for, those of
  its regiment's terrain, plus those of its own tile while straggling.
  Holding the same property twice counts once: the second copy adds
  nothing. [code]
- Unpacking happens where the truck stands: the truck drives to the spot
  the order names and the building appears at the truck's position less
  its `packed_position_offset`, which is `0,0` for every stock truck
  ([buildings](buildings.md)). So the regiment's position when its
  terrain stops being looked at, the truck's and the building's centre
  are one point, and the terrain kept is that of the tile under the
  building's centre. [code, data]
- A deployed Division HQ showing the vehicle Forest property
  (`ger-division-hq-terrain-tooltip.jpg`) is its regiment's: the regiment
  gained it while its one member was still a truck, standing on a forest
  tile, and unpacking doesn't move the regiment onto another terrain, so
  it stays. No trees need stand there. [code, game] In that screenshot
  the HQ most likely stands on the bare forest ground beside the Berlin
  start described above. [guess] The HQ building itself, not being mobile or a vehicle, doesn't hold it
  and its damage taken is unchanged. Packing again makes the truck gain
  it at once. [code]
- What a held terrain property does: its `DV` and `ResistDamageModifier`
  entries change damage taken ([combat](combat.md)). Its
  `TerrainSpeedModifier` doesn't work through the holder at all: the
  speed limit belongs to the terrain ([movement](movement.md)). How it
  shows: [object display](object-display.md). [code]

## Open questions

- How does the game draw the ground where it steps between tiles: at the
  edges of mountain tiles (their noise), and where the mountain map stops
  counting a tile beyond mountains and water? Drawn with one height per
  vertex, 165 of the 232,139 objects stored in shipped maps stand more than
  0.5 off the drawn ground (24 more than 1); drawn tile by tile with walls
  closing the steps, those walls show along every edge of the mountain
  map's reach. Which does the game show, and which height does each vertex
  take? (implementer, 2026-11-08)
