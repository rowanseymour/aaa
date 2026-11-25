# Economy

What each side earns and spends: production and upkeep, the money
stockpile, and the numbers the resource bar and tooltips show.

Games: traced in AA, with Kohan II's resources checked against the
resource rules. Kohan II runs the same code, but the rest hasn't been
checked against its data.

## Resources

- The resources are those in `Game/resources.tgi`: `money` (a purchase
  resource), `ammo` and `oil` (upkeep resources), and the regiment and
  building limits (`unit_limit_provided` / `_consumed`,
  `building_limit_provided` / `_consumed`). [code]
- Only purchase resources are stockpiled: money accumulates, ammo and oil
  never do. Ammo and oil are flows. Each side's production either covers
  its upkeep or it doesn't. [code]
- A resource that something else is `limited_by` (the regiment and building
  limits provided) is capped, in total, at `EconomyLimitedResourceMax` (20 in
  AA's SVars). Production beyond that is lost. [code]
- The limits are ordinary production and upkeep: a thing provides limit in
  its `[Production]` and uses it in its `[Upkeep]`. [code]
  - The building limit comes from the side's start location (`start_loc`,
    `Special/Things/thing_start_location.tgi`), an invisible thing that
    produces `building_limit_provided = 20`. So a side with one start
    location and a Corps HQ (which uses 1) shows 1/20. [code, game]
  - The Corps HQ, the support buildings and their trucks (ammo and oil
    depots, artillery and engineer brigades, motor pool, airfield) each use 1
    building limit. The supply truck uses none. [code] A Division HQ
    uses 1 as well, packed (`AAUnitVehicleHQTruck`) or unpacked: its
    recruit tooltip lists "Upkeep 1 building" and its economy line shows
    it (`berlin-corps-hq-build-tooltip.jpg`,
    `berlin-division-hq-truck-moving.jpg`). [data, game] It counts on the
    resource bar from the moment it's commissioned: with a Corps HQ and
    one Division HQ, still a truck, the bar reads 2/20
    (`berlin-division-hq-truck-moving.jpg`). [code, game]
  - Division HQs provide the regiment limit, packed or unpacked
    (`unit_limit_provided`, 5 for Infantry), as do battleships (5) and
    carriers (3). [code]
  - Every fighting regiment uses 1 regiment limit: `unit_limit_consumed =
    1` in the regiment base templates of
    `Templates/template_organization_aa.tgi` (`AAOrganizationArmor`,
    `AAOrganizationInfantry` and the rest), which each nation's regiments
    inherit. The Division HQ regiments (`AAOrganizationHQ`) use 0. [data]

## Who produces

- Every actor with an `EconomyComponent` that a side owns adds its
  `[Production]` to that side's totals and its `[Upkeep]` to that side's
  costs. Buildings, trucks and regiments all count, and all their rates are
  summed per resource. [code]
- The value is the actor's own data, with no multiplier, divisor or
  rounding: a packed Infantry Division HQ (`rus_truck_infantry`,
  `jpn_truck_infantry`, `usa_truck_infantry`, `gbr_truck_infantry`: money
  12, ammo 5, oil 2) adds +12 / +5 / +2, and unpacked (`*_hq_infantry`:
  25 / 10 / 5, USA 30 / 10 / 5) it adds +25 / +10 / +5. Germany's truck is
  18 / 7 / 3. [code, game]
- A regiment counts as one: its production is its own plus its members',
  and its upkeep is its own plus that of every position in it, whether
  the position's member is alive, lost or not yet arrived. So a regiment
  costs its full upkeep, and uses its regiment limit, from the moment
  it's recruited as a lone leader, and losses don't make it cheaper.
  Units inside a regiment don't also count on their own. [code]
- A Division HQ is a regiment whose one member is its truck or, unpacked,
  its building, and it counts like any other: what it adds is its
  member's production and upkeep, regiment limit provided included, and
  nothing of its own. The HQ regiments' own data (`AAOrganizationHQ` and
  the `regiment_*_hq_*` templates, packed and `_unpacked` alike) has no
  production and only `unit_limit_consumed = 0` as upkeep. So the side
  gets the truck's rates while it's packed and moving, the building's
  once unpacked, and the switch between them is the one under Unpacking
  and packing below. The regiment itself adds nothing of its own,
  whichever form it's in, except a production technology's extra: the
  HQ regiments carry `money`, so Military Surplus adds its +5 money to
  each of them (What changes the rates). [code, data]
- Towed trailers count for nothing. Every towed thing in stock data is a
  trailer (`AATrailer`, the `Trailers/*/*_trailer_*` things), and none
  has an `EconomyComponent`, so a map's trailer, though stored as a thing
  of its own beside the truck that tows it, adds no production or upkeep
  and uses no limit. The truck counts as usual, on its own or through
  its regiment. [code, data]
- A thing a map places without room is dropped as the map loads
  ([objects](objects.md), Loading a map), and gives its side nothing:
  no production, upkeep or limit, so a dropped start location's
  building limit is lost. Lying partly off the map drops a thing just as
  overlapping a blocking thing or standing on ground it can't does. A
  regiment loses everything with a dropped member, since the whole
  regiment is dropped with it. A start location blocks nothing, so only
  lying off the map can drop it. [code, data]
- A building that is disabled or still under construction produces nothing,
  a technology's extra production included. Its upkeep still counts.
  [code]
  - That holds for a thing counting on its own. A member of a regiment is
    counted through its regiment and isn't judged this way, so a Division
    HQ building would produce in full while disabled. In stock data it
    never is: only city buildings can be disabled, and Division HQ
    buildings are destroyed at 0 health instead. [code, data]
- A thing being taken out of play (destroyed, or removed from the map)
  stops counting for production and upkeep alike. That, with the rules
  above, is all: nothing else is left out. Units and regiments carried
  in a transport, falling paratroopers and dropped crates in the air
  keep their production and upkeep, and a carried regiment keeps its
  regiment limit. [code]
- Unpacking and packing aren't construction: a building produces from the
  moment its truck turns into it, through its unpacking, and through its
  packing until the truck replaces it. Which rate a Division HQ or depot
  gets over those seconds is the out-of-supply rule below, judged by the
  ground it stands on ([supply](supply.toml), Being in supply). [code]
- A building that packs into a truck and is out of supply produces its
  truck's `[Production]` instead of its own (so a Division HQ drops to its
  packed rate). Its upkeep is unchanged. [code] A production technology
  still adds to it, and whether one applies is judged by the building, by
  the technologies it holds, not by the truck's properties: an oil depot
  cut off earns its truck's 12 oil plus Oil Production's 5. (Stock trucks
  carry the same economy property as their buildings, so it comes out
  the same either way.) [code, data] In stock data that can
  only happen to Division HQs and depots, once they're cut off from supply
  and their supply stockpile has run out; a Corps HQ is never out of
  supply, and the other packable buildings are never judged. When and why:
  [supply](supply.toml). [code, data]
- `supply_required` (EconomyComponent, default false) stops an actor with
  no packed form from producing while it's out of supply. The Division HQ
  and depot building templates set it, but they all have packed forms, so
  the rule above applies to them instead and nothing stock ever produces
  nothing for want of supply. [code, data]
- A captured building's production and upkeep move to its new owner along
  with it. [guess]

## What changes the rates

- Research: a technology with a `ResourceProductionModifier` adds its
  `[Production]` to every actor of that side that has the technology's
  `required_properties`, per actor. For example `tech_oil_production` gives
  +5 oil to each building with the `oil` property. [code]
  - It's added whatever the actor's own production, none included: an
    actor with the property and an `EconomyComponent` gets the extra.
    So Oil Production also gives +5 oil to each motor pool and engineer
    brigade and to oil trucks, and Ammunition Production +5 ammo to
    artillery and engineer brigades and ammo trucks, as well as to the
    depots. Military Surplus (`tech_military_surplus`, `money`) gives +5
    money to the Corps HQ and to every Division HQ regiment. [code, data]
  - It counts only while the actor itself counts (Who produces): not on
    a disabled building or one under construction. [code]
  - A regiment counts it once for itself and once more for each member
    that has it: the regiment holds the technology when it has the
    properties itself or any of its positions does (a member, or for an
    empty position the type that belongs there), and each member holds
    it by its own properties. So a regiment with two qualifying members
    gets the extra three times. In stock data only the Division HQ
    regiments qualify, by their own `money`, and their members (the HQ
    truck or building) have none of the economy properties, so each
    gets the extra once. [code, data]
- `NO_UPKEEP` (a flag modifier) zeroes the upkeep of the actors it applies
  to, every upkeep resource alike: money, ammo and oil, and the regiment
  and building limits a thing uses. So a thing alone with it uses no
  limit, and the resource bar's used figures and its economy line leave
  it out; what's provided is production and doesn't change. [code]
  - For a regiment it zeroes only the regiment's own upkeep, not its
    positions': a regiment with it stops using its regiment limit (the
    1 its base template uses) but still pays every member's money, ammo
    and oil. [code, data]
  - A recruit's place in its queue is reserved against the limits by its
    type's upkeep, before the finished thing holds the flag. [code]
  - Stock data gives it only through `tech_noupkeep`, which has no
    `required_properties`. The random map setups' starting bunker groups
    (`template_rmc_aa.tgi`, and the World War campaign's
    `template_rmc_mg_aa.tgi`) give it to the bunkers they place, so those
    bunkers cost no ammo. [data]
- Difficulty and handicaps don't touch production. The stock handicaps
  change only `RECRUIT_COST` and health (see [players](players.md#handicaps)).
  [code]
- Nation, map and scenario matter only through which actors are on the
  map and their data. Starting money (below) is a one-off grant, not a
  rate. [code]
- Health doesn't scale production: a damaged building produces in full
  until it's disabled. [code]

## Ammo and oil shortfalls

- When a side's upkeep of ammo or oil exceeds its production, the shortfall
  is charged in money at that resource's `[PurchaseCost]` (ammo 2 money,
  oil 3 money per unit). It's added to money upkeep. [code]
- So overspending ammo or oil drives the money rate down, and below zero
  if it outweighs money production. Shown this way, money can go negative,
  and ammo and oil show their own negative balance. [code]
- That charge is all a shortfall does by itself. Units fire and move as
  before, and supply is untouched, until the money runs out (The
  stockpile). [code]

## The resource bar

- The bar has a cell for every resource defined, and what each cell shows
  follows from its resource's kind (the `[Resource]` template): [code]
  - a purchase resource (`PurchaseResource`, AA's money) shows its
    stockpile and rate;
  - an upkeep resource (`UpkeepResource`, ammo and oil) shows its rate;
  - a limited resource (`LimitedConsumedResource`, the `_consumed`
    limits) shows used against what its `limited_by` resource provides;
  - a resource that something is `limited_by` (the `_provided` limits)
    never shows a figure, though it's an upkeep resource by kind: its
    total appears only as the limit's `/provided`.
- Where each cell sits comes from the interface data, not from the order
  the resources are defined in: `EconomyBarWidget` in `UI/Game/
  template_game.tgi` has a child per resource, named after its IDS
  (case doesn't matter: `Ammo`, `Building_Limit_Consumed`), whose `[View]`
  `L` is the cell's offset from the bar's left edge. [code, data]
  - AA's offsets put money (6), ammo (139), oil (224), the building limit
    (306) and the regiment limit (381) left to right, which is why the
    bar reads that way while `Game/resources.tgi` and
    `Game/resource_list.tgi` list the regiment limit first.
  - The `_provided` children are parked at `L = 100%`, the bar's right
    edge, with `visible = false`. Being empty, they show nothing either
    way. [data]
  - Kohan II's data follows the same rule: gold (11), stone (109), wood
    (170), iron (231), mana (292), then the unit limit (354), with
    `unit_limit_provided` parked at `L = 100%`. Its `UI/Game/
    economy_bar.tgi` places the bar itself and the preview bar below it.
    [data]
  - A resource with no child in the layout still gets a cell, at the
    template's default place, which no shipped data exercises. [guess]
- Money, ammo and oil each show net production less upkeep, with the
  shortfall charge above already in money's figure. That's the rate per
  minute. [code]
- The regiment and building limits show used against provided (`0/5`), with
  provided capped as above. [code]
- Money also shows the stockpile. [code]
- Hovering something that would change the economy (a building to deploy,
  Unpack, a regiment to recruit) opens a second bar with the totals it would
  give: the item's cost taken off the stockpile, its production less upkeep
  added to the rates, and the caps and shortfall charge worked out again on
  the new totals. Unpacking a Division HQ truck previews the swap from the
  truck's rate to the building's (+37 to +50 money with a Corps HQ beside
  it). [code, game]
- How each figure reads, after the resource's icon character: [code]
  - A rate is a whole number with its sign always shown (`+12`, `+0`,
    `-12`), rounded to the nearest.
  - Money is the stockpile rounded down, then its rate in brackets:
    `1250(+12)`.
  - A limit is used over provided, each rounded to the nearest, with a
    trailing space: `3/5 `.
- Colours: only shortfalls stand out, in `UIColorOptionInvalid` (red, 1,0,0,
  a `point3` in `data/UVars.tgi`); everything else is in the bar's normal
  text colour. [code, data]
  - A rate below 0 is drawn in red, sign and all. A rate between -0.5 and 0
    still counts, so it shows a red `-0`. Positive and zero rates aren't
    coloured: there's no green.
  - The money stockpile is red when below 0. It never is on the main bar,
    but the preview bar can show it, when the item costs more than you have.
  - A limit's used figure is red when it's above provided (`6/5`). A full
    limit (`5/5`) isn't coloured, nor is the `/provided` part.
  - Nothing else changes colour: a stockpile at 0 or an upkeep shortage
    leaves the bar as it is, apart from the negative rates themselves.
- The preview bar follows the same rules, on its own totals, and shows only
  the figures the item would change; a figure it leaves alone is hidden.
  [code]
- A tooltip's Production row is the item's own `[Production]`. When you
  commission a building from the Corps HQ, that item is the truck that gets
  built, so the Infantry Division HQ's tooltip shows +12 / +5 / +2 (the
  truck's), not the unpacked building's values. [code, game]

## The selection panel's economy line

The line of resource figures along the bottom of the object display
(`Economy` in `UI/Game/object_display.tgi`): ammo -20, oil -20 and 1
regiment for a tank regiment, +25 money for a Corps HQ.

- It is live, not the template's listing: each figure is what the
  selected thing counts for in its side's totals now, its counted
  production less its counted upkeep. All the rules under Who produces
  and What changes the rates apply, so: [code]
  - a building that is disabled or under construction shows no
    production, only its upkeep;
  - a Division HQ or depot out of supply shows its truck's production;
  - a technology's extra production is included, and `NO_UPKEEP` removes
    the upkeep figures;
  - a regiment's line is the regiment's whole: its members' production
    and every position's upkeep. A unit shown alone out of a regiment
    has no line of its own, since it doesn't count by itself;
  - a city's line sums its buildings.
- It changes when the thing's count does (on packing, unpacking, being
  disabled, going in or out of supply), not continuously. [code]
- Format: one entry per resource of `Game/resources.tgi`, in that
  file's order (money, ammo, oil, `unit_limit_provided`,
  `unit_limit_consumed`, `building_limit_provided`,
  `building_limit_consumed`), each the resource's `icon` character and
  then the figure, with two spaces after each entry. A resource whose
  figure is 0 is left out. [code, data]
  - Limit provided and limit used are two resources with the same icon,
    so they are two entries, never netted against each other: a thing
    that both provided and used the regiment limit would show the
    regiment icon twice, "+n" and then "m". Regiments come before
    buildings either way. [code, data]
  - Every resource but the two `_consumed` ones is written with its
    sign and no padding, as `%+g` prints it: "+25", "-20", "+6" for
    regiment limit provided, a fraction if there is one. [code, game]
  - A `_consumed` resource (one that is `limited_by` another) is written
    as a bare whole number, the size of the figure: "1" for a regiment
    using one regiment limit, "1" for a truck or building using one
    building limit. [code, game]
  - Seen: a packed Infantry Division HQ reads money +12, ammo +5, oil
    +2, regiments +6, buildings 1
    (`berlin-division-hq-truck-moving.jpg`); an unpacked Armor Division
    HQ +20, +10, +15, regiments +3, buildings 1
    (`temperate-division-hq-hovered.jpg`); a tank regiment ammo -20, oil
    -20, regiments 1 (`temperate-tank-regiment-selected.jpg`). [game]
- Nothing in it is red, or any colour but the label's own: unlike the
  resource bar, a negative figure isn't picked out. [code]
- The line is blank for an enemy's thing, for a thing the player isn't
  seeing now, and when the viewer has no side. An ally's thing shows
  it. [code]
- Hovering the line shows the same text as a tooltip. [code]
- The thing tooltip gives the same figures split into Production and
  Upkeep, with a Maximum beside them when the thing counts for less
  than its template could
  ([object display](object-display.md#the-economy-rows)). [code]

## The stockpile

- The money stockpile is credited continuously, every simulation tick, by
  `net rate * elapsed seconds / 60`. Rates are per minute, and fractions
  carry over in the stockpile. [code]
- It never goes below 0 or above `EconomyStockpileResourceMax` (99999). [code]
- When the stockpile would go below 0, the side is in an upkeep shortage:
  the message "Your production is N% short of your costs" appears, where N
  is how far income falls short of costs, both in money terms, as a
  percentage cut down to a whole number (12.9% shows as 12). It's shown when
  the shortage starts and again whenever the shortfall moves more than 5
  points from the figure last shown, and a "repaired" event follows when
  the shortage ends. [code]
- While the shortage lasts, the side's regiments resupply more slowly and
  those out of supply lose health: see [supply](supply.toml). Nothing else
  changes: production, queues and buildings carry on. [code]

## Starting money

- At the start of a match each side's money is set as follows, in order:
  [code]
  - A side whose map entry uses the default stockpile (the default for a
    new side) starts at `ResourceStockpileDefault`, kept within 0 and
    `EconomyStockpileResourceMax`. A side that doesn't keeps the money
    stored for it in the map.
  - Then any stockpile bonus the match setup gives the side is added. Only
    the World War campaign gives one: `StockpileBonus`, or
    `WaterStockpileBonus`, probably when attacking from water [guess].
  - Then the side's handicap's `ExtraResources` are added. No stock
    handicap has any.
- `ResourceStockpileDefault` is the Money world rule in
  `Game/world_rules_aa.tgi` (default 500, at most 99999). Its value for
  the match replaces the SVar of the same name. That value is the one
  chosen when setting up the match. In the Scenario Map dialog it starts
  at the map's stored value, so Moscow starts at 1000 unless it is
  changed ([match start](match-start.md#world-rules)). [code]

## Paying for commissions

- Recruiting a regiment, commissioning a truck and starting research all
  work the same way: the order is checked, paid for in full at once, and
  joins the building's queue. Costs are never paid as a rate. [code]
- What a recruit costs is its `[Cost]` scaled by the `RECRUIT_COST`
  multipliers the recruiting building or HQ carries, the side's
  technologies included (a handicap's, or `tech_conscription` on an
  Infantry Division HQ): cost × (1 + the amounts added together), never
  below `ModifierAttributeClampMultiply` times the cost. A thing built in
  place by a builder (bunkers, supply depots) goes by `STRUCTURE_COST`
  instead, which no shipped property sets. Research costs aren't scaled.
  [code, data]
  - The multipliers are the recruiter's alone, the thing whose
    `[RecruitComponent]` lists the recruit: its own properties' and those
    of the side's technologies it holds (by their `required_properties`,
    or every one without). The recruit's own properties play no part. A
    regiment's whole cost, its own `[Cost]` and every position's, is
    scaled the same way. [code]
  - So `tech_conscription` (`infhq`) scales recruits only at Infantry
    Division HQs, `tech_mechanized_production` (`mechhq`) at Mechanized
    ones and `tech_tank_production` (`armorhq`) at Armor ones. The
    Infantry Division HQ truck carries `infhq` too, but recruits nothing.
    [code, data]
  - A handicap's technology has no `required_properties`, so it scales
    everything every recruiter charges: the Division HQs and trucks a
    Corps HQ commissions, regiments at Division HQs, and the Corps HQ
    trucks a battleship lands. [code, data]
  - Rapid Deployment (`rapid_deployment_tech`, `hq`) lands on the
    Division HQ regiments, which recruit nothing; the Division HQ
    buildings that do recruit don't carry `hq`. So it changes no cost.
    [code, data]
- The scaled cost isn't rounded: a 150 recruit at × 0.75 takes 112.5 from
  the stockpile. Costs are shown rounded up to a whole number, in the
  recruit's tooltip and in "Insufficient funds" (113 here), and the
  tooltip shows the cost through the selected building's own multipliers.
  [code]
- An order the side can't afford is refused, not queued: the message
  "Insufficient funds for X. Requires Y." appears and nothing is taken.
  Only money is checked, against the stockpile. [code]
- Nothing caps how many of one type a side may have or have queued.
  An order is checked against the side's allowance for the type
  ([recruiting](recruiting.md#offered-greyed-or-hidden)), but that
  counts nothing in AA: every type a side isn't barred from by its
  map's restrictions is allowed without limit. The only allowance of one
  is a Kohan II hero's ([companies](companies.md)). [code, data]
- The regiment and building limits aren't checked when the order is
  placed. An order is accepted, and paid for, even when the limit is
  already full. [code]
- Limits are checked when the item reaches the front of the queue. If the
  side has room (what it uses, plus the item's own use, is no more than
  what it's provided, after the cap) the item takes its share of the limit
  at once. It shows as used on the bar while it's built, and the finished
  thing then holds it. [code]
- If there's no room, the queue pauses: the item's progress stops, and it
  carries on from where it was once room is free. The items behind it wait
  too. The message "Insufficient unit limit to recruit X" appears when the
  pause starts (for the building limit too), and the item's tooltip shows
  Paused: Unit Limit. [code]
- A queue also pauses, the same way, while its building is disabled ("X
  paused production queue - disabled"), and while a building that relays
  supply is out of supply ("X paused production queue - no supply"; see
  [supply](supply.toml) for when that is). These apply to research too.
  [code]
- Cancelling a queued item refunds its whole cost, however far it had got,
  and frees any limit it had taken. [code]
- A building's whole queue is cancelled, each item refunded in full, when
  the building starts packing up, when it changes owner (the refund goes to
  the side that owned it), and when it leaves the map. [code]
