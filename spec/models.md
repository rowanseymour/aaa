# Models

How the game draws what a model file holds: where the meshes of a model end
up once its nodes, bones and animations have had their say, and which of
them hide which.

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Skinned meshes

A skinned mesh is a shape with a skin: a skeleton root, a list of bone
nodes, one overall transform for the skin and one bind transform per bone.
Transforms below are written so that the rightmost applies first, and a
node's "place" is its full current transform (rotation, translation and
uniform scale) after animation.

- A skinned vertex is drawn at

  `shape × overall × inverse(root) × bone × bind × vertex`, summed over its
  bones by weight,

  where `vertex` is the position stored in the mesh, `bind` is that bone's
  bind transform from the skin data, `bone` is the bone node's current place
  in the world, `root` is the current place in the world of the skeleton
  root the skin names (that node itself, not its parent), `overall` is the
  skin data's once-per-skin transform, and `shape` is the skinned shape's
  own current place in the world, built from its own transform and every
  node above it exactly as a rigid mesh's is. [code]
- So neither the shape's own transform nor the skin's overall transform is
  ignored. The bones alone give the vertex a place measured from the
  skeleton root; the overall transform then moves that into the mesh's own
  space, and the shape's place in the scene carries it into the world like
  any other mesh. [code]
- When the shape hangs somewhere below the skeleton root, as in every stock
  model, this is the same as: take each bone's place measured from the
  skeleton root, apply the overall transform, then apply the shape's place
  measured from the skeleton root, then the root's place in the world.
  Anything that moves the skeleton root or a node above it therefore moves
  the whole mesh once. [code]
- Where the shape's place measured from the skeleton root times the overall
  transform is the identity, the two cancel and the bones alone decide, which
  is the case for most stock skins. Where they don't cancel, the mesh is
  drawn away from its bones by what is left over. `snow_owl` in
  `Units/Ambient/SnowOwl/SnowOwl.nif` and `bird_vulture` in
  `Units/Ambient/Vulture/Vulture.nif` are drawn at the height their shape
  nodes give (about 10.4 and 12.9 above the model's origin, and so above
  their bones), not down at the bones; the `Fish.nif` shapes are likewise
  drawn their 1.1 to 1.9 to the side, and the same goes for the smaller
  offsets in the effect, waypoint and portrait models. [code, data]
- The shape's place is read afresh every frame, so a skinned shape whose
  own node, or a node between it and the skeleton root, is animated moves
  with that node even while every bone holds still. [code]
- A node that sits above both the shape and a bone, but below the skeleton
  root, counts twice: once through the bone's place and once through the
  shape's. [code]
- Normals follow the same sum using only the rotations, and are brought
  back to unit length afterwards. [code]
- The mesh's bounds for culling follow the bones too: each bone's bounding
  sphere from the skin data is carried by that bone, the spheres are merged,
  and the result is placed by the same `overall × inverse(root)` step under
  the shape. [code]

## Bone weights

- A skin's file may hold, besides the per-bone weight lists, a second
  description that splits the mesh into parts for drawing. Each part lists
  up to 4 bones and gives every one of its vertices 4 weights, each naming
  one of the part's bones. Where it is present the game draws from it, not
  from the per-bone lists. [code, data]
- So at most 4 bones pull on a vertex. [code, data]
- The weights are used as stored, with no renormalising by the game. In
  every stock part the 4 weights of a vertex sum to 1 (to within rounding),
  and unused slots hold 0. [code, data]
- The two descriptions don't always agree. Where a vertex had more bones
  than a part could give it, the part's weights drop some and share the
  rest out again, and it is the part's weights that show. Stock models where
  they differ (62 vertices in all): the pack, unpack and death models of
  `GERHQArmor` and `GERHQMechanized`, `RUSHQMechanizedDeath.nif`,
  `Sheep01.nif`, `Sheep02.nif` and `MontgomeryPortrait.nif`. Four vertices
  in the two German HQ death models have 5 bones in the per-bone lists and
  4 or fewer in the parts. [data]
- A vertex shared by two parts is drawn once in each, with that part's
  weights. [data]
- 24 stock skins hold only the per-bone lists (among them `AtomicShock.nif`,
  the `CarpetBomber01.nif` models, `IndustrialSabotage.nif`, the V1 rockets
  and the tall tropical grass). None of their vertices has more than 2
  bones, so drawing them from the per-bone lists as stored loses nothing.
  [data]

## Billboard nodes

- A billboard node that always faces the camera (billboard mode 0, such
  as `Plane01` in `SpotLight02Bright.NIF`) has its rotation replaced every frame,
  whatever its file and animation give: in the world its z axis points
  back along the camera's viewing direction (towards the viewer), its y
  axis along the camera's up, and its x axis is y × z, to the right on
  screen. [code]
- It's the same rotation for every such node in view, taken from the
  camera's direction rather than aimed at each node from the camera's
  position. The node keeps its own place and scale, and what hangs below
  it keeps its own transforms relative to it. [code]
- So geometry a model puts off the node's origin along z is drawn that
  far towards the viewer: it moves out of walls it was placed against,
  without moving on screen. [code, data, game]
- The `billboard` user property some model files carry plays no part;
  only the node's kind does. [code]

## Texture coordinates

- A shape's texture coordinates are used as the file stores them, with
  nothing flipped: u = 0 is the image's first stored column and v = 0
  its first stored row (the top of a DDS as stored), u and v = 1 the far
  edges. [code, data, game]
- The texture slot's clamp mode and filter apply as stored, sampling
  the image's own mip levels where it has them (filter 2 blends between
  levels). Where the slot wraps (clamp 3), filtering near an edge blends
  in texels from the opposite edge, so a coordinate just inside 0 takes
  part of its colour and alpha from the last row or column, more so in
  the smaller mip levels. [code, data, guess for the mip levels]

## One-sided faces

- A triangle is drawn only from its front: the side from which its three
  corners, in the order the file stores them, run counter-clockwise.
  From behind it isn't drawn, writes no depth and hides nothing. In the
  stock models that front is the side the stored vertex normals point
  to. [code, data, game]
- A shape whose stencil property sets draw mode 3 (both) is drawn from
  both sides, both faces together as stored (the night spotlights'
  cones). Without one, shapes are one-sided. [code, data]

## Depth and draw order

How shapes hide one another. "Blending" means the shape's alpha property has
its blend flag (bit 0) set; "alpha test" is bit 9. A Z-buffer property holds
two flags, test (bit 0) and write (bit 1), and a comparison function.

### Which depth settings a shape gets

- Depth behaviour comes from the Z-buffer property alone. The alpha property
  never changes it: a blending shape tests and writes depth exactly as an
  opaque one with the same Z-buffer property would. [code]
- The Z-buffer property that applies is the shape's own, or else the nearest
  one on a node above it, like every other property. Nearly every stock
  model carries one on its root node (flags 3), which is where shapes that
  list none of their own get theirs: all the waypoint markers do. [code,
  data]
- With no Z-buffer property anywhere above it, a shape tests and writes
  depth with less-or-equal (the same as flags 3, function 3). About 1,000
  stock shapes are in this case (those of `Lamp04.nif`, `Glass.nif` and
  `LampLight08.nif`, for example). [code, data]
- Test flag set: a pixel is drawn only where it passes the comparison
  against the depth already there. Test flag clear: depth is off for the
  shape altogether, so every pixel is drawn over whatever is there and
  nothing is written to depth, whatever the write flag says. [code]
- Write flag set (with test set): each pixel drawn records its depth, so
  later, farther pixels are hidden behind it. Write flag clear: the shape
  is hidden by what was drawn before it but hides nothing drawn after. [code]
- Function values: 0 always, 1 less, 2 equal, 3 less or equal, 4 greater,
  5 not equal, 6 greater or equal, 7 never. Every stock Z-buffer property
  holds 3. [code, data]
- Flag values in the stock models: 3 (test and write) on most; 1 (test, no
  write) on most effect shapes, glows and every particle system but one; 0
  (neither) on the four status bar models (`HealthBar.NIF`,
  `HealthBarMedium.NIF`, `HealthBarLarge.NIF`, `SupplyStockpileBar.nif`).
  [data]
- A pixel that fails the alpha test isn't drawn and writes no depth. With
  alpha test off, every pixel of a triangle writes depth (when the shape
  writes at all), fully transparent texels included. [code]

### Order of drawing

- The world is drawn in passes, each pass complete before the next: terrain;
  shapes that don't blend (skinned, then the rest); unit shadows; water;
  blending shapes; particle systems; floating world text; then the
  interface. [code]
- What puts a shape in the blending pass is the blend flag of an alpha
  property in the shape's own property list. Whether it also tests alpha
  makes no difference, and no other alpha flag is looked at: bit 13 ("no
  sorting") is ignored, and no stock model sets it. Every stock shape that
  blends has the alpha property on itself. [code, data]
- A shape that only tests alpha (no blend flag) is drawn with the opaque
  shapes. [code]
- Particle systems go in the particle pass whatever their alpha property
  says. [code]
- Shapes under billboard nodes (and other special node kinds) are placed in
  passes one by one, like any other shape; the node doesn't draw them as a
  group. [code]
- Shapes that don't blend aren't sorted by distance. They are grouped to
  suit the graphics card, so their order is not something to match; depth
  testing settles what shows. [code]
- Blending shapes are sorted back to front, farthest first, across every
  model in view: one shape of a model can be drawn before, and another
  after, a shape of a different model. [code]
- The sort is by one point per shape: the centre of the shape's bounding
  sphere in the world (as moved by its nodes and animation). Its depth
  along the camera's view direction is cut into 1,024 equal slices between
  the camera's near and far distances (`CameraNearPlane` 1,
  `CameraFarPlane` 512, so a slice is about half a world unit deep), and
  shapes are drawn slice by slice from the far end. Shapes whose centres
  fall in the same slice keep the order in which they joined the scene,
  earliest first. [code]
- Particle systems are sorted the same way among themselves
  (`particles.md`). [code]
- Within one shape nothing is sorted: its triangles are drawn in the order
  the file stores them, both faces together when it is double-sided. [code]

### What that looks like

- A blending shape with depth test and write (flags 3) hides its own
  farther parts behind its nearer ones, like an opaque shape, because each
  triangle is tested against the triangles of the same shape drawn before
  it. The only difference from an opaque shape is at soft or transparent
  texels: where a nearer triangle is stored earlier, its see-through
  texels have already written depth, so the farther part doesn't show
  through them; the scene drawn in earlier passes shows there instead.
  Where the farther triangle is stored earlier, it shows through properly.
  [code]
- The guard marker is this case. In `WaypointDefendLocation.nif` and
  `WaypointDefendActor.nif` the textured shape (shield and rifles) blends
  without alpha test and takes flags 3, function 3 from the root node, so
  whichever of shield and rifles is nearer the camera covers the other as
  the marker turns. [code, data]
- The other markers: `WaypointRoutLocation.nif` has two blending shapes and
  `WaypointUnpack.nif` one (`Arrow01`, additive), all with flags 3 and so
  the same behaviour; the attack, bombard and move markers only test alpha,
  and the build, explore and run markers have no alpha property, so all of
  those are plain opaque drawing. [data]
- A blending shape with test but no write (flags 1), which is what most
  glows, muzzle flashes, explosions and road and shoreline overlays are, is
  hidden by opaque things in front of it but never hides anything itself.
  Its own triangles overlap in stored order, and against other blending
  shapes only the back-to-front sort decides. Additive shapes (destination
  factor one) look the same in any order. [code, data]
- A shape that doesn't blend but has write off (flags 1: `V1Rocket.nif`,
  `TropicalLilyPads01.nif`, `GBRDepotAmmo.nif` and a few more) is drawn in
  the opaque pass and can be painted over by anything drawn later, even
  something behind it. [code, data]

## Open questions

- For a skin with no parts in its file, the game seems to make its own split
  before drawing. What does it drop when the three corners of a triangle need more
  than 4 bones between them? No stock vertex has more than 2 bones there,
  but a triangle could still reach 6. (analysis, 2026-11-09)
- Is the owl's and vulture's height seen in the running game as the rule
  says? It is read from the code and the files, not yet watched. (analysis,
  2026-11-09)
- The status bar models have depth off entirely (flags 0) and no alpha
  property, which puts them in the opaque pass: are they then painted over
  by water, blending shapes and particles behind them, or does the game
  draw them some other way? (analysis, 2026-11-11)
- Is the guard marker's nearer-part-in-front behaviour seen in the running
  game as the rules say? Read from the code and the file, not yet watched.
  (analysis, 2026-11-11)
- A shape whose only alpha property is on a node above it would blend but
  be drawn in the opaque pass. No stock shape is like this; is that right
  for models that aren't stock? (analysis, 2026-11-11)
