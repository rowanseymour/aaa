# Triggers

How a map's triggers run: when each one is tested, how its events,
conditions and effects combine, how a parameter gets its value, and what
each event, condition and effect the shipped maps use does, multiplayer
and campaign. How a campaign's missions follow one another is in
[campaigns](campaigns.md). What a
trigger holds is the TRIGGERS section as `rmp.ksy` lays it out (a list of
triggers, each with `all_conditions`, `active`, `repeats`,
`activation_time`, and lists of `events`, `conditions` and `effects`,
each an element naming a template with its `parameters`; then the
section's `variables`).

Games: traced in AA and Kohan II, which share the trigger engine; the
element and source numbering in this spec is by name, which both games'
data (`Game/trigger_elements.tgi`) and maps use. Kohan II adds the hero
elements; AA's has the game counter.

## When triggers are tested

- Every trigger that is `active` is tested once on every simulation
  tick, during the tick, after the tick's game time has moved on. So the
  first test is on the first tick of the match, a tick after game time 0.
  [code]
- Triggers are tested in the order the TRIGGERS section lists them. A
  trigger's effects take hold at once, so a trigger later in the list
  sees what an earlier one did in the same tick (a variable it set, a
  trigger it switched on or off). [code]
- A trigger that isn't `active` is never tested, whatever its events or
  conditions, until an effect switches it on (Switching triggers, below).
  [code]

## Testing a trigger

- With events: the trigger goes ahead only if at least one of its events
  happened since the last tick (Events, below); events are always "any
  one". With no events it always goes ahead. [code]
- Conditions: with none, the effects run. Otherwise every condition is
  evaluated; when `all_conditions` is 1 all must hold, when it's 0 any one
  is enough. There's no negation: a condition is either true or false,
  and the "not" forms are separate templates (`actor_does_not_have_state`
  beside `actor_has_state`). [code]
- Effects run in their listed order, every one of them, even if one
  fails (an effect fails when a parameter names nothing, a missing
  kingdom say); a failed effect simply does nothing. [code]
- After the effects have run, a trigger with `repeats` stays active and
  its activation time is set to the current game time; one without is
  switched off for good (unless an effect switches it on again). A
  trigger whose conditions fail stays as it was and is tested again next
  tick. [code]
- A trigger's activation time is the game time it was last switched on
  or, for a repeating one, last fired. The map's saved value (0 in every
  shipped map) is the starting value, so a trigger active from the start
  counts from game time 0. `activation_time` conditions compare against
  it (below). [code, data]
- So the shipped "Map Init" triggers (active, not repeating, no events,
  no conditions) run their effects once, on the first tick, and switch
  off. The ambient-sound triggers (active, repeating, one condition
  "activation_time >= N") fire N seconds into the match and then about
  every N seconds, each firing restarting the count. [code, data]

## Areas

An area is a region placed in the map (an `area` record in
DYNAMIC_THINGS) that triggers test things against. Its own template
(`Template Area`) makes it invisible, not blocking and not targetable.

### Its shape

- The area's `shape` is 1 for a circle or 3 for a box; the shipped maps
  use only these. Other values are the other shape kinds (0 a point, 2 a
  square) and aren't used. [code, data]
- A circle is `radius` around the area's position. The bounds are kept
  in the record but play no part (some shipped circles carry left-over
  bounds from being a box once). [code, data]
- A box is its four bounds (min x, min y, max x, max y) measured from
  the area's position, not absolute. The shipped boxes are centred, the
  min bounds the negatives of the max ones, and their `radius` is half
  the box's diagonal (a 76.2 by 4 box has `radius` 38.15). [code, data]
- Everything is measured flat, by x and y; height plays no part. [code]

### Who is in an area

- The area keeps a list of the things in it, rechecked every tick. The
  check runs as part of the tick's world update, before that tick's
  trigger pass, so an entry is heard on the tick it happens
  ([Events](#events)). [code]
- A thing can be in an area when it is an actor that can be targeted
  (its template's `targetable` flag) and has health or is a regiment:
  [code, data]
  - Units, buildings and building sites count, whoever owns them,
    independents included. [code, data]
  - A regiment counts as a thing of its own, apart from its units (the
    regiment template is `targetable`). [code, data]
  - Aircraft in flight count like any other unit, the delivery transport
    among them, as do men coming down under parachutes. [code, data]
  - Things whose template isn't `targetable` never count: areas
    themselves, scenery, trailers, ambient animals (`AAAmbientUnit`).
    [data]
  - A regiment still on its way in a transport doesn't exist yet, so it
    can't be in an area until the transport starts putting it down
    ([deliveries](deliveries.md#special-operations-and-triggers)). [code]
  - A dying thing stops being targetable the moment it starts to die, so
    it leaves the list on the next check and can't enter one. A thing
    taken off the map is gone from it too. [code]
- The test for being in it is looser than the area's outline. A thing is
  in the area when both hold: [code]
  - its centre is no further from the area's position than the area's
    `radius` plus the thing's soft radius (the bounding radius of its
    soft shape, [objects](objects.md#shapes)); and
  - its soft shape covers some map tile the area covers. For a circle
    those are the tiles under the square from its position minus
    `radius` to plus `radius`; for a box the tiles under the box, taken
    square to the map (the area's facing doesn't turn it). [code, guess]
  - So a thing near a box's corner, or just outside a circle, can count
    without its shape touching the area. [code]
- A regiment's place is its own position, which follows its leader's
  ([movement](movement.md)), with its soft radius of 1 (the regiment
  template's `soft_radius`). So a regiment is in an area by where its
  position is, not by its units: its first units can be inside before it
  is, and its stragglers outside after it is. [code, data]

### Entering

- A thing enters an area when a check finds it in the area and it isn't
  on the area's list yet. It's added to the list and an entry is posted
  for `actor_enters_area`. [code]
- A thing on the list that a check doesn't find is dropped from it. So a
  thing that leaves and comes back enters again, each time. [code]
- A regiment and each of its units are separate things: each enters on
  its own, when its own place first counts. [code]
- At the start of a match the lists are empty (the editor saves them
  empty), so everything standing in an area enters it on the first tick,
  heard by that tick's triggers. A saved game keeps each list, so loading
  it posts no entries for things already inside. [code, data]
- A thing that changes into another and hands over its id (a truck
  unpacking into its building, a building packing into its truck, an
  upgrade) is the same thing to an area. The new thing takes the old
  one's place on the list, so it doesn't enter again if it's still in
  the area at the next check. If the change takes it out of reach (the
  building stands at an offset from the truck, and its size differs),
  it leaves; if it brings it into reach, it enters then, as the new
  thing. [code]

## Events

- An event element is true when what it names happened since the last
  trigger pass. [code]
- The game keeps one list of the happenings an event can name: a thing
  destroyed, a thing captured, a thing entering an area, a building
  begun or finished, a recruit made, a listened-for script command run.
  Each is added as it happens. The list is emptied once per tick, at the
  end of the trigger pass. So every trigger tested in a pass sees
  everything that happened since the previous pass. [code]
- Most happenings come before the pass: the tick's orders, movement,
  combat, recruiting and construction run first, and things that died
  that tick are finished off just before it. So a trigger hears of them
  on the same tick. [code]
- Something a trigger's effect causes during the pass (a building site
  made, say) is heard only by triggers later in the list, that same pass.
  Triggers earlier in the list never hear of it. A thing an effect kills
  dies after the pass, so its destruction is heard on the next tick.
  [code]
- An event happening while its trigger is switched off isn't kept for
  later. [code]
- Every event element in a trigger is tested, not just up to the first
  one that holds, so the event things from all of them are gathered.
  [code]
- An event can name a particular thing (`mi_actor_id`) or any thing
  (`mi_actor_any`). With "any", every thing that matched becomes one of
  the trigger's event things; with a particular one, that thing does.
  [code]
- The trigger is then run once for each event thing, in turn: conditions
  are tested and effects run with `mi_actor_event` meaning that thing. A
  trigger that doesn't repeat stops after the first run whose conditions
  pass. [code]
- `actor_enters_area` (`ma_actor`, `ma_area`): the thing entered the
  area (by its id `mi_area`) since the last pass, as
  [Entering](#entering) says. Used by AA's `(2v2)Moscow` and
  `(4FFA)TropicalCruise`, and by the tutorial. [code, data]
  - A named thing fires only on its own entry. Naming a regiment
    (Tutorial 1's) waits for the regiment's position to come in; its
    units entering first doesn't fire it. [code, data]
  - With `mi_actor_any`, every thing that entered becomes an event thing,
    each once. A regiment walking in gives the regiment and each of its
    units, as each enters, usually over several ticks. [code]
  - Ownership plays no part: things of any side enter. Test the owner
    with a condition on `mi_actor_event`. [code]
- `actor_captured_by_kingdom` (`ma_actor`, `ma_kingdom`): the thing
  changed hands to that kingdom by capture since the last pass: taken by
  damage, or a city taken by presence
  ([buildings](buildings.md)). A change of side by
  `actor_transfer_control_to_kingdom`, or by any other effect, doesn't
  count. [code]
  - The thing must be named (`mi_actor_id`, `mi_actor_named`); there's no
    "any thing" form, and `mi_actor_any` never fires. The event thing is
    the captured thing. [code]
  - If the same thing changed hands twice in one tick, only its latest
    capture is looked at: it fires only if that one was by the named
    kingdom. [code]
  - Kohan II: a capturable structure that belongs to a settlement counts
    as its settlement being captured. [code, guess]
  - The shipped campaigns use it more than any other event (over 300
    times), always with `mi_actor_id` and `mi_kingdom_named`. [data]
- `actor_destroys_actor` (`ma_actor` the killer, `ma_actor` the
  destroyed): a thing died since the last pass. Every death counts,
  regiments' included (a regiment dies with its last unit). The killer
  is whatever last damaged the dying thing; a death with no damager (no
  one ever hit it) has no killer. [code]
  - Destroyed named, killer any (the form the campaigns use): fires if
    the named thing is now dying or gone and it was killed by something;
    the event thing is the killer. [code]
  - Both named: fires if the destroyed thing is dying or gone, the named
    killer still exists, and the killer was that thing or, when it's a
    regiment, one of its units. [code]
  - Killer named, destroyed any: fires if the named thing killed
    anything; the event thing is the killer itself (not what it
    killed). Here a regiment doesn't stand for its units: the kill must
    be credited to exactly that thing. [code]
  - Both any: fires on any death; every killer and every destroyed thing
    of the tick become event things. [code]
- `kingdom_begins_construction_on_actor` (`ma_kingdom`,
  `ma_actor_data`): a building site of that template
  (`mi_actor_data_named`, e.g. `usa_bunker`) was placed for that side.
  `mi_actor_data_any` matches any template. The template must be the
  site's own, exactly. There's no event thing. [code]
- `kingdom_finishes_construction_on_actor` (`ma_kingdom`,
  `ma_actor_data`): a thing of that template was finished for that side:
  a building site completed, an upgrade completed, or a packed building
  finished unpacking (so `rus_hq_infantry` fires when a Corps HQ unpacks
  from its truck). The side is the thing's owner. No event thing. [code,
  data]
- `kingdom_recruits_actor` (`ma_kingdom`, `ma_actor_data`): that side
  recruited something of that template (a regiment such as
  `regiment_rus_infantry`, or a truck such as `rus_truck_ammo`): heard
  once per recruit, when the recruit is made. For one that arrives by
  transport that's when the transport is made and sets off, not when it
  lands: the delivery isn't heard again, and a recruit whose transport
  can't be made (and is refunded) isn't heard at all. Things made by
  triggers don't count. No event thing. [code]
- `event_script_heard` (`ma_script_command`, `mi_script_command`): the
  player ran that interface command since the last pass, while the map
  was listening for it. [code]
  - A map listens with `effect_ui_script_listen` (the command, and on or
    off); the commands listened for are kept with the saved match.
    [code]
  - The command is the text an interface button or key runs (`ActorCommand
    move`, `ActorDataSelectCommand regiment_rus_infantry`, `FormationChange
    0 1`, `MessageLogToggle`, `UseFactionAbility 0`). It must match the
    listened text whole, ignoring case. [code, data]
  - A listened-for command is sent to every machine as an order before
    it's heard, so it's heard on the tick that order is carried out.
    It still runs as usual. [code, guess]
- `cutscene_complete` (`mi_cutscene`): the named video, started by
  `play_cutscene`, has finished playing (players' machines report it as
  an order). No shipped AA map uses it. [code, guess]

## Parameter values

- A parameter's `source` decides where its value comes from; `set`
  (whether the map author filled it in) makes no difference to the game,
  which uses the stored value either way. Shipped maps leave
  `ma_compare` unset at 3, which the game reads as "at least". [code,
  data]
- Numbers (`ma_value` and the like): `mi_int` is the stored whole
  number, `mi_fixed` the stored decimal, `mi_variable` the current value
  of the named trigger variable (0 if there's none of that name). Any
  other source gives 0. [code]
- `mi_compare` is how two numbers are compared: 0 less than, 1 at most,
  2 equal, 3 at least, 4 more than, 5 not equal. A comparison given by
  any other source means "equal". [code]
- Kingdoms (`ma_kingdom`): [code]
  - `mi_kingdom_named`: the kingdom of that name in the map (`kingdom_0`,
    `independent_2`).
  - `mi_kingdom_id`: the kingdom whose thing id is stored.
  - `mi_kingdom_event` and `mi_kingdom_condition`: a kingdom an earlier
    parameter picked (below). [code]
  - `mi_kingdom_any`, `mi_kingdom_all` and the ally, enemy and team
    sources aren't read by any of the elements traced here, apart from
    `kingdom_gains_technology` with `mi_kingdom_all`
    ([Technologies](#technologies)): they give no kingdom, so the element
    fails. No shipped map uses them. [code, data]
- Things (`ma_actor`): `mi_actor_event`, the trigger's current event
  thing (Events); `mi_actor_condition`, the thing a condition picked.
  `mi_actor_any` only means something to events. [code]
  - `mi_actor_id`: the thing whose `id` (the one in its own saved
    record) is stored. A regiment is a thing with an id of its own, as
    is each of its units, so the id names exactly one of them. It's
    found as long as the thing is still in the world, even while it's
    dying; once it has been taken off the map the parameter names
    nothing ([combat](combat.md#leaving-the-map) says when that is). A
    new thing takes over an old one's id only by packing, unpacking or
    upgrading into it. [code]
  - `mi_actor_named`: the thing that bears that name: the `name` in its
    saved record (empty for most things), or the one an effect such as
    `actor_create_transported_named` gave it as it was made. Names match
    regardless of case. When two things bear the same name, the one that
    came into being last (later in the map's list, or made later in play)
    is the one found. Only actors are found (units, regiments, buildings;
    not areas or cameras), dying or not, until taken off the map like an
    id. A name nobody bears names nothing. `actor_create_named` won't
    give a name that a thing in the world already bears
    ([Making and removing things](#making-and-removing-things)). [code]
  - A thing that hasn't come into being yet can't be named either way: a
    regiment still on its way in a transport exists only once the
    transport starts putting it down
    ([deliveries](deliveries.md#special-operations-and-triggers)).
    [code]
- A kingdom picked by an event's parameter becomes the event kingdom
  (`mi_kingdom_event`); one picked by a condition's parameter becomes
  the condition kingdom (`mi_kingdom_condition`). Kingdoms that effects
  pick aren't remembered. [code]
- Each is overwritten only by the next parameter of its kind. Nothing
  clears it between triggers or ticks. So a trigger whose events name no
  kingdom sees, as `mi_kingdom_event`, the last one another trigger
  named. [code]
- Conditions are tested in order, so a condition's `mi_kingdom_condition`
  is the kingdom an earlier condition named. In effects, it's the one
  named by the last condition tested. [code]
- A thing a condition's parameter names becomes the condition thing
  (`mi_actor_condition`), as does the thing `actor_inside_area` finds
  with "any". It's forgotten once the trigger has been tested, so it
  doesn't carry to the next trigger. [code]
- Heroes (`ma_hero`, Kohan II): `mi_hero_named` names a hero by its id;
  `mi_hero_random` asks the effect to pick one (below). [code]
- Templates (`mi_actor_data_named`), sounds (`mi_sound`), objectives
  (`mi_objective`), suns (`mi_sun`) and other named things are looked up
  by their id; a name that matches nothing makes the element fail.
  [code]
- `mi_location` is a stored map position; `mi_trigger` the stored id of
  another trigger in the map. [code, data]

## Conditions the shipped maps use

- `activation_time` (`ma_compare`, `ma_value`): game time since this
  trigger's activation time, compared with the value. Every shipped
  Kohan II multiplayer map uses it, always as "at least". [code, data]
- `true`: always holds. [code]
- `value_compare_value` (`ma_value`, `ma_compare`, `ma_value`): compares
  two numbers, usually a variable with a constant. [code]
- `actor_is_type_actor_data` (`ma_actor`, `ma_actor_data`): the thing is
  of that template (`mi_actor_data_named`, e.g. `ger_depot_oil`). AA's
  `(4FFA)TropicalCruise` uses it on the event thing. [code, data]
- `kingdom_has_compare_value_primary_resource` (`ma_kingdom`,
  `ma_compare`, `ma_value`): the kingdom's stockpile of the primary
  resource (AA's `money`), rounded to the nearest whole number, compared
  with the value. `(4FFA)TropicalCruise` wins a side at 2000. [code,
  data]

## Effects the shipped multiplayer maps use

Kohan II's multiplayer maps use only the first three; the rest are AA's.

- `kingdom_gains_hero` (`ma_kingdom`, `ma_hero`), Kohan II: [code]
  - with `mi_hero_named`, the side gets that hero in its pool, unless it
    already holds it (then the effect fails);
  - with `mi_hero_random`, the side gets one hero picked at random, all
    equally likely, from every hero whose `random_hero` is set, that the
    side doesn't hold, and, if the side has a faction, whose factions
    include it [code, guess]. If there's none, it gets nothing and the
    effect fails. The pick uses the match's shared random numbers, so
    every machine picks the same.
  - The hero goes into the pool, not onto the map
    ([companies](companies.md#heroes)).
- `kingdom_loses_hero` (`ma_kingdom`, `ma_hero`), Kohan II: [code]
  - with `mi_hero_named`, the hero leaves the side's pool, but only while
    it's in the pool (available, or dead and healing); a hero leading a
    company on the map isn't taken and the effect fails;
  - with `mi_hero_random`, one hero is picked at random from the side's
    `random_hero` heroes that are in the pool, as above, and taken.
    [code, guess]
- `play_sound` (`ma_sound`): plays that sound (`mi_sound`, an
  `[Audio]` id such as `arctic_bed`) for every player. [code, data]
- `play_sound_at_location` (`ma_sound`, `ma_location`): the same, placed
  at that point on the map. [code]
- Either way every player hears it, whatever the sound's `who_flags`: a
  trigger's sound belongs to no side. How loud it is follows the sound's
  own definition ([sounds](sounds.md)): one without `WORLD` in its
  `control_flags` (the shipped ambient beds, `company_engaged_sfx`) plays
  at its volume for everyone, wherever they look; a `WORLD` sound at a
  place is heard only by a player whose view is within the far end of its
  `range` of that place, quieter with distance, unless its `world_flags`
  has `GLOBAL`. [code, data]
- `kingdom_gains_objective` and `objective_complete_for_kingdom`: in
  [objectives](objectives.md). `kingdom_mission_won`: in
  [outcome](outcome.md#triggers). [code]
- `global_set_sun_colors` (`ma_sun`, `ma_value`): changes the map's
  lighting to that sun definition (`sun_night`, `sun_dusk`), over the
  value in seconds (0 at once; AA's maps use 3600 to change slowly
  through an hour); see [lighting](lighting.md#changes-during-play).
  [code, data, guess]
- `actor_moves_to_location` (`ma_actor`, `ma_location`, `ma_value`): the
  thing, if it can move (it moves itself or is a regiment), is given a
  move order there, as in [Orders](#orders). The value is the speed to
  move at, as a fraction of the thing's top speed (0.5 = half speed);
  0 means top speed, and a regiment never goes faster than its top
  speed. The multiplayer maps give 0; two campaign intros move slower
  (`allies01_el_alamein` 0.5, `allies09_battle_of_the_bulge` 0.8).
  [code, data]
  - The fraction is of the top speed at the moment of the order: a lone
    unit's own, a regiment's (its slowest member's,
    [movement](movement.md#a-regiments-speed)). A regiment hands the
    slower speed to every member, as for any move that carries a speed,
    so the whole regiment keeps formation at it. [code]
  - It holds for the whole of that move: through every fight on the way
    (the move resumes at the same speed when a fight ends) until it
    arrives or a new order replaces the move. Any later order, a
    person's or a trigger's, brings its own speed: a person's is always
    top speed. [code; guess: that a regiment's speed falling below the
    order's during the move lowers it]
  - Fails if the thing names nothing or can't move, or if the order is
    refused (below); otherwise succeeds. [code]
- `actor_transfer_control_to_kingdom` (`ma_actor`, `ma_kingdom`): the
  thing changes side to that kingdom, as a capture does. [code]
- `sai_disabled_for_kingdom` and `sai_disabled_for_actor`: in
  [Computer control](#computer-control).

## Morale and experience

Both games. Neither effect cares where the thing is or what it's doing:
on the move, fighting, routing or aboard a transport. A regiment whose
last unit has died is gone before the triggers are next tested
([combat](combat.md#leaving-the-map)); one killed during the trigger
pass itself is still found by later triggers that tick, and while dying
can still gain morale and experience (passing the experience on to its
side) but loses no morale. [code]

- `actor_modify_morale` (`ma_actor`, `ma_value`): adds the value to a
  regiment's morale; a negative value takes it away. [code]
  - The regiment is the thing itself, or, for a unit (or an unpacked
    building, or a Kohan II hero) that is a member of one, its regiment.
    Changing a unit's morale changes its whole regiment's. [code]
  - A thing that is neither a regiment nor in one (a lone building, a
    bunker) has no morale: the effect fails and nothing changes. So does
    a value of exactly 0, or a parameter that names nothing. [code]
  - A gain or loss then follows the rules for any other gain or loss
    ([morale](morale.md#losing-morale)): kept within 0 and the maximum,
    routing at once on falling to or below the breakpoint, the
    fall-straight-to-0 rule, no loss while unpacked. BERSERK doesn't stop
    it. [code]
- `actor_gains_experience` (`ma_actor`, `ma_value`): the value is
  added to the regiment's experience as a gain earned in play would be,
  with all that follows: new levels and their message, the side's share
  in AA, and in Kohan II the company's hero
  ([morale](morale.md#a-regiments-experience), [special
  operations](special-ops.md#earning-experience)). [code]
  - Only a regiment itself takes it. Naming one of its units (a Kohan II
    hero included), or a thing in no regiment, gives nothing to anyone.
    [code]
  - A value below 0.0001, negatives included, gives nothing: a trigger
    can't take experience away. [code]
  - The effect fails only when the parameter names nothing; a thing that
    takes nothing still counts as success. [code]
- `actor_set_experience_max` (`ma_kingdom`, `ma_hero`, `ma_value`),
  Kohan II's maps only, though both games have it: sets a ceiling on
  the named hero's experience (`mi_hero_named`, one of the side's
  heroes, in its pool or leading a company). A value of 0 or less means
  no ceiling, which is where every hero starts. While the hero leads a
  company, each gain it gets is cut back to the ceiling, so experience
  already above it is cut only at its next gain. It fails with
  `mi_hero_random`, or when the side doesn't hold that hero. It has
  nothing to do with regiments or the side's own experience. [code,
  data]

## Technologies

How a side holds a technology, from one grant or several, is under
[research](research.md#how-a-side-gets-and-loses-technologies).

- These elements take a kingdom or a thing, then the technology
  (`ma_technology`), named by its id. Any property can be named, not only
  one marked `is_technology`; restrictions bar only technologies. Each
  succeeds whenever both parameters name something, whether or not
  anything changed. [code, data]
- `kingdom_gains_technology` (`ma_kingdom`, `ma_technology`): gives the
  side a grant of the technology, as research would, but saying
  "technology gained" when it's new to the side. Already having it adds
  a silent grant; a technology barred for the side isn't given and
  nothing is said. With `mi_kingdom_all` every side but the independent
  ones gets a grant, each told for itself. [code]
- `kingdom_loses_technology` (AA; `ma_kingdom`, `ma_technology`): takes
  away one of the side's grants of the technology that came from
  research or from `kingdom_gains_technology`. Those grants are alike, so
  which of them makes no difference; one is taken per run, so a side
  given it twice by trigger needs two. Start technologies, the
  handicap's and buildings' grants are never taken. So: [code]
  - the side loses the technology, with "technology lost", only when the
    grant taken was its last;
  - with another grant left (a crate standing, a second trigger grant)
    it keeps the technology and nothing is said;
  - with no research or trigger grant (none at all, or only a start
    technology or a building's), nothing happens.
  - `mi_kingdom_all` gives no kingdom here, so it fails.
- `actor_gains_technology` (`ma_actor`, `ma_technology`): gives the thing
  the property for itself, as if its own `[PropertiesComponent]` listed
  it: it counts for that thing alone, not its side, and doesn't appear
  in the technology display. Nothing is posted, and restrictions aren't
  checked: a technology barred for its side is given too. [code]
  - Nothing happens to a thing with no `[PropertiesComponent]`, or one
    that doesn't meet the property's `required_properties`. A regiment
    meets them when it or any of its members does, as for terrain
    properties ([terrain](terrain.md#terrain-properties)).
  - On a regiment it goes on the regiment, and its members get it as
    they get any of the regiment's properties, unless it's
    `pass_down_members = false`
    ([movement](movement.md#a-units-top-speed)).
  - A thing that already has the property, from its side or anywhere
    else, keeps it once: copies are held apart as a side's grants are,
    and the effects come once.
  - It stays through a change of hands: the side's technologies come
    and go with the side, this one stays with the thing. It lasts until
    `actor_loses_technology` or the thing is gone.
- `actor_loses_technology` (`ma_actor`, `ma_technology`): takes away one
  of the thing's copies of the property, any but those its side's
  buildings, start technologies, handicap or general put on it: one
  given by `actor_gains_technology`, a `LOCAL` research result, one its
  template lists [code, guess], or the copy its side's research or
  trigger grant put on it. So it can strip a side's researched
  technology from one thing while the side keeps it; the thing goes
  without until it joins another side that has it. A thing with another
  copy left keeps the effect. Nothing is posted. [code]
- `kingdom_has_technology` (AA condition; `ma_kingdom`,
  `ma_technology`): true while the side holds any grant of the
  technology, from any source: research, the map's start technologies,
  the handicap, a building (a crate counts while it stands, its remains
  included) or a trigger. A barred technology is never held. Things'
  own copies (`actor_gains_technology`, `LOCAL` research, a template's
  own properties) don't count, nor do an ally's technologies. [code]

## Variables and switching triggers

- Trigger variables are the TRIGGERS section's named numbers (none in
  any shipped multiplayer map). `set_variable` (`ma_variable`,
  `ma_value`) sets one, making it if there's none of that name (a
  variable needs a name: an empty one fails). `incr_variable` and
  `decr_variable` add or take away the value from an existing one, and
  fail if it doesn't exist. A number parameter with `mi_variable` reads
  one. [code]
- There are no timers of their own: a delay is a trigger switched on at
  the right moment with an `activation_time` condition. [code, data]
- `trigger_active` (`ma_trigger`, `mi_trigger`): switches on the trigger
  with that id and sets its activation time to now; it fails, changing
  nothing, if that trigger is already on or doesn't exist.
  `trigger_deactive` switches one off; it fails if it's already off. A
  trigger switched on is tested from the next trigger in list order on,
  so if it comes later in the list it's tested in the same tick. [code]
- No shipped multiplayer map uses variables or switches triggers;
  campaign maps do both. [data]

## Conditions the campaign maps use

- Where a condition names a thing that doesn't exist (gone, or never
  made), it's false. The only exception is `actor_is_dead`. Both
  `actor_has_state` and `actor_does_not_have_state` are false for a
  thing that isn't there. [code]
- `actor_is_dead` (`ma_actor`): true once the thing is dying (from the
  moment it dies, while its body lies there) or is no longer on the map.
  A parameter naming nothing (a name nobody bears, an id that's gone)
  also counts as dead. [code]
- `actor_owned_by_kingdom` (`ma_actor`, `ma_kingdom`): the thing
  belongs to that side now. [code]
- `actor_comparison_life` (`ma_actor`, `ma_compare`, `ma_value`): the
  thing's health against the value as a percentage of its maximum (`at
  least 99` means at least 99% health). A thing with no health of its
  own, a regiment included, is false. The campaigns' "at least 0" is a
  test that the thing still exists. [code, data]
- `actor_comparison_morale` (`ma_actor`, `ma_compare`, `ma_value`):
  the same for a regiment's morale against its maximum. Anything but a
  regiment itself is false. Both games have it; only Kohan II's maps use
  it. [code]
- `actor_has_state` (`ma_actor`, `ma_state`): the thing is in that state
  now; `actor_does_not_have_state` is its opposite. Only the thing's own
  states count: a regiment's are its own, not its units'. [code]
  - `mi_state` is a number, fixed by the game. The states, by number:
    0 `mind_attacking`, 1 `mind_bombard`, 2 `mind_building`, 3
    `mind_chasing`, 4 `mind_engaged`, 5 `mind_exhausted`, 6
    `mind_guarding`, 7 `mind_joining`, 8 `mind_moving`, 9
    `mind_reattaching`, 10 `mind_regrouping`, 11 `mind_repairing`, 12
    `mind_resting`, 13 `mind_retreating`, 14 `mind_routing`, 15
    `mind_reforming`, 16 `mind_exploring`, 17 `mind_rampaging`, 18
    `mind_razing`, 19 `out_of_supply`, 20 `under_siege`, 21
    `construction`, 22 `organization_entrenched`, 23 `dead`, 24
    `falling`, 25 `structure_packing`, 26 `structure_unpacking`, 27
    `structure_normal`, 28 `structure_researching`, 29
    `structure_upgrading`, 30 `structure_razing`, 31
    `structure_recruiting`, 32 `active_construction`, 33 `summoning`, 34
    `unpacked`, 35 `mind_packing`, 36 `mind_unpacking`, 37
    `missing_primary`, 38 `unloading`, 39 `disabled`, 40 `targeted`, 41
    `hq_attached`. The names are the `IDS` of `Game/actor_states.tgi`,
    whose own order differs. [code, data]
  - The campaigns test attacking (0), engaged (4), moving (8),
    retreating (13), routing (14), out of supply (19) and attached to an
    HQ (41). [data]
- `actor_inside_area` (`ma_actor`, `ma_area`): tested now, not from the
  area's list. [code]
  - A named thing: true when its soft shape, placed at its position and
    turned to its facing, overlaps the area's own outline, turned to the
    area's facing; touching counts. This is the exact outline, stricter
    than [who is in an area](#who-is-in-an-area): a thing can have
    entered without being inside by this test. A named regiment is
    tested by its position with its soft radius of 1. A thing that names
    nothing makes it false. [code]
  - With `mi_actor_any`: true if anything counts as in the area by the
    looser test of [who is in an area](#who-is-in-an-area), regiments
    included, of any side. [code]
  - The first one found becomes the condition thing
    (`mi_actor_condition`); the others are ignored. The search goes over
    the map's tiles under the area row by row, from the lowest y and,
    within a row, the lowest x; the first tile holding such a thing gives
    it. Which of several things in that one tile comes first isn't
    defined, so a regiment or one of its units can be the one. Ownership
    isn't considered. [code]
  - The one shipped use, Hawaii's spy, tests "anything in the area" and
    then that the condition thing belongs to the player's side, so a
    thing of another side found first makes it false that tick. [data]
- `actor_selected_by_kingdom` (`ma_actor`, `ma_kingdom`): the thing is in
  the current selection of the player playing that side. Clicking a unit
  selects its regiment ([selection](selection.md)), so name the regiment.
  A side with no player selecting things is never true.
  `actor_data_selected_by_kingdom` (`ma_actor_data`, `ma_kingdom`): any
  thing of that exact template is in that selection. The selection is
  the one every machine knows, kept in step by the player's selection
  orders. [code, guess]
- `actor_visible_to_kingdom` (`ma_actor`, `ma_kingdom`): the side can see
  the thing now, under the [fog](fog.md) rules: allies' sight shared,
  everything visible with the fog off, and a regiment visible when any of
  its units is (a unit, when any unit of its regiment is). No shipped AA
  map uses it. [code]
- Kohan II: `actor_explored_to_kingdom` (`ma_actor`, `ma_kingdom`): the
  ground the thing stands on has been explored by the side (or, the same
  way, any of its regiment's). [code, guess]
- `difficulty_compare_difficulty` (`ma_difficulty_level` twice,
  `ma_compare`): compares two difficulty levels. `mi_difficulty_level_map`
  is the match's difficulty (the `DifficultyLevel` rule), and
  `mi_difficulty_level_specified` names a level (`easy`, `medium`,
  `hard`). Each level is its place in `Game/difficulty_level.tgi`'s list
  (easy 0, medium 1, hard 2), on both sides of the comparison, so they
  rank in that order. [code, guess]
  - The campaigns only test "equal" against `easy` and `hard`, nearly
    always to give a side technologies at that level. [data]
  - Nothing else in a mission reads the difficulty: not the computer
    players, nor handicaps ([campaigns](campaigns.md#difficulty)). Outside
    triggers it's read only when a random map is made. [code]
- `objective_succeeded` and `objective_failed` (`ma_objective`,
  `ma_kingdom`): in [objectives](objectives.md).
- `kingdom_default_lose` (`ma_kingdom`): true while the side meets the
  default defeat rule's test ([outcome](outcome.md#the-default-defeat-rule)):
  it has nothing that keeps it alive and, with `TeamVictory` on, neither
  does any team-mate. The rule's switch and timing make no difference.
  The campaigns use it on several sides. [code, data]
- `kingdom_has_no_settlements` (`ma_kingdom`): Kohan II's test that the
  side holds no settlement. In AA, where no side holds settlements, it's
  always true. No shipped map uses it. [code, data]

## Scripted scenes

Every campaign mission's intro is built the same way (Map Init / "Intro -
Begin" triggers, then "Intro - Step" triggers chained by `trigger_active`
and `activation_time`, then "Intro - End"): screen fade and letterbox on at
once, cutscene mode on, camera pans and speech, then letterbox off over
3–5 seconds, fade off over 1–3 seconds, cutscene mode off. [data]

- `global_toggle_cutscene_mode` (`ma_boolean`): [code]
  - On: the game speed goes to normal (100%), whatever the player had
    set, and the speed the player had is remembered; off: that speed is
    restored. Switching to the state it's already in changes nothing.
  - While on: the player can't change the game speed or pause; computer
    players don't think (they give no new orders; their things carry on
    with what they were doing and still fight for themselves); no game
    events are raised, so no alert lines or event sounds
    ([game-events](game-events.md)); visual effects marked
    `show_in_cutscene = false` (the speed and resupply decals) are hidden;
    the camera is free of its usual height limits.
  - Switching it off also puts the camera back to its default zoom and
    angle, keeping where it looks. [code]
  - While on, the player's input goes to the cutscene alone: clicks on
    the map or the interface and every key do nothing, except Esc
    (skip), Print Screen (screenshot) and Pause, which does nothing
    because pausing isn't allowed. So the player can't give orders during
    a cutscene, letterbox or not. [code, data, guess]
  - Esc skips a scripted cutscene [code, game]: the game runs at its
    fastest film speed
    (`GameSpeedFilmMax`, ×16), and instead of the map the screen shows
    only "Please wait while advancing cutscene..."
    (`ui_skipping_cutscene`). Esc again stops skipping and goes back to the
    remembered speed. Skipping ends when the map switches cutscene mode
    off, which restores the remembered speed. It only hurries the scene
    on: even after the match is decided it doesn't end the mission (in
    every shipped map; the map type that would is never used). [code,
    data]
  - While skipping with the letterbox on, speech isn't played (other
    sounds are); each line's subtitle is shown at once instead. [code]
  - If the player's side wins or loses while cutscene mode is on, alone or
    as host, no outcome dialog opens: the mission ends at once, as End
    Mission would ([outcome](outcome.md#what-the-player-sees)). [code]
- `global_toggle_letterbox` (`ma_boolean`, `ma_value` seconds): black
  bars slide in at the top and bottom of the screen over the seconds (0 at
  once), or out when off. While it's on the in-match interface is hidden,
  and comes back when it's switched off. [code, data]
  - What it hides is every widget whose `render_threshold` is below 20:
    the map view (`VWorld`, 15) and the in-match interface (unset, so 10).
    Hidden widgets take no clicks or hotkeys either. The subtitle panel,
    game menu, message boxes and console (20) stay. [code, data]
- `global_toggle_screenfade` (`ma_boolean`, `ma_value` seconds): on fades
  the screen to black over the seconds (0 at once); off fades it back in.
  Missions start with it on at 0 so the first frame is black. [code, data]
- Both bars and fade are timed in game time from the moment the effect
  runs. [code, guess]
- `play_speech` (`ma_speech`, an `AudioCutscene` `[Audio]` id such as
  `t1l02_tutor`): plays the line for every player, not placed on the map.
  When it starts playing and subtitles are on, its `[Subtitle]` text is
  shown for the subtitle's duration: in the letterbox while that's on,
  otherwise as a line in the message log. A line that can't be played
  (while skipping, say) has its subtitle shown at once. [code, data]
  - Only one speech line plays at a time, and lines don't wait for one
    another: a new line cuts off the one playing (speech is
    `INTERRUPT`) and plays from its start in its place. The line cut off
    stops at once (`AudioSpeech` has no fade, `fade_time_s` 0). [code,
    data]
  - The same line asked for again while it plays starts over from the
    beginning; it never plays twice at once. [code, data]
  - When no line is playing yet but one is waiting to start, a new line
    takes the waiting one's place: the waiting line is dropped without
    playing and without its subtitle. So when several lines are asked
    for in one tick, only the last plays. [code]
  - Speech never cuts off music or other sounds, nor they speech; they
    are counted apart. [code]
  - While a line plays, other kinds of sound are turned down
    (`CAUSEFADE`). A line that hasn't started within `AudioDropTimeout`
    (5 s) of being asked for is dropped. [code, data]
- `display_banner_text_fade` (`ma_localized_string`, `ma_value` seconds):
  shows the map's string (its id, such as `trigger_63_element_2`, names a
  string the map carries) in the banner panel (`UI/Shared/display_banner.tgi`:
  yellow text on a backdrop, lower left of centre) for the seconds, then
  it fades. The seconds are counted in real time. A new banner replaces
  the one showing. [code, data, guess]
- `kingdom_display_message`: in [messages](messages.md).
- `play_cutscene` (`ma_string`, a video path such as
  `/Video/Campaign/AxisVictory.bik`): plays the video full screen over the
  match, which carries on underneath, unpaused; when it ends, a
  `cutscene_complete` event naming it is raised. The two final missions
  use it after victory and wait out its length with `activation_time`
  (58 seconds) before `kingdom_mission_won`. [code, data]
  - Esc stops the video early, which counts as its end and raises the
    event too. [guess] A mission that waits by `activation_time` still
    waits the full time after a skipped video. [code, data]
- `game_speed_set` (`ma_value`): sets the game speed to the value times
  normal (2 = double, 0.5 = half), held within `GameSpeedMin` and
  `GameSpeedMax` (25% to 400%; while cutscene mode is on, the film limits).
  A value of 0 or less fails. No shipped map uses it. [code, data]
- `global_adjust_sun_dimmer`: in [lighting](lighting.md#changes-during-play).
- `change_weather` (`ma_weather`, `snow_heavy` or `rain_heavy`): switches
  the map's weather to that one (falling snow or rain); it only changes
  the look. Always succeeds. [code, guess]
- `vfx_at_location` (`ma_vfx`, `ma_location`) plays the effect once at
  the place; `vfx_at_location_with_rotation` adds `ma_value`, degrees to
  turn it about the vertical. Used for explosions (`water_explosion_3`,
  `v1_explosion`). [code, data]

## The camera

Each of these is for one side (`ma_kingdom`): only the player of that side
sees it. [code, guess]

- `camera_pans_to_camera_over_time` (`ma_kingdom`, `ma_camera`, `ma_value`
  seconds, `ma_boolean`): the view moves from where it is to the camera
  object's (one placed in the map, by its id): the point it looks at, its
  zoom and its angle, over the seconds (0 jumps). The boolean eases the
  move in and out (slow start and stop) instead of a steady glide. The
  seconds are game time. [code, data]
- `camera_pans_to_camera_over_time_in_realtime` (`ma_kingdom`,
  `ma_camera`, `ma_value`): the same, never eased, timed in real time, so
  game speed doesn't change it. [code]
- `camera_returns_to_default_over_time` (`ma_kingdom`, `ma_location`,
  `ma_value` seconds): the view moves over the seconds to look at the
  place with the default zoom and angle, not eased. [code, data]
- Fails when the side or camera names nothing. [code]

## Pointing things out (the tutorial)

- `flash_actor` (`ma_actor`): the thing flashes as an order's target does
  when picked ([commands](commands.md)); on a regiment, its units flash
  too. Fails if it names nothing. Tutorial 1 repeats it every second on
  its infantry regiment until it's selected. [code, data]
- `flash_ui_button` (`ma_script_command`, a button's script command such
  as `ActorCommand recruit`, `MessageLogToggle`, `GoToTeamCommands`,
  `UseFactionAbility 0`, `ShowFloater 2`): makes the in-match interface
  button that carries that command flash. Fails when no button on the
  interface carries it (for an action button, when nothing selected shows
  it). [code, data]
- `effect_ui_script_listen` (`ma_script_command`, `ma_boolean`): true
  starts listening for that interface command, false stops (and fails if
  it wasn't being listened for). While it's listened for, each time the
  player uses it (by button or key) a script event naming it is raised
  for `event_script_heard`; the command still does what it always does.
  The command must match in full, ignoring case (`ActorCommand recruit`
  doesn't match `ActorCommand recruit_location`). What's listened for is
  kept in a saved game. [code, data, guess]
- `team_command_flare_on_position` (`ma_flare_data_location`,
  `ma_kingdom`, `ma_location`, `ma_value` seconds) and
  `team_command_flare_on_actor` (`ma_flare_data_actor`, `ma_kingdom`,
  `ma_actor`, `ma_value`): place a team-command flare (`attack_location_flare`,
  `defend_actor_flare`; [team commands](team-commands.md)) for that side
  as its player would, lasting the
  seconds (maps use 300 to 10⁹, in effect for good; a negative value takes
  the flare's own). Computer players of that side's team take it as a
  request at priority `SAITriggerTeamCommandPriority` (20000), above
  their own plans; this is how missions point computer allies at a goal.
  [code, data, guess]

## Making and removing things

- `actor_create_named` (`ma_actor_data`, `ma_kingdom`, `ma_location`,
  `ma_string` name): makes the template for the side at the nearest clear
  spot within 8 of the place, with the name (empty for none).
  `organization_create_named`, `unit_create_named` and
  `building_create_named` are the same effect, the editor offering only
  that kind of template (their template parameter is `ma_unit_data`
  with `mi_unit_data`, or `ma_organization_data` with
  `mi_organization_data`, looked up by id as `mi_actor_data_named` is).
  Campaigns mostly make objective markers
  (`thing_objective_primary01`) and sight-givers
  (`thing_visibilityactorsmall`) this way, and remove them by name later.
  [code, data]
- It fails, making nothing, when: [code]
  - the template names nothing (an empty template, say; the place it
    gives doesn't matter);
  - the side names nothing. That includes `mi_kingdom_id` with id 0: no
    thing ever has id 0. A side is needed even for a template that can't
    be owned (below);
  - no clear spot is found;
  - the name isn't empty and a thing still in the world already bears it
    (matched regardless of case; a dying thing still bears it). A name
    whose last bearer has been taken off the map is free again.
- The clear spot: up to 65 points are tried, in order, and the first
  clear one is taken; none clear and the effect fails. [code]
  - First the place itself, then 8 rings, at 1, 2 and on to 8 world
    units from the place (a ring every half tile), 8 points to a ring.
    These are exact points, not tile centres.
  - Each ring's first point is straight out from the place in the
    direction (0, −1); each next one is turned a further 45° the same
    way, (0, −1) to (−0.71, −0.71) to (−1, 0) and on round, as a
    summon's are ([special operations](special-ops.md#where-and-how-it-arrives)).
  - A point is clear by the test of
    [room to stand](objects.md#room-to-stand) for the template's own
    blocking shape there, never grown by padding: the shape's bounding
    box on the map, every tile it overlaps passable for it (land, or water
    for a `water_based` thing) and not already blocked by a building or
    other standing thing, and no `blocking` thing's shape overlapping it.
    Units and regiments' units count as obstacles. [code]
  - The test is the same whatever the template's own flags: unlike room
    to stand, a thing that isn't `blocking`, or is `ethereal`, still
    needs such a point. So a sight-giver or objective marker is never
    made on top of a unit or a building: it goes to the nearest clear
    point, and one aimed at water lands on the nearest land. They block
    nothing themselves, so any number can be made at the same point, and
    units can be made on them. [code, data]
  - The thing faces (0, −1) when made at the place itself, and otherwise
    faces away from the place, along the ring's direction to its point.
    [code]
- A regiment is searched for in two steps, never as a whole formation:
  [code, data]
  - first the search above, for the regiment's own blocking shape (the
    `Organization` template's default circle of radius 0.2), giving a
    point and a facing;
  - then from that point, its leader's own blocking shape is placed as
    a summoned regiment's leader is
    ([special operations](special-ops.md#where-and-how-it-arrives)):
    the point, else up to 10 rings spaced by the leader's blocking radius
    (at least a quarter tile), 8 points to a ring, the first along the
    facing from the first step and each next turned 45° as above. No
    point for the leader and the effect fails. The regiment stands where
    its leader is placed, facing the way the first step gave;
  - each other unit is then placed on its own, as there: out of a
    nearby friendly building that recruits its kind, else its place in
    the formation, else the nearest clear point to it. A unit that finds
    no room isn't made, and the regiment is made without it.
- What's made: [code]
  - A thing comes into the world at once, bearing its name, so a later
    effect in the same trigger, or a condition in the same tick, can name
    it. Its `WhenCreated` effects play (an objective marker's minimap
    blip).
  - It's made from its template as it stands: full health, already
    built (a building isn't put under construction), and none of the
    effect's parameters change anything else about it.
  - A regiment template comes with its units, placed as above.
  - A single unit's template (not a regiment) gives a bare unit, in no
    regiment: a loose unit, like one left over when its regiment is gone.
  - Experience and stance are those of any newly made thing of its kind.
    [guess]
- Whose it is: a template that can be owned (`ownable`, the default)
  belongs to the effect's side. One with `ownable = false` belongs to
  nobody, whatever side the effect names, as does one placed in a map.
  So objective markers (`ObjectiveObject`) and `objective_compass` never
  have an owner, and sight-givers (`AAVisibilityActor`, ownable) belong
  to their side. [code, data]
- Sight-givers, objective markers and `objective_compass`: [code, data]
  - A sight-giver sees for its side, and that side's allies, as any
    other thing does ([fog](fog.md)): `thing_visibilityactorsmall`,
    `medium` and `large` see 15, 30 and 45, with remote ranges the same.
    One placed in a map belongs to the side it was saved with.
  - Objective markers have no sight. `objective_compass` has 25, and 12
    remote, but with no owner it shows nothing to anyone.
  - None of them is `targetable`, so none can be attacked, kept
    selected ([selection](selection.md)), or counted as in an area by
    `actor_enters_area` or by `actor_inside_area` with `mi_actor_any`.
    A named one is still found by `actor_inside_area` by name.
  - `actor_owned_by_kingdom` is true of a sight-giver for its side, and
    of an objective marker or the compass for no side.
  - None of them keeps a side alive ([outcome](outcome.md)): they have
    neither of the components that do.
- `actor_create_transported_named` (`ma_actor_data`, `ma_kingdom`,
  `ma_location` from, `ma_location` to, `ma_string` name): the template's
  transport (`transport_actor`; a template with none fails) appears at the
  first place, at its flying height if it flies, or else on the nearest
  clear ground, and delivers to the second place as in
  [deliveries](deliveries.md#special-operations-and-triggers); a boat
  lands at the shore point nearest the second place, and fails if there's
  none. The delivered thing bears the name. [code, data]
- `object_destroyed` (`ma_actor`): the thing dies as if killed (its death,
  remains and "lost" line; no one is credited). `invulnerable` doesn't
  stop it. A thing with no health of its own (an objective marker, a
  sight-giver) is taken off the map straight away, leaving no remains. One
  with health lies dead for its `rot_time` and `sink_time` as
  [combat](combat.md) describes: `objective_compass`'s are 9999 and 5
  seconds, so its remains stay for the rest of any mission. The "lost"
  line goes only to the thing's owner, so a thing with no owner posts
  none. Destroying a thing that's already dead does nothing more. [code, data]
- `object_removed` (`ma_actor`): the thing is taken off the map at once,
  with no death, no remains and no line. [code]
- Both fail if the parameter names nothing. [code]

## Orders

Each of these gives the order straight to the thing, and first unlocks
it if `actors_lock` had locked it (the thing and, for a regiment, its
units; a unit named alone is unlocked alone). [code]

How a trigger's order differs from a person's: [code]

- It reaches any side's things, the enemy's and neutrals' included: no
  owner test, and none of the click's target refusals
  ([commands](commands.md#targeted-commands)) apply. The thing's side
  doesn't need a computer player, or a person.
- It's refused, silently, when the thing doesn't offer that command
  right then: the same set of commands its grid would show
  ([commands](commands.md#command-grid)), so a routing or Exhausted
  regiment refuses them all ([morale](morale.md#the-players-orders)),
  a truck at its unpacking spot refuses everything, and a thing with no
  such command at all (Bombard for a regiment with no artillery, Unpack for anything but a truck or the
  regiment it leads, Air Attack for anything but an airfield) refuses
  it. A refused order changes nothing: the thing carries on with what
  it was doing.
- It's never queued: it replaces whatever the thing was doing and
  throws its queue away, as an unqueued order of a person's does
  ([queued orders](commands.md#queued-orders)).
- It carries no facing, so the thing ends as for a person's order given
  without one ([commands](commands.md#targeted-commands), Facing).
- A regiment that is in a person's group isn't taken out of it, as a
  person's order would take it. [code] What the group does with it
  afterwards is open.
- Once given, the order plays exactly as a person's would: the same
  moves, the same engagements on the way, the same arrival
  ([movement](movement.md), [combat](combat.md)). Nothing in the order
  remembers that a trigger gave it. [code]

The effects:

- `actor_performs_command_on_actor` (`ma_actor`, `ma_command`, `ma_actor`
  target) and `actor_performs_command_at_location` (`ma_actor`,
  `ma_command`, `ma_location`): orders the thing to carry out the command
  (`ma_command_actor` or `ma_command_location`, `mi_command`, by its id:
  `kill`, `bombard`, `air_attack`, `guard` on a thing; `move`, `unpack`,
  `air_attack` at a place) on the target or at the place. Fail if the
  thing names nothing, or the target or place is missing; otherwise they
  succeed, whether the thing takes the order or refuses it. [code, data]
  - Shipped uses: on a thing, `kill` 57, `air_attack` 17, `bombard` 10,
    `guard` 1; at a place, `unpack` 3, `air_attack` 2, `move` 1. [data]
- Who takes the order: [code]
  - A regiment takes it as a regiment, as a person's order to it.
  - A unit named alone, one in a regiment included, takes it itself; its
    regiment isn't ordered. A move sends it off on its own, out of
    formation, under the same rules as a unit moved by
    `kingdom_actors_in_area_move_to_location` (below): it may give up
    and go back to its regiment. An attack or bombard order sends it
    after the target on its own. [code; guess: whether an attack takes a
    unit back to its regiment as a move does] A unit doesn't take up
    `guard`. [code] What it does instead is open.
  - A building, or any other thing that can't take orders of that kind,
    refuses it, except that an order a settlement's central building
    can't take passes to its settlement. [code] Nothing in a settlement
    is known to take the commands triggers use, so the end is the same.
    [guess]
- `kill` on a thing is a person's Attack on it
  ([combat](combat.md#attack-orders)): [code]
  - on an enemy regiment, unit or building, the regiment goes after that
    very thing (a unit of an enemy regiment is the target itself, not
    its regiment). A regiment that bombards instead of attacking
    bombards it ([combat](combat.md#bombard-orders)); AA has none;
  - on a thing that isn't an enemy of the regiment's side (its own, an
    ally's, a neutral's), or that none of its units can attack, it moves
    to the thing instead, as a move onto it would. The effect still
    succeeds.
- `kill` at a place (no shipped use) is a person's attack on the ground.
  [code]
- `guard` on a thing is a person's Guard order on it, whoever owns it
  ([combat](combat.md#guard-orders)): on an enemy the regiment follows
  and guards it like any other thing. Only guarding itself is refused.
  [code; guess: what guarding an enemy amounts to in play, since those
  fighting it are the regiment's own side or allies, which it won't
  attack]
- `bombard` on a thing or at a place is a person's Bombard order
  ([combat](combat.md#bombard-orders)). A regiment without a unit that
  can bombard, or a building, refuses it. [code]
- `air_attack` is an airfield's command, not a regiment's or a special
  operation: [code, data]
  - The airfield sends out its bombers against the thing or the place,
    as for a person's Air Attack, paying for them, or posts that its
    side can't afford it ([airfields](airfields.md)). Either way the
    effect succeeds. [code, data]
  - Anything but an airfield refuses it. [code]
  - Shipped use: `allies10_berlin`'s intro has the two Soviet
    airfields air-attack the city's buildings. [data]
- `unpack` at a place: a person's Unpack order there
  ([buildings](buildings.md#an-unpack-order-from-start-to-finish)): the
  truck, or the regiment it leads, drives to the place and unpacks.
  [code]
  - With no facing in the order, the building faces (0, -1) in map
    coordinates, whatever way the truck came. [code]
  - If it can't get there, or there's no room at the place, it gives up
    with "Cannot unpack %s" and stays a truck, as for a person's order.
    The effect still succeeds. [code]
- `move` at a place is a person's Move there. [code]
- `actor_performs_command` (`ma_actor`, `ma_command`), with neither
  target nor place, always fails. No shipped map uses it. [code]
- `kingdom_actors_in_area_move_to_location` (`ma_kingdom`, `ma_area`,
  `ma_location`): every thing on the area's list (as the last check left
  it, [who is in an area](#who-is-in-an-area)) that belongs to that side
  is ordered to move to the place, at top speed, as
  `actor_moves_to_location` with value 0 would. A locked thing is
  unlocked first. [code]
  - The list holds regiments and their units alike, so a regiment in the
    area is ordered and so is each of its units that is in it. [code]
  - A unit in a regiment takes a move order of its own: it sets off for
    the place alone, out of formation. [code]
  - When its regiment is ordered too, the regiment's move wins: at its
    first look after the order it hands every member its place on the
    regiment's route, as for any regiment move
    ([movement](movement.md#members-routes)), replacing the units' own
    orders. So a regiment in the area moves as a regiment. [code, guess:
    that the hand-out always comes after the units' own orders]
  - A unit ordered without its regiment (it's in the area, its
    regiment's position isn't) heads off alone. While it's still within
    its regiment's control zone nothing pulls it back. Once outside, it
    keeps going only if the place lies inside the regiment's control
    zone and the regiment isn't retreating or routing; otherwise after
    `MoverDetachedSecondsUntilReattach` seconds (stock 2) outside, it
    gives up the move and goes back to its regiment. [code, data]
  - If that unit is its regiment's leader, the regiment's position
    follows it as usual ([movement](movement.md#arriving)), so the
    leader stays with its regiment and the regiment's place goes along
    with it. [code, guess]
  - Things that can't move (buildings) take no order. It succeeds
    whenever the side, area and place all name something, even if
    nothing is ordered. [code, guess]
  - No shipped AA map uses it. [data]
- `actor_performs_animation` (`ma_actor`, `ma_animation`): the thing plays
  that animation of its model (`build1`). Fails if it has no such
  animation. [code, data]
- `ability_used_by_kingdom_from_location_at_location` (`ma_ability`,
  `ma_kingdom`, `ma_location` from, `ma_location` at): the side uses that
  special operation (`v_weapons`) at the second place, as if from the
  first, without paying for it or waiting for its recharge; nothing
  happens if the place is out of its range. Only operations aimed at an
  area work. [code, guess]

## Locking

- `actors_lock` (no parameters): every thing of every side that is in
  the world at that moment is told to stop (Stop, as in
  [Computer control](#computer-control)) and is locked. Things that come
  into the world afterwards aren't locked. [code]
- What a lock holds back: [code]
  - Queued orders. A locked thing that runs out of orders doesn't go
    on down its queue ([queued orders](commands.md#queued-orders)); the
    queued orders wait, all of them, and once it's unlocked it takes
    them up in turn as usual, the next time it's idle. [code; guess:
    how soon after unlocking]
  - A unit's answer to being hit: a locked unit doesn't turn on what
    hit it.
- What it doesn't: an order given straight to the thing, not queued, is
  carried out at once, a person's included. So is a trigger's, which
  unlocks it first anyway. A locked thing still fires at enemies in its
  range, and its regiment still engages as usual
  ([combat](combat.md#engagement)). [code; guess: that nothing else
  reads the lock]
- A regiment is locked with its units: locking or unlocking a regiment
  does the same to each of them. Each still has its own lock, so a unit
  unlocked alone (a trigger's order naming it) leaves its regiment and
  the other units locked. [code]
- `actors_unlock` unlocks every thing of every side. [code]
- One mission (`axis10_moscow`) locks everything through its intro.
  [data]

## Computer control

- A side or a thing can have computer control switched off. It only
  matters to a computer player: [code]
  - `sai_disabled_for_kingdom` (`ma_kingdom`): the side's computer player
    stops planning and giving orders, and every thing of the side is told
    to stop, whether a computer plays it or not. `sai_enabled_for_kingdom`
    lets its computer player carry on. Enabling doesn't give a side a
    computer player it didn't have; on a side played by a person it only
    stops everything. [code]
  - `sai_disabled_for_actor` (`ma_actor`): the side's computer player
    lets go of that one thing (drops it from whatever it had it doing) and
    gives it no orders, and it's told to stop. `sai_enabled_for_actor`
    hands it back. [code, guess]
  - The thing is told to stop whoever plays its side: a person's thing
    stops too. [code]
  - "Told to stop" is the Stop order, as a person's Stop button gives
    it: the thing drops its order, its queue and any fight it's in, and
    stands where it is. Only the ordinary engagement rules start a new
    fight ([combat](combat.md#engagement)). A thing that doesn't offer
    Stop at that moment (one routing, or a truck at its unpacking spot)
    refuses it and carries on. [code; guess: how soon a fight starts
    again against an enemy still in range]
  - Switched off for a thing, it stays off when the thing changes side
    (a capture, `actor_transfer_control_to_kingdom`): the new side's
    computer player doesn't use it either, until
    `sai_enabled_for_actor` names it. [code]
  - Either way the things themselves behave as any unit without orders:
    they keep their stance, fight what comes into range and return fire,
    and morale works as usual ([combat](combat.md), [morale](morale.md)).
    Only the computer player's decisions are switched off. [code]
  - While its side is off the computer player forgets nothing, and it
    carries on from where it was once switched back on; a thing switched
    back on is simply free for it to use again
    ([computer players](computer-players.md#switched-off-by-a-trigger)).
    [code]
  - `sai_change_profile_for_kingdom` exists, but no shipped map uses it.
    [data]
  - Each fails only when its parameter names nothing. [code]
  - Shipped use: Map Init switches off the enemy side (the tutorial's for
    good), and a later trigger (a timer, a landing) switches it on. [data]

## Resources

- `kingdom_gains_value_primary_resource` (`ma_kingdom`, `ma_value`): adds
  the value to the side's stockpile of the primary resource (AA's
  `money`); the campaigns give 1000 or 2500. [code, data]

## Objectives and outcome

- `kingdom_loses_objective`, `objective_failed_for_kingdom`,
  `objective_incomplete_for_kingdom`: in
  [objectives](objectives.md). `kingdom_mission_lost`,
  `kingdom_enable_default_victory_condition`: in
  [outcome](outcome.md#triggers). [code]

## Counter

- `game_counter_show` (`ma_value` seconds): shows a countdown from the
  seconds on screen, starting now; a second one replaces the first. 0 or
  less fails. `game_counter_hide` takes it away. It's only shown: nothing
  happens when it reaches 0, so maps pair it with an `activation_time`
  trigger of the same length (`allies09`: 1200 seconds to defend
  Bastogne). It counts game time, stops at 0 and stays on screen until
  hidden, and every player sees it; how it's written is in
  [interface](interface.md#the-time-labels). It's kept in a saved game.
  [code, data]

## Kohan II's other elements

- `camera_shake` does nothing while cutscene mode is on.
  `camera_follows_actor` keeps the side's view on the thing. [code]

The rest are listed by name, not yet specified. [data]

- `actor_performs_command_with_actor_data_at_location`,
  `actor_teleports_to_location`, `fog_restored_in_area`,
  `global_toggle_fidgets`, `kingdom_actors_in_area_teleport_to_location`,
  `play_game_music`, `play_music`, `stop_music`, `vfx_on_actor`.

## Open questions

- What a unit named alone does with a `guard` order, which it doesn't
  take up, and whether an attack order takes it back to its regiment
  as a move does. (implementer, 2026-11-22)
- What a person's group does with one of its regiments that a trigger
  has ordered. (analysis, 2026-11-22)

- Does destroying a sight-giver with `object_destroyed` post a "lost"
  line to its side? Which things post that line hasn't been traced.
  (analysis, 2026-11-22)
- Does a thing made by `actor_create_named` start with no experience and
  its kind's default stance, as guessed? (analysis, 2026-11-22)
