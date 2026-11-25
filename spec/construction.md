# Construction and repair

How regiments put up new buildings (AA's engineers and their bunkers),
how a building site grows to a finished building, cancelling a site, and
how buildings are repaired. The buttons and the Build menu are in
[commands](commands.md#build-menu).

Games: traced in AA's engine and data. Kohan II runs the same code for
both, with its own builders and its settlements' Build Structure menu
(below); its data is unchecked against these rules.

## Who builds and what

- A unit can build when its template has a `BuilderComponent` with at
  least one `BuildActor` entry (`actor_ids`), and it's a member of a
  regiment. In AA's data that's the five nations' engineers, each listing
  its nation's `<nation>_bunker`, `<nation>_bunker_aa` and
  `<nation>_bunker_artillery` ("Bunker", "Anti-Aircraft Battery",
  "Artillery Battery"). [code, data]
- A regiment offers what its members can build: the union of their
  lists, each template once. The first member's entries come in its list
  order; an entry only a later member lists goes before them. An
  engineer regiment's six engineers list the same three, so its menu is
  those three in that order. Its captain lists nothing and adds
  nothing. [code, data]
- A regiment can be ordered to build while any member can, and not
  while it routs or has no members ([commands](commands.md#command-grid)
  for the greyed button in combat). [code]
- Kohan II only: its settlements offer their `AvailableStructures`
  through the separate Build Structure command (`build_structure`).
  No AA thing offers it. [code]

## Placing

- Choosing an entry in the Build menu closes the menu and starts aiming
  the building, in the same way as aiming a Deploy to Location
  ([deliveries](deliveries.md#aiming)), with these differences: [code,
  data; game: the preview, the cost preview and the grid going back to
  the commands]
  - the cursor is `construct` ("Build Cursor", frames
    `Cursors/Build_%d.tga`, looping each second);
  - the preview is the one building, at the mouse's place, and the
    resource bar previews its cost;
  - the building can be turned: pressing on the map and dragging before
    letting go sets its facing along the drag, unless its template is
    `axis_locked`. Without a drag it faces the map's default direction
    (0, −1). No shipped bunker is `axis_locked`. [code, data]
- A place is valid by the same tests as Deploy to Location's: every tile
  under the building explored by the player, and room to stand
  ([objects](objects.md#room-to-stand)), where things the player can't
  see right now don't count against it. Nothing else is tested: not the
  cost, supply, distance from the builders, whose ground it is, or the
  regiment and building limits. [code]
- Room to stand is tested with the building's shape padded by
  `BuildingPaddingMeters` (2) on every side, as for unpacking, and every
  `blocking` thing in sight counts against it, units included (not only
  buildings, as when unpacking). So a bunker can't be aimed where it
  would come within 2 of a soldier, a tank or another building. [code,
  data]
- An invalid place draws the preview with the invalid-placement effect,
  and a click there does nothing. A valid click gives the order with the
  place and facing, and ends aiming. A right click, or choosing another
  command, ends aiming with nothing ordered. [code]
- Shift or Ctrl held while clicking queues the order after the
  regiment's current ones ([commands](commands.md#queued-orders)). So
  several Build orders can be lined up, each placed and aimed on its
  own. A queued Build order is otherwise like any other: its place is
  tested only at the click and again when the site appears (below), and
  it's paid for only when the regiment gets there and the site appears.
  [code]
- With several regiments selected, each gets its own Build order, for
  the clicked place plus its own offset from the selection's centre, as
  for a move ([movement](movement.md#several-regiments-ordered-together);
  `construct` keeps the default `apply_group_offset`). Only the clicked
  place was tested while aiming. Each regiment that can build goes to
  its own spot and puts up and pays for its own building there; one whose
  spot turns out to have no room gets "Could not create %s" (below).
  Regiments that can't build ignore the order. [code, data]
- Nothing is paid and nothing appears on the map when the order is
  given, queued or not: no preview or footprint stays behind for a
  building not yet started. [code] All that shows is the order's marker,
  `waypoint_build_actor`, at the place, while the regiment is selected
  ([commands](commands.md#order-markers)). [code, data]

## Going to the site

- The regiment walks to the place as on a move. It starts building as
  soon as the site's tile lies inside its control zone
  ([objects](objects.md#zones)), not when it stands on it. [code]
- An engagement on the way interrupts the order like any other, and it
  resumes afterwards ([combat](combat.md)). [code]
- On arrival the first member (in the regiment's member order) that
  lists the template builds it. Then, in order: [code]
  - the order is dropped silently if the side may not build that
    template (a map's restriction group bars it; the menu still offers
    it);
  - if the side can't afford its cost, "Insufficient funds for %s.
    Requires %s." (`construct_error_affording`, the template's name and
    the cost) is posted to the side, located at the builder, and the
    order ends with nothing built ([messages](messages.md));
  - otherwise the site appears at the aimed place and facing, owned by
    the regiment's side and in no regiment, and the cost is taken: the
    template's `[Cost]` with the side's cost adjustments, as for
    recruiting ([economy](economy.md)). [code]
- The place is tested for room to stand again as the site appears,
  padded as when aiming, but now against every `blocking` thing there,
  seen or not: units (the builders' own regiment included), buildings
  and other sites. If anything is in the way the site isn't made: "Could
  not create %s" (`construct_error_creating`, the template's name) is
  posted the same way, nothing is paid and the order ends. So troops
  standing on the place when the builders arrive stop it. [code]
- Kohan II only: a settlement centre can't be started with an enemy in
  the area round the place: "There are enemies nearby"
  (`construct_error_enemies_nearby`). [code]
- The regiment then goes straight on to building the site (below), as if
  ordered to; its panel's status reads "Building". [code, game]

## The site

- A site starts with 1 health, against the template's full health
  (1250 for a bunker, 1000 for the AA and artillery batteries). It's
  finished the moment its health reaches full. [code, data]
- While under construction (the data's `construction` state, "Under
  Construction") a site: [code, data]
  - can be seen, targeted and damaged like any building, sees round it
    (its `DetectionComponent`), and can be under siege;
  - has no weapons, so a bunker doesn't fire, and provides no supply,
    recruiting or research, and produces nothing;
  - pays its upkeep from the start (3 ammo for a bunker, 5 and 7 for
    the batteries); bunkers use no building limit;
  - offers only Cancel (`construction_cancel`): no Sell, Raze or
    Upgrade;
  - doesn't count towards keeping its side in the match, even where the
    finished building would ([outcome](outcome.md); bunkers never do).
- How a site looks while it goes up: [code, data, game]
  - The building's own model, whole, sunk into the ground by the
    model's height times the share of health missing
    (1 − health / full health): `construction_raise_vfx`, a
    `VFXConstructionRaiseModifier` started on entering the
    `construction` state at `BASE`. So a new site is all but buried and
    rises smoothly with its health, standing at full height when
    finished. Shaking and swaying (`apply_shaking`, `apply_swaying`)
    are off in the stock effect. [code, data] Parts the effect doesn't
    move, such as a bunker's gun turret, show at their full height over
    the footing from the start. [game]
  - The `Stage<n>` and `Final` nodes the templates name
    (`construction_node_name_prefix`, `finished_node_name`) are loaded
    but never used: there are no stages. [code]
  - Over it, the scaffold `bunker_construction`
    (`Effects/Construction/Construction.nif`, timber beams and
    scaffolding): it rises out of the ground into place over its 1.33
    second animation as the site appears, then holds there. [code,
    data, game]
  - Dust, `building_pack` (`Construction/Pack.nif`), while the site is
    in `active_construction` ("Active Construction"). Once a second, a
    site (or a building being upgraded) is put in that state if its
    health changed, by healing or by damage, within the last
    `ActiveConstructionTimeout` (2.5) seconds, and taken out of it
    otherwise. Entering it starts the dust, which puffs for its model's
    4 seconds of emission, and leaving it stops the dust at once, cut
    off mid-puff if need be ([effects](effects.md#how-long)). Builders
    healing once a second keep the state on, so a site at work shows
    one 4-second burst as work begins, then none until work stops for
    over 2.5 seconds and starts again; the state ends 2.5 to 3.5
    seconds after the last heal, finishing included. [code, data, game]
- What the object display shows for a site: its state line reads
  "Under Construction" (`actor_states_construction_name`), the
  highest-priority state it holds (`construction` has priority 10,
  `active_construction` 2). It has no weapons, so no attack values,
  only the shield. Nothing else is drawn for a site: the area right of
  the portrait shows the control panel's background, which is where the
  large nation emblem comes from
  ([object display](object-display.md#object-display-a-lone-things-panel)). In
  the several-thing display its card's picture is greyed while it's
  under construction or being upgraded. [code, data]
- On finishing: [code]
  - it gets everything a finished building has (its guns start working)
    and its full set of commands;
  - it starts counting towards keeping its side in the match, if its
    template is one that counts;
  - its owner gets "%s construction complete"
    (`building_construction_complete`, the building's name, with
    `building_construction_complete_sfx`) ([messages](messages.md)); seen as "Bunker construction complete" [game];
  - its `WhenConstructed` effects play (for a bunker, the ground decal
    for the terrain: `ground_bunker_dirt`, `_desert` or `_snow`), and
    its `WhenLeave` `construction` sound plays (a bunker's
    `bunker_construction_complete`, BuildBunkerComplete.wav, to its
    owner);
  - the effects its entering construction started end with the state,
    by the game's rule for permanent attached effects
    ([effects](effects.md#how-long)): the scaffold goes and the model is
    no longer lowered. Leaving the
    state starts `bunker_deconstruction` (`Deconstruction.nif`), the
    scaffold sinking back into the ground over 1.33 seconds. [code,
    game]
  - So a finished building never sinks, however damaged: only a site
    under construction (or a building being upgraded, which also
    starts from part health) is drawn lowered. [code, game]
- Kohan II only: a finished settlement centre founds its settlement.
  [code]

## Building and repairing work

The same work raises a site and mends a damaged building: each use of a
unit's repair ability adds health.

- An engineer has two such abilities, both healing 5 per use
  (`RepairEffect amount = 5`), with range 1: "Construct"
  (`AAConstructAction`) for a site, and "Repair" (`AARepairAction`) for
  a finished building. Each plays `hammer` and the `build1` animation.
  [code, data]
- How often each engineer heals: once a second. Both abilities' own
  definitions set `recharge_time = 0.0` (`AARepairLocal`,
  `AAConstructLocal`), and that's the recharge that applies, not the
  1.5 of `AARepairAllAction` their templates inherit
  ([data](data.md#templates)). With no recharge to wait for, each use
  lasts as long as its `build1` animation (`Build1.kf`, exactly 1.0
  second), and the engineer starts the next as soon as one ends
  ([combat](combat.md#range-and-recharge)). [code, data, game]
- So `RECHARGE_TIME` multipliers make no difference to engineers'
  building or repairing: a zero recharge stays zero, and the
  animation's length is unaffected. Only a change to the amount
  (`ABILITY_HEAL`) speeds it up. [code, data]
- In the game a Soviet engineer regiment (5 a use) took about 40
  seconds over a 1250-health bunker, and a USA one with Construction
  Battalions (8 a use) under 30, against about 42 and 26 by this rule
  (a 1.5-second recharge would give 62 and 39). The game clock under
  Wine kept to the wall clock in these runs. [game]
- What each may work on (`targets`): [data]
  - Construct: a thing with a `ConstructionComponent` (`constructable`),
    under construction, of a template in the unit's own build list (not
    `cannot_build`), owned by its side or an ally (`friend`);
  - Repair: a `constructable` thing not under construction, own or an
    ally's.
  `constructable` things are every building and the battleships, which
  carry a `ConstructionComponent`; units, trucks and the like can't be
  repaired.
- Each use adds `amount` plus the unit's `ABILITY_HEAL` additions (none
  below 0). Construction Battalions research (`tech_construction_battalions`,
  `+3` for `engineer`) makes it 8. [code, data]
- Health never goes above full. Repairing costs nothing. [code]
- When a regiment works on a building: [code]
  - it closes in as on an attack order, until its centre is within
    `CAITargetInRangeZOCMultiplier` (0.6) times its control range of the
    building ([combat](combat.md#attack-orders));
  - then every member that can work on it does, each moving to within
    its ability's range and using it again as each use ends; members
    that can't (an engineer regiment's captain) move up to the
    building;
  - stacking is plain addition: every member of every regiment on the
    building heals it; the captain doesn't. An engineer regiment has
    six engineers (its layout's first slot, `front`, holds 3, then
    three more single slots) and its captain, so it adds 30 health a
    second, 48 with Construction Battalions: a bunker takes about 42
    seconds from 1 health, or 26. [code, data, game]
- Building a site goes on until it's finished, the site is gone, or the
  regiment is given another order. An engagement interrupts it, and it
  resumes afterwards. [code]
- Repairing a finished building ends when it's at full health, when
  it's gone or no longer the side's or an ally's, or as soon as the
  regiment's side is in an upkeep shortage
  ([economy](economy.md)): finished buildings aren't repaired while the
  side's production falls short of its costs. Building sites carry on
  in a shortage. Being under siege doesn't stop it. [code]
- A Repair aimed at a regiment works on that regiment's damaged member
  nearest the repairing regiment, chosen afresh as it goes. [code]
- When a damaged building (not a site) reaches full health from below,
  by any healing, its owner gets "%s repair complete"
  (`building_repair_complete`, with `building_repair_complete_sfx`).
  [code]

## Ordering a repair

- Repair (`repair`) is offered by a regiment with a member that has a
  repair ability. Its button ("Repair", "Commands selection to repair
  target object.", slot 5, key I) arms the `repair` cursor ("Repair",
  frames `Cursors/Repair_%d.tga`, looping each second); a click on a
  building gives the order. A right click on a damaged own or allied
  building gives it too. Which targets it takes, and the message when it
  refuses one: [commands](commands.md#targeted-commands). [code, data]
- Neither Repair nor Build accepts a site under construction as a
  target, so a site the builders left can't be resumed by ordering a
  regiment onto it: it stays at the health it reached until it's
  finished some other way, cancelled or destroyed. [code]

## Cancelling and losing a site

- Cancel (`construction_cancel`: "Cancel", "Cancels the current
  action.", slot 11, `high_importance`, key C) shows while the selected
  building is under construction. It removes the site at once, without
  a death (no death effects, no lost line), and refunds the template's
  `[Cost]` in full, unadjusted, capped by the stockpile's maximum
  ([economy](economy.md)). [code, data]
- A site destroyed before it's finished pays its owner the template's
  sale value, as any destroyed building whose template has `can_sell`
  true does ([buildings](buildings.md#disbanding-and-selling)); bunkers
  inherit `can_sell = true`. [code, data]
