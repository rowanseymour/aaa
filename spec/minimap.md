# Minimap

How the minimap's picture is made: the ground's colours, the fog shading
over them, the supply border, and which things are marked, in what
colour, shape and size, and which mark wins where they overlap.

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## The picture

- The picture is `minimap_size` pixels across (from the widget), covering
  the whole map; a map position lands on the pixel at position × picture
  size / map size, rounded down. [code]
- It's four layers, each redrawn on its own clock and stacked in this
  order: [code, data]
  1. the ground's colours (below), drawn once;
  2. the fog shading, every `MinimapPeriodUpdateFog` seconds, which
     multiplies the ground's colour channel by channel;
  3. the supply border, every `MinimapPeriodUpdateSupply` seconds
     ([supply](supply.toml), What the player sees), painted over the
     shaded ground;
  4. the things' marks, every `MinimapPeriodUpdateObjects` seconds,
     painted over everything else, fully opaque.
- So fog darkens only the ground: the supply border and the marks are
  never shaded by it. [code]

## Ground colours

- Each tile has one flat colour: its terrain variation's
  `terrain_color`, or the terrain's own `terrain_color` when the
  variation doesn't give one. Nothing else goes into it: no height,
  slope, light, shadow or time of day, and no brightening. [code, data]
- Water is coloured the same way, by its terrain's `terrain_color`
  (`temperate_river` 0.126, 0.298, 0.384, for instance); water depth
  doesn't show. [code, data]
- Roads, bridges, trees and other scenery aren't part of the ground
  picture. A road shows only where the ground under it is painted with a
  terrain of its own colour. [code; guess: that Berlin's tan roads are
  painted terrain]
- Between tiles: a pixel in the half of its tile nearer the map's
  first tile (tile 0, 0), on each axis, takes the tile's colour; in the
  second half it's mixed with the neighbouring tile on that side, the
  neighbour's share being how far across the tile the pixel lies (so
  from half at the tile's middle to all of it at the far edge). The
  mixing is done on both axes at once. With about one tile per pixel,
  as on a stock map, it barely shows. [code]

## Fog shading

- Each tile's shade, from the viewing side's shared sight
  ([fog](fog.md)): visible ground keeps its full colour (× 1), explored
  ground is × 0.7, and unexplored ground is black (× 0). [code]
- The shades are blended smoothly between neighbouring tiles: each pixel
  mixes the four tiles around it by how close it is to each, so the edge
  between visible and explored ground is a soft ramp about one tile
  wide. A thing's circle of sight shows as a lighter soft-edged disc on
  the slightly darker explored ground. [code, game]
- No shading at all (everything at full colour) when fog is None, under
  the fog cheat, during Radar Scan ([special operations](special-ops.md))
  and for a viewer with no side of its own. [code]

## Which things are marked

- A thing is marked when all of these hold: [code]
  - the viewing side sees it now or remembers it, as on the map
    ([fog](fog.md), What shows under fog); under the fog cheat, during
    Radar Scan or with fog None this test is passed;
  - it isn't hidden from the viewing side, unless that side still
    remembers it where it was last seen; [code; guess: that hidden means
    taken off the map, carried inside a transport for instance]
  - it belongs to a side, and that side is the viewer, an ally, an
    enemy, or a side that isn't independent and stands neutral to the
    viewer. Things belonging to nobody (trees, rocks, hedges, roads)
    aren't marked, nor are things of an independent side that stands
    neutral to the viewer (Berlin's "Civilian (Neutral)").
- A thing with a `minimap_icon` is marked whatever its side, even when
  it belongs to nobody: oil and ammunition resources and the objective
  markers show, in the neutral colour. [code, data]
- So enemy units out of sight aren't marked: units leave no remembered
  picture on the minimap any more than on the map. [code]
- Remembered things are marked in the colour of the side they belonged
  to when last seen; during Radar Scan, in their current side's colour.
  [code]

## Colours

- How the thing's side stands towards the viewer, read from that side's
  relations ([players](players.md)), picks the colour
  (`MiniMap/minimap.tgi`): [code, data]

  | Side | Building (`*_static`) | Unit (`*_mobile`) |
  | --- | --- | --- |
  | the viewer's own, selected | `color_owned_selected` (white) | same |
  | the viewer's own | `color_owned_static` (0.169, 0.600, 0.145) | `color_owned_mobile` (0.533, 0.863, 0.604) |
  | ally | `color_allied_static` (0.106, 0.192, 0.510) | `color_allied_mobile` (0.290, 0.404, 0.827) |
  | enemy | `color_enemy_static` (0.612, 0.145, 0.145) | `color_enemy_mobile` (0.776, 0.294, 0.294) |
  | neutral, an independent enemy, or nobody's (icons only) | `color_neutral_static` (0.729 grey) | `color_neutral_mobile` (same) |

  "Building" here is anything that never moves (buildings and other
  fixed things); everything else uses the unit colour. [code]
- `color_owned_selected` is only for the viewer's own things, and a
  thing counts as selected when it or its regiment is in the selection:
  a selected regiment's units are all white, and so is a selected HQ.
  An ally's or enemy's selected thing keeps its usual colour. [code,
  game; guess: that the second test is the regiment]
- Independent sides that are enemies (Berlin's "Civilian (Aggressive)")
  are marked in the neutral grey, not enemy red. [code, data]
- Things of the viewer's own side also blink when in trouble ([supply](supply.toml),
  What the player sees). [code]
- `color_white` is loaded but not used for marks. [code]

## Shapes and sizes

- A thing with a `minimap_icon` is drawn as that picture, at its own
  pixel size (one texel to one pixel), centred on the thing. Only its
  texels that aren't fully transparent are drawn, fully opaque, each the
  texel's colour times the mark's colour channel by channel, so dark
  parts of the icon stay dark. If the mark's colour is pure white
  (`color_owned_selected`), every drawn texel is white instead, frame
  included, so a selected HQ shows as a solid white silhouette. Icons get
  no outline. [code, game]
- A thing without an icon that's a building, or whose shape's radius is
  at least `min_icon_radius` (2.0 units), is drawn as its footprint: its
  shape's rectangle turned to its facing (a round shape as a square as
  wide as the circle), scaled to the picture, filled with the mark's
  colour. [code, data]
- Any other thing (an ordinary unit) is a square `object_point_size` (2)
  pixels across, its top-left corner one pixel up and left of the
  thing's pixel. [code, data]
- Footprints and squares get a one-pixel outline in
  `color_object_border` (black) on every pixel round them, diagonals
  included, so a unit is a 2 × 2 dot in a 4 × 4 black-edged square. The
  outline goes only where a lower-ranked mark (below), or nothing, lies;
  it never covers a pixel already of the mark's own colour, and a mark
  drawn later can cover it. [code, data]

## Which mark is on top

- Each mark has a rank, and where marks overlap the higher rank wins the
  pixel; at equal ranks the one drawn first keeps it. Things are drawn in
  the game's own order of things, so equal ranks fall out arbitrarily.
  [code]
- The rank, from `MiniMap/minimap.tgi` and the thing's data: [code, data]
  - start: the thing's `radar_factor` (1 in `template_object.tgi`), or
    `priority_multiplier_icon` (2) if it has a `minimap_icon`;
  - then times `priority_multiplier_enemy` (4) for an enemy;
    or `priority_multiplier_owned` (3) for the viewer's own, and a
    further `priority_multiplier_selected` (10) when selected; or
    `priority_multiplier_friendly` (2) for an ally; neutral things get
    no factor;
  - then times `priority_multiplier_static` (2) for a building.
- So with the stock values: an ordinary unit is 1 (neutral), 2 (ally),
  3 (own), 4 (enemy) or 30 (own, selected); a building without an icon
  doubles those; an icon starts at 2 instead of 1, so an own Corps HQ is
  12, selected 120, an enemy one 16. [code, data]
- A footprint's outline needs the pixel to rank at least two below the
  mark; a unit square's, at least one below. [code]
- The rank is kept in a byte: it wraps past 255, which the stock values
  never reach. [code]

## The selected thing's destination

- When the viewer has a thing of its own selected, the minimap also
  marks where one of them is headed: for a unit, the place (or the
  thing) its current order is taking it to, drawn as `waypoint_icon`;
  for a building, its rally point, drawn as `rallypoint_icon`. Nothing
  is drawn when there's no such place. It's drawn over every other
  mark, in the icon's own colours, centred on the place. Only one is
  shown, for one of the selected things, however many are selected.
  [code, data; guess: what a unit with no order shows]
- Only the current order counts: orders waiting in a regiment's queue
  ([commands](commands.md#queued-orders)) are never marked on the
  minimap, and the mark moves on to the next one only once it has become
  the current order. [code]

## Open questions

- Which selected thing's destination is shown when several are
  selected? (analysis, 2026-11-22)
- A further test, not identified yet, can leave things off even during
  Radar Scan (the open question in [special operations](special-ops.md)).
  (analysis, 2026-11-22)
- The user's Berlin screenshots (custom 2v2, playing USSR) show neither
  the computer teammate's HQ nor its ground lit, though teammates share
  sight from the start ([fog](fog.md), Shared sight). Was the teammate
  really on the player's team in the lobby? An allied HQ is drawn dark
  navy (`color_allied_static`), which is easy to miss. Check in the game
  (the politics window's Team lines). (implementer, 2026-11-22)
