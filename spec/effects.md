# Effects

Particle effects the game plays on the map, such as dust while a building
goes up and explosions when regiments fight, and the tints it lays over
models: what triggers them, where they appear and how long they last. How their particles move and are drawn is
in [particles](particles.md).

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

The data files say almost all of it. A thing's definition holds lists of
effect commands keyed by event (`[WhenDo template=FXC_CreateVFX]`), and the
effects themselves are `[VFX]` things in `Effects/*.tgi`. The templates
(`Templates/template_fx.tgi`, `template_vfx.tgi` and their `_aa` versions)
document each field. The rules below are how the game acts on that data.

## Which effects play

- No effect is tied to an event by the game itself: every one comes from an
  effect command in some thing's data, so which model plays is a data field
  (`VFX_IDS`), set per command. Each command names exactly one effect. [code]
- Commands are inherited from templates, so most units and buildings get
  theirs from `template_unit_aa.tgi`, `template_building_aa.tgi` and
  `template_abilitydefinition_aa.tgi`. [code]
- The events and when they happen:
  - Ability definitions (attacks): `WhenDo` once each time it fires,
    `WhenDoPerTarget` once per target the shot lands on (each target of an
    area attack). [code] `WhenStart` plays as the attack begins and
    `WhenEnd` as it finishes. [guess] A rifle or machine gun puts its
    muzzle flash, tracers and the hit puffs on the target all in `WhenDo`;
    a tank gun puts its muzzle flash in `WhenStart` and leaves the impact
    to its projectile. [code]
  - A projectile's `View` `WhenDestroyed` plays when it reaches the end of
    its flight: the shell explosions and scorch marks. That's the same game
    tick as the attack's damage and its `WhenDoPerTarget` effects, so the
    explosion and the hit land together. How long the flight takes is in
    [combat](combat.md#projectiles). [code] Its `FXC_PlaySFX` commands
    (`explosion_cannon_medium` and the like) play in that same tick, so
    the bang is heard as the shell lands, from the spot where it lands
    (see [Sounds](#sounds)). [code]
  - Body `WhenHealth`: on a change of health, measured as a fraction of
    maximum health, against the command's `health`: [code]
    - `ABSOLUTE_BELOW`: health was above `health` and is now at or below
      it.
    - `ABSOLUTE_ABOVE`: health was at or below `health` and is now above
      it.
    - `RELATIVE_DECREASE` / `RELATIVE_INCREASE`: one change lowers or
      raises health by more than `health`. These also play whenever any
      other `WhenHealth` command of the same thing plays. Stock data
      doesn't use them.
    - A command without `health_trigger_type` never plays.
  - Every crossing counts, so a `WhenHealth` effect plays again each time
    health is repaired back above its threshold and then damaged below it.
    Nothing stops the earlier one: `rising_flame` is permanent and there's
    no stop command, so a packable building repaired and burnt again gets
    a second flame on each of its points (80, 60 and 20% health), and a
    tank repaired above 40% and hit again gets a second `smoke_tank`.
    [code] Health changes while a thing is under construction trigger
    nothing. [code]
  - Health a map stores is simply set as the match loads; it isn't a
    change, so a thing placed already damaged shows none of its
    `WhenHealth` effects. Each one first plays when health next crosses
    its threshold: a depot stored at 50% shows no flames, gets the 40%
    explosion if it's damaged further, and gets its 80 and 60% flames
    only after being repaired above those points and damaged below them
    again. [code]
  - `WhenHealth` commands are inherited like the others, and a thing's own
    add to its templates' (see [data](data.md)). In stock data they come
    almost all from templates: [code]
    - Vehicles (tanks, halftracks, jeeps, trucks, HQ trucks):
      `smoke_tank` at 40%, and at 0 debris sized to the vehicle
      (`light_tank_debris`, `medium_tank_debris` or `heavy_tank_debris`).
    - Aircraft: `airplane_engine_smoke` at 80%, `airplane_engine_fire` at
      60%, `plane_disable` at 0.
    - Ships: `rising_flame` at 80 and 60% (and 40% on battleships and the
      like), `water_explosion_1` at 0. Water transports add their cargo's
      death effect (`usa_infantry_death`, `ger_truck_death` and the like)
      at 0.
    - Packable buildings: `rising_flame` at 80, 60 and 20%, and
      `temperate_large_explosion_1` at 40% and at 0. HQs add a flame at
      40% and a second explosion at 40% and at 0 on other points; depots
      add `smoke_oil` at 40%.
    - Constructed buildings (bunkers and the like): `smoke_bunker` at 80
      and 60%. Resource buildings: a flame at 80%, `smoke_oil` at 60%, an
      explosion at 40%.
    - Village and city buildings: `rising_flame` at 80, 60 and 40%,
      `temperate_large_explosion_1` at 0.
    - Infantry have none.
  - `WhenEnter` / `WhenLeave` with `actor_state`: when the thing enters or
    leaves that state (`construction`, `structure_packing`, `falling`,
    `dead` and so on). [code] Some effects a `WhenEnter` starts end by
    themselves when the state is left (How long, below).
  - Mover `WhenStartMoving` / `WhenStopMoving`: exhaust puffs and the dust
    trail. [code]
  - `View` `WhenCreated` (lights, flags), `WhenConstructed` (when a building
    is finished: window lights) and `WhenDestroyed` (building collapse).
    [code]
- In stock data, then: a constructed building (bunker and the like) shows
  scaffolding (`bunker_construction`) and dust (`building_pack`) while it's
  built, and `bunker_deconstruction` once construction is over; a packable
  building starts `building_pack` (attached, at `base`) as it starts to pack
  or unpack, so the dust rises centred on the building, on every packable
  building, packing and unpacking [code], as it does round a GBR supply
  depot that unpacks [game], and it ends when the packing or unpacking
  does (How long, below) [code]; buildings
  collapse with `building_destruction` or `building_destruction_small` at
  their feet; vehicles burst into debris at 0 health; infantry have no death
  effect beyond their animation. [code]
- Terrain variants are separate commands with `terrain_group_filter`
  (`dirt`, `desert`, `snow`): a command with a filter plays only when the
  terrain group where it's triggered matches. [code]
- `target_flags` on a command filters on the event's target, the same way
  as ability target filters (see [combat](combat.md)): stock attacks use
  it to pick a puff for infantry and a spark for vehicles and buildings.
  [code]

## How many

- An attack shows its effects once per shot: `WhenDo` plays every time the
  ability fires, so the count follows the shots, not hits or kills (every
  shot hits; see [combat](combat.md)). [code]
- `frequency = n` on an attack's command plays it on the 1st, (n+1)th,
  (2n+1)th... shot of that ability only. [code] Commands on other events
  ignore it. [guess]
- `delay` holds the effect back that many seconds after its event; stock
  data staggers several tracers or flashes on one shot this way. [code]
- There's no cap on how many effects play at once; what thins them out is
  the detail setting and the culling below. [guess]

## Detail, fog and the screen

- Effects whose VFX has `critical = false` (all the stock temporary models:
  explosions, flashes, smoke) are subject to the effects slider in the
  options (`ParticleGeneratePCG` in `UVars.tgi`, 0 "none" to 1 "full",
  default 1; the graphics presets set 1, 0.6 and 0.25): each
  time one would play, it plays only if a uniform random number in [0, 1)
  is below the setting. Permanent effects are always critical. The setting
  decides whether a whole effect plays, not how many particles it emits.
  [code]
- A temporary effect is not created at all when its spot (or, attached, its
  thing) is off screen, unless it has `never_discard_temporary`. One that's
  already playing is dropped when its spot leaves the screen. Permanent
  effects are created and kept off screen too. A command's own `duration`
  makes its effect count as temporary here, even for a permanent VFX.
  [code]
- With fog of war on, a temporary effect isn't created where the local
  player can't see; with fog off (`FogOfWar` 0 in `SVars.tgi`, or fog
  revealed by the cheat) it is.
  Permanent effects attached to a thing ignore fog. [code]
- `nightonly` effects show only at night, as [lighting](lighting.md#night)
  defines it. [code]

## Where an effect appears

- `at` picks the spot: [code]
  - `SELF` / `TARGET_THING`: the thing raising the event or its target. With
    a `location` (a named point in the model, such as `gun_ae_01`,
    `cannon_ae_01`, `exhaust_point_01`, `explosion_point_death01` or a bone
    such as `spine`), at that point; without, at the thing's origin.
  - `SOURCE_GROUND` / `TARGET_GROUND`: the ground under the thing or its
    target.
  - `TARGET_HIT_POSITION`: a point on the target's outside, on the side
    facing the attacker (below).
  - `SOURCE_ABILITY_POSITION`: the attacker's firing point, where its
    projectiles start (below). No stock command uses it.
  - `MAP_CENTER`: the middle of the map, on the ground.
- A `location` names a point in the thing's current model. `Game/bones.tgi`
  turns some ids into node names (`position` is `Scene Root`, the same as
  no location). The name is looked for among the model's nodes, searching
  down from the top, each node before its children, and the first match
  wins. Only nodes count: a shape (mesh) or a property (material, texture)
  with that name is passed over, and so is a model with no such node.
  Names must match exactly, case included: `base` doesn't find a node
  called `Base`. [code]
- Every model the game loads gets one node it didn't have in the file: its
  top node is renamed `base` and put under a new top node, which takes the
  old top's name (`Scene Root`) and has no offset of its own. So `base`
  is in every model: the file's own top node, placed as the file places
  it (normally the model's origin), and it turns and moves with the model;
  `position` is the new top node above it. A node already called `base`
  deeper in the file (the USA artillery brigade's models have one) is
  never reached, since the renamed top is found first. [code]
- `TARGET_HIT_POSITION` needs a living target with a model; without one
  there's no effect. [code] The point is:
  - Across the ground: on the edge of the target's footprint (its circle,
    or its rectangle), going out from its centre in the direction opposite
    to the way the attacker faces. An attacker faces what it shoots, so in
    practice it's the side toward the attacker. With no attacker, the
    target's centre. [code] Which shape field gives the footprint, and
    exactly where on a rectangle's edge the point falls, aren't settled.
    [guess]
  - Height: `ProjectileTargetHeightRatio` (0.65, `SVars.tgi`) of the way up
    the attacker's model, held between the bottom and top of the target's
    model. So a tank's hit on a tall building lands at two thirds of the
    tank's height, and a tall ship's hit on a low jeep lands on the jeep's
    top. With no attacker, 0.65 of the way up the target. [code]
- `SOURCE_ABILITY_POSITION` is on the attacker. It's worked out from the
  `[Ability]` entry in the attacker's `[PerformerComponent]` (the block
  around the `[AbilityDefinition]`, where `turret_ids`, `spawn_bone_ids`,
  `spawn_position` and `reload_time` sit; see
  `Templates/template_actorability.tgi`): [code]
  - If the entry names a turret (`turret_ids`) and the attacker has a
    model: at that turret's node in the model, with the command's
    `translation` turned with the turret.
  - Else, if the attack fires projectiles: `spawn_position` from the
    attacker's origin, turned with the attacker. The offset is in the
    model's own axes, where forward is negative y (as the template says).
    Stock projectile attacks leave it at `0,0,0` and use
    `spawn_bone_ids`, so for them this spot is the attacker's origin.
  - Else, for a thing that can attack, at the front edge of its footprint,
    straight ahead, `attack_height` (from its `[PerformerComponent]`, 1.45
    by default) above it; for anything else, its origin.
  - `spawn_bone_ids` plays no part here; it's where projectiles leave (see
    [combat](combat.md#projectiles)).
  - It keeps that height above the ground where it's drawn. If the attacker
    has gone, the same is worked out from where it was.
- `attached = true` (the default) makes the effect follow its thing as it
  moves; `false` leaves it where it started. Only `SELF` and
  `TARGET_THING` can be attached; the game detaches any other. [code]
- A `location` gives an effect its position, and one direction, never
  the point's whole orientation. Every effect is created with a heading,
  a single direction, the same way attached or not: [code]
  - with no `location`, the thing's facing;
  - with a `location`, the command's `orientation` (default (0, −1, 0),
    the model's forward) turned by that point's orientation at that
    moment, the thing's own turn included. For a bone whose axes are
    turned from the model's this is some other level direction, not the
    thing's facing.
- A model effect is stood up from that heading alone: it is turned (and
  pitched, if the heading isn't level) to lie along the heading, and its
  roll is taken from the world's up, not from the point. So an effect on
  a bone lying on its side still stands upright; only a heading that
  points up or down a slope tips it, and one pointing straight up or
  down is ignored, leaving the model as it was made. [code] The
  paratroopers' closing canopy, on the `neck`, stands upright over the
  man. [game]
- An attached model effect with `gyroscope = true` (the default) follows
  its point's position only. Its orientation is fixed in the world at
  what the heading gave it when it was created: it doesn't turn with the
  thing or with the bone afterwards, however they move. With
  `gyroscope = false` (stock: the status bars) it hangs straight off the
  point and takes the point's whole orientation, turning and tilting
  with it. [code] The USSR Corps HQ's lamp on `(2v2)Berlin` at night
  points its beam along its point's −y as this says
  ([lighting](lighting.md#where-the-night-lights-show)). [game]
- A detached effect with a `location` is placed and turned the same way
  once, and then left. [code]
- A missing source or target (dead before the effect played) means no
  effect. [code]
- A `location` the model doesn't have: a detached effect plays as though
  there were no location, at the thing's spot. [code] An attached one finds
  nothing to hang from and doesn't show at all. `base` and `position` are
  never missing (see above). [code]

## Height

- Effects at a ground spot (`SOURCE_GROUND`, `TARGET_GROUND`,
  `MAP_CENTER`) stand on the land's height there, not the water surface.
  [code]
- `SELF` / `TARGET_THING` without a `location`, detached: on the ground
  under the thing, on the ground or the waterline (whichever is higher) for
  a `water_based` thing, and at the thing's own height for a `flying` one.
  [code]
- With a `location`, the effect is at that point of the model, at whatever
  height the model stands. [code]
- `TARGET_HIT_POSITION` and `SOURCE_ABILITY_POSITION` heights are worked
  out above where the thing stands (see [Where an effect
  appears](#where-an-effect-appears)), so a hit on a unit on a hill is drawn
  on the unit, not below it. [code]
- `translation` on a command offsets the spot; `orientation` overrides the
  effect's facing. [code]

## Size

- A model effect is drawn at its model's own size in metres: it doesn't
  grow with the thing it plays on (`attached_scale` is 0 throughout stock
  model effects). [code] An attached effect probably takes on the scale of
  the thing it's attached to. [guess]
- Effects are part of the 3D scene, so they shrink and grow with the
  camera zoom like the models around them, except VFX with
  `constant_screen_size`. [guess] `uiscale` doesn't change them. [guess]
- `random_orientation` in a VFX's `[Flags]` (most explosions) gives each
  one a random turn about the vertical. [guess]

## How long

- Each VFX lasts one of four ways: [code]
  - `permanent = true`: until stopped by a `FXC_StopVFX` with the same id,
    or until its thing goes. A stop reaches only attached effects that the
    same thing's commands created.
  - A `duration` (on the VFX, or on the command, which overrides it):
    that many seconds, with the model's animation stretched or squeezed to
    play exactly once over that time. Its particle systems run on the same
    stretched clock: an effect given twice its model's length emits at half
    the rate over its emitters' stretched on-times, and its particles move,
    grow, fade and die at half speed. [code]
  - Temporary with no duration: the length of the model's own animation,
    played once. A model that doesn't animate ends at once.
  - Temporary effects without a model (tracers, text): a length the game
    works out per kind; a tracer flies at `speed` up to `length`. [guess]
- So a stock explosion ends when its model's animation does: the emitter's
  own keys stop it emitting, and the effect is gone when the model's last
  key passes, whether or not every particle has faded. [guess]
- Some permanent effects are stopped for the thing by the game, with no
  `FXC_StopVFX` in the data: [code]
  - one a `WhenEnter` starts stops when the thing leaves that state;
  - one a `WhenConstructed` starts stops when the building is destroyed,
    unless the command says `stop_on_destructed = false`.
  Either way only an effect whose VFX is `permanent`, started by a
  `FXC_CreateVFX` that gives no `duration` (0, the default) and is
  `attached` (the default; at `SELF` or `TARGET_THING`, since others
  are detached anyway) is stopped. A detached one, one with a
  duration, or a temporary one runs its own course. The same holds for
  effects in a command's chained lists. Stopping ends the effect at
  once, wherever its animation or emission has got to. [code]
  - So in stock data a site's scaffold and its sinking end when
    construction does, and the packing dust when packing does
    ([construction](construction.md#the-site)). [code, game]
- Effects run on game time unless `force_real_time`, which puts them on
  the wall clock. [code] Game time stops while the game is paused and
  speeds up with it. [guess]
- A command in a `Chain` list runs when its parent's effect ends;
  `ChainImmediate` runs alongside it. Both get the parent's source and
  target. [guess]

## Ground decals

A VFX made from `VFXProjectedTexture` (or a template built on it:
`VFXProjectedTextureLit`, `VFXScorchMark`, `AABuildingGroundPlane`,
`VFXDecalDirt` and so on) has no model. It is a flat picture laid on the
ground: scorch marks, the charred patch under a tank wreck, the worn
ground under buildings and some scenery, headlight pools. It is created by
an effect command like any other VFX, so everything above about which
commands play, where and for how long applies. Fields are documented in
`Templates/template_vfx.tgi`.

- The picture is a square `texture_size` world units on a side, centred on
  the effect's spot, showing the whole `texture` once. It lies on the
  terrain and follows its slopes, like paint on the ground; it isn't drawn
  on models, and the things standing on it are drawn over it. [code]
- With `attached_scale = 0` (every stock scorch, wreck and ground patch)
  the size is `texture_size` whatever it is attached to: it doesn't grow
  with the thing or with the thing's stored scale. [code, data]
- Turn: with `random_orientation = false` (the default) the square is
  turned to the facing of the thing it belongs to, and keeps turning with
  it if it's attached. With `random_orientation = true` it gets a random
  turn about the vertical when it's made and keeps it. [code]
  `rotate_degrees_per_sec` spins it steadily. [code]
- Which way round: the texture's top row lies along the edge the thing
  faces towards, and the picture isn't mirrored. With the thing facing
  (fx, fy) in map coordinates, the top edge is the one towards (fx, fy)
  from the centre and the left edge the one towards (-fy, fx); so looking
  straight down with map x to the right and map y up the screen, a thing
  facing up the screen shows the picture as stored. A randomly turned
  decal is the same picture turned, never flipped. [code] The wreck
  patches can't confirm this: each is a ragged, near-round blob centred
  in its texture, with no shape to line up against the wreck. [data]
- Blending: the texture is alpha-blended over the ground (its alpha
  channel gives the patch its ragged edge) and multiplied by `color`.
  `additive = true` adds it to the ground instead (light pools). [code]
- Lighting: by default a decal ignores the sun, so it looks the same day
  and night. `respect_lighting = true` (`VFXProjectedTextureLit`) shades
  it by the scene's light as the terrain is, so it darkens at night with
  the ground around it. A lit decal can't also set `color`. [code]
  Decals are dimmed under fog of war like the ground they lie on. [guess]
- Fading: `decal_fade_time = in, out` in seconds. Opacity rises linearly
  from 0 over `in` when the decal is made and falls linearly to 0 over
  `out` when it ends; 0 means no fade. One that ends while still fading in
  fades out from the opacity it had reached. The fade-out comes after the
  decal's life: a scorch mark with `duration = 15` and a fade of `0,5` is
  at full strength for 15 seconds and then takes 5 more to go. [code]
- A fade-in is skipped when the effect starts at game time exactly 0: the
  decal is at full strength at once. [code]
- Decals aren't kept in a saved game. As a map loads, new or saved, the
  game plays `WhenCreated` again for the things in it and
  `WhenConstructed` for every building that is finished (not one still
  being built), each starting at the game time the map holds. [code] A new
  match starts at time 0, so the dirt under the buildings a map starts
  with is fully shown from the first frame, with no fade. [code] A saved
  game resumes at the time it was saved, so after loading the dirt under
  every finished building fades in again over its 10 seconds. [guess]
- Where decals overlap, `decal_type` sets the order, bottom first:
  `terrain`, `scorch`, `track`, `generic` (the default), `ui`, `shadow`.
  [code]
- Life is the VFX's own: `permanent = true` (the default for projected
  textures) lasts as long as its thing; a temporary one lasts its
  `duration`. Temporary decals with `critical = false` (the scorch marks)
  are subject to the effects slider and to the off-screen and fog rules
  in [Detail, fog and the screen](#detail-fog-and-the-screen), so a shell
  landing off screen leaves no mark. [code]

What stock data lays down: [data]

- Tank wrecks. Each of the ten wreck scenery objects
  (`Doodads/Shared/<nat>_tank01.tgi` and `_tank02.tgi`: `ger_destroyed01`,
  `usa_destroyed02` and so on) has a `View` `WhenCreated` command creating
  a decal defined at the top of the same file (`GERDestroyed01Ground` and
  the like): texture `TanksDead/<NAT>Destroyed0nGround.dds` (256 x 256
  with alpha), `texture_size = 6`, lit, permanent, no fade, type
  `generic`, not randomly turned. So the charred patch is part of the
  wreck: a 6 x 6 square centred on the wreck's origin, turned with the
  wreck, there from the moment the map loads for as long as the wreck
  stands. Each texture is painted to fit its own wreck model. The same
  command list starts the wreck's fire or smoke (`tank_death_smoke`,
  `tank_death_fire`) at a `damage_point` of the model.
- Shell and bomb marks. A projectile's `WhenDestroyed` commands (in
  `Projectiles/*.tgi`) create a `VFXScorchMark` from
  `Effects/scorch_marks.tgi` where it lands, detached, one command per
  terrain group so it plays on dirt, desert and snow alike:
  `scorch_mark_small_vfx` (3 units; light tank shots, grenades),
  `scorch_mark_medium_vfx` (7; medium and heavy tank shots, the Goliath),
  `scorch_mark_large_vfx` (10; bombs, napalm) and
  `scorch_mark_extra_large_vfx` (24; unused). They are unlit, randomly
  turned, type `scorch`, and last 15 seconds plus a 5 second fade-out.
  `atomic_scorch` (50) and `v1_scorch` (18) are the same with a 15 second
  fade-out and `critical = true`.
- Nothing leaves a mark when a unit dies or a building is destroyed: the
  vehicle debris and building collapse effects are models only. Campaign
  buildings (`Buildings/Campaign/*.tgi`) have ground patches of type
  `scorch` of their own, and stock buildings their `AABuildingGroundPlane`
  dirt (type `terrain`, fading in over 10 seconds and out over 1).
- Other users: the `ground_*_vfx_*` dirt, desert and snow patches in
  `Effects/ground_decals.tgi`, obstacles and fences in `Doodads/`, and
  the headlight pools in `Effects/headlights.tgi`.

## Tints

A `VFXTintModifier` (such as `out_of_supply_vfx` and `under_siege_vfx` on
buildings, or `banzai_charge_glow`) has no model of its own: attached to a
unit or building, it changes the colour of that thing's model.

- Every tint is permanent: it lasts until a `FXC_StopVFX` with its id ends
  it, or its thing goes. [code, data]
- How strongly it shows is a level from 0 (no tint) to `tint_max`
  (default 1; above 1 is a data error), and the level drives the colour:
  [code]
  - With `lirp = false` (the default, and every stock building tint) the
    tint multiplies the model's colours, channel by channel, by a colour
    that runs in a straight line from white at level 0 to `color` at
    level 1: white − (white − `color`) × level. It only ever darkens or
    shifts the hue; at full level a building under `out_of_supply_vfx`
    shows its own colours × (0.5, 0.55, 0.5). The data's comment "go to
    absolute white" is misleading. All of the model's material colours
    are multiplied alike (lit, ambient, highlight and glow), through every
    part of the model.
  - With `lirp = true` the model's colour is instead blended towards
    `color` itself by the level, so a tint can brighten
    (`territory_focus_vfx` lifts towards white with `tint_max = 0.3`).
    [code, data]
- Over time the level goes: [code]
  1. When the tint starts, it rises in a straight line from 0 at
     1 / `enhance_duration` per second until it reaches `tint_max` (so
     over `tint_max` × `enhance_duration` seconds). With
     `enhance_duration = 0` it jumps straight to `tint_max`.
  2. Then, if `fade_duration` is above 0, it falls in a straight line at
     1 / `fade_duration` per second back to 0. With `fade_duration = 0`
     (the default) it stays at `tint_max` for as long as the tint lasts.
  3. When it reaches 0: with `repeating = true` it starts rising again at
     once (the next cycle begins from 0, with no pause at either end);
     without, it stays at 0, the model looking untinted, until the tint is
     stopped.
  There's no easing anywhere: both ramps are linear, and the turn at the
  top and bottom is sharp. `out_of_supply_vfx` thus takes 1.0 s from
  plain to full grey-green and 0.8 s back, a 1.8 s pulse; `under_siege_vfx`
  0.9 s up and 0.7 s down.
- A tint that doesn't repeat can't have a `fade_duration` above 0 unless
  `flash_period` is above 0 too: loading the data refuses it. [code] That
  is all `flash_period` does to a tint; it doesn't make one blink. [code]
- The clock: a tint with `force_real_time = true` (every stock building
  tint) runs on the wall clock, so it keeps pulsing at the same pace while
  the game is paused and whatever the game speed. [code] Others run on game
  time. [code]
- Starting and stopping: [code]
  - A tint always starts from level 0 and rises, so a building going out
    of supply starts plain and turns grey-green over the first second.
  - Stopping it (a building coming back into supply) takes it off at
    once: the model is back to its own colours that frame, with no fade
    out, wherever the cycle was.
  - The tint restarts its cycle from 0 when its thing comes back into
    view, as well as when it's first made. [guess]
- One tint at a time per thing: [code]
  - Among the tints on one thing, only the one with the highest `priority`
    shows; the rest wait. A new tint takes over only with a strictly
    higher priority than the one showing; with an equal or lower one it
    waits.
  - When the showing tint stops, the highest-priority waiting tint takes
    over (on a tie, whichever the thing holds first).
  - Tints don't mix with each other, but a tint does combine with an
    alpha (`VFXAlphaModifier`) or glow (`VFXEmitModifier`) modifier on the
    same thing, each kind having its own one-at-a-time slot.
  - So a building both out of supply (priority 0) and under siege
    (priority 1) shows only the siege's red pulse, whichever began first.
    When the siege ends, the out-of-supply pulse comes back. While waiting
    its cycle wasn't reset, and kept counting on its own clock: if the
    ramp it was on would have finished meanwhile, it starts the next ramp
    from that moment (falling from `tint_max` if it was rising, rising
    from 0 if it was falling), otherwise it carries on where its ramp has
    got to.

## Sounds

- `FXC_PlaySFX` commands sit in the same event lists as the effect
  commands and play on the same events. Each plays its `audio_ids` once
  (`looping` is turned off with a data warning). [code]
- Where it's heard from, by `at` (`SELF` when not given): [code]
  - `SELF` / `TARGET_THING`: from a unit or building that can attack, the
    sound goes with the thing as it moves; from any other thing (a
    projectile, say), at the spot where the thing is as the sound starts.
    If the thing has gone, at its spot when the event happened.
  - `SOURCE_GROUND` and `SOURCE_ABILITY_POSITION`: at the source's spot
    when the event happened; `TARGET_GROUND` and `TARGET_HIT_POSITION`: at
    the target's.
  - `MAP_CENTER`: the middle of the map.
- Unlike visual effects, a sound command isn't held back by the detail
  setting, the fog of war or its spot being off screen. [code] How the
  sound then fades or drops out with distance from the camera isn't
  settled. [guess]

## Open questions

- Is a ground decal clipped to its square, or does the texture's edge
  smear outwards over the terrain cells the square only partly covers?
  The stock textures are transparent at their edges, so it doesn't show.
  (analysis, 2026-11-12)
- After loading a saved game, does the dirt under finished buildings
  fade in again over 10 seconds, as the rules suggest, or is it there at
  once? Saving next to an unpacked building and loading would show it.
  (implementer, 2026-11-12)
- Is a turned decal's picture the right way round as described under
  Ground decals? A building's ground texture with a clear shape (a corps
  HQ's, say) seen next to its building would confirm it. (implementer,
  2026-11-12)
- When a thing with an attached, fading decal goes, does the decal stay
  behind to fade out, or vanish with it? (analysis, 2026-11-12)

- What ends `building_pack`? It's permanent and loops, and nothing stops
  it. Does it vanish when the building swaps from its packing or unpacking
  model to its standing one, or does it go on until the building does?
  Watching an unpack through to the end would settle it. (analysis,
  2026-11-08)
- Does a pulsing tint start its cycle over when its building scrolls back
  on screen, or only when it's first made? Scroll a building out of
  supply off screen and back, watching whether it comes back plain.
  (reader, 2026-11-10)
