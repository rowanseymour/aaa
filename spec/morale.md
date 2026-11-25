# Morale

A regiment's morale, how it rises and falls, routing when it breaks, and
the regiment's own experience and the levels it earns. Morale and
experience belong to the regiment, not its units. The constants are in
`Game/SVars.tgi`, with AA's own values in `Game/SVars_AA.tgi` overriding
several of them; the values below are AA's.

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Maximum and breakpoint

- A regiment's maximum morale is the sum, over every slot of its layout
  whether filled or not, of that slot's unit `morale_value`, plus the
  `MORALE` bonuses of its present units and of the regiment itself (the
  experience levels below give some). [code]
- A bonus held by the regiment and its units alike, as a level is,
  counts once for each present unit and once more for the regiment; an
  empty slot counts its template's plain `morale_value`. A Veteran
  infantry regiment (+0.5) of 8 slots with 6 men present has 8 × 5 + 6 ×
  0.5 + 0.5 = 43.5, once the maximum is next worked out (below). [code, data]
- Stock `morale_value`: 5 for infantry (`AAUnit`), 14 for vehicles, 18
  for self-propelled guns, 20 for tanks, with overrides on many units
  (most of them 10). [data]
- The maximum is worked out when the regiment is formed or loaded, when
  it changes hands, and when it packs or unpacks; its morale keeps the
  same share of the maximum when it changes. A recruited regiment starts
  at full morale, and one comes back to full when packing or unpacking
  replaces its leader. [code]
- Nothing else changes the maximum. Gaining or losing a property with a
  `MORALE` bonus (an experience level, a technology) leaves the maximum
  and the current morale as they were until the next of those moments,
  when the bonus counts. So a regiment that reaches Veteran in a fight
  doesn't get the extra maximum until it next packs, unpacks, changes
  hands or is loaded, and its morale isn't raised by the level. [code]
- A regiment placed on a map (or loaded from a save) starts with the
  morale the map stored for it, read as a share of its maximum: 1.0 is
  full, 0.5 half. The stock maps checked store 1.0. [code, data]
- The breakpoint is the regiment's `morale_breakpoint`, a number of
  morale points rather than a share, plus any `MORALE_BREAKPOINT`
  bonuses: 20 for infantry and vehicle regiments, 15 for armour
  (`Templates/template_organization_aa.tgi`). [code, data]
- Morale never goes below 0 or above the maximum. [code]

## Losing morale

- Fighting: each second that at least one enemy regiment is engaging it
  (and was the second before), a regiment loses `MoraleEngagementLoss`
  (0.5) × (1 + (`MoraleEngagementLossFactorMax` (3) − 1) × the share of
  its slots now empty, a slot counting as empty from the moment its
  member dies), divided by (1 + its `MORALE_LOSS_RATE`
  bonuses). So a full regiment loses 0.5 a second and one down to its
  leader nearly 1.5. It's the regiment's own `MORALE_LOSS_RATE` that
  counts, not its members'. An entrenched regiment holds +0.5 for each
  of the two entrenchment properties it has: one of infantry or of
  vehicles alone loses a third slower, one with both kinds of member
  loses half as fast ([stances](stances.md#what-it-gives)). [code, data]
- A unit dying: its regiment loses the unit's `morale_value` ×
  `MoraleDeathFactor` (0.25): 1.25 for an infantryman, 5 for a tank.
  [code, data]
- Running while engaged: `MoraleRetreatPenalty`, 0 in AA (5 in
  `SVars.tgi`). [code, data]
- Weapons and operations with a `MoraleLossEffect`: its `amount` from the
  regiment of every unit hit, once per unit, so an area hit on several
  men of one regiment counts several times. An area attack's falloff
  scales it, as it does main damage ([combat](combat.md#area-attacks)).
  Stock amounts: mortars and halftrack artillery 0.75 (Japanese mortars 1), self-propelled guns 1,
  flamethrowers 1 (the British flame tank 0.5), battleships 1.5, bombers
  0.5 (one American bomb 2), rocket trucks 0.25, the decoy tanks of
  Maskirovka and Panzer Hohle 0.25; Demoralization 3 and Propaganda War
  4 ([special operations](special-ops.md#enchantments)). [code, data]
- The trigger effect `actor_modify_morale` with a negative amount, taken
  from the regiment of the thing it names
  ([triggers](triggers.md#morale-and-experience)). [code]
- No morale is lost at all while a regiment is unpacked. A `BERSERK`
  regiment loses none from fighting, running or weapons, though it still
  does from deaths and triggers; the only shipped property with it
  (`tech_unbreakable`) is never granted. [code, data]

## Gaining morale

- Resting, guarding, reforming or exhausted, a regiment regains morale as
  [supply](supply.toml#what-supply-gives) describes: `MoraleReplenish` (2)
  a second, plus `MoraleSupplyBonus` (1) in supply, times (1 + its
  `MORALE_RECOVERY_RATE` bonuses). Nothing else regains it over time:
  not moving, fighting, running or routing. [code, data]
- A kill: when one of its units kills an enemy unit (unit against unit,
  not a building), the killer's regiment gains `MoraleSlayerBase` (0.5) ×
  ((the victim's combat value ÷ the killer's) + 1) ÷ 2, the factor kept
  within `MoraleSlayerFactorMin` (0.5) and `MoraleSlayerFactorMax` (3),
  comparing the two units' values at full health ([combat](combat.md#combat-value)).
  Nothing if the killer's value is 0. [code, data] The killer is the one
  credited with the kill for experience too ([special
  operations](special-ops.md#earning-experience)), so a kill with no
  recent enemy hit, or one made by a strike, gains no one morale.
  [code]
- Breaking an enemy: when a regiment it is engaging routs or runs, each
  regiment engaging that one gains `MoraleBreakerBase` (3) × (the fleeing
  regiment's maximum morale ÷ its own), the factor kept within
  `MoraleBreakerFactorMin` (0.5) and `MoraleBreakerFactorMax` (2). [code,
  data]
- `MoraleGainEffect`: its `amount` to the regiment of every unit it
  reaches (Courage and Purge, 1 each). [code, data]
- The trigger effect `actor_modify_morale` with a positive amount. Like
  every gain it can come at any time, while routing, Exhausted or
  unpacked included, and does nothing more than raise morale: a routing
  regiment goes on routing, and an Exhausted one goes back to being idle
  only once its morale is above the breakpoint, as below. [code]

## Breaking

- A regiment whose morale falls to or below its breakpoint while still
  above 0 routs at once, mid-fight or not, unless it's unpacked. [code]
- One whose morale falls straight to 0 in one step routs only if it isn't
  fighting at the time. One that is fighting fights on, at full strength,
  until its fight is over, and then routs. [code]
- A regiment still at or below its breakpoint (but above 0) routs again
  whenever it's in a fight, and one at 0 routs when it next has nothing
  to do. [code]
- At 0 morale a regiment shows the Exhausted property (control range
  −25%) and takes none of the orders listed under [The player's
  orders](#the-players-orders). [code, data]
- Morale has no other effect: it doesn't change damage, accuracy or
  speed. [code]

## Routing

- When a regiment routs: [code, data]
  - its owner is told `company_routed` ("%s has routed from battle");
  - every regiment engaging it gains morale (above) and experience
    ([special operations](special-ops.md#earning-experience));
  - it drops all its targets, breaks formation and won't start a fight
    while routing (see [combat](combat.md)). It carries the `routing`
    property, which target filters can name;
  - it takes none of the orders listed under [The player's
    orders](#the-players-orders) until the rout ends.
- Where it runs: it scores `RoutRandomCandidates` (20) points in random
  directions, plus `RoutSupplyZoneCandidates` (2) for each supply provider
  of its own side or a friendly one, aimed at that provider within
  ±`RoutSupplyZoneMaxAngleDelta` (50°). Each point is its control range
  plus a random 0 to `RoutMaxDistanceDelta` (24) from where it stands,
  cut short at the map's edge; points it can't reach are dropped. It
  runs to the point that scores highest: [code, data]

  score = `RoutBaseValue` (0)
  + the average, over the regiments engaging it, of (the angle in degrees
  between the way to that enemy and the way to the point ×
  `RoutFactorEnemyAngle` (0.25) + that enemy's distance to the point,
  at most `RoutFactorEnemyDistCap` (2000), × `RoutFactorEnemyDist` (10))
  − the distance to the point, at most `RoutFactorMovedDistCap` (2000),
  × `RoutFactorMovedDist` (8)
  − `RoutPenaltyEnemyZOC` (50) if the point is in an enemy's control zone
  − `RoutPenaltyEnemyZOG` (75) if the point is in an enemy's guard zone
  + `RoutBonusFriendlyZOS` (50) if the point is in its side's supply
  + `RoutBonusSupplyInRange` (50) for each friendly provider whose
  supply reaches the point
  − the distance to the nearest friendly provider, at most
  `RoutFactorSupplyDistCap` (2000), × `RoutFactorSupplyDist` (1)
  + a random 0 to `RoutFuzziness` (20).

  An enemy's guard zone is the circle of `guard_range` around each
  living enemy thing with a `[DenizenComponent]` (the things that
  send out units of their own). Most take the template's default of 10
  (HQs, Corps, water transports); AA's airfields and carriers set 30,
  Kohan II's centres 38. The two zone penalties add up for a point in
  both. [code, data]

  So it runs away from its attackers and towards supply. With no point
  it can reach, it waits 2 seconds and tries again. [code, data]
- It runs as one body led by its leader, the way any regiment moves
  ([movement](movement.md)). With no formation, each unit keeps the place
  it held relative to the leader when the regiment broke formation (an
  offset on the map, which doesn't turn with the leader's heading) and
  chases that place as the leader drives the route. [code]
- While it has no formation the regiment and its units hold the layout's
  Broken Formation property: `speed_max` ×1.25 and desired attack
  efficiency 0.5 ([combat](combat.md#attack-efficiency))
  (`BrokenProperty` in `Templates/template_organization_layout_aa.tgi`,
  which every AA layout uses). So a routing regiment runs a quarter
  faster than it marches. [code, data]
- It never fights while routing: hits don't draw it into an engagement,
  enemies coming near don't either, and its units pick no targets, so it
  doesn't fire back even at what's in range. [code]
- It goes on losing morale while enemies still count it as engaged.
  [code]
- The rout ends after `RoutingTime` (10 seconds) or on reaching the
  point, whichever comes first: [code, data]
  - with morale at 0, the regiment is Exhausted (below);
  - otherwise, at the time limit it simply stops routing: it stands where
    it stopped, idle, and takes orders again. With morale still at or
    below its breakpoint it routs again as soon as it's in a fight;
  - otherwise, on reaching the point it reforms (below).
  Nothing resets morale; it comes back only at the rates above.

## Reforming

- A reforming regiment takes up its formation where it stopped and
  regains morale. Reforming doesn't weaken it: its
  `actor_state_reforming` property's `amount = 0` sets its attack
  efficiency's target to full strength, not to 0
  ([combat](combat.md#attack-efficiency)). [code, data]
- The reform ends once the regiment has stopped moving, its leader is
  gone, or after `ReformTime` (10 seconds), whichever comes first; it is
  then idle. [code]
- It takes orders as an idle regiment does; an order ends the reform.
  [code]
- It is aggressive: an enemy coming within its `control_range`, or a hit,
  starts an engagement ([combat](combat.md#engagement)), which ends the
  reform. It then fights at full strength, or, with morale still at or
  below its breakpoint, routs again at once. [code, data]

## Exhausted

- An Exhausted regiment's units stop where they are, without a
  formation (Broken Formation, above). It regains morale, and takes none
  of the orders listed under [The player's orders](#the-players-orders).
  [code, data]
- It is aggressive: an enemy coming within its `control_range`, or a
  hit, starts an engagement as for any regiment, and it fights in its
  combat formation at full strength. While its morale stays at 0 it
  fights on until the fight is over, then goes back to being Exhausted.
  If its morale rises above 0 during the fight (from kills, say) while
  still at or below its breakpoint, it routs. [code, data]
- Once its morale is back above the breakpoint it's idle again and takes
  orders. [code]

## The player's orders

- While routing or Exhausted (or at 0 morale) a regiment loses these
  orders: attack, bombard, guard, repair, construct, move, stop, Run,
  Rout, explore, disband, unpack and pack. An order it hasn't got is
  refused whoever gives it: the player, a trigger or the computer
  player. [code]
- Its other orders stay offered on their usual terms: attaching to or
  detaching from an HQ, choosing a formation (the choice is kept and
  taken up once the regiment has a formation again), and in Kohan II
  attaching or detaching a hero. [code]
- Rout (the order) is offered only while the regiment is in a fight of
  its own (engaging enemies, whether it started the fight or answered
  fire) and can move. Merely being shot at while doing something else
  isn't enough. It routs at once, with everything above, enemies' gains
  included. [code]
- Run is withdrawn while the regiment is engaged and its morale is below
  its breakpoint + `MoraleRetreatPenalty` + 1: below 21 for a regiment
  with a breakpoint of 20. [code, data]

## A regiment's experience

- Every AA regiment has an `[ExperienceComponent]`; units don't have their
  own. How a regiment earns experience is in [special
  operations](special-ops.md#earning-experience), which hands the same
  amount to its side. [data, code]
- A recruited regiment starts at 0, the first level ("Recruit"). One
  placed on a map starts with the experience the map stored for it, at
  the level that amount reaches. [code]
- The trigger effect `actor_gains_experience` gives a regiment
  experience as a gain in play does, levels and all
  ([triggers](triggers.md#morale-and-experience)). [code]
- Kohan II: a company's hero has experience of its own, and whenever the
  company gains experience the hero gains the same amount, reaching its
  own levels. A ceiling set on the hero by a trigger caps the hero's,
  not the company's. [code, data]
- There's no ceiling: a regiment's experience grows without limit,
  though the levels stop at the last one listed. Experience is never
  lost, and levels never go down. Losing units or taking replacements
  doesn't change it. [code]
- It stays with the regiment through packing and unpacking (the truck or
  building that takes a place in it is given the level like any new
  member) and when the regiment changes hands; the side's own
  experience isn't moved with it. [code]
- Levels, each with a property for the regiment that every unit in it
  also holds, replacements included. A new level's property replaces the
  last one's rather than adding to it. [code, data]

  | Component | Regular | Veteran | Elite |
  |---|---|---|---|
  | `AAInfantryExperienceComponent` (infantry, engineers, infantry artillery) | 150: AV +4, DV +1, health ×1.05, recovery +5% | 350: AV +8, DV +2, health ×1.10, morale +0.5, recovery +10% | 600: AV +16, DV +3, health ×1.20, morale +1, recovery +15% |
  | `AAParatrooperExperienceComponent` | 100: as infantry Regular | 250: as infantry Veteran | 500: as infantry Elite |
  | `AAMechanizedExperienceComponent` (mobile, HQ, halftrack artillery) | 150: AV +5, DV +1, health ×1.05, morale +1, recovery +5% | 350: AV +10, DV +2, health ×1.10, morale +2, recovery +10% | 600: AV +20, DV +3, health ×1.20, morale +3, recovery +15% |
  | `AAArmorExperienceComponent` (armour, tank artillery) | 150: AV +10, DV +1, health ×1.05, morale +1, recovery +5% | 350: AV +20, DV +2, health ×1.10, morale +2, recovery +10% | 600: AV +40, DV +3, health ×1.20, morale +4, recovery +15% |

  The figure before each level's bonuses is the experience it needs. AV
  adds to each attack's main damage, once per attack
  ([combat](combat.md#damage)), and DV to defense; health multiplies
  maximum health, morale adds to maximum morale (see [Maximum and
  breakpoint](#maximum-and-breakpoint) for when), and recovery is
  `MORALE_RECOVERY_RATE`. The level names are Recruit, Regular, Veteran
  and Elite (`template_component_aa_ExperienceLevel_Property_name` and
  `_2` to `_4`), with icons `IconPropertyExperience2.png` to `4`;
  Recruit has none. [data, code]
- A level's health multiplier, like any `HEALTH` multiplier (a
  technology, an enchantment), rescales each unit's maximum health at
  once and keeps its share of health: a unit at 30 of 60 that reaches
  Regular (×1.05) is at 31.5 of 63. Health never drops below 1 by
  this. Swapping one level for the next rescales twice, which comes to
  the same share. [code]
- A replacement builds up as [supply](supply.toml#what-supply-gives)
  describes and appears holding the regiment's level, at full health of
  the raised maximum. [code]
- Reaching a level by play posts `company_experience_gained_level` ("%s
  has gained a level"); several levels can come at once. [code, data]
- A level raises its units' combat values, so each later fight is worth
  less experience to it. [code]
- Where the player sees it: [code, data]
  - the thing tooltip's "Experience" row, for the regiment and for each
    of its units: the level's name, then the points held over the next
    level's figure ("Regular 212/350"), or the points alone at the last
    level ([object display](object-display.md#the-thing-tooltip));
  - the level's property among the regiment's property icons (Regular
    to Elite; Recruit has no icon) and in the tooltip's "Properties"
    row;
  - the banner's rank decal (`AAExperienceBannerDecal`): the level's
    place in the list picks the decal, Recruit `BannerRankBase`, Regular
    `BannerRank1`, Veteran `BannerRank2`, Elite `BannerRank3` (a level
    past the last decal would keep the last). Only that one texture
    shows: `BannerRankBase` isn't laid under the others. It changes as
    soon as the level does. `BannerRankBase.png` is fully transparent, so a Recruit
    regiment shows no rank; Regular has one gold chevron, Veteran two,
    Elite three and an emblem. Each decal is a full-size overlay on the
    banner's decal plane, so where it shows is in the picture: the
    chevrons sit at the banner's right edge, centred top to bottom,
    beyond the frame. A thing with no experience shows none. [code,
    data] That it's drawn above the Engaged decals and under the HQ
    badge, in the order `AABanner` lists them, is a guess;
  - the message when a level is gained.
  Nothing else shows it.

## Open questions

- Does a weapon's morale loss fall off with distance as its damage does?
  (analysis, 2026-11-15)
- Some regiments tied to a building (the one a building produces, a
  garrison) lose no morale at all. Which ones, in play? (analysis,
  2026-11-15)
- How long does a reform last in play: does a regiment that arrives at
  its rout point stop at once and end its reform within a moment, or
  does settling into formation take most of `ReformTime`? Watch in the
  game. (analysis, 2026-11-17)
