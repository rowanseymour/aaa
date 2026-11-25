# Particles

How the particle systems in effect models emit, move and are drawn. Which
effects play, where and for how long is in [effects](effects.md).

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Birth and movement

63 of the 179 effect models have particle systems; the rest are animated
meshes (flags, lights, the construction scaffolding). A particle system's
look comes from its emitter and modifier blocks in the model, and they act
as below. Names are the model format's fields. Random draws are uniform.

- Birth: while the emitter's on/off keys have it on, particles are born at
  `birth_rate` per second, itself keyed over the animation: the k-th
  particle after the emitter switched on is born k / rate seconds after
  it. Several born within one frame start at their own ages, already
  moved on by the time since their birth. Nothing is born on a system's
  first frame, or while it already holds its maximum number of particles.
  [code]
- Nothing runs ahead: a system starts empty at the moment its effect is
  created, including the ambient effects map objects carry from the start
  (a refinery's chimney smoke), which fill up over their first particles'
  lifespans after the map loads. [code]
- Each new particle gets: [code]
  - speed `speed` plus a random amount within ±`speed_variation` / 2;
  - a direction `declination` (radians from the emitter's +Z axis) and
    `planar_angle` (radians about +Z, from +X), each plus a random amount
    within ± its variation;
  - lifespan `life_span` plus a random amount within ±`life_span_variation`
    / 2;
  - colour `initial_color` and radius `initial_radius` (no variation), at
    full size.
- Where it starts, in the emitter object's position and orientation (which
  also turns the direction above): [code]
  - Box (52 stock models): anywhere in a `width` × `height` × `depth` box
    centred on the emitter.
  - Cylinder (4): the cylinder stands along the emitter's +Z axis. A
    random angle about it (in the emitter's XY plane), a distance from the
    axis random between 0 and `radius` (so more particles near the axis),
    and a random height along Z within the centred `height`.
  - Sphere (1): a distance from the centre random between 0 and `radius`,
    with two random angles each between 0 and 2π, so particles crowd the
    centre and the poles.
  - Mesh (26): a random mesh from the emitter's list, each equally
    likely, then a spot on it by the emission type. [code]
    - Vertices (0; one stock model): a random vertex, each equally
      likely. [code]
    - Face centre (1) and face surface (3; every other stock model): a
      random triangle, each equally likely whatever its size. Type 1 puts
      the particle at the triangle's centre (the mean of its corners).
      Type 3 puts it inside the triangle: from its first corner, a
      fraction a, uniform between 0 and 1, of the way along the edge to
      the second corner, plus a fraction b of the edge to the third, b
      uniform between 0 and 1 − a. That always lands inside the triangle
      but not evenly: particles crowd towards the second corner. [code]
      Which corner is "first" is the order the mesh stores them in.
      [guess]
    - By the velocity type, the particle flies along the spot's normal at
      the speed above, in the declination and planar direction turned by
      the mesh, or along the emitter's fixed `emission_axis` at a fresh
      random speed. For a vertex the normal is that vertex's; for a face
      (either type, wherever on it the particle starts) it's the mean of
      its three corners' normals, normalised, not the flat face's own
      normal. A mesh without normals leaves the direction as it was.
      [code]
    - The spot and the direction then move with the mesh's own position
      and orientation, as for a vertex. [code]
- Every frame, in the order the model lists its modifiers, skipping any
  that's switched off [code]:
  - Gravity (13 models): adds to the velocity, per second, `strength` ×
    1.6 along the normalised axis (planar), or towards the gravity
    object's position (spherical), scaled by e^(−`decay` × d), where d is
    the distance from the gravity object along the axis (planar) or
    straight-line (spherical). `turbulence` adds, per axis, a random
    acceleration within ±`turbulence` × `turbulence_scale` × 500. Without
    a gravity object, the axis is taken in the particle system's own
    frame, centred on its origin. [code]
  - Drag (41): only with a drag object. The part of the velocity along
    the drag axis shrinks by 30 × `percentage` × the frame's seconds,
    fully within `range` of the drag object, fading linearly to nothing at
    `range_falloff`. The rest of the velocity is untouched. [code]
  - Colliders (23 stock systems, planes and spheres; below).
  - Position: each particle moves by velocity × the frame's seconds, or
    after a collision this frame, by its new velocity × the time left
    after the hit. [code]
  - Age and death: age grows with game time and a particle is removed
    once it's older than its lifespan, spawning new ones first if the
    model asks for that. [code]
  - Grow and fade (61): size = age / `grow_time` while younger than
    `grow_time`, and (lifespan − age) / `fade_time` within `fade_time` of
    its end, the smaller of the two when both apply, otherwise 1, and
    never below 0.0001. Each applies only to particles of its own
    generation (`grow_generation`, `fade_generation`); emitted particles
    are generation 0. [code]
  - Colour (63): the colour keys are read at age / lifespan, each channel
    held within 0 to 1. [code]
  - Rotation (56): it spins each particle about its axis (fixed, or random
    per particle), but the drawn particles don't turn, so it has no
    visible effect. [code]
- Controllers on the system (read at the model's animation time, like
  its other keys) [code]:
  - A modifier's on/off keys: off, the modifier does nothing that frame,
    so a switched-off position, age or colour modifier freezes that part
    of every particle. If an on key falls anywhere in the time since the
    last frame, the modifier counts as on for this frame even if it's off
    again by now. Particles born while it's off are still set up by it
    (a switched-off rotation still gives them their spin). An emitter
    switched off this way still emits: births follow only the emitter's
    own on/off keys above.
  - The emitter's speed and initial-radius keys set `speed` and
    `initial_radius` each frame to the keyed value. Particles born from
    then on take it (speed still with its variation); those already born
    keep theirs. Whether that frame's own births get the new value or the
    previous frame's depends on whether the model lists the controller
    before or after the emitter's birth controller. [guess]
- Colliders, all the colliders on one system together [code]:
  - Each frame each particle is tested against its straight path from
    where it was at the last frame to where its velocity would take it.
    Of all the colliders it hits, the earliest hit counts; at most one
    collision per particle per frame.
  - Plane: through the collider object's position (the particle system's
    origin without one), spanned by its `x_axis` and `y_axis` turned by
    the object's orientation, facing their cross product x × y. A hit
    counts only within `width` × scale / √2 of the centre along the x
    axis and `height` × scale / √2 along the y axis (scale being the
    object's). A particle already within 0.001 of the plane at the start
    of the frame hits it straight away.
  - Sphere: centred on the collider object (the system's origin without
    one), of `radius` × the object's scale. A particle outside hits where
    its path enters the sphere, one inside where its path would leave it,
    so it keeps particles in or out; one already on the surface (squared
    distance within 0.001 of the squared radius) doesn't hit.
  - On a hit the particle moves to the hit point. Its velocity is
    mirrored about the surface (the plane's facing, or the line from the
    sphere's centre to the hit point) and multiplied by `bounce`. On a
    plane, a hit landing exactly at the frame's end, unless the particle
    is moving away from the plane faster than 0.001, instead just takes
    away its velocity across the plane, without bouncing. Then
    `spawn_on_collide` spawns new particles from it through the spawn
    modifier, and `die_on_collide` removes it.
- With the system's world-space flag set, particles stay where they were
  born when the model moves; otherwise they move rigidly with the model.
  [code]

## Emitters on paths

A path controller moves a node of a model, an emitter in 43 stock effect
nodes (the explosions' "Streamer" nodes) and one doodad, along a path of
position keys. [data]

- Each frame it reads its percentage keys at the model's animation time,
  giving a fraction p of the way along the path. [code]
- With the constant-speed flag (0x10 in the path flags; every
  stock effect has it), the node goes to the point p × the path's total
  length along the curve, measured along the curve itself, so it moves
  evenly as p rises evenly. p outside 0 to 1 wraps to its fractional
  part. Without the flag, p is matched against the position keys' times
  instead: the node goes between the two keys whose times bracket p, at
  the fraction of the way between them. [code]
- The curve between keys is the position keys' own interpolation, by
  their key kind. [code]
- With the follow flag (0x20; every stock effect and the lily-fish
  doodad), the node also turns. The direction of travel T is the curve's
  own tangent at the point the node is put at, scaled to unit length,
  and L = up × T scaled to unit length, up being the parent's +Z, is
  level. The follow axis (0 X, 1 Y, 2 Z) picks how they're laid on the
  node's axes [code]:
  - 0 (X, the lily fish): X along T, Y along L, Z = X × Y.
  - 1 (Y): Y along T, X along −L (travel × up), Z = X × Y.
  - 2 (Z, every stock effect): Z along T, Y along L, X = Y × Z.
  So when the travel is level, the axis that is neither following nor
  level points straight up under X or Y follow, and straight down under
  Z follow.
- The flip flag (0x40; set on the lily fish, unset in every stock
  effect) turns the node half a turn about that third axis: the
  following axis points back against the travel and the level axis is
  reversed, the third unchanged. So the lily fish's −X leads, its Z still
  up. [code]
- When the travel is straight up or down (|up × T| no more than a
  millionth), L is taken as zero, and so is the third axis built from it:
  for that frame only the following axis is left, and the node and all it
  carries are squashed onto the line of travel. No smoothing or last good
  orientation stands in. [code]
- At the ends of the path the tangent is the curve's own there, with
  nothing blended across the join: at p = 0 the direction leaving the
  first key, at p = 1 exactly the direction arriving at the last key.
  Under constant speed p just past 1 (or below 0) wraps, so the node
  jumps back to the start and its heading snaps from the last key's
  arriving direction to the first key's leaving direction in one frame
  (they agree only where the path's keys make them agree). [code]
- The path's curvature doesn't roll the node: banking (0x8) and the
  smoothing of the heading near keys (0x4) are unset in every stock path,
  so neither is needed. Without follow, the node's orientation is left as
  it is. [code]
- A path controller with no position keys, or no percentage keys, does
  nothing: the node keeps its own place and orientation, whatever its
  flags. The JPN depots' pack and unpack models have such controllers
  (flags 3), so nothing is played for them. [code]
- Model files older than version 10.0.1.2 have no path-flags field: the
  path flags are packed into the controller's own flags instead, and its
  flags are read as [code]:
  - bits 0–3 (0xF): its own timing flags as usual (0x1 timing, 0x6
    cycle, 0x8 active);
  - bits 4 and up: the path flags, shifted down by four (so 0x10 in the
    file is path flag 0x1). They matter only to a path controller; on any
    other controller they're dropped;
  - "play backwards" (0x10 of the controller's own flags in newer files)
    isn't stored, so it's off;
  - and before 10.0.1.3 its manager-update flag (0x20 of its own flags
    in newer files) is set; a path controller plays the same either way.
  41 stock models are at 10.0.1.0 or 4.2.2.0, so read this way; none is
  at 10.0.1.1, which packs them slightly differently. [data]
- The tropical fish (`TropicalFish01`/`02.nif`) are such files, with
  controller flags 0x708: active and looping, and path flags 0x70,
  constant speed, follow and flip, the lily fish's own. Their follow axis
  is 0 (X), also as on the lily fish. [code, data]
- Their percentage keys go from 0 at 0 s to 1 at 12 s, the controller's
  range is 0 to 12 s, and it loops, so p rises evenly from 0 to 1 every
  12 s. Their position keys (quadratic) are at times 0, 0.25, 0.5, 0.75,
  1 and 12, but under constant speed the times don't matter: all six make
  up the path, and its length includes the last stretch, from the fifth
  key to the sixth. [code, data]
- The first five keys trace a closed loop, the fifth back at the first's
  point with the first's tangents. The sixth is at that point again but
  with zero tangents, so (under quadratic keys' own interpolation) the
  last stretch runs on along the loop's starting direction and comes back
  to the start: about 0.66 out for `TropicalFish01` and 0.37 for `02`,
  some 8% and 6% of each path's length. [data, guess]
- So each 12 s the fish swims its loop at an even speed, carries on
  straight a little way, turns about at once where it stops (its travel
  reverses there), swims back to the start, and turns about again as the
  loop restarts. Throughout it's turned to the travel with its −X leading
  and Z up, as the lily fish is, while its own keyframe animations wag
  it. [code, data] That doodad models are updated each frame, as effect
  models are, is assumed. [guess]

## Drawing

- Each particle is drawn as a square that always faces the camera, edges
  along the screen's axes, never rotated. Half its width is radius × size
  × the model's scale. The square carries the whole texture and the
  particle's colour (white if the system has no colours). [code]
- Every corner of a particle's square has the same normal, pointing
  straight back at the camera, so lighting is flat across it and depends
  on where the light is relative to the view, not on the particle.
  [code]
- Whether a system is lit comes from its vertex colour property, as for
  any model [code]:
  - Lighting mode 0 (emissive only) with vertex mode 1 (emissive), as in
    most explosions: unlit, drawn in the particle's colour as it is.
  - Lighting mode 0 with vertex mode 0 or 2: lit, but by no light, so
    only the material property's emissive colour shows.
  - Lighting mode 1 (emissive, ambient and diffuse), as in the
    construction dust: lit by the lights reaching the model (those
    switched on with a dimmer of at least 0.01), with the material
    property's colours, except that the particle's colour replaces the
    emissive colour under vertex mode 1, or the ambient and diffuse
    colours under vertex mode 2 (the dust). Colour is then emissive + the
    lights' ambient × ambient + each light's diffuse × diffuse × the
    cosine between the normal and the direction to the light (nothing
    from a light behind the square). The particle's alpha comes from its colour under
    vertex mode 2, otherwise from the material.
  - A base texture applied in replace mode overrides all of this: unlit,
    the texture alone.
  Which of the map's lights reach effect models (the sun, presumably) is
  open below.
- Particle systems are drawn after the opaque scene, the water and every
  other alpha-blended model, and before the interface, so a particle is
  never hidden behind a nearer alpha-blended model. [code]
- Particle systems are sorted back to front among themselves, across all
  effects, by the camera depth of each system's bounding centre, cut into
  1024 equal slices of the camera's depth range. Systems in the same
  slice draw in no particular order. An effect model with several systems
  sorts each one on its own. [code]
- Within a system, particles aren't sorted: they draw in the order the
  system stores them. [code]
- Other alpha-blended models are sorted the same way among themselves, in
  their own pass before the particle systems (`models.md`, Depth and draw
  order). [code]
- How each model blends comes from its own alpha property: stock effect
  models use both ordinary alpha blending (source alpha over
  one-minus-source-alpha) and additive (source alpha onto one). [code]

## Older particle systems

Models from before 10.1.0.0 describe particles differently: a particle
object (NiParticles, NiRotatingParticles or NiParticleMeshes) with its
data, driven by a NiParticleSystemController that holds the emitter's
values and points to a chain of old modifiers (NiParticleGrowFade,
NiParticleColorModifier, NiParticleRotation, NiGravity,
NiParticleMeshModifier and others). Stock examples are the burnt-out
tank's smoke (`TankDead.NIF`, three systems, 4.2.2.0) and the lake mist
(`TemperateLakeMist01.nif`, 10.0.1.0). [data] The game plays each one as
the newer kind of system described above, set up from the old values as
below. Anything in the old blocks not named here has no effect. [code]

- The particle object keeps its name and its properties (texture, alpha,
  vertex colour, material and so on), so it's drawn as under Drawing. It
  doesn't keep its own position, orientation or scale: the system sits
  exactly at its parent node's frame. Its flags (hidden or not) and extra
  data don't carry over either. [code]
- Particles are never kept in world space: they move rigidly with the
  model. [code]
- The system holds at most as many particles as its data has vertices (10
  in each of the tank's systems, 20 in the mist, the same as the length of
  their saved particle lists). [code, data]
- The particles saved in the model (the list, its valid count, their ages,
  life spans, velocities, sizes and the data's radius) are all ignored:
  the system starts empty, as any other does, and "reset" doesn't change
  that. [code]

### Emitting

- Particles are emitted as by a box emitter whose emitter object is the
  controller's emitter node, with the controller's speed and its
  variation, declination and its variation, planar angle and its
  variation, initial colour, initial size as the radius, life span and its
  variation, and the emitter size as the box's width, height and depth.
  So, by the rules under Birth and movement: [code]
  - each is born anywhere in a box the full emitter size across, centred
    on the emitter node and lying along its axes (the tank's 0.4 × 0.4 ×
    0, the mist's 8 × 8 × 0.3);
  - its direction is the declination from the emitter node's +Z (the
    tank's π: straight along its −Z) and the planar angle about +Z from
    +X (the mist's: declination π/2, planar π/4, level and diagonal),
    each plus a random amount within ± its variation;
  - its speed is `speed` plus a random amount within ± half its
    variation, and its life span likewise within ± half its variation;
  - once born it moves in the system's frame (the particle object's
    parent node's), not with the emitter node.
- The controller's three-float normal after the planar angles ([1, 0, 0]
  in both stock models) is unused. [code]
- The controller's initial size is each particle's radius, so half the
  width of its square is that size × the particle's size factor (grow and
  fade) × the model's scale: the tank's 0.35 makes squares 0.7 across at
  full size. [code]
- The birth rate is one constant rate throughout [code]:
  - with "use birth rate" set, the controller's birth rate;
  - otherwise the capacity (the data's vertex count) divided by the life
    span when the controller's time range (its end time less its start
    time) is longer than the life span, else the capacity divided by (emit
    stop − emit start).
  The tank's range of 5 s is longer than its life spans (2.67, 3.33 and
  4 s), so its systems emit 3.75, 3 and 2.5 a second; the mist's 33.3 s
  is longer than 16.67 s, so 1.2 a second. Either way about the capacity
  is alive at once, and nothing is born while the system is full. [code,
  data]
- The emit start and stop times become the emitter's on/off switching,
  against the controller's start and end times [code]:
  - emit start at or after the end time: never on (so nothing is ever
    born) [guess];
  - otherwise on from emit start, or from the start time if emit start is
    before it (and off from the start time until emit start if it's
    after);
  - off from emit stop if that's before the end time, and off at the end
    time in any case;
  - an emit stop equal to emit start is taken as 0.00001 later.
- That switching plays on the controller's own clock, with its frequency,
  phase and cycle type (both stock models loop), so it repeats every
  cycle. In both stock models it's on throughout. A negative emit start
  just means on from the cycle's start: nothing is emitted ahead of time
  and the system still starts empty. [code, data]
- Births then follow the rule under Birth and movement on that looping
  clock: the k-th birth of a cycle comes k / rate after the cycle starts
  (k from 1). When the cycle wraps, any births due between the cycle's
  start and the first frame after the wrap are skipped, so each cycle has
  a slightly longer gap at its start (the tank's first system: 18 births
  each 5 s at 0.27 s spacing, then a gap of about 0.47 s). [code]
- If the controller's flags have 0x1 set the emitter never runs, so
  nothing is born; the controller's own active flag (0x8) is ignored.
  Both stock controllers have flags 0x8. [code, data]
- With "reset" set, every particle is removed each time the controller's
  clock wraps, the controller then always looping whatever its cycle
  type. Neither stock model sets it. [code, data]
- Ageing, movement and the modifiers run on the model's clock scaled by
  the controller's frequency and phase, continuously, never looping (as in
  newer systems). So with the doodad's model updated, the tank's smoke and
  the mist carry on for ever. [code]

### Old modifiers

- Each frame the modifiers act in this order: ageing and removal first;
  then grow and fade, colour, rotation and mesh placement, in the order
  the old chain lists them; then gravity; then colliders; then movement.
  So a particle's size and colour use its age after this frame's ageing,
  and gravity's change of velocity applies to this frame's movement.
  [code]
- NiParticleGrowFade (grow, fade): the grow and fade rule under Birth and
  movement, linear, with `grow_time` = grow and `fade_time` = fade. Grow
  applies to emitted particles (generation 0), fade to particles of the
  generation equal to the controller's number of spawn generations (0 in
  every stock model, so emitted particles too). A fade of 0 is no fade.
  The tank grows over 1/3 s and fades over 1.5, 1 and 1 s; the mist grows
  over 6.67 s and doesn't fade. [code, data]
- NiParticleColorModifier: its colour keys are read at age / life span and
  replace the particle's colour (the initial colour, white in both, is
  never seen). The colour then draws as under Drawing. [code]
- NiParticleRotation (random, axis, speed): a particle's angle about its
  axis starts at 0 when it's born and grows by speed radians a second (the
  tank's 2π, a turn a second; the mist's 0.118), kept within 0 to 2π. The
  axis is the given one, or with random set a random one per particle:
  its z a random cos(πu) and its direction about z a random 2πv, u and v
  uniform 0 to 1 (so axes crowd towards the poles). Squares never turn
  (as under Drawing), so it shows only on mesh particles; and a system
  whose data has no rotations (plain NiParticles) has nothing to turn.
  [code]
- NiGravity (decay, strength, kind, position, direction): the gravity rule
  under Birth and movement with that decay and strength (so still ×
  1.6), planar for kind 0 and spherical for kind 1, the direction as its
  axis and no turbulence. Its gravity object sits under the particle
  object's parent, at the gravity's position plus the particle object's
  own position (the offset not turned), turned as the particle object is
  and scaled by its scale, so although the system itself drops the
  particle object's place, gravity keeps it. Neither stock model has
  gravity. [code, data]
- Spawn on death and the spawn values feed the spawn and age-and-death
  rules as in newer systems; neither stock model spawns. [code, data]

### Mesh particles

NiParticleMeshes (the lake mist) draws meshes instead of squares. [data]

- Each particle shows its own copy of a mesh from the
  NiParticleMeshModifier's list, picked by the particle's generation (0,
  so the first, for every emitted particle; past the end of the list, the
  last). The mist's list has one mesh, a "NewRoot" node that isn't in the
  scene itself. The copy is made when the particle is born, shown from
  the frame of its birth, and goes with the particle. [code, data]
- Each frame each copy is put at its particle's position, turned by its
  particle's rotation (above) and scaled evenly by radius × size (the
  mist's 3.5 × its grow factor), in the system's frame. [code]
- The copies draw as ordinary meshes, with their own properties; the
  particle colours aren't applied to them, so the mist's colour keys have
  no visible effect. [code]
- Each copy's own controllers run each frame at the particle system's
  clock, not the particle's age, with their own start and end times and
  cycle type. So at any moment every live copy shows the same frame of
  its animation. [code]
- The mist's copies have a material alpha controller over 0 to 10 s
  (keys 0 at 0 s, 0.2 at 2.2 s, 0.2 at 7.37 s and 0 at 10 s, quadratic)
  whose flags 0xc make it clamp: past its end it holds the value at
  10 s. So the copies' alpha rises from 0 to 0.2 and back to 0 over the
  system's first 10 s, all together, and stays 0 from then on: every copy
  born after 10 s, and every earlier one, is drawn fully transparent.
  The model's own "Parent" mist quad has the same keys and flags on the
  model's clock, so it too is transparent from 10 s on. Nothing loops
  them. [code, data]
- The copies saved under the data's mesh node (18 in the mist) are
  ignored. [code]

### Drawing old systems

- NiParticles and NiRotatingParticles draw as the squares under Drawing,
  with their own properties: the tank's are ordinary alpha blended (source
  alpha over one-minus-source-alpha), vertex colour mode 1 with lighting
  mode 0, so unlit in the particle's colour, alpha included. [code, data]
- Of the extra data on these nodes, "zMode10", "effect" and "enabled"
  aren't read by the game. "sgoKeep" only keeps the node from being
  dropped when the game tidies a model as it loads it, which doesn't
  change how these particles look. [code, guess]

## Open questions

- Which lights reach effect models: the sun alone, or others too, and
  with what scene-wide ambient. (analysis, 2026-11-08)

- Whether a doodad's model (scenery with no animations of its own in its
  template, like the tropical fish, the burnt-out tank and the lake mist)
  has its built-in controllers run each frame, so its paths, keyframes and
  particles play; if so, whether its clock starts when the map loads or
  when the doodad first comes into view, and whether it keeps running
  while off-screen. (analysis, 2026-11-08)

- Mesh emitters: how particles are placed for the edge emission types (2
  and 4), which no stock model uses. (analysis, 2026-11-08)

- Which way a particle's texture lies on its square: which corner of the
  image goes to which corner of the screen. (analysis, 2026-11-08)

- Older models' colliders (NiPlanarCollider, NiSphericalCollider) and
  NiParticleBomb: which of their values become which, and where their
  objects sit, and whether any stock model uses them. (analysis, 2026-11-08)
