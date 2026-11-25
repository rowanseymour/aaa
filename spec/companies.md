# Companies

What a Kohan II company is made of, how a new one fills its positions,
and the heroes who lead companies. Recruiting them, at a settlement from
a design the player puts together, is in [recruiting](recruiting.md).
Denizen companies (militia and workers) aren't recruited: see
[settlements](settlements.md#denizens). AA's regiments are the same
kind of thing ([games](games.md#words-for-the-same-things)).

Games: traced in Kohan II. AA's data has no heroes.

## Make-up

- A company is an organization: a thing whose layout (`layout_IDS`, in
  `Organizations/Layouts/company_*.tgi`) lists its `[Slot]`s, each slot
  one or more positions for units. [code, data]
  - A position takes a unit that has the slot's `property_IDS` (`front`,
    `flank`, `support`, `captain`). `number` repeats a slot (the
    standard front is 4 positions); `required = false` lets it stay
    empty; `locked = true` fixes it to the company template's own unit.
  - The `PrimarySlot` is the captain's: the company's leader.
  - The template's `[OrganizationComponent]` `[Slot]`s give the unit for
    each slot in layout order (`element_IDS`, `null` for empty).
- The layout's `[Formation]`s place each position around the company
  (`offset`; the captain is at 0,0 in every layout). [data]

## A new company's positions

- A recruited company is made with only its captain (the layout's
  primary position) and any hero named in its design (Heroes, below).
  [code]
- Every other position starts empty and is filled by resupply, one unit
  at a time, as a regiment's lost member is ([supply](supply.toml)): each
  grows at its `resupply_rate` times its maximum health per second, 0.1
  for most Kohan II units (0.05 for undead, less for some large ones), so
  about 10 seconds a unit while the company is in supply. [code, data]
- A template could ask for all of them at once with `recruit_full`;
  none does. [code, data]

## Heroes

- A side's heroes are a pool it owns. A hero in the pool isn't a thing
  on the map; it appears only when it joins a company. [code, data]
- A hero is a unit on the `Hero` template (`K2Hero` and its faction
  variants). Every shipped hero has the `captain` property, so it fits a
  company's captain position. Heroes have no `[Cost]`. A side never holds
  the same hero twice. [code, data]
- Ways a side gets heroes: [code, data]
  - the heroes the map file lists for its kingdom (Heroes in the map,
    below);
  - the `kingdom_gains_hero` trigger effect, naming a hero or asking for
    a random one (below); `kingdom_loses_hero` takes one away;
  - on a random map only, the settings screen's hero count (Random map
    heroes, below).

### Heroes in the map

- A map file lists each kingdom's pool in its KINGDOMS section: the
  kingdom record's recruits list (the format doc's
  `num_recruits`, which the shipped multiplayer maps leave empty and
  campaign maps fill). Each entry is: [code, data]
  - a 4-byte kind: 0 for a hero (kind 1, another sort of entry, is read
    but its meaning is untraced);
  - a 4-byte version (0), the hero's id as a 2-byte length then that many
    UTF-16 characters (e.g. `naava_daishan`), and a 1-byte available
    flag;
  - for a hero, a 4-byte version (1 or 2), the id of the hero's thing on
    the map (0 when it's in the pool), 4 more bytes (0 in shipped maps),
    its experience (4-byte float) and its health fraction (4-byte float,
    1.0 for full); version 2 adds 4 more bytes.
  After the list comes the 4-byte index of the hero being healed (-1 for
  none; When a hero dies, below).
- A map's kingdom heroes are in the pool from the start: the map editor's
  "Available Heroes" and "Kingdom Heroes" lists edit them. [code, data]

### Heroes on a fixed multiplayer map

- A fixed map (one not made by the random map generator, such as
  `(1v1)Battleground`) never uses the settings screen's hero count: only
  random maps do. Its heroes come from the map file and its triggers.
  [code]
- No shipped multiplayer map lists heroes in its kingdoms. Instead their
  "Map Init" trigger, run as the match starts, has `kingdom_gains_hero`
  with a random hero for each player kingdom: twice each on
  `(1v1)Battleground` and `(2v2)VolcanoIsle`, three times each on the
  3v3 and 4v4 maps, never on the 8-player and `Portals` maps. So each
  side on `(1v1)Battleground` starts with two random heroes in its pool.
  [data]
- A random hero from that effect is drawn from every hero whose
  `random_hero` is set (an `ElementComponent` field, true by default),
  that fits the side's faction (heroes of no faction fit every side) and
  that the side doesn't already hold; if there's none, the side gets
  nothing. [code, guess]
- A named hero from that effect is added unless the side already holds
  it. [code]

### Random map heroes

- A random map's settings carry one hero count, chosen on the settings
  screen ("Heroes") between the map setting's `[Heroes]`
  `random_heroes_min` and `random_heroes_max` (0 and 9 in `K2RMC`). The
  same count applies to every side. [code, data]
- Each side first gets the heroes its kingdom in the map setting names
  in a `[Hero]` list, if any; no shipped map setting has one. [code, data]
- Then it's given that many heroes drawn at random without repeats from
  every hero whose `random_hero` is set, keeping only heroes of the
  side's faction and heroes of none. A hero the side already has is
  skipped and another drawn; if the candidates run out, it gets fewer.
  Shipped heroes with `random_hero = false` are the campaign and cutscene
  ones and a few unique ones. [code, data]
- They go into the pool, not onto the map. [code]

### The heroes menu

- The Heroes menu ("Heroes", "Hide/unhide the heroes menu.", the
  `RecruitItemFloater` in `UI/Game/floaters.tgi`) is a scrolling column
  with a button for every hero in the side's pool. Each shows the hero's
  icon (its `[View] icon`, e.g. `AndyrWayth/AndyrWaythIcon.tga`) and a
  health bar of its health fraction, with the "detached" border when it
  isn't leading a company. [code, data]
- A hero's tooltip gives its name (its template's `name`) and a Status
  line (`ui_tooltip_item_title_recruit_item_status`, "Status"): the name
  of the company it leads (`ui_tooltip_recruit_item_attached_to|s`) or
  "Detached" (`ui_tooltip_recruit_item_detached`). [code, data]
- The hero lists elsewhere name a hero as "%s" (`hero_name_available|s`)
  when it's available and "%s (%s)" (`hero_name_unavailable|ss`) with the
  company it's in when not. [code, data]
- Attach Hero needs a hero chosen first; without one the player is told
  "Must select a hero to attach" (`ui_error_missing_single_recruit_item`).
  [data, guess]
- In the company editor, the side's available heroes are listed as
  candidates for the captain position alongside its captain units
  ([recruiting](recruiting.md#company-designs-kohan-ii)); heroes that
  aren't available aren't listed. [code]

### Joining and leaving a company

- A company's Attach Hero command ("Prompts you to attach a Hero from the
  Hero Pool to the company. You may only attach while the company is in
  supply.") puts a hero from the pool into its captain position; Detach
  Hero ("Detaches the current Hero from the company, returning it to the
  Hero Pool.") takes it out. [data]
- Both are refused, with a message naming the company, when, checked in
  this order: [code, data]
  - the side holds no heroes at all (refused silently) [code, guess];
  - the company is in the `out_of_supply` state (`Game/actor_states.tgi`;
    "%s cannot currently attach or detach heroes"). That's the only state
    that stops it: fighting, moving or routing don't;
  - the company isn't in supply ("%s out of supply");
  - it has fewer than two members ("%s needs at least two units").
  Attaching also needs the hero to be alive ("%s is not alive").
- Attaching: the hero appears in the company at once, in the captain's
  place in its formation, and the captain it replaces leaves the map. A
  hero already leading another company leaves that one first (which must
  meet the same conditions), so a hero can be moved straight from one
  company to another. [code]
- Detaching: the hero leaves the map without dying and is back in the
  pool at once. The captain position goes back to the company template's
  own captain, which comes back as a lost member does. [code, guess]
- A design can name a hero for its captain
  ([recruiting](recruiting.md#company-designs-kohan-ii)): recruiting it
  brings that hero at once, as the company's captain. [code]
- Such a design can be recruited only while its hero is available: in
  the pool, alive and not leading a company. Until then it isn't offered
  at all (its favourite, Load Last Company and the editor's Recruit button
  do nothing), so a company is never made from it with a plain captain or
  none. [code]
- A hero keeps its experience and its health fraction while in the
  pool. [code]

### When a hero dies

- Its side is told `hero_killed` ("Hero killed"). The hero stays in the
  pool, unavailable, with no health. [code, data]
- The pool restores one hero at a time: its health fraction grows by its
  `resupply_rate` (0.1 on `K2Hero`) per second until full, and it's then
  available again. So a killed hero is back after about 10 seconds; one
  detached wounded is available at once and heals in the pool the same
  way. [code, guess]

## Open questions

- The random map settings screen's default hero count. (analysis,
  2026-11-15)
- What clicking a hero in the Heroes menu does (select its company, or
  start Attach Hero with it), and what "Awaken Hero?"
  (`activate_recruit_item`) asks. (analysis, 2026-11-16)
- Whether a hero stays with its company when the company changes side.
  (analysis, 2026-11-15)
