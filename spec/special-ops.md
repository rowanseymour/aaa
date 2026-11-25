# Special operations

A side's commander (its general) and the experience it spends on the
general's special operations: where the experience starts, how it's
earned, what using an operation costs, and the in-match panel that shows
it, and what each operation does. The supply operations (Supply Drop
and the Resupply operations) are in [supply](supply.toml).

Games: traced in AA. Kohan II's data has no special operations.

## Which sides have a general

- A side has a general when it's given a `[Faction]` (a `Generals/*.tgi`
  entry). Only a side with a general has experience and special
  operations; a side without one earns nothing and shows no panel.
  [code]
- The map's own choice for a side wins. Only a side the map leaves
  without a general takes the one chosen for it in the lobby. A side
  still without one whose nation has generals is given one of that
  nation's at random. [code]
- The computer's sides follow the same rules and earn and spend
  experience exactly as players do. [code]

## Starting experience

- At the start of a match every side with a general has its experience
  set to the `StartingExperience` world rule: the lobby's Starting
  Experience option (`Game/world_rules_aa.tgi`, default 200, at most
  5000). It's then kept within 0 and the general's `experience_max`.
  [code, data]
- This replaces whatever experience the map stored for the side, so a
  scenario's saved value never reaches play; only the world rule counts.
  [code]
- The rule's value is the one chosen when setting up the match. The
  Scenario Map dialog starts it at the map's own stored rule (0 in every
  shipped scenario); the Random Map dialog starts it at the default, 200
  ([match start](match-start.md#world-rules)). [code, data]
- A saved game carries each side's experience as it was. [guess]

## Earning experience

- Experience is a fractional amount. It's always kept within 0 and the
  general's `experience_max` (5000 for every shipped general, from the
  `AAGeneral` template in `Templates/template_faction_aa.tgi`); a
  negative `experience_max` would mean no ceiling. Gains past the
  ceiling are lost. [code, data]
- Steady income: every side with a general gains
  `ExperienceFactionGainPeriodic` (`Game/SVars.tgi`, 1) per second of
  game time, continuously. [code, data]
- Its regiments' experience: whenever one of the side's regiments gains
  experience, the side gains the same amount times
  `ExperienceFactionGainMultiplier` (1). A regiment's own experience
  has no ceiling, so the full amount always passes on (what it does for
  the regiment is in [morale](morale.md#a-regiments-experience)). A
  thing that isn't in a regiment and has no `[ExperienceComponent]` of its own (a building,
  say) earns nothing, for itself or the side. Every shipped regiment,
  HQs included, has one. [code, data]
- Regiments gain experience from four events. Each is worth a base
  amount comparing the two sides' combat values
  ([combat](combat.md#combat-value)): a unit's or building's own value,
  and a regiment's full value, counting every member at full health
  and an empty slot at its template's value:

  base = 0.5 + 0.5 × victim's value ÷ gainer's value

  An even fight is worth 1, a target twice as strong 1.5, half as
  strong 0.75. A gainer with no value gets 0. A victim with no value
  counts as −1, unless its `[ExperienceComponent]` sets `fake_cv_value`,
  in which case it counts as `ExperienceDefaultValue` (1) rather than the
  value it names. No shipped component sets it, so a valueless victim
  (an unarmed building) is worth less than half. [code, data]

  | Event | Who gains | Victim | Amount |
  |---|---|---|---|
  | A kill | the killer's regiment | the thing killed (below) | base × `ExperienceKillMultiplier`, kept within `ExperienceGainMinimum` and `ExperienceGainMaximum` (0 and 10) |
  | A capture | the capturer's regiment | the thing captured (below) | base × `ExperienceCaptureMultiplier`, kept within 0 and 10 |
  | A rout | every regiment that was engaging the regiment that routs | the routed regiment | base × `ExperienceRoutMultiplier`, kept within 0 and 10 |
  | Engagement | a regiment while it stays engaged | its target (below) | base × `ExperienceEngageMultiplier` each second, not capped |

  All four multipliers are 1. [code, data]
- Kills: [code, data]
  - The killer is the enemy that last hit the thing, provided that hit
    came within `KillCreditTimeout` (`Game/SVars.tgi`, 2 seconds) of its
    death; otherwise nobody is credited. Only hits from enemies count,
    and the killer must still be an enemy when the thing dies.
  - The same killer is the one that gains kill morale
    ([morale](morale.md#gaining-morale)). With no killer (no enemy hit
    in the last 2 seconds: it died of supply shortfall, say, or of hits
    that were all older) no regiment gains anything.
  - An enemy strike's hit counts as a hit by the side that used it. It
    replaces whatever unit hit came before it, so a thing a strike hits
    within 2 seconds of dying is the side's kill, whoever fired the
    last shot. No regiment gains experience or morale from it.
  - Killing a unit compares the unit's own value with the killing
    unit's own value, and pays the killer's regiment.
  - When the unit killed was the last of its regiment, the regiment
    dies too, and that is a second kill, paid straight after the first:
    the dead regiment's full value (by then every slot is empty, so the
    sum of its slots' template values) against the killer's regiment's
    full value. The killer is whoever last hit any of its members, which
    is normally the one that killed the last unit. So the last kill of a
    regiment pays twice.
  - Destroying a building is a kill like any other, compared with the
    building's own value; an unarmed building has none, so it's worth
    the valueless amount above (0.5 − 0.5 ÷ the killer's value, never
    below 0). A building that is disabled rather than destroyed hasn't
    died, and pays nothing. An unpacked building that is a regiment's
    member counts as one of its units.
  - A killer that isn't in a regiment (a bunker, a defending building)
    earns nothing.
- Captures: [code]
  - A building captured by an attack pays the regiment of the unit whose
    hit captured it, comparing the building's own value with that
    regiment's full value.
  - A city that changes hands because of who holds its zone is taken by
    a side, not by a regiment: no regiment earns capture experience for
    it, whoever stands in the zone, and the side gains nothing from it
    either.
- Engagement: [code]
  - The gain is paid on the regiment's own once-a-second beat, a whole
    second's worth each time, while it is engaged. The first beat on
    which it is found engaged pays nothing; each beat after that, while
    it stays engaged, pays. So a fight is worth nothing for its first
    second or so, and a fight that ends within a second pays nothing.
  - It compares the regiment's full value with the value of the first
    of the things it is engaging: an enemy regiment's full value, or a
    lone thing's (a bunker's, say) own.
- Gains below 0.0001 are dropped. [code]
- A trigger's `actor_gains_experience` on a regiment passes on to the
  side in the same way, × `ExperienceFactionGainMultiplier`, and counts
  towards `EXPERIENCE_GAINED`; one that names anything else gives the
  side nothing, and so does a value below 0.0001
  ([triggers](triggers.md#morale-and-experience)). [code]
- Only gains add to the scoring's `EXPERIENCE_GAINED`; spending adds to
  `EXPERIENCE_SPENT`. [code]

## Using an operation

- An operation can be used when it has recharged and the side's
  experience is at least its `cost_faction_experience` (from
  `Templates/template_ability_special_op_aa.tgi`; equal is enough). Using
  it takes the cost from the side's experience at once. Experience can't
  go below 0 this way, since a side short of the cost can't use it.
  [code, data]
- Pressing an operation that isn't ready is refused with a message and
  nothing is spent: [code, data]
  - still recharging: `ui_error_faction_ability_recharge_time` ("Special
    Operation %s has not yet recharged");
  - short of experience: `ui_error_faction_ability_cost` ("Insufficient
    experience to use Special Operation %s");
  - an operation that summons something unique the side already has
    (`summon_unique`): `ui_error_faction_ability_already_summoned`
    ("Special Operation %s can not summon an additional %s").

  `%s` is the operation's name. The recharge check comes first. [code]
- Recharge: after use the operation is unavailable for its definition's
  `recharge_time` (`SpecialOps/*.tgi`, such as 300 for Armored Resupply),
  or `attack_time` + `reload_time` if that's longer (0 for shipped
  operations). The time counts in game seconds. [code, data]
- Every operation is ready at the start of a match: none starts
  recharging. [code]
- An operation can also be held back by what it summoned (see
  [Summoning more than one](#summoning-more-than-one)). [code, data]

## Giving an operation

- Pressing a ready operation (its button or key) does one of two things,
  decided by its `targets`: [code, data]
  - With `self` among them (Radar Scan: `+ACTOR, +SELF, +INVULNERABLE`)
    it's used at once, with no aiming.
  - Otherwise the cursor is armed for it, and the next left click in the
    world gives it.
- While armed, an operation with an `area` above 0 shows its
  `target_location_vfx` on the ground at the cursor: `crosshair_decal_vfx`
  (`Templates/template_abilitydefinition_aa.tgi`) unless the operation
  sets its own, as the friendly operations do with
  `friendly_target_decal_vfx`. An operation with no area (Supply Drop,
  the summons, Lend Lease) shows nothing, whatever its `volley_area` or
  `target_location`. [code, data]
- The marker is a square ground decal 2 × `area` world units on a side,
  so `area` is the radius it marks: Carpet Bomb's 16 is 32 across,
  Double Guard's 30 is 60. It replaces the decal's own `texture_size`
  (1 in both). [code, data]
- It's centred on the ground point under the cursor and moves with it
  every frame. It lies on the terrain and follows its slopes like any
  ground decal (see [Ground decals](effects.md#ground-decals)), drawn as
  a `ui` decal: unshaded, over every decal but shadows. It isn't turned: it
  keeps the default facing, with the texture's top towards map +y, and
  doesn't spin. `crosshair_decal_vfx` is alpha-blended,
  `friendly_target_decal_vfx` additive. [code, data]
- It looks the same whatever is under the cursor: nothing changes it
  over a valid or invalid target. It goes when the cursor is disarmed.
  [code]
- The click: [code]
  - On a thing that matches the operation's `targets`, judged from the
    side's point of view (`+FRIEND +ORGANIZATION +ORG_TANK` for Armored
    Resupply: a friendly armour regiment), it's used on that thing.
  - On a thing that doesn't match, nothing is given.
  - On the ground (no thing under the cursor), it's used at the clicked
    point if the operation has an area or `target_location` (Supply
    Drop, whose `targets` is `NULL`, is always given this way). An
    operation with neither gives nothing.
  - Either way the cursor is disarmed by the click. A click that gives
    nothing spends nothing and shows no message.
- The checks in [Using an operation](#using-an-operation) are made again
  when the operation is carried out, so it fails, costing nothing, if
  the side can no longer afford it by then. [code]

## What an operation reaches

- An operation works around its centre: the clicked point, or the
  clicked thing's centre when the click lands on a thing that matches
  its `targets`. `range` (10 in the template) plays no part: an
  operation can be given anywhere on the map. [code, data]
- What it works on is decided thing by thing from its `targets`, judged
  from the user's side (`+FRIEND` its own side and allies, `+ENEMY`
  enemies; an enemy's Corps HQs, Corps HQ trucks, HQ landing ships and
  battleships count as `+FRIEND` too, [combat](combat.md#a-things-target-types)). An operation with an `area` works on every thing that
  matches and stands within `area` of the centre, measured to the
  thing's edge as for [area attacks](combat.md#area-attacks): the
  centre-to-centre distance less the thing's `soft_radius`. Only
  targetable things are caught. Height plays no part, so aircraft in
  flight overhead are caught when their types match, as are men coming
  down under parachutes. [code]
- With `max_targets` above 0, only that many are taken, the nearest by
  centre-to-centre distance (no radius). A thing that ends up getting
  nothing (it already holds the enchantment, or is immune) still takes
  a place. Without `max_targets` (Sand Bagging, Double Guard, the
  strikes) every thing caught is taken. [code, data]
- `-ORGANIZATION` in `targets`, or a required type no regiment has
  (`+TANK`, `+INFANTRY`, `+VEHICLE`, `+BUILDING`), means the operation
  works on units and buildings one by one: each soldier or vehicle of a
  regiment is a target of its own. Every operation below does; only the
  Resupply operations work on whole regiments. [code, data]
- Given straight onto a thing (not the ground), an enchanting operation
  is called off, costing nothing, starting no recharge and showing no
  message, when that thing already holds the operation's property, or
  when it's not of the user's side or an ally's and is immune to every
  effect of the operation (only Smoke Barrage can be stopped this way,
  on an enemy under Courage, say). Clicking the ground beside it works.
  The Resupply operations behave the same way on a regiment still
  holding their property. [code]

## Enchantments

Blitzkrieg, Communist Zeal, Courage, Banzai Charge, Purge,
Demoralization, Propaganda War, Smoke Barrage, Forced March, Mechanized
Advance, Sand Bagging and Double Guard each give a timed property (an
`EnchantmentEffect`'s `[EnchantmentProperty]`) to the things they reach,
some with a morale effect or damage alongside.

### Giving and keeping the property

- Each thing reached gets its own copy of the property. It starts when
  the operation takes effect (at once, except Propaganda War, below) and
  lasts `duration` seconds of game time. It's dropped at the thing's
  first once-a-second check of its properties after that, so it lasts
  `duration` to `duration` + 1 seconds. [code]
- A thing already holding that same property, from anyone, gets nothing
  new: the time doesn't start again and nothing stacks. Different
  properties add up, even of the same `enchantment_type`: Courage and
  Banzai Charge both apply to one unit, and German and American
  Propaganda War are separate properties, so both can hold at once.
  [code, data]
- `required_properties` on the property (Blitzkrieg's `tank`, for
  example) isn't checked when an operation gives it: `targets` alone
  decides. [code, data]
- The property belongs to the unit or building, not its regiment: men
  who join the regiment later, replacements included, don't have it, and
  it goes when the thing dies. A unit that leaves its regiment keeps it
  for the rest of its time, and so does one that joins another. Nothing
  ends it early on a rout. [code]
- Casting an operation again where its property still holds is never
  held back to wait for it to run out: the operation goes ahead at once
  (or is called off, given straight onto such a thing, as in [What an
  operation reaches](#what-an-operation-reaches)), and the things
  already holding it keep their old time. The waiting rule in
  [combat](combat.md#choosing-an-ability) is for units choosing their own
  abilities, and no operation is chosen that way. [code]
- Each thing gets the property's `WhenAdded` effects as it gains it,
  attached to it; a command's `target_flags` picks by the thing (Courage
  shows its small cloud on `+infantry`, its large one on `+vehicle`).
  Every lasting visual among them (tints, ground decals, cloud and flag
  models) ends with the property. [code, data]
- When the property ends its modifiers go with it: speed returns, and
  maximum health is rescaled back keeping the share of health (never
  below 1). [code]

### Enchantment types and immunity

- `enchantment_type` (`ench_type_generic`, `ench_type_courage`,
  `ench_type_fear`, `ench_type_radiation`) does nothing by itself: it's
  the name an immunity refers to. [code, data]
- An `EnchantmentImmunityModifier` for a type, held by a thing: [code]
  - stops new properties of that type reaching it from a side that isn't
    its own or an ally (enemy or neutral); its own side's and allies'
    still land;
  - when the immunity arrives, removes at once any properties of that
    type the thing holds that came from such a side, their effects and
    visuals with them;
  - blocks only properties: morale loss and damage from the same
    operation still apply.
- So the friendly operations that grant immunity to `ench_type_fear`
  (Blitzkrieg, Communist Zeal, Courage, Banzai Charge, Purge, Mechanized
  Advance) clear Demoralization, Propaganda War and Smoke Barrage from
  what they reach and keep them off while they last. Every building is
  immune to `ench_type_radiation` through its `building` property.
  [code, data]
- Those are the only immunities in AA's data: no unit is immune to
  anything by itself, so a unit is immune to the fear operations only
  while it holds one of those friendly properties, and nothing is immune
  to Forced March, Sandbagging or Double the Guard. [data]
- Immunity shows only as the property that carries it: its icon in the
  thing's row of property icons, and in that property's tooltip the
  "Immune to …" line ([Effect lines](#effect-lines)). Nothing is drawn on
  the unit for it, and a blocked operation shows nothing on the units it
  didn't reach. [code, data]

### What the modifiers do

- `AddModifier AV` +n: each attack the holder makes does n more main
  damage, melee and ranged alike; it doesn't add to extra damage. [code]
- `AddModifier DV` +n: n more defense against main damage (never below
  0 in all). [code]
- `MultiplyModifier` a: the value times (1 + the sum of the holder's
  multipliers for that attribute), never below 0.05 times
  (`ModifierAttributeClampMultiply`). [code, data] The two attack
  efficiency attributes don't add up this way
  ([combat](combat.md#attack-efficiency)).
  - `SPEED_MAX`: top speed, added to the unit's other speed
    multipliers (formation, stance, state, technology) as
    [movement](movement.md#a-units-top-speed) describes. Terrain speed
    caps still apply. A regiment moves at its slowest member's top
    speed ([movement](movement.md#a-regiments-speed)), so speeding up
    only some of its men (Forced March catching half of it) gains it
    nothing unless every one of its slowest men is reached, while
    slowing any of them (Smoke Barrage, Propaganda War) slows the whole
    regiment. [code]
  - `HEALTH`: maximum health, keeping the share of health: a unit at
    full health gains that much health, and loses it again at the end.
  - `RECHARGE_TIME`: works as attack speed. The time between the
    holder's attacks is divided by (1 + a), so +0.5 attacks 1.5 times as
    often and −0.5 half as often. [code]
- `AbilityEffectModifier` with `ability_type = attack`: every attack the
  holder makes also does the effect to each thing the attack hits
  (Communist Zeal's and Mechanized Advance's extra `armor_piercing`
  damage, times the target's resistance, no defense). [code, data]
- `ResistDamageModifier`: adds to the holder's resistance to that damage
  type (a building's own 0.20 against `armor_piercing` becomes 0.60
  under Sand Bagging's 0.40). [code, data]
- `PeriodicDamageModifier`: that much damage of its type once a second,
  times resistance, no defense. [code]
- `MoraleGainEffect` / `MoraleLossEffect` beside the enchantment: each
  unit reached raises or lowers its own regiment's morale by `amount`,
  so a regiment with 8 men caught moves 8 times the amount. A unit with
  no regiment, and a building, has no morale to change. How morale
  moves and what a fall does is in [morale](morale.md). [code]
  - It's a single change, made as the operation takes effect (for
    Propaganda War, as it lands), not spread over the property's time.
    [code]
  - It's made whatever the regiment is doing: engaged, routing or
    Exhausted alike. The limits are morale's own
    ([morale](morale.md#losing-morale)): a gain stops at the maximum
    and doesn't end a rout, a loss stops at 0 (so an Exhausted regiment
    has nothing left to lose), and an unpacked regiment loses none. A
    loss that takes a regiment to its breakpoint routs it even
    mid-fight ([morale](morale.md#breaking)). [code]

### The operations

Costs are `cost_faction_experience` in
`Templates/template_ability_special_op_aa.tgi`; the rest is each
`SpecialOps/*.tgi`. Display names are the operation's `name`. [data]

| Operation (`IDS`) | Cost | Recharge | Works on | Area | At most | Lasts |
|---|---|---|---|---|---|---|
| Blitzkrieg (`blitzkrieg`) | 1000 | 420 | friendly tanks | 20 | 16 | 120 |
| Communist Zeal (`communist_zeal`) | 1000 | 420 | friendly infantry | 20 | 36 | 120 |
| Fighting Spirit (`courage`) | 300 | 300 | friendly units, not buildings | 20 | 24 | 60 |
| Banzai Charge (`banzai_charge`) | 1000 | 300 | friendly units, not buildings | 20 | 28 | 60 |
| Communist Purge (`purge`) | 200 | 240 | friendly units on foot | 20 | 32 | 30 |
| Demoralization (`fear`) | 800 | 240 | enemy units, not buildings | 15 | 24 | 20 |
| Propaganda War (`propaganda_war_ger`, `propaganda_war_usa`) | 800 | 300 | enemy units, not buildings | 15 | 24 | 20 |
| Smoke Barrage (`smoke_barrage`) | 600 | 300 | enemy units, not buildings | 15 | 24 | 30 |
| Forced March (`forced_march`) | 200 | 180 | friendly units, not buildings | 15 | 24 | 30 |
| Mechanized Advance (`mechanized_advance`) | 1000 | 360 | friendly vehicles that aren't tanks | 20 | 24 | 120 |
| Sandbagging (`sand_bagging`) | 300 | 480 | friendly buildings | 20 | all | 180 |
| Double the Guard (`double_guard`) | 400 | 480 | friendly buildings | 30 | all | 180 |

"Friendly units" takes in everything that isn't a building: infantry,
vehicles, trucks, and ships or aircraft caught in the area. [data]

- Blitzkrieg: AV +50, speed +20%, immune to fear. Each tank glows
  (`blitzkrieg_tank_glow`) over a `speed_decal`. [data]
- Communist Zeal: +3 `armor_piercing` extra damage on every attack,
  health +40%, speed +20%, immune to fear; the same glow, with
  `speed_decal_small`. [data]
- Fighting Spirit: DV +2, speed +20%, immune to fear, and +1 morale to
  the regiment for each unit reached, given even to a unit that already
  holds the property. `courage_glow`, with `courage_cloud_small` on
  infantry or `courage_cloud_large` on vehicles. [data, code]
- Banzai Charge: attacks 1.5 times as often (`RECHARGE_TIME` +0.5),
  health +50%, immune to fear. `banzai_charge_glow`, with
  `banzai_decal` on infantry or `banzai_decal_large` on vehicles. Its
  description speaks of regiments, but like the rest it works on the
  units. [data, code]
- Communist Purge: first 800 `gunfire` damage to each unit caught that
  is a regiment's commanding officer (the `captain`), which kills an
  infantry officer (200 health). This effect is hidden from the
  tooltip. Then every unit caught gets "Communist Inspiration": AV +5,
  immune to fear, and +1 morale to its regiment per unit.
  `banzai_charge_glow` with `speed_decal_small`. [data, code]
  - The officer's death is an ordinary death for its regiment: it costs
    the regiment the usual death morale
    ([morale](morale.md#losing-morale)), alongside the +1s the
    survivors give. [code]
  - Being shot by its own side doesn't count as being attacked: it
    doesn't engage or provoke the regiment, and nobody is credited with
    the kill. The officer counts as killed by an enemy only if an enemy
    hit it within the usual 2 seconds before
    ([Earning experience](#earning-experience)); that enemy is then the
    killer as for any death. Forced March's flame damage is the same:
    a unit it finishes off has no killer of its own. [code]
- No operation's damage earns any regiment experience or kill morale:
  a friendly operation's isn't an enemy hit at all, and an enemy
  strike's is credited to the side, as [Earning
  experience](#earning-experience) describes. [code]
- Demoralization: attack rate × 0.7 (`RECHARGE_TIME` −0.3), and 3
  morale lost by the regiment per unit caught, even by units immune to
  fear. A fear cloud (`fear_cloud_small` on infantry,
  `fear_cloud_large` otherwise) and a `rout_flag` over each, the
  `fear_glow` tint, and `fear_sfx` from each unit enchanted. It plays
  only `AreaEffect_vfx` at the centre, no sound there. [data, code]
- Propaganda War: DV −2, speed −20%, and 4 morale lost per unit;
  `cowardice_glow`. It takes effect 2 seconds after use, when its
  projectile (`projectile_life` 2) lands; its sound and bomber
  (`bomber_drop_ger` / `bomber_drop_usa`) play on use. The two nations'
  versions differ only in the bomber and the property's id. [data,
  code]
- Smoke Barrage: attack rate halved (`RECHARGE_TIME` −0.5), speed −50%.
  A `smoke_cloud` over each and a grey `smoke_tint`. Units immune to
  fear get nothing from it. [data, code]
- Forced March: speed +50%, and 1 `flame` damage a second for its 30
  seconds (about 30 in all, enough to finish off a badly hurt unit).
  Red tint `Forced_red`. [data, code]
- Mechanized Advance: +10 `armor_piercing` extra damage on every attack,
  DV +5, speed +20%, immune to fear, `speed_decal`. It reaches every
  vehicle that isn't a tank: halftracks, and trucks, jeeps, HQ trucks
  and armoured cars too. [data, code]
- Sandbagging: resistance +0.40 against `gunfire`, `armor_piercing`
  and `explosive`; `sand_bagging_glow`. [data]
- Double the Guard: AV +15, which only a building with weapons gains
  anything from; `sand_bagging_glow`. [data]
- Each operation's `WhenDo` sound and visual (Blitzkrieg's
  `blitzkrieg_sfx` and `blitzkrieg_eagle`, and so on) plays once per
  use, on the ground at the centre, as it's used. [code, data]

## Strikes

Carpet Bomb (`gbr_carpet_bomb`, `rus_carpet_bomb`, `usa_carpet_bomb`),
Kamikaze, V-Weapons and Atomic Bomb do damage over an area.

| Operation | Cost | Main damage | Extra against buildings | `area` | `attenuation_range_min` | `attenuation_factor_max` |
|---|---|---|---|---|---|---|
| Carpet Bomb | 1800 | 400 `explosive` | 1200 `explosive` | 16 | 4 | 0.25 |
| Kamikaze | 1600 | 400 `armor_piercing` | 800 `flame` | 12 | 2 | 0.5 |
| V-Weapons | 1800 | 600 `explosive` | 600 `explosive` | 12 | 3 | 0.5 |
| Atomic Bomb | 2000 | 500 `flame` | 900 `explosive` | 20 | 4 | 0.25 |

All recharge in 300 seconds. [data] The three Carpet Bombs differ only in
the bombers drawn. [data]

- Aiming: at a point on the ground, or at a thing matching `targets`
  (`+ACTOR -ORGANIZATION -AIR`; the Atomic Bomb without `-AIR`): any
  unit or building, the user's own included, but not an aircraft for
  the first three. [code, data]
- When it lands: Carpet Bomb, Kamikaze and V-Weapons land 3 seconds of
  game time after use, whatever the distance (their projectiles'
  `projectile_life` 3). The Atomic Bomb has no projectile and lands the
  moment it's used. [code, data]
- Given on a thing, the strike follows it and lands where it is after
  the 3 seconds. If the thing is gone by then, it lands where the thing
  was last; if it's still there but no longer matches `targets` or
  can't be targeted, nothing happens at all. [code]
- One blast, at one point: no volleys or spread. Carpet Bomb's carpet is
  only its picture and its staggered sounds. [code, data]
- What it hits: every targetable thing within `area` (edge distance)
  matching `targets`, with no limit on how many. There's no friend or
  enemy test: the user's own and allies' units and buildings are hit as
  hard as the enemy's. Regiment members are hit one by one. [code,
  data]
- Damage to each: the main damage as in [combat](combat.md#damage) with
  efficiency 1, less defense, times resistance, times falloff (as for
  [area attacks](combat.md#area-attacks)), at least
  `CombatDamageMinimum`. Things with the `building` target type also
  take the extra damage times resistance, with no falloff, defense or
  minimum. Kamikaze's main hit on a building is cut by the building's
  0.20 `armor_piercing` resistance. [code, data] For Carpet Bomb at 10
  from the centre, the falloff is 1 − 0.75 × (10 − 4) ÷ (16 − 4) =
  0.625.
- The user's side as a whole is the attacker, not a unit or regiment,
  and the hit counts as an operation's, not a shot: [code]
  - it's never a bombardment hit: no one is put out of supply by it, and
    no one is remembered as having bombarded what it hit
    ([supply](supply.toml#bombardment));
  - nothing it hits fires back, engages or is told it was fired on: it
    draws no fire, whoever's thing it hits;
  - it never captures. A building that can be captured
    ([buildings](buildings.md#capture-by-damage)) and is brought to no
    health is left with 1 health, disabled but not destroyed, whoever
    owns it; one that is being razed is not spared. A building that
    can't be captured dies or is disabled at 0 health as from any damage
    ([combat](combat.md#death)). The user's own and allies' buildings are
    treated the same way as the enemy's.
- Hits on an enemy of the user's side are enemy damage, so they put the
  building or unit hit under siege, or keep it there, for
  `UnderSiegeRecentlyDamagedSeconds` (7) as any enemy hit does
  ([buildings](buildings.md#under-siege)). Hits on the user's own side's
  or an ally's things don't. [code]
- `impact_impulse` (10000) pushes scenery as in [scenery](scenery.md),
  and gives each thing hit that allows it a visual shove away from the
  centre, the same at any distance. It changes nothing in play. [code,
  data]
- Kills by a strike give no one experience or kill morale: no regiment
  made the kill ([Earning experience](#earning-experience)). [code]
- Sight: on use, each strike also places
  `thing_visibilityactormedium_decay` at the centre: an invisible,
  untargetable thing of the user's side that sees 30 around it, sight and
  remote detection alike. It's placed the moment the operation is used,
  before any blast. It loses 1 of its 5 health at each of its
  once-a-second property checks, the first of which comes at a random
  moment within a second of its placing, and dies at the check that takes
  it to 0, the fifth; its sight goes with it. So it shows the area for
  4 to 5 seconds from use: until 1 to 2 seconds after the blast of the
  first three, and the whole time after the Atomic Bomb's. [code, data]
- Atomic Bomb's radiation: every thing its blast hits also gets the
  `radiation` property (`ench_type_radiation`, `duration` 10), whatever
  the falloff: 15 `flame` damage a second, times resistance, no defense,
  for its 10 seconds (about 150 in all). It's carried by the thing, not
  left on the ground: nothing that enters the area later catches it.
  A second bomb doesn't renew it. Buildings are immune. The thing shows
  `atomic_radiation`, with `atomic_sparks_large` on vehicles or
  `atomic_sparks_small` on infantry. [code, data]
- Sound and pictures, all on the ground at the centre and timed from
  the use by their `delay`: [code, data]
  - Carpet Bomb: the nation's bombers and `carpet_bomb_sfx` at 0, then
    light, medium and heavy bomb explosions at 3, 3.2 and 3.6.
  - Kamikaze: `kamikaze_zero` and `kamikaze_sfx` at 0, a heavy explosion
    at 3.
  - V-Weapons: rockets at 0, 0.5 and 0.8, `v_weapons_sfx` at 0, the
    explosion sound at 2.8 and the impact (`v1_scorch`, `v1_shockwave`,
    `v1_groundfire`, `v1_explosion`) at 3.
  - Atomic Bomb: `atomic_shockwave`, `atomic_scorch` and
    `atomic_bomb_sfx` at 0, with the blast.

## Summons

Maskirovka, Panzer Hohle, Secret Agent, Sabotage, Reorganization and
Code Breaker bring something new onto the map for the user's side, as
do some of the economy operations below.

### Where and how it arrives

- What's summoned belongs to the side that used the operation. It costs
  nothing beyond the experience and doesn't check the side's regiment or
  building limits. [code]
- It's placed at the centre (a summon given on a thing goes to that
  thing's position), facing the map's default direction (0, −1). [code]
- An `ethereal` thing (Code Breaker's tower, Lend Lease's money,
  Assembly Line's parts) goes exactly there. [code, data]
- A single thing that isn't ethereal (Reorganization's truck, the
  crates) goes to the nearest clear spot: the point itself, then rings 1,
  2 and so on out to 12 world units, 8 spots to a ring. Each ring's first
  spot is straight out from the centre in the direction (0, −1), and each
  next one is turned a further 45° the same way, (0, −1) to (−0.71,
  −0.71) to (−1, 0) and on round. [code]
- Clear is the test of [room to stand](objects.md#room-to-stand) for the
  thing's own shape: on the map, every tile it overlaps passable (land,
  or water for a `water_based` thing), and no `blocking` thing's shape
  overlapping it. Units count as blocking things here, as well as
  buildings and scenery. Fog and exploration play no part: a spot no one
  has seen counts the same. [code]
- A regiment (Maskirovka's and Panzer Hohle's decoys) isn't looked for as
  a whole and needn't find room for its whole formation: [code]
  - its position is where its leader fits: the point, else the same
    rings, 10 of them, spaced by the leader's own blocking radius (at
    least a quarter tile), each first spot straight out in (0, −1) and
    turning as above. No spot means the summon fails (below);
  - the regiment faces (0, −1), and each other member is placed as a
    lost member's replacement is
    ([supply](supply.toml#what-supply-gives)): out of a nearby
    friendly building that recruits its kind, if there is one, otherwise
    at its own place in the formation, or failing that the nearest clear
    spot to it, out to 10 rings spaced by its own blocking radius;
  - a member that finds no room isn't made. [code] The regiment then
    refills that place as it would a lost member. [guess]
- With `primary_island_only` (Maskirovka, Panzer Hohle, Reorganization)
  the point must be on the map and on its main land mass. Otherwise the
  operation is refused with `ability_effect_invalid_location` ("Invalid
  location for %s", the operation's name) and nothing is spent. [code,
  data]
- The main land mass is worked out from the terrain alone, tile by
  tile: [code]
  - each tile is land (passable on land, not on water), water (on water,
    not on land), impassable (neither), or, if it's passable both ways,
    whatever its terrain type is (land, mountain or water). Only the
    terrain and the map's own tile flags decide; buildings, scenery,
    bridges, height and slope play no part;
  - neighbouring tiles of the same kind make up one body, joined along
    their sides, not at corners alone [guess];
  - the main land mass is the land body with the most tiles; of two the
    same size, the one the game numbered first wins (a tie no shipped map
    is likely to meet). Nothing in the map file
    marks it, and start positions and cities don't count;
  - the test is on the aimed point's tile: the point must lie on a tile
    of that body. On a map of islands only the largest island will do,
    and a point on water, on impassable ground or on a smaller island is
    refused;
  - a map with no land at all has no main land mass, so these
    operations are always refused there.
- The game works this out when it prepares a map and stores it with the
  map; loading a map from before the stored form recomputes it. The
  stored and recomputed results can differ by a few small slivers of
  land, which matters only when two bodies are almost the same size.
  [code]
- If no clear spot is found, the side is told
  `ability_effect_summon_failed` ("Failed to summon %s", with the
  summoned thing's template `name`, as its tooltip title shows it, not a
  numbered name or its id) and nothing appears; the experience is spent,
  the recharge has started and the sounds have played. [code]
- A thing with a `transport_actor` (the Secret Agents, the saboteurs,
  the Economic Aid and Military Gear Up crates) is flown in by that
  aircraft (the nation's `<nation>_transport_air`) and comes down by
  parachute, as [deliveries](deliveries.md#special-operations-and-triggers)
  describes for operations: the plane comes in along one map axis from
  the edge farthest from where it's going, and nothing else about its
  route or drop differs from a summoned delivery. Where it's going: [code]
  - a crate (a single thing) is first given the clear spot above; with
    none, the summon fails then and there, everything spent and no plane
    sent. The plane flies to the spot found, and the crate's landing
    looks for room again as it comes down;
  - a regiment (an agent, a saboteur) gets no search at use: the plane
    always flies, straight to the aimed point, and room is looked for
    only when it unloads there, as for any delivered regiment.
- Everything else appears at once. [code, data]
- `controllable`: a summon placed directly with `controllable = false`
  (the default) can't be selected by anyone: Code Breaker's tower, Lend
  Lease's money, the pallets. Maskirovka, Panzer Hohle and
  Reorganization set it true, so theirs are ordinary selectable things.
  A flown-in summon is commanded like any regiment either way: the flag
  goes on its aircraft. [code, data]
- A summon lasts as long as its data lets it: one that loses health
  each second through a periodic-damage property lives health ÷ amount
  seconds unless something else hurts it. [code, data]

### Summoning more than one

- `summon_multiple` is true in the `AASpecialOp` template, so every
  operation has it except Reorganization. [data]
- Without it, the operation isn't ready while anything it summoned in
  place still exists, even if it has since changed hands; it's ready
  again (recharge allowing) once that thing is gone, destroyed or
  expired. Flown-in summons never count. [code]
- `summon_unique` (Reorganization): the operation isn't ready while the
  side itself owns anything matching its `unique_target_flags`
  (`provides_corps`: a Corps HQ, a Corps HQ truck, an HQ landing ship,
  and every battleship). A Corps HQ captured or destroyed frees it.
  [code, data]

### The operations

| Operation (`IDS`) | Cost | Recharge | Summons |
|---|---|---|---|
| Maskirovka (`maskirovka`) | 500 | 300 | `regiment_rus_tank_maskirovka` |
| Panzer Hohle (`panzer_hohle`) | 500 | 300 | `regiment_ger_tank_hohle` |
| Secret Agent (`gbr_secret_agent`, `usa_secret_agent`) | 200 | 240 | `regiment_gbr_agent`, `regiment_usa_agent` |
| Sabotage (`gbr_sabotage`, `rus_sabotage`) | 800 | 300 | `regiment_gbr_saboteur`, `regiment_rus_saboteur` |
| Reorganization (`<nation>_reorganization`) | 1000 | 3600 | `<nation>_truck_corps` (`<nation>_truck_corps_dynamic` in the World War campaign) |
| Code Breaker (`code_breaker`) | 200 | 180 | `code_breaker_tower` |

[data]

- Maskirovka and Panzer Hohle: a decoy tank regiment appears at once,
  with `tank_summon_sfx`. It has the name, look, icon and layout of the
  nation's medium tank regiment. Its three front tanks are fakes: 100
  health, defense 1, a gun doing 5 `armor_piercing` and 0.25 morale
  loss at range 16 every 6 seconds (a real medium tank has 440 health,
  defense 7, and 70 at range 20). Its captain is a real tank commander.
  Upkeep 5 ammo and no oil; it takes one place in the regiment limit;
  it lasts until destroyed. Each member shows `maskirovka_shell` as it
  appears. [data]
- Secret Agent: one unarmed agent, parachuted in: speed 2.5, 200
  health, sight 16, no upkeep and no place in the limit. It loses 2
  health a second, so it lives 100 seconds. It's `invulnerable`, which
  doesn't stop damage but means defending buildings never choose it as
  a target. The British and American agents differ only in sounds and
  aircraft. [data, code]
- Sabotage: one saboteur, parachuted in: 200 health, defense 5, speed 3,
  40% resistance to `gunfire`, no upkeep or limit, 2 health lost a
  second (100 seconds). Its one attack, on an enemy building not
  already sabotaged and within 0.25, once every 10 seconds: 1000 fire
  damage and the `property_sabotage` property for 60 seconds (supply
  range −100, 10 `flame` damage a second, and the building counts as
  `sabotaged`), with the `dynamite` sound and `sabotage_dynamite`
  visual. [data]
- Reorganization: a Corps HQ truck appears at once. It's an ordinary
  truck that unpacks into a Corps HQ. [data]
- Code Breaker: a tower at the exact point, with `code_breaker_sfx`. It
  sees 40 around it for its side (remote detection 10), can't be
  targeted, selected or damaged by anyone, and doesn't block. It loses
  1 of its 15 health a second, so it's gone after about 15 seconds.
  [data, code]
- The tower is neither a building nor a unit: a plain thing with health,
  sight and nothing else. [code, data] So:
  - it neither gives nor needs supply, has no upkeep, and takes no place
    in any limit;
  - it doesn't keep its side in the game: a side left with only a tower
    is beaten as if it had nothing ([outcome](outcome.md));
  - it can't be captured, and when its health runs out it's simply
    gone, with no "destroyed" or "lost" line and nothing in the score;
  - it isn't on the minimap, not even during Radar Scan, and has no
    `building` target type, so nothing aimed at buildings can name it;
  - it gets none of a building's states (under siege, out of supply).


## Drops and economy

| Operation (`IDS`) | Cost | Recharge |
|---|---|---|
| Lend Lease (`economic_aid_money`) | 400 | 300 |
| Economic Aid (`<nation>_economic_aid`) | 500 | 600 |
| Military Gear Up (`ger_military_gear_up`, `rus_military_gear_up`) | 300 | 480 |
| Rapid Deployment (`rapid_deployment`) | 300 | 420 |
| Assembly Line (`assembly_line`) | 300 | 600 |
| Espionage (`espionage`) | 400 | 300 |
| Industrial Sabotage (`industrial_sabotage`) | 500 | 300 |

[data]

- Lend Lease: an invisible thing (`economic_aid_money`) appears at the
  point and produces 1000 money a minute for its side. It loses 1 of
  its 6 health a second, so it produces for about 6 seconds, roughly
  100 money, and goes. Nothing is picked up. `money_jingle_sfx` and
  `filch_assets_cloud` at the point. [data, code]
- Economic Aid, Military Gear Up and Supply Drop first set off a hidden
  smoke grenade at the point (smoke and `smoke_grenade_sfx` for 75
  seconds; it can't be targeted and doesn't block), then the nation's
  aircraft parachutes in a crate. [data, code]
- Economic Aid's crate (`<nation>_economic_aid`) produces 15 ammo and
  15 oil a minute for its side for as long as it stands. It doesn't
  wear out: it has 1500 health and no defense, can be shot by enemies,
  and can't be captured. [data, code]
- Military Gear Up's crate (`ger_infantry_supplies`,
  `rus_infantry_supplies`), Rapid Deployment's pallet
  (`deployment_crates`, appearing at once at the nearest clear spot)
  and Assembly Line's parts (`assembly_parts`, appearing at the exact
  point) each give the side a technology while they stand. Each has
  3000 health and no defense, can be shot, and loses 10 a second, so it
  lasts 300 seconds. [data, code]
  - Military Gear Up: `military_gearup_tech`, infantry +20% maximum
    health.
  - Rapid Deployment: `rapid_deployment_tech`, `RECRUIT_COST` +0.10 on
    things with `hq`. Only the Division HQ regiments carry `hq`, and a
    recruit's cost goes by the recruiting building's multipliers, which
    the Division HQ buildings never get, so it changes no cost
    ([economy](economy.md#paying-for-commissions)). [code, data]
  - Assembly Line: `assembly_line_tech`, vehicles +15% maximum health.
  - The technology is gained when the crate appears (a falling crate
    already counts) and lost when it goes, with the usual "technology
    gained" and "technology lost" lines ([research](research.md)).
    Maximum health rises and falls with it, keeping each unit's share
    of health. A second crate of the same kind adds nothing and says
    nothing; the technology stays until the last of them is gone. A
    scenario that bars the technology bars it here too. [code]
- Espionage, given on an enemy building: first the user's side alone is
  shown `ability_effect_describe_economy`, "<side> Economy: " and that
  side's resource bar as its owner sees it (money stockpile and rate,
  ammo and oil rates, regiment and building limits, two spaces between
  entries, then its general's experience). Then the smaller of 50
  (`[ResourcesAmount]` `money`) and the enemy's money moves from its
  stockpile to the user's (kept within `EconomyStockpileResourceMax`).
  The enemy is told nothing. `espionage_glasses` and `espionage_sfx` at
  the building. [code, data]
- Industrial Sabotage, given on an enemy depot: a projectile
  (`industrial_sabotage_projectile`, `projectile_life` 2) lands 2
  seconds later and does 1200 blast damage to the depot, with
  `industrial_sabotage_sfx` and its visual. [data, code]

## Radar Scan

- Radar Scan (`radar_scan`, 400, recharge 300) is used at once on the
  side itself. The side holds `property_radar_scan` (`MINIMAP_NO_FOG`)
  for `duration` 10 seconds, shown for 10 to 11. [code, data]
- While it holds: the minimap's fog layer isn't drawn (see
  [minimap](minimap.md)), and the minimap marks the things it would
  show whether or not the side can see them, each in its current
  owner's colour rather than as last seen. The world's fog, sight and
  targeting don't change. [code]
- On use, `radar_scan_sfx` plays and a blip (`IconRadarScan.tga`)
  blinks 3 times at the minimap's centre, magenta then white each
  second, as in [minimap blips](game-events.md#minimap-blips). [data,
  code]

## The panel

- Top left: the side's experience, and below it a floater holding one
  button per operation in the general's `[Faction]` block, in the order
  listed there (four for every shipped general). Layouts are in
  `UI/Game/game_interface.tgi` (`FactionExperience`) and
  `UI/Game/floaters.tgi` (`FactionAbilitiesFloater`). [code, data]
- It's the local player's side's. The experience text is hidden and the
  floater has no buttons when that side has no general. The floater is
  rebuilt when the local player changes side. [code]
- `UIShowFactionExperience` (`AVars_AA.tgi`, true) turns the experience
  text off when false. [code, data]

### Experience

- The text is the experience glyph (`FontFactionExperienceGlyphIndex`,
  133 in `AVars_AA.tgi`: the medal), a space, the experience rounded to
  a whole number, then "/" and the ceiling when the general has one:
  "474/5000". [code, data]
- It's refreshed at least once a second. It only ever shows the side's
  current experience: nothing previews what an operation would leave.
  [code]
- Its tooltip, from the top: [code, data]
  - Title "Experience" (`game_interface_FactionExperience_ToolTip`).
  - After a blank line, two titled lines: "General"
    (`ui_tooltip_item_general`) with the general's `name`, left out when
    the name is blank, and "Experience" (`ui_tooltip_item_experience`)
    with the experience rounded to a whole number, without the glyph or
    ceiling.
  - Then "Spend experience to activate Special Operations."
    (`game_interface_FactionExperience_ToolTip_2`).

### Operation buttons

- Each button shows the definition's `icon`, with a red bar under it.
  [data]
- The bar shows how far the operation has recharged: 1 − time left ÷
  recharge time. Full when ready (and so at the start of a match),
  empty straight after use, filling steadily. It doesn't reflect
  experience. [code]
- A button is greyed out (disabled) when the operation is recharging,
  the side's experience is below its cost, or it's waiting on a summon
  (above); otherwise it's enabled. [code]
- The buttons run the commands `UseFactionAbility 0` to `3`, the
  operations in list order. The stock keys (`Localization/Hotkeys/
  hotkeys_custom.txt`) are Y, H, N and J for the first to fourth. [data]
- Tooltip, from the top: [code, data]
  - The operation's `name`, marked with its key as in [Hotkeys in
    tooltips](commands.md#hotkeys-in-tooltips): "(N)Armored Resupply".
  - What its effects do, in the order its `[Effect]` blocks are listed
    (see [Effect lines](#effect-lines)).
  - "Cost" (`ui_tooltip_item_title_cost`) with the experience glyph and
    `cost_faction_experience`, when the cost is above 0.
  - "Area" (`ui_tooltip_item_title_area`) with a size word, when the
    operation has an area: the larger of `area` and `volley_area`, named
    by the `AREA` or `AREA_VOLLEY` descriptions in
    `Game/descriptions.tgi` (Small below 2.5, Medium below 4, Large from
    4). Armored Resupply's area 15 reads "Large".
  - While recharging, `ui_floater_ability_recharge_time` ("Recharging
    (%d:%.2d left)"), minutes and seconds left.
  - The operation's `description`.

  A blank line separates the title, the effects, the Cost and Area pair
  and the description, as in `ger-armored-resupply-tooltip.jpg`. [code,
  game]

### Effect lines

- Each of the operation's effects gives one line, except one with
  `hide_from_tooltip = true` (the summons behind Carpet Bomb or
  Kamikaze), which gives none. Lines are white unless said otherwise.
  [code, data]
- By effect kind (strings in `Localization/strings_rtse.tgi`): [code,
  data]

  | Effect | Line |
  |---|---|
  | `MainDamageEffect` and its kinds (`BlastDamageEffect` and so on) | `ability_effect_desc_main_damage_glyph_long`: the damage type's glyph, then `amount`, then " Damage" |
  | `AddDamageEffect` | `ability_effect_desc_add_damage_glyph_long`: glyph, "+", `amount`, " Extra Damage" |
  | `MoraleGainEffect` / `MoraleLossEffect` | "`amount` Morale Gained" / "`amount` Morale Lost" |
  | `HealEffect` / `RepairEffect` | "`amount` Healed" / "`amount` Repaired" |
  | `SummonEffect` | "Summons " and the summoned thing's name (`object_ids`; `dynamic_campaign_object_ids` instead in the World War campaign when set) |
  | `StealResourcesEffect` | "Steal Resources: " and its `[ResourcesAmount]` |
  | `DescribeEconomyEffect` | "Describe Economy" |
  | `EnchantmentEffect` | `ability_effect_desc_enchantment`, "%s for %g Seconds": the property's `name` and `duration` ("Resupply for 30 Seconds"), then the property's modifier lines below it, indented |

  Amounts print in the shortest form (`%g`: 900, 0.5).
- An effect with its own `targets` adds " vs " and the target words to
  its line: the required ones, then "Not" and the excluded ones, all
  separated by spaces. A thing category gives its `name`
  (`Game/thing_categories.tgi`), so `targets = building` reads
  "+900 Extra Damage vs Building" after the glyph. [code, data] The
  fixed keywords (`friend`, `self` and the rest) show as written.
  [guess]
- A modifier line (under an enchantment, and in a property's tooltip,
  see [Property icons](object-display.md#property-icons)) depends on its
  kind; a modifier kind not listed gives no line: [code, data]
  - `FlagModifier`: `modifier_desc_flag_<flag>` ("Supplied" for
    `SUPPLIED`, "No Fog on Minimap" for `MINIMAP_NO_FOG`). Green
    (`UIColorModifierPositive`, `UVars.tgi`) for `SUPPLIED`,
    `INVISIBLE_TO_ENEMY`, `REVEAL_ENEMY_RZOD`, `MINIMAP_NO_FOG`,
    `NO_UPKEEP` and `TRAILBLAZING`; red (`UIColorModifierNegative`) for
    `PARALYZE` and `ENTANGLE`; white (`UIColorModifierNeutral`) for
    `BERSERK`, `SUMMONED`, `NOT_CAPTURABLE` and `NOT_DISPOSABLE`.
  - `AddModifier`: `modifier_desc_add_<attribute>` with the amount
    signed ("+5 to Attack Value" for `AV` +5); `ABILITY_HEAL` reads
    `..._add_ability_repair` for a repair ability. `AUTO_UPGRADE` gives
    "Auto Upgrade in m:ss", or "Auto Upgrade" when there's no time;
    `TERRAIN_SPEED_CAP` is the speed word in [object display](object-display.md#property-icons).
  - `MultiplyModifier`: `modifier_desc_multiply_<attribute>` with the
    amount × 100, signed, as a percentage ("+25% to Resupply Rate" for
    `RESUPPLY_RATE` 0.25; `RECHARGE_TIME` reads "to Attack Speed", so
    −0.5 is "-50% to Attack Speed").
  - Add and Multiply lines are green when the amount is above 0, red
    below 0, white at 0, except that the sense is reversed for
    `MORALE_BREAKPOINT`, `TERRAIN_SPEED_CAP`, `STRUCTURE_COST` and
    `RECRUIT_COST` (a rise is red). `RECHARGE_TIME` isn't reversed; it
    works as attack speed (see [Enchantments](#enchantments)), so a
    slower attack (−50%) shows red and a faster one (+50%) green.
  - `EnchantmentImmunityModifier`: "Immune to " and the enchantment
    type's name, or "Immune to all" with no type; green.
  - `AbilityEffectModifier` and its kinds (an `[Effect]` added to the
    holder's attacks, as in Communist Zeal): its effect's line from the
    table above, green.
  - `ReverseAbilityModifier`: "<the ability's name> reverse effect",
    green; nothing without an ability.
  - `ResistDamageModifier`: as in [object display](object-display.md#property-icons).
  - Periodic damage and healing: "glyph`amount` damage every second"
    (red) and "`amount` healed every second" (green).

## Open questions

- When two things are equally near an operation's centre and only one
  can be taken, which wins? (analysis, 2026-11-15)
- Can an operation reach units inside a water transport? (Aircraft in
  flight are caught like anything else: [combat](combat.md#what-can-hit-a-plane).)
  (analysis, 2026-11-15)
- Where does a strike's projectile start, and is it drawn at all? The
  bombers and rockets the player sees are only pictures. (analysis,
  2026-11-15)
- Exactly how long do the summons that wear out last, and so how much
  money does Lend Lease give (between about 83 and 100)? Best timed in
  the game. (analysis, 2026-11-15)
- Do units fire on a Secret Agent? (analysis, 2026-11-15)
- Where does Industrial Sabotage's projectile start, and does 1200
  always destroy a depot? (analysis, 2026-11-15)
- Which things does the minimap still leave off during Radar Scan, and
  is its sound heard everywhere? (analysis, 2026-11-15)
