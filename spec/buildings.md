# Buildings

How buildings pack into trucks and unpack again, how cities hold their
members, what decides who holds a city, when a building is under siege,
how buildings are captured by damage, and disbanding and selling. What a
building recruits is in [recruiting](recruiting.md).

Games: traced in AA. Kohan II's data has no trucks or cities, so packing
and the city rules are AA's alone; the rest runs the same code in Kohan
II, unchecked against its data.

## Packing and unpacking

- A `MobileDeployment` truck names the building it unpacks into
  (`unpack_to`). The building's own data names no truck: the truck it packs
  into is worked out from the trucks. [code]
- Where several trucks unpack into the same building, the building packs
  into the one defined first in the data. [code] In shipped data only the
  Corps have two trucks; for every nation the plain Corps truck comes first,
  so `<nation>_corps` packs into `<nation>_truck_corps` and
  `<nation>_corps_dynamic` into `<nation>_truck_corps_dynamic`, never the
  amphibious truck. That a battleship recruits the amphibious truck plays no
  part. [code]
- `packed_position_offset` is a world-space offset, never turned by any
  facing: [code]
  - Unpacking puts the building at the truck's position minus the offset,
    facing the way the unpack order gives.
  - Packing puts the truck at the building's position plus the offset,
    facing `packed_orientation` (also in world space).
  - Every shipped offset is 0, 0. An offset longer than the building's
    radius is a logged error at load but still used. [code]
- The truck always comes from the truck's own template, whichever building
  packs. [code]
- Room to unpack is the room-to-stand check in [objects](objects.md), as
  it applies to unpacking. Nothing else in placing the building refuses
  an unpack (see [Settlements and spacing](#settlements-and-spacing)), so
  a spot the placement refuses is always one without room. [code, data]
- Scenery (trees, hedges, tall grass) never refuses an unpack; the
  building clears the scenery under its footprint as it arrives, and
  packing doesn't bring it back. See
  [objects](objects.md#scenery-under-a-building). [code]
- Supply isn't tested either: a building may be unpacked outside every
  supply zone. One that relays supply (a Division HQ, a depot) then simply
  doesn't work until supply reaches it, as [supply](supply.toml) describes;
  the tutorial's advice to unpack the Supply Depot inside the Corps HQ's
  zone is advice, not a rule. [code, data]
- The building faces the way the unpack order gives. How the player sets
  that facing is open. [code]
- Unpacking and packing keep the place in an organization: the building
  takes the truck's slot in the truck's organization, and packing puts the
  truck back in the building's slot of the building's organization. The
  organization and its other members are untouched and it keeps its id;
  no new organization is made. A building in no organization packs into a
  lone truck. [code]
  - So a Division HQ unpacked from a commissioned
    `regiment_<nation>_hq_*` packs back into that same regiment.
  - A Division HQ a map stores unpacked in an organization with a bunker
    packs into a truck in that same organization, beside the bunker; no
    `regiment_<nation>_hq_*` is made.
- Packing makes a single truck, not a new organization, unless the truck's
  own template is an organization; no shipped truck is. [code]
- Each change hands over the thing's id: the building gets the truck's id,
  and the truck that packs gets the building's. So a truck that unpacks and
  packs again comes back under its old id, though it's made afresh from the
  truck template (only its health fraction carries over). Selection passes
  to the new thing as well. [code]

### An unpack order, from start to finish

An unpack order (Unpack, `unpack`) sends a truck, or the regiment it
leads (a Division HQ's `regiment_<nation>_hq_*`), to a spot to unpack
there. It goes in two stages. [code]

- **On the way.** The truck or regiment drives there like any move. Its
  state reads as moving, and it keeps every command it has: the grid
  shows its usual buttons (Unpack enabled, Move, Stop and the rest), plus
  Command All for an HQ and Team Commands, and it takes orders from
  buttons, hotkeys and right-clicks alike. [code]
  - Any new order that isn't queued (Stop, a right-click move, another
    Unpack elsewhere, a group order that reaches it) replaces the unpack
    order: the unpack is called off and the truck stays a truck where it
    is. A queued order (Shift) waits behind the unpack instead
    ([queued orders](commands.md#queued-orders)). [code]
  - If it can't get there it gives up with "Cannot unpack %s"
    (`tai_error_unpack`) and stays a truck. [code]
- **At the spot.** Once there, its state reads "Unpacking"
  (`mind_unpacking`) and it has no commands at all until the building
  has finished unpacking. A lone truck turns into the building straight
  away; a regiment hands its truck the order and turns into the building
  a moment later. A truck that can't unpack there (no room) gets "Cannot
  unpack %s" and stays a truck; a regiment tries twice before that.
  [code]
  - Every order given to it in this stage, by any means (a button, a
    hotkey, a right-click, Stop, a control group), is dropped without a
    message. It can't be called off, sold or disbanded: once a truck has
    reached its spot, the unpack always runs to the end, unless the
    building is destroyed. [code]
  - In a selection with other things, each thing is ordered on its own
    and the others obey; only the unpacking one ignores the order. A
    button shows when any selected thing has the command, so the grid
    still shows it for the others. [code]
  - The grid shows only what isn't a command: Command All for an HQ,
    Team Commands, and the building's greyed Pack once it exists
    ([commands](commands.md#a-buildings-state-and-its-commands)). [code,
    game]
- **The truck becomes the building.** The truck leaves the map and the
  building takes its place, id and organization slot (above). If the
  truck was selected, the building is selected now. [code]
  - A truck in a regiment: the regiment stays, with the building in the
    truck's slot. There's no truck left to select. Clicking the building
    selects its regiment, as for any thing in a regiment, so the
    regiment and the building are never selected apart. [code, game]
  - A lone truck (a supply depot's): the building is a lone thing,
    selected by itself. [code]
- When the building has finished unpacking, the regiment's commands come
  back (those its building has, like Deploy and Sell), and any order
  queued behind the unpack is carried out. [code; guess: the queued
  order]

## Settlements and spacing

- The placement check has settlement-area and spacing conditions on top
  of room to stand, but the shipped data never sets them off: [code, data]
  - They concern settlements (things whose template has a
    `SettlementComponent`, with the zone a settlement claims) and templates
    flagged `needs_space_for_settlement_zone`. That flag defaults to
    `false` in the base object template and no shipped template sets it;
    no shipped template is a settlement, and no shipped map holds one.
  - The zone's size comes from the settlement templates, so with none it's
    0. Nothing in the settings files sets it.
  - A `CityComponent`'s `zone_radius` plays no part in placement.
- So nothing is refused for settlement areas or spacing anywhere: not when
  unpacking, not as a map loads, not in the editor, nor anywhere else that
  places a building. Spacing isn't even
  tested when unpacking or loading a map. Building these conditions isn't
  needed for this game. [code, data]

## City members

- A city centre lists the ids of the things that belong to its city. The
  ids are kept as stored and only looked up when used; one that names
  nothing on the map is skipped without error. [code]
- Capturing a building goes by the building's own city, not the centre's
  list, and city counts and victory count city centres, so an id that names
  nothing has no effect. [code]
- Except: a thing created during play can be given an id that a list still
  holds, and then it counts as that city's member (changing owner with the
  city). [code] Whether shipped maps' unresolved ids can be reused like that
  is open.

## Holding a city

- A city centre's zone is a zone as [objects](objects.md) describes
  (Zones): whole tiles around the centre, a thing being in it by the tile
  under its centre, not by any edge. [code] Its radius is probably the
  `CityComponent`'s `zone_radius` (42 for stock city centres). [guess]
- A city centre keeps track of the things in its zone that count towards
  holding it, its occupants. A thing counts when all of these hold: [code]
  - it isn't the centre itself, and is alive;
  - it belongs to a player, and not an independent one;
  - it isn't a member of a regiment (the regiment counts as one occupant
    instead, for all its members);
  - it isn't airborne; [guess]
  - it has some weight (below);
  - it has no health, or it can be captured, or it's destructible.
- An occupant's weight is its combat value ([combat](combat.md#combat-value))
  times its health fraction; a regiment's is the sum of that over its
  members. A thing's combat value comes from its best weapon, and the
  weapons of a `[DefenderComponent]` count as well as its own abilities. So armed buildings count like
  units: a Corps HQ, a bunker, a Division HQ through its regiment when it
  has one. Unarmed buildings weigh nothing and don't count. Shipped
  defenders are all destructible, so none is left out for that. [code,
  data]
- An owned city stays its owner's unless enemies of the owner have weight
  in it and the owner's side has none. Then, and for an unowned city, it
  goes to the first player with weight in it whom no one present threatens
  and no ally outweighs or is threatened. [code]
- The same weights decide whether a held city gives supply:
  [supply](supply.toml) (Who gives supply). [code]
- An owned city is contested while anyone is in its zone and the owner's
  side (the owner and its allies) doesn't outweigh the owner's enemies
  there: equal weights count as contested. It isn't contested in the
  moment the owner itself has no weight there and an enemy has, since the
  city then changes hands instead (above). [code]
- The city is re-weighed once a second while anyone is in its zone. When
  the last occupant leaves, it stops being contested within that second
  and isn't re-weighed again until someone comes in. A city nobody has
  entered since the game began isn't contested. [code]
- A contested city gives no supply ([supply](supply.toml), Who gives
  supply), and its centre is under siege (below). [code]

## Under siege

A building's under-siege state, which the game also calls contested. It
shows as a red pulse on the building, a red blink on the minimap and the
state "Under Siege" in its panel.

- Every building can be under siege: the state belongs to the `Building`
  template, which all buildings inherit (HQs, depots, bunkers, airfields,
  city blocks and city centres alike). Armed or not makes no difference.
  [code, data]
- A building is under siege while any of these holds: [code, data]
  - an enemy damaged it within the last `UnderSiegeRecentlyDamagedSeconds`
    (7 in AA's SVars; the base SVars leave it unset). Only damage counts,
    and only damage whose source is a thing of an enemy side: a shot that
    misses, an ally's or its own side's damage, and damage with no thing
    behind it (properties' damage over time, supply-shortfall damage)
    don't start or prolong it. Any enemy damage does: a direct hit, an
    area hit, a bombardment, a special operation;
  - a bombarder still holds it as a bombardment target: from a
    bombardment hit until that bombarder's `BombardTargetTimeout` (plus
    its weapon's recharge) runs out, as for regiments
    ([supply](supply.toml), Bombardment). Since a bombardment hit is enemy
    damage too, this only matters when the bombarder's time outlasts the
    7 seconds;
  - it's a city centre and its city is contested (Holding a city).
- What doesn't count: enemies being near, being seen, being targeted,
  being engaged by a regiment or fired at without effect, and the building
  firing on enemies itself. A building with its own guns (a bunker, an
  HQ's `[DefenderComponent]`) that spots or shoots at enemies isn't under
  siege until an enemy actually damages it. [code]
- Timing: [code]
  - it starts the moment the first enemy damage lands;
  - otherwise the building checks once a second (each building on its
    own second), so it ends at the first check after
    `UnderSiegeRecentlyDamagedSeconds` have passed since the last enemy
    damage and nothing else above holds: 7 to 8 seconds after the last
    hit. Each new enemy hit restarts the 7 seconds;
  - a city centre also re-checks at once when its city's zone empties;
  - it ends at once when the building is destroyed. Being disabled
    (brought to 1 health but not destroyed) doesn't end it, nor, as far
    as traced, does a change of owner. [code, guess]
- While a building is under siege: [code, data]
  - it pulses red (`under_siege_vfx`): the `Building` template starts the
    tint when the state begins and stops it when the state ends, so the
    pulse and the state start and end together. The tint outranks the
    out-of-supply one ([effects](effects.md#tints));
  - the minimap blinks it red if it's one of the viewer's own
    ([supply](supply.toml), the minimap);
  - its panel can show the state "Under Siege" (priority 12, so it
    usually wins over the other states); a city centre also lists the
    "Contested" property ("City is contested and does not provide
    supply."), which shows only on cities and does nothing itself;
  - it can't be sold: `AllowSellUnderSiege` is false in AA's SVars (true
    in the base ones);
  - healing from its properties' periodic heal (such as the city blocks'
    `city_building_repair`) pauses until the state ends;
  - it keeps giving supply, since `SupplyProvideUnderSiege` is true in
    AA's SVars ([supply](supply.toml)). A contested city's centre stops for
    being contested, not for being under siege.
- It doesn't raise the "%s is under siege" message: in AA nothing does
  ([messages](messages.md)). [code, data]
- Units carry the same state, under the same damage rule
  ([combat](combat.md), Engaged and under siege). [code]

## Capture by damage

- A building can be captured by damage when its `captureable` is true,
  unless it's a city centre or a member of a city that still stands, or
  a modifier has made it not capturable. City centres change hands only
  by their zone (Holding a city). [code, data]
- When an attack by another player brings a capturable building to no
  health, it isn't destroyed: it goes to the attacker's owner with its
  health set to `CaptureHealthFactor` (0.3) of its maximum. [code, data]
- Either way of changing hands tells both players: [game
  events](game-events.md) (Capture). [code]

## Disbanding and selling

Disband ("Permanently disbands selection.") gets rid of a regiment or a
lone unit; Sell ("Instantly sells the structure for %s.") gets rid of a
building for money. Both prefer slot 11, are `non_group_command`, ask for
confirmation and are on the Delete key: see
[commands](commands.md#command-grid) for those rules, which they share.
A thing in a regiment is selected as its regiment, so a Division HQ,
packed or unpacked, is its HQ regiment (`regiment_<nation>_hq_*`).
[game, guess]

### Who can disband

- A regiment can be disbanded unless: [code]
  - it's in combat (engaged with an enemy);
  - it's routing;
  - it's attached to an HQ (its slot 11 then holds Detach from HQ);
  - one of its members offers Sell (an unpacked Division HQ, below);
  - it holds a `NOT_DISPOSABLE` modifier.
  Being out of supply, under siege, moving or exhausted doesn't stop it.
- So a packed Division HQ shows Disband and an unpacked one shows Sell.
  An unpacked HQ that can't be sold at that moment (under siege) shows
  Disband in its place, if nothing above rules it out. [code, data]
- A unit that isn't in a regiment (a supply or depot truck) can be
  disbanded unless it's a building or holds `NOT_DISPOSABLE`; nothing else
  stops it, combat included. A unit in a regiment can be disbanded only
  while its regiment isn't attached to an HQ. [code]
- Nothing has a command while it's unpacking, or from the moment a truck
  reaches its unpack spot ([An unpack order](#an-unpack-order-from-start-to-finish)),
  and a building has none while it packs, so neither Disband nor Sell
  shows then. [code] What the grid
  shows for a building while it packs, unpacks, researches and so on:
  [commands](commands.md#a-buildings-state-and-its-commands).
- `NOT_DISPOSABLE` comes from the `indispensable` property; no shipped
  template holds it, so in stock play it comes only from what a map or
  trigger gives. [data]

### What disbanding does

- It happens at once, with no delay or animation and no refund. [code]
- A regiment is destroyed: its owner gets "%s destroyed" and enemies who
  can see it "%s's %s destroyed" ([messages](messages.md#destroyed-and-lost)).
  Its units leave the map with it without dying one by one: no death
  effects play for them. [code]
- A lone unit (a supply or depot truck) just leaves the map: no destroyed
  or lost line for anyone and no death effects. [code]
- Disbanding an HQ regiment ends every attachment to it, as losing the HQ
  does ([supply](supply.toml)). [code]
- What it provided or used of the regiment and building limits goes with
  it, as for anything that leaves the map ([economy](economy.md)). [code]

### Who can sell

- A building offers Sell when its `ConstructionComponent` has `can_sell`
  (true by default) and none of these holds: [code, data]
  - it's still being built;
  - it's under siege (`AllowSellUnderSiege` is false in AA's SVars, true
    in the base ones; see [Under siege](#under-siege));
  - it holds a `NOT_DISPOSABLE` modifier;
  - it's packing or unpacking (above).
  Recruiting or researching doesn't stop it, nor does being out of supply.
- Who built it doesn't matter: a building placed with the map (the
  starting Corps HQ) or captured from another side sells like one the
  side made. [code]
- `can_sell = false` means no Sell button at all. The village, farm and
  city centre templates (`template_buildingvillage_aa`), the battleship
  and the campaign's beach bunkers and island airfields set it, so city
  buildings are never sold. [code, data]
- `can_raze` matters only when `can_sell` is false: such a building
  offers Raze instead. Every shipped template that sets `can_sell` false
  also sets `can_raze` false, so Raze never appears in AA. [code, data]
- An unpacked Division HQ is sold through its regiment: the regiment
  offers Sell while its building does. [code]

### What selling does

- It happens at once. The owner is paid `SellFactor` (0.5, base SVars) of
  the building's worth, resource by resource, each rounded to the nearest
  whole number (halves up). [code, data]
- A building's worth is the `[Cost]` of its own template, unmodified: not
  what was paid for it or for the truck it came from, and no
  `RECRUIT_COST` or `STRUCTURE_COST` modifier or handicap applies. Every
  resource in that cost is paid back (money, ammo or oil alike), whatever
  the building's health. [code]
  - So an unpacked Infantry Division HQ (`ger_hq_infantry`, 250 money)
    sells for 125 money, and a Corps HQ (500) for 250. [data]
  - A template with no cost sells for nothing. [code]
- The payment is added to the stockpile like other income, capped at
  `EconomyStockpileResourceMax` ([economy](economy.md)). [code]
- The Sell button's tooltip shows the same figure, written as a cost is;
  for a regiment it's the sum over its buildings. [code]
- The owner gets "%s sold for %s" (`building_sold`, the building's name
  then the payment, with its sound `building_sold_sfx`). Nobody gets a
  destroyed or lost line. [code, data]
- The building leaves the map without dying: no death effects, no
  rubble. [code]
- Its queue is cancelled and every item refunded in full, as when a
  building leaves the map ([economy](economy.md)). [code]
- If it's an HQ, every regiment attached to it is detached, as when the
  HQ is lost ([supply](supply.toml)). [code]
- If it was the last member of its regiment (an unpacked Division HQ),
  the regiment goes with it, silently. Any other members stay. [code]
- Its limits go as for anything leaving the map, above. [code]
- A building whose template has `can_sell` true also pays its owner its
  sale value when it is destroyed. Only `can_sell` counts: none of the
  other conditions on the Sell button apply, so being under siege (as a
  building that has just been destroyed nearly always is), holding
  `NOT_DISPOSABLE`, or being mid-pack or mid-unpack doesn't stop the
  payment. The owner gets the same "%s sold for %s" line, always after
  the "%s destroyed" one. The worth is paid only once. [code]

## Open questions

- What a regiment whose building is packing offers on the grid, and
  whether orders reach it then. (analysis, 2026-11-22)

- How does the player set the facing an unpack order gives: does the
  placement preview turn (dragging, a key, the wheel), and which way does
  it face if left alone? Check in the game. (implementer, 2026-11-08)
- Can the ids in shipped city lists that name nothing on the map be handed
  to things created later in a match? (analysis, 2026-11-08)
- How often is a city's holder re-decided, and what radius is its zone?
  (analysis, 2026-11-08)
- Does a building with `can_sell` true that is destroyed really pay its
  owner its sale value with a "sold for" line, even under siege? Check in the game. (analysis, 2026-11-13)
