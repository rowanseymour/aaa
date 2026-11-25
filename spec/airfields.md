# Airfields

What an airfield does with its aircraft: the air attack, which a person
gives with the Air Attack button and a trigger with `air_attack`
([triggers](triggers.md#orders)), the escort that can go with it, air
recon, and the fighters that defend the airfield. How a plane that is
hit or dies behaves is in [combat](combat.md#things-in-the-air).

Games: traced in AA. Kohan II's data turns air attacks off
(`UseAirAttacks = false`, [games](games.md)).

## Stock and groups

- An airfield keeps a stock of aircraft, its `[Inventory]` entries in
  the `[DenizenComponent]` (stock: 2 bombers and 4 fighters of its
  nation). Each entry is a place in the stock for one plane of that
  kind, with a health of its own. Planes at home aren't on the map.
  [code, data]
- Each `[DenizenGroup]` says which of them go out together for one job:
  `actor_ids`, `size` (fewest, most), and `behavior`. The jobs: [code,
  data]
  - an air attack uses the group whose `behavior` has `AIR_OFFENSIVE`
    (stock: 1 bomber);
  - an escort the `AIR_ESCORT` group (stock: 1 to 2 fighters);
  - air recon the `AIR_RECON` group, and the airfield's defence the
    `AIR_DEFENSIVE` group; stock data has one group of 1 fighter with
    both.
- A plane is ready only when its place in the stock is at home and at
  full health. [code]

### Building and repairing planes

- An airfield made during play starts with every place in its stock
  empty, at no health, and builds its planes there. [code] A map can
  save an airfield with its stock already full.
- The airfield works on one place at a time: a damaged plane at home
  first, and only when none is damaged, an empty place. That place gains
  the plane's `resupply_rate` × its maximum health each second (aircraft
  0.05, so 20 seconds from nothing to full), times one minus the side's
  shortage fraction ([economy](economy.md)), and times
  `DenizenResupplyRateUnderSiege` (0.25) while the airfield is under
  siege ([buildings](buildings.md#under-siege)). It's the same rule as for
  the denizens of [settlements](settlements.md#healing-and-replacing).
  [code, data]
- A plane that dies away from home frees its place at once, at no
  health, so the airfield rebuilds it as above. One that comes home
  damaged keeps its damage, as a share of its maximum health, and is
  repaired in turn. [code]
- What a stocked plane can do is on the grid as soon as enough planes
  are ready: Air Attack while the attack group's fewest are ready, Air
  Recon likewise. [code; guess: that the buttons are greyed rather than
  hidden otherwise]

## Air attack

- Air Attack takes a thing or a place. The target tests a person's click
  goes through are in [commands](commands.md#targeted-commands). [code]
- The side must afford `[AirAttackCost]` (stock 150 `money`, Germany's
  airfield 100). If it can't, nothing goes out and the side is told
  "Insufficient funds for Air Attack. Requires %s." with the cost
  (`denizen_error_affording`). [code, data]
- If at least the group's fewest planes are ready, that many go out, up
  to its most; the cost is paid then. If too few are ready, nothing goes
  out and nothing is paid, without a message. [code]
- One order sends one group. Ordering again while one is out sends
  another, if planes are ready and the side can pay. [code]

### Taking off

- The planes appear at the airfield's `[Runway]` and take off from it
  one at a time, the runway's `delay` seconds apart (stock 1), as for a
  plane a runway launches with a delivery ([deliveries](deliveries.md)).
  Each then flies to its job at its `operational_height` above the
  ground. Every job below takes off this way. [code, data]

### The attack

Each plane is given an attack on the target and flies it on its own.
[code]

- It goes round in passes, each made of: [code; guess: the exact
  shape of each leg]
  - an approach to a point short of the target, 1.75 times its longest
    weapon range back along the line it's coming in on (a bomber's bombs
    reach 15, so about 26), at its operational height, aiming ahead of a
    moving target by how far it will have moved in the plane's flight
    time;
  - a run in at the target, dropping down towards half its operational
    height;
  - when it's below half that height, or has passed the target (the
    target is no longer ahead of it), a climb out straight ahead;
  - a turn away, alternately 45° to the left and to the right of the
    line to the target on successive passes, then round again.
- It drops its bombs while it's on a run in, pointed at the target
  (within about 26°, cosine 0.9) and the target is within a bomb's reach,
  using the bomber's abilities as any attack does
  ([combat](combat.md#choosing-an-ability)). [code]
- A bomber carries a load, not an endless supply: its bombs cost
  `mana_cost` (stock 5) from its `mana_max` (stock 60) and nothing
  refills it in the air (`mana_regeneration = 0`), so it drops twelve
  bombs' worth. [code, data]
- On a thing: [code]
  - On a regiment, it picks the nearest of the regiment's things that
    it can attack, and when that one dies picks the next nearest again.
  - It keeps making passes until the thing (for a regiment, every one of
    its things it can attack) is dead or no longer something it can
    attack, or its bombs run out.
- On a place: it makes passes over the point (kept within the map)
  until its bombs run out. [code]
- Then it flies home ([Coming home](#coming-home)). [code]

## The escort

- An airfield's Escort can be switched on and off with its commands
  (`escort_on`, `escort_off`; the button shown is the one that changes
  it). It starts off. [code, data]
- While it's on and an air attack is out, the airfield also sends its
  `AIR_ESCORT` group, if ready. It costs nothing. [code; guess: whether
  an escort launches when the attack is ordered, or only once the
  attack is in the air]
- The escort flies with the attack's planes. Whenever an enemy hits one
  of them, or one of the escort, it goes after the one that hit most
  recently, as an attack on it ([The attack](#the-attack)). When the
  attack's planes have all finished, it comes home. [code]
- Switching Escort off drops the escort from the attack; it comes home.
  [code, guess]

## Air recon

- Air Recon takes a place. The side must afford `[AirReconCost]` (stock
  25 `money`), or it's told "Insufficient funds for Air Recon. Requires
  %s." (`denizen_error_affording`). [code, data]
- If the recon group's fewest planes are ready, they go out and the cost
  is paid; otherwise nothing happens. [code]
- The plane flies to the point at its operational height and, on getting
  there, turns straight for home. What it sees on the way is what any
  plane sees ([fog](fog.md)); there's no other reveal. [code; guess: that
  nothing else is revealed]

## Defending the airfield

- An airfield notices intruders within its `guard_range` (30): any
  enemy thing that can be targeted and that its side can see. A unit in
  a regiment counts as its regiment. [code, data; guess: that the test
  is sight]
- An intruder is forgotten once it's dead, no longer an enemy or a
  target, outside `guard_operational_range` (48), or (a regiment) has
  none of its units with it. [code, data]
- While it has intruders, the airfield sends its `AIR_DEFENSIVE` group
  as soon as it's ready, free of charge. [code]
- Each of those fighters attacks the nearest intruder it can attack, as
  in [The attack](#the-attack), keeping to the one it's fighting while
  that one is still an intruder, and picking again when it's gone. With
  no intruders left, or its airfield gone, it comes home. [code]
- A fighter's guns hit any enemy, on the ground or in the air
  (`targets = +enemy, -mine`). [data]

## Coming home

- A plane that has finished flies back to its airfield at its
  operational height, lands on the runway and goes back into its place
  in the stock, keeping its health. [code; guess: the landing itself]
- A plane that dies on a sortie doesn't come back; its place is rebuilt
  ([Building and repairing planes](#building-and-repairing-planes)).
  [code]

## When the airfield goes

- Destroyed: planes in the air carry on with their job. When one turns
  for home and its airfield no longer exists, it dies where it is, as
  if shot down ([combat](combat.md#dying-in-the-air)). [code; guess: when
  in its flight home that's noticed]
- Captured: the airfield keeps its stock and its planes in the air; they
  keep their old side, and on coming home go back into the stock, now
  the new owner's. [code; guess: that nothing else in a change of owner
  touches the planes]
- Packed: the truck has no stock, so the stock is lost. Planes in the
  air fly home to the truck, which now holds the airfield's identity,
  and are removed when they reach it, without being stored. The
  airfield it later unpacks into starts with an empty stock. [code,
  data; guess: that the planes find the truck]
- Upgraded or otherwise replaced by a building that has a stock of the
  same kinds of plane: each place's health and the planes in the air
  pass over to it. [code]

## Open questions

- Are the Air Attack and Air Recon buttons greyed or hidden while too
  few planes are ready? (analysis, 2026-11-22)
- Does an escort take off when the attack is ordered, or once the
  attack's planes are in the air? (analysis, 2026-11-22)
