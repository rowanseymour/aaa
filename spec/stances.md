# Stances

A thing's stance: the one `[Stance]` of its template that applies to it
at the moment, which picks the model it's drawn with and its animation
set, and gives it a property. Also entrenchment, which AA's stance data
mentions but which reaches units another way. The stance templates are
`Templates/template_stance.tgi` (fields) and `template_stance_aa.tgi` /
`template_stance_k2.tgi` (the stock stances).

Games: traced in AA. Kohan II follows the same rules for stances and
entrenchment (its entrenchment check read); its data is surveyed below.

## Which stance applies

- A thing has at most one stance at a time, chosen from the `[Stance]`
  blocks its template lists. A stance applies when both hold: [code]
  - the thing's health, as a share of its maximum, is at or below the
    stance's `health` (so 1.0 is any health, and 0.5 is half or less);
    a thing without a `[BodyComponent]` never takes a stance;
  - the thing is in every state the stance's `actor_state` lists: the
    comma-separated list means all of them, not any. An empty or `NULL`
    list asks for no state, so only health counts.
- Of the stances that apply, the one listed last wins. A thing to which
  none applies has no stance and is drawn and behaves plainly. [code]
- The list is in the order the template gives its `[Stance]` blocks;
  those a template inherits come before its own, as for any `list`
  section ([data](data.md#templates); `[list Stance]` is declared in
  `template_actor.tgi`). [code]
- The states a stance can name are only the first 32 in the game's
  fixed list of states, `mind_attacking` through `structure_recruiting`
  (`Game/actor_states.tgi` notes the list is fixed). A later one
  (`active_construction` onward, `disabled` and `targeted` among them)
  silently tests a different state instead. No stock stance names one.
  [code]
- A state the game doesn't know, a stance `health` outside 0 to 1, a
  stance `aspect_ids` that isn't one of the thing's own `[Aspect]`s, an
  unknown `property_ids`, or two stances with the same `ids` on one
  thing is a data error. [code]
- When it's decided: whenever any of the thing's states comes or goes,
  and whenever damage or healing changes its health. There's no timer;
  a stance that has stopped applying ends at the next such change.
  [code] When an activity gives way to another, the stance is decided
  once with the old activity gone and again with the new one in place,
  so a stance both need still drops in between (see the delay below for
  why that can show). [code]

## The stock stances

AA (`template_stance_aa.tgi`), with who lists them: [data]

| Stance | `health` | `actor_state` | Look | Property | Listed by |
|---|---|---|---|---|---|
| `StanceKneel` | 1.0 | `mind_engaged, under_siege` | `kneel` | `infantry_kneel` | all infantry (`AABipedKneel`, `AABipedProne`) |
| `StanceProne` | 0.5 | `mind_engaged, under_siege` | `prone` | `infantry_prone` | after kneel, on `AABipedProne` |
| `StanceFalling` | 1.0 | `falling` | `falling` | `airborne` | paratroopers, crates, saboteur, secret agent |
| `StancePacking` | 1.0 | `structure_packing` | `aspect_packing` | | packable buildings |
| `StanceUnpacking` | 1.0 | `structure_unpacking` | `aspect_unpacking` | | packable buildings |
| `StanceDisabled` | 0.40 | none | `aspect_disabled` | | packable buildings, bunkers |
| `StanceDeath` | 1.0 | `dead` | `aspect_death` | | vehicles, ships, packable buildings, bunkers and others |
| `StanceUpgrade` | 1.0 | `structure_upgrading` | plain | `building_upgrade` (the depots set it) | supply depots |
| `StanceMinorDamage`, `StanceMajorDamage`, `StanceEntrenchmentInfantry`, `StanceConstruction` | | | | | nothing |

- Infantry: kneeling and going prone need the man both engaged (he's
  attacking a target of his own) and under siege (an enemy has damaged
  him within the last 7 seconds), as [combat](combat.md#engaged-and-under-siege)
  defines them. Either alone isn't enough: a man firing who hasn't been
  hit stands, and so does one being shot at who isn't firing back.
  [code, data]
- Those with both (`AABipedProne`: riflemen, officers, engineers,
  snipers, assault infantry, banzai, conscripts, paratroopers, saboteur,
  secret agent) kneel above half health and go prone at half or less;
  those with only `StanceKneel` (`AABipedKneel`: machine-gunners,
  anti-tank teams, flamethrowers) kneel at any health. [code, data]
- A packable building or depot (`AABuildingPackable` lists packing,
  unpacking, disabled, death in that order): dead it shows its death
  look; otherwise at 40% health or less its disabled look, even while
  packing or unpacking; otherwise its packing or unpacking look while
  doing that. [code, data] A depot adds `StanceUpgrade` last, so while
  it upgrades it's drawn plainly even when damaged or dead, and holds
  `building_upgrade` (heals 25 a second). [code, data]
- Bunkers list `StanceDisabled` then `StanceDeath` ("so that death
  overrides Disabled", their template says). [data]
- Kohan II (`template_stance_k2.tgi`) has only `StanceDisabled` (0.30)
  and `StanceDeath`. `StanceDeath` is listed by many units and buildings
  (skeletons, golems, catapults, drakes, the racial building templates);
  `StanceDisabled` by nothing. The Haroun village centre has a stance of
  its own, `construction`: while it's being built it holds
  `haroun_village_autoheal` (heals 5 a second) and keeps its plain look.
  No Kohan II unit kneels or goes prone. [data]

## Moving, firing and changing stance

- A stance doesn't care whether the thing is moving: a man who stays
  engaged and under siege keeps his stance as he moves, at its speed
  (below), using the stance's own move animations (AA's kneel sets jog
  crouched, `KneelJog`; the prone sets crawl, `ProneJog`). [code, data]
- There are no getting-down or standing-up animations: the change is a
  swap of animation set. [data]
- Things without a `[CharacterComponent]` (buildings, vehicles, ships)
  change stance the moment it's decided. [code]
- A thing with one (infantry) changes at once if the new stance uses the
  same animation set as the old one, or if the animation it's playing
  loops (`idle` and `move` types: standing, walking, jogging, idling to
  attack). Then the animation it was playing starts over from the new
  set, so a man walking when he's to kneel goes straight into the
  crouched jog. [code]
- Otherwise (it's firing, fidgeting, dying) the change waits
  until that animation ends and the next one starts. Until then it
  keeps its old stance in every way: look, property and speed. So a man
  told to kneel mid-shot finishes the shot standing, at standing speed.
  [code]
- A waiting change isn't taken back: if the stance stops applying
  before it happens, the man still takes it when his next animation
  starts, and keeps it until his stance is next decided (a state or
  health change, above), which then waits for the following animation
  in turn. [code]
- Dying: a man stops being under siege the moment he dies, so in AA,
  where infantry list no death stance, his kneel or prone stance stops
  applying then. The change follows the rules above, and his death
  animation is chosen in the set of the stance he's drawn in when the
  change has or hasn't happened ([combat](combat.md#death)): [code, data]
  - if he's playing a looping animation (idling to attack between
    shots, moving), he stands at once and his death animation is chosen
    from the standing set;
  - otherwise (mid-shot, fidgeting) it's chosen from his kneel or prone
    set, and he stands as it starts: what plays is the standing set's
    animation with the same `IDS` (the riflemen's kneel and prone `die1`, `die2`,
    `die17`, `die18`, `die21` all have standing namesakes), or the
    standing set's first `die` animation if there were none, while the
    remains' time still follows the `length` of the one chosen;
  - if the kneel or prone set has none that fits (flame, explosive,
    armour-piercing, bleeding), no death animation starts, so the
    change never happens: he stays drawn kneeling or prone.
- Kohan II's characters have no per-look animation sets
  (`AspectAnimationSet` is used nowhere in its data), so every look of a
  unit uses its one set and a stance change, death included, is never
  delayed: a unit with `StanceDeath` swaps to its `aspect_death` model
  as it dies and plays its death animation from that one set. [code,
  data]
- Firing and turning aren't changed by a stance in AA, apart from what
  its property does; the property is the whole of a stance's effect on
  play. [code, data]

### Idling and fidgets on the map

What a man (anything with a `[CharacterComponent]`) plays when he isn't
moving, firing or dying. This is part of the game's simulation, not only
its drawing: when a fidget starts and which one is the same in every
player's game, and its random draws come from the game's shared random
numbers, so they use up draws as any other game rule does. [code] (The
portrait's idle and fidget cycle in the object display is separate and
only drawn: [object display](object-display.md).)

- Standing idle, he plays his shown set's first `IDLE` animation for a
  number of loops drawn at random from `idle_loops_min` up to but not
  including `idle_loops_max` (template defaults 1 and 3, which no data
  file changes: so 1 or 2 loops, 3 or 6 seconds for AA's 3-second idle).
  [code, data]
- Then he plays one `FIDGET` animation once, picked at random by its
  `weight`: each fidget's chance is its weight over the set's total
  (AA's infantry `fidget1`–`fidget5` weigh 5, 4, 3, 2, 1 and last 3, 3,
  3, 7 and 10 seconds). When it ends he goes back to idling, with a new
  loop count, and so on. [code, data]
- Fidgets only happen while the game variable `CharacterFidget` is on
  (default on; the stock data doesn't set it). With it off he idles
  without end. [code, data]
- Every animation set has at least one idle and one fidget (a set
  without is a data error), and a stance's set takes any animation it
  doesn't list from the man's own set, so AA's kneel and prone sets
  fidget with the standing fidgets. [code, data]
- A man with an attack never fidgets once he has used it: between
  shots, for the attack's `reload_time`, he plays its `idle_animation`
  (`idleattack` and the like, for the set he's drawn in), and afterwards,
  while he's engaged or bombarding, he idles with that animation instead
  of his `IDLE` and draws no fidget. Attack idles loop, so a stance
  change between shots is at once. [code, data]
  - An engaged man who hasn't fired yet still idles and fidgets as
    above, so a man drawn into a fight mid-fidget finishes it in his old
    stance: up to 10 seconds for AA's `fidget5`. [code; not seen in the
    game]
- `HIT` animations are never played when a thing is struck, in either
  game: damage starts no animation. AA's data has the template but no
  unit uses it; four Kohan II units list a `hit` (dhavrum, anvil,
  khafra, slayer) that never plays. The trigger effect
  `actor_performs_animation`, which plays any animation by its `IDS`,
  could play one; no stock ability names one. [code, data]

## A stance's property

- While a thing holds a stance, it holds that stance's `property_ids`,
  with all of that property's modifiers (speed, DV, resistances,
  anything); the property comes with the stance and goes with it, at
  the same moment the look changes. [code]
- It's given only if the thing meets the property's own
  `required_properties`, and only to the thing itself: a unit has no
  members to hand it to, and a stance never reaches the unit's regiment.
  [code]
- Holding the same property from elsewhere as well counts it once, as
  for any property ([combat](combat.md#damage)). [code]
- AA's stance properties: `infantry_kneel` −0.2 and `infantry_prone`
  −0.6 `SPEED_MAX` ([movement](movement.md#a-units-top-speed)), nothing
  else; `airborne` does nothing ([deliveries](deliveries.md)). Kneeling
  and lying prone give no defence or resistance in AA. [data]

## Look

- The stance's `aspect_ids` names one of the thing's own `[Aspect]`s
  (`AspectKneel`, `AAAspectDeath` and so on in its `[View]`), and that's
  the model it's drawn with while it holds the stance. `aspect_ids =
  NULL`, or no stance at all, is the plain look (`AspectNormal`).
  [code, data]
- One stance is held at a time, so one look shows: the stance that wins
  (Which stance applies, above). [code]
- AA's looks: `kneel` and `prone` are the plain model with the
  kneel or prone animation set; `falling` is `<resource_prefix>Falling.nif`
  with animations off; `aspect_packing` and `aspect_unpacking` are
  `…Pack.nif` and `…Unpack.nif`; `aspect_death` is `…Death.nif` (the
  wreck or ruin); `aspect_disabled` keeps the model and swaps its skin
  to `…DisabledBase.dds`. [data]
- So a destroyed vehicle, ship, bunker or packable building is drawn
  as its death model for the whole of its remains' time
  ([combat](combat.md#death)), and a bunker or packable building at 40%
  health or less in its damaged skin. [code, data; unchecked in the
  game]
- The animation set is the one the thing's character gives for that
  look (`template_aspect_animation_set_aa.tgi`, `aspect_ids`); a look
  with no set of its own uses the character's last listed set.
  [code, data]

## Regiments and entrenchment

- Regiments take no stances: no regiment template lists one, in either
  game. Every stance above is a unit's or a building's. [data]
- Entrenchment belongs to the regiment as a whole, and reaches its units
  as properties, not as a stance. [code, data]

### Becoming entrenched

- A regiment becomes entrenched once it has been resting or guarding
  (the activities whose state is `entrenchable`: `mind_resting`,
  `mind_guarding`) for its `entrenchment_time` divided by its
  `ENTRENCHMENT_RATE` (1 plus the amounts of the `ENTRENCHMENT_RATE`
  modifiers it holds). AA: 30 seconds (`AAInfantryOrganizationComponent`,
  `AAVehicleOrganizationComponent`, `AAArmorOrganizationComponent`), 24
  with `tech_entrenchment` (+0.25); ambient regiments 99999. It's
  checked once a second, so it comes up to a second late. [code, data]
- The time counts from when the regiment last went into resting or
  guarding from any other activity; going between resting and guarding
  doesn't restart it. Any spell in another activity, however short,
  restarts it: a move or guard order shows Moving for at least a moment
  even when the regiment is already where it's sent, and a fight shows
  Engaged, so a regiment that hasn't yet entrenched starts its 30
  seconds again once it's back to resting or guarding. The regiment's
  activity is the one the state line and the activity rules use
  ([combat](combat.md#guard-orders) for what Guarding covers). [code]
- While the activity is anything else (Engaged, Moving, Attacking,
  Reforming, Chasing and so on) the regiment can't become entrenched;
  the count doesn't pause and resume, it starts again as above. [code]
- A regiment counts from when it was made: one recruited during a match
  from the moment it appears, one placed on the map from the start of
  the match, so one that sits from the start entrenches 30 seconds in. A
  saved game keeps both the count and whether the regiment is
  entrenched. [code]
- A regiment whose leader is a thing that can't move never entrenches.
  [code]

### What ends it

- Entrenchment ends only when the regiment sets off along a route of
  its own: when, carrying out an order or a move it makes itself, it
  isn't yet within 0.5 of where it's going and hands its leader a
  route ([movement](movement.md#arriving)). Nothing else ends
  it: not being engaged, shot at or losing men, not its activity
  changing, not a formation change. [code]
- So these end it, as soon as the regiment starts out: [code]
  - a move order to anywhere more than 0.5 from where it stands, however
    close;
  - an attack or bombard order whose target is out of reach, as it
    closes in; a guard order whose post is more than 0.5 away;
  - a guard on a thing, once that thing moves and the post beside it
    moves more than 0.5 from the regiment; a move onto a thing to rest
    beside it, if it's not yet close enough to stop (0.6 × its
    `control_range`, [movement](movement.md#several-regiments-ordered-together)),
    or once the thing moves off and it follows;
  - the regiment chasing an enemy that pulled away, retreating, routing,
    and moving to build, repair, besiege, unpack, explore or join
    another regiment.
- And these don't: [code]
  - a move order to within 0.5 of where it stands, with or without a
    facing (a change of facing alone is such an order);
  - a guard order on its own spot, and an attack on a target already in
    reach;
  - a move onto a thing it's already close enough to rest beside
    (it stays Resting throughout, so this doesn't restart the count
    either);
  - a formation change: the regiment stays where it is and its members
    walk to their new places, the leader included;
  - members stepping out on their own to fire, close in or chase within
    a fight, and a replacement walking in to fill a slot;
  - a move that fails before it starts (no route found,
    [movement](movement.md#when-a-move-fails)). A move to somewhere it
    can't reach that heads for the nearest spot it can instead isn't a
    failure: it ends entrenchment like any move.
- What the regiment's leader or members do on their own doesn't count
  either way: entrenchment and its count go by the regiment, so members
  catching up to their places after a formation change, or the leader
  shifting while the regiment stays put, neither end it nor stop it
  coming. [code]

### What it gives

- Being entrenched is the state `organization_entrenched`
  (`actor_states_aa.tgi`). It holds two properties: [code, data]
  - `infantry_entrench` "Infantry Entrenchment" (`required_properties =
    infantry`): DV +4, `armor_piercing` and `explosive` resistance 0.40,
    `MORALE_LOSS_RATE` +0.5;
  - `vehicle_entrench` "Vehicle Entrenchment" (`vehicle`): DV +4,
    `armor_piercing` and `explosive` resistance 0.20, `MORALE_LOSS_RATE`
    +0.5.
- The regiment itself takes each property any of its members qualifies
  for, as it does terrain properties
  ([terrain](terrain.md#terrain-properties)): a slot
  that's empty counts as the unit template it's waiting for. Regiment
  templates themselves hold neither `infantry` nor `vehicle`. So an
  all-infantry regiment holds `infantry_entrench`, an all-vehicle one
  `vehicle_entrench`, and a regiment with both kinds of member holds
  both. The two are different properties, so both count: a mixed
  regiment's `MORALE_LOSS_RATE` is +1.0 and it loses morale at half the
  rate, against two thirds for one holding only one of them
  ([morale](morale.md#losing-morale)). [code, data]
- Which properties it holds is settled when it becomes entrenched and
  doesn't change while it stays entrenched: a member joining later is
  never weighed for the regiment itself, so a member of a kind the
  regiment didn't count adds no property, neither to the regiment nor
  to the joiner, until the regiment next entrenches. Losing members
  takes nothing away either. A joiner filling a slot usually brings
  nothing new anyway, since the empty slot already counted as the unit
  it was waiting for. [code]
- Each member then takes the ones it qualifies for, foot soldiers the
  first and vehicles the second, as any regiment property is handed
  down ([movement](movement.md#a-units-top-speed)); so does a member
  that joins while it lasts, of the ones the regiment holds. A member's DV and resistances come from its own
  copy. [code, data]
- Kohan II's `organization_entrenched` gives `actor_state_entrenched`,
  which has no `required_properties`: every company and every member
  takes it. [data]

### How it shows

- The regiment's property icon row ([object
  display](object-display.md#property-icons), which shows the
  regiment's own properties, not its members') gains an icon per
  property it holds: one for a regiment of one kind, two for a mixed
  one. Both properties use the same picture
  (`/properties/PropertyIcons/IconPropertyEntrenched.png`); their
  tooltips are their names, "Infantry Entrenchment" and "Vehicle
  Entrenchment", then the "Affects" line and the modifiers. [code, data]
- The regiment's banner shows its entrenched decal while it's entrenched
  (`AAEntrenchedBannerDecal`, `Shared/BannerEntrenched.png`, not
  flashing; function `ENTRENCHED` in `Templates/template_banner.tgi`), and
  drops it when entrenchment ends. [code, data]
- The regiment display's state line names the regiment's
  highest-priority state ([object
  display](object-display.md#name-owner-and-state-lines); AA turns the
  line on with `UIShouldUseStateLabel`). "Entrenched"
  (`actor_states_organization_entrenched_name`) has priority 6, so it
  shows instead of Resting (4), but Guarding (8), Engaged (12) and the
  other activities above 6 show over it. A tie goes to the state with the lower
  number ([triggers](triggers.md), `actor_has_state`), so Out of Supply
  (also 6) shows over it.
  [code, data]
- Nothing else marks it: no message, sound or effect, and the regiment's
  own tooltip has no line for it. [code, data]
- Kohan II entrenches companies by the same rules. Its
  `actor_state_entrenched`: `AV_RANGED` +3, DV +3, `DETECTION_RANGE` +3,
  `MORALE_LOSS_RATE` +0.33. `entrenchment_time` is set in every fielded
  company's unit file (30, 60, 90, 300 or 999; most often 300 or 60);
  only ambient animals and random-company placeholders leave it to
  `K2AmbientCompany`'s 9999. [code, data]

### Stance data

- `StanceEntrenchmentInfantry` and its look `aspect_entrenchment`
  (which has no model) are listed by nothing, and couldn't apply to a
  unit anyway: only regiments are ever `organization_entrenched`.
  Entrenched men keep their ordinary stances. [code, data]

## Open questions

- Check in the game that a destroyed tank or bunker shows its wreck
  model while its remains last. (analysis, 2026-11-19)
