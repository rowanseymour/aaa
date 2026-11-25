# Research

How a building researches technologies, who gets them, how they're lost,
and how they unlock what a side can recruit.

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## What a building researches

- A building's `[ResearchComponent]` lists `ResearchItem`s. Each names a
  property (`property_IDS`), a `type`, a `research_time` in seconds, a
  `[ResearchCost]`, and optional prerequisites: `required_properties` and
  `required_structures`, each with an `_any` flag that makes the list
  "any of" instead of "all of". [code, data]
- `type` says who receives the property when the research completes:
  [code]
  - `GLOBAL`: the side. Every shipped item is `GLOBAL`.
  - `LOCAL`: the building that researched it.
  - `SUPPORT`: the settlement the building belongs to. Settlements are
    a grouping of buildings AA never uses: belonging to a city doesn't
    count, so no AA building has one (see "Who keeps what research gives"
    for what happens then).
- The list belongs to the building's template, not its owner. A side
  that captures another nation's research building gets that building's
  whole list, nation technologies included (the King Tiger on a German
  motor pool, say), offered and judged by the same rules against the new
  owner's own technologies. A `GLOBAL` item it finishes is its own for
  good, even if it loses the building again. [code]
- `required_structures` is met by a structure of the side's that is
  finished and of that template, or upgraded from it however many times.
  Which of the side's structures count follows the researching building:
  those in its settlement (the centre included) and those belonging to no
  settlement, but not those in the side's other settlements. AA has no
  settlements, so there any of the side's finished structures counts,
  wherever it stands. A structure still being built doesn't count, nor
  one that's been destroyed, nor one that has left the side (captured,
  sold, packed up). It's the same test recruiting uses for its
  `required_structures` ([recruiting](recruiting.md#offered-greyed-or-hidden)).
  [code]
- A structure downgraded below the named template no longer meets it,
  unless what it's downgraded to was itself upgraded from that template.
  No AA template upgrades or downgrades, so this matters only in
  Kohan II. [code]
- In shipped data every item takes 60 s and costs only money (50 to 150).
  The one item with a prerequisite is Germany's King Tiger, which needs
  `tech_heavy_tank`. [data]
- A technology's own `required_properties`, in `properties_technologies.tgi`
  (for example `tech_mechanized_production` has `mechhq`), aren't a
  research prerequisite. They pick which of the side's actors the
  technology applies to (see [economy](economy.md#what-changes-the-rates)).
  [code, data]

## The Research menu

- A building with a research list has the `research` command
  ("Research", `preferred_position` 6). It stays as long as the building
  has the list, even once everything is researched. Pressing it opens the
  research menu in the grid, like the Deploy menus in
  [commands](commands.md#deploy-menus). [code, data]
- The menu lays out the items in list order from slot 0, row by row, up to
  slot 10. With several research buildings selected, their lists follow
  one another in selection order. [code]
- Each item is in one of four states: [code]
  - available: a button.
  - greyed: a greyed button, when a prerequisite building is missing.
    The menu is judged afresh each time it's shown, so losing the last
    such building greys an item that was available, and getting one back
    makes it available again.
  - done or under way: the slot stays empty, and the items after it don't
    move up. That covers an item already researched (for `GLOBAL`, the
    side has the technology; for `LOCAL`, this building researched it; for
    `SUPPORT`, the settlement has it), one in this building's queue, and a
    `GLOBAL` item being researched in any building of the same player.
    Only that player's own buildings and technologies count: an ally's
    or teammate's research, finished or under way, doesn't empty the
    slot, so allies can each research the same technology, at the same
    time if they like.
  - hidden: no button and no slot, so the items after it move up. That
    covers a missing prerequisite property, and a technology the side is
    barred from (see Scenario restrictions).
- Slot 11 holds Cancel, which goes back to the commands, as in the Deploy
  menus. [code]
- So each item is researched once only (`SUPPORT` items aside, below).
  It's shown as an empty slot once it's done, never greyed. A `GLOBAL`
  technology can't be researched twice at once by two buildings of the
  same player. [code]
- An available item's tooltip, top to bottom: [code, data]
  - the technology's `name` (with its key marked, as for any button; see
    [commands](commands.md#hotkeys-in-tooltips));
  - a line titled `ui_tooltip_item_title_cost` with the item's
    `[ResearchCost]`, written the same way as a recruit's cost; no line
    when the cost is empty;
  - then what the technology's own property tooltip shows (see
    [object display](object-display.md#property-icons)): the "Affects" line naming
    the technology's `required_properties`, and one line per modifier;
  - last, the technology's `description`, when it has one.
  The research time isn't shown, nor whether the side can afford it.
- A greyed item's tooltip is only
  `research_error_missing_requirements` ("%s required: %s"): the
  technology's name, then what's missing. [code, data]
- A research key (`PropertyDataSelectCommand <property>` in the hotkey
  files) works only while the item's button is showing in an open
  research menu. [code, data]

## Researching

- Ordering research is checked, paid in full at once from the stockpile
  and queued, like recruiting (see
  [economy](economy.md#paying-for-commissions)). Only money is checked.
  [code]
- An order the side can't afford is refused with the message
  "Insufficient funds for %s. Requires %s." (`research_error_affording`:
  the technology's name, then its cost). An order for an item that isn't
  available (by its key, say, when the slot is empty) is refused with
  "Could not research %s" (`research_error_generic`, the technology's
  name). Both are order-failure messages: `GENERIC_ERROR`, shown only to
  the player who gave the order, located at the building, and so with
  that kind's `error` sound (see [messages](messages.md)). Neither shows
  in the tooltip. Nothing is taken or queued. [code, data]
- Prerequisites are judged only when research is ordered. Research under
  way carries on to completion if the side then loses the prerequisite
  building (destroyed, captured, sold, packed up) or it's downgraded, and
  the technology is gained as usual; nothing is cancelled or refunded.
  The same holds for a prerequisite property lost meanwhile. While the
  item is queued, and once it's done, its slot stays empty as usual, not
  greyed. [code]
- Research shares the building's one queue with its recruits, in order.
  It waits behind them and they wait behind it. [code]
- While research is at the front of the queue the building is in the
  state "Researching" (`actor_states_structure_researching_name`); while
  a recruit is at the front, "Deploying"
  (`actor_states_structure_recruiting_name`), switching back and forth as
  the front changes; with the queue empty, "Normal". [code, data] What
  the state line makes of these, and why a Division HQ reads "Resting"
  while it recruits, is in [object
  display](object-display.md#name-owner-and-state-lines): a lone
  building that researches reads "Researching".
- An item takes `research_time` seconds once it reaches the front. Research
  takes no unit or building limit, so it never pauses for one. It does
  pause while the building is disabled or out of supply, like any queue
  item. [code]
- Cancelling refunds the whole cost. A building's whole queue, research
  included, is cancelled and refunded when it starts packing, changes
  owner (the refund goes to the old owner) or leaves the map, destroyed
  or otherwise. Nothing is researched in those cases. [code]
- On completion the property goes where `type` says (below) and "%s
  research complete" (`research_complete`) is posted. That's the only
  line: research never posts "technology gained". [code, data]

## Who keeps what research gives

- `LOCAL`: the property stays with the building for good, through a
  change of owner too: the new owner's building keeps it, and keeps the
  item's empty slot, so it can't be researched again there. [code]
- `SUPPORT`: with no settlement (always, in AA) the research is ordered,
  paid for and completes as usual, with "%s research complete", but the
  property goes nowhere. Nothing records it as done, so the item is
  offered again straight away and can be paid for and researched any
  number of times. No shipped item is `SUPPORT`. [code, data]
- A side's researched technologies are covered next.

## How a side gets and loses technologies

- A side gets a technology: [code]
  - by finishing `GLOBAL` research;
  - from the scenario: each side's start technologies in the map, and the
    `kingdom_gains_technology` trigger effect
    ([triggers](triggers.md#technologies));
  - from its handicap's property (see [players](players.md#handicaps)), and
    from its general's `properties` (none set in shipped data).
    [code, data, guess]
  - while it holds a building whose `[StructureComponent]` lists
    `[TechnologyProperty]` entries (the special operations' supply drops,
    such as assembly parts, infantry supplies and deployment crates).
- Each of these is a grant of its own, and a side can hold several grants
  of the same technology at once (two buildings, a building and research,
  a trigger twice). The side has the technology while it holds at least
  one. [code]
  - The first grant takes effect, and says "%s technology gained"
    (`technology_gained`), except research, which says only "research
    complete" (above). Later grants add nothing and say nothing.
  - Losing a grant while another remains changes nothing and says
    nothing. Losing the last takes the technology and its effects away,
    with "%s technology lost" (`technology_lost`).
  - A barred technology is never granted at all
    ([Scenario restrictions](#scenario-restrictions)).
- What takes a grant away: [code]
  - a building's grant goes when the building leaves the side (below);
  - start technologies, the handicap's and the general's are never taken
    away;
  - research's and the trigger's grants are taken away only by the
    `kingdom_loses_technology` trigger effect, one grant at a time
    ([triggers](triggers.md#technologies)). Losing the building that did
    the research takes nothing away. A researched technology taken away
    this way is offered in the research menu again, since the side no
    longer has it.
- A building's grant: [code, data]
  - It starts the moment the building joins a side: as it appears (made,
    placed by the map, unpacked or dropped in; a falling crate already
    counts), or as it changes hands. Being under construction or disabled
    makes no difference: a building site already grants.
  - A building of an independent side grants nothing.
  - It ends when the building leaves the side: when it changes hands, is
    sold or packed up, or its remains are taken off the map. A destroyed
    building keeps granting while its remains stand, `rot_time` +
    `sink_time` after it's destroyed
    ([combat](combat.md#death)); for the supply drops that's 2 seconds,
    the default.
  - On a change of hands the old side's grant goes first (with
    "technology lost" if it was its last), then the new side's starts
    (with "technology gained" if it's new to it), at the same moment.
  - Gaining and losing take effect in the tick the building joins or
    leaves, and units' health and other effects change with it.
- The side's technologies are listed in the control panel's technology
  display (see [object display](object-display.md#technology-display)). [code]
- A property an actor carries in its own `[PropertiesComponent]` (the
  battleship's `tech_battleship`, the Corps HQ's `mechhqproduction`) counts
  only for that actor, even when it's marked `is_technology`. So does one
  given to an actor by `actor_gains_technology`
  ([triggers](triggers.md#technologies)). [code, data]

## Scenario restrictions

- A map can bar sides from technologies, units and buildings with
  restriction groups (`RESTRICTION_GROUPS`, made in the editor). Each
  team and each side can name one group. [code, data]
- A group holds four lists. Only the third and fourth, which the map
  format names `required_actors` and `required_technologies`, matter in
  a match: they're what the group bars (the editor shows them as
  "Selected Restrictions"). The first two (`allowed_actors`,
  `allowed_technologies`) are only the editor's other column, everything
  not selected, and play no part. Nothing in a group is given to a side,
  at the start or later. [code, data]
- A side is barred from a technology, unit or building when it's on the
  barring list of its team's group or of its own group. An empty list, a
  group with nothing selected, or no group bars nothing. [code]
- A barred technology:
  - is hidden in the research menu (no slot; the items after it move up);
  - is never gained, however it would come: research, the scenario's start
    technologies, the `kingdom_gains_technology` trigger, a building's
    `[TechnologyProperty]`. The grant just doesn't happen. [code]
- A barred unit or truck is hidden in its Deploy menu, keeping its slot
  empty, as an entry hidden for missing properties does (see
  [commands](commands.md#deploy-menus)), and can't be ordered. A barred
  building can't be built by a unit that builds things. [code]
- Unpacking doesn't look at restrictions: a truck the side already has
  (placed by the map, given by a trigger, or bought before) unpacks into
  its building even when that building is barred, and packing back up
  isn't checked either. Only ordering the truck is barred. [code]
- Shipped maps use them. 15 of the maps carry groups, mostly campaign
  missions and the tutorials (the tutorials bar the advanced units and
  technologies; the Saint-Lô and Normandy missions, and the
  `(3v3)Saint_lo` skirmish map, bar the King Tiger and its technology for
  one side, and on two of the three Napalm for the other), plus two
  developer maps
  (`DeveloperMaps/(1v1)BorealForest`, `(4FFA)TropicalCruise`) whose group
  bars a long list of other nations' units and technologies. [data]

## Gating recruits

- A recruit is gated on its members: `required_properties` and
  `required_structures` sit on each member element's `[ElementComponent]`
  and are checked member by member. A regiment template's own
  requirements play no part. [code]
- A property requirement is met by the recruiter's own properties or, for
  a technology, by the side's. Hidden or greyed follows the member's
  `hide_if_missing_properties` (default true: hidden). A missing building
  only greys. The details are under
  [recruiting](recruiting.md#offered-greyed-or-hidden). [code]
- A greyed recruit's tooltip carries a "Requires" section listing what's
  missing: technologies by their names, buildings by theirs. It lists only
  what's missing. [code]
- Commissioning at the Corps HQ is the same recruiting, with the same
  rules: the trucks are its recruits. For example a Mechanized Division HQ
  truck needs `mechhqproduction` (which the Corps HQ carries itself) and
  an Infantry Division HQ (`required_structures`). So it's greyed until
  one stands. [code, data, game]
- No shipped Corps HQ research unlocks a truck. Corps HQ technologies
  change units and the economy instead. [data]
