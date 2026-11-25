# Match start

What happens to the map's things between loading a map and the first
tick of play: placeholders and random things become real things, each side
gets its start location, money and camera, which world rules apply, and
what each game's sides have once play begins. Money is in [economy](economy.md#starting-money);
what keeps a side in the match afterwards is in [outcome](outcome.md).

Games: traced in both.

## Order of events

- When a new match starts (single-player or multiplayer, a map or a random
  map), in this order: each side is given its starting money and its
  starting properties (faction, handicap); then the map's placeholders and
  random things are replaced; then the fog setting is applied; then each
  player's camera is placed; then AA's Time of Day is applied
  ([lighting](lighting.md#time-of-day)); then the match is running. All of it happens
  before game time 1.0 s, so before the default defeat rule is first
  tested. [code]
- Loading a saved game doesn't do this again: the saved things are
  already the replacements. [code, guess]
- The map editor loads a map without starting a match, so in the editor
  placeholders and random things stay as placed, and the map is saved
  with them. [code, guess]

## What the player sees as play begins

- From launch until play begins the loading screen fills the window: its
  splash art, the map's name and description and a status line. The map
  isn't drawn behind it or anywhere else. Game time stands still at 0
  throughout ([multiplayer](multiplayer.md#loading)): everything in Order
  of events above is done, but no tick has played and no trigger has been
  tested. [code, data]
- In single-player, play begins only when the player presses Continue,
  however long ago loading finished. Until then nothing plays: the first
  tick comes after the press, never before. [code]
- In multiplayer there is no Continue. Each machine shows "Waiting on
  Other Players" once it has finished, still with no map in sight, and
  play begins on every machine at the same step once all have finished.
  [code]
- When play begins the loading screen goes and the match's first tick
  plays almost at once (within a fraction of a 1/16 s step), running the
  first test of the triggers ([triggers](triggers.md#when-triggers-are-tested)).
  [code]
- The player never sees the map as it stood before that tick. After
  Continue the screen stays black for about a second, and when the map
  first appears the start-up triggers have already taken effect:
  `(2v2)Berlin`, whose only trigger turns the sun to `sun_night` at once,
  appears already at night, with no frame of its own daylight. [game,
  data]
- What keeps the screen black for that second hasn't been traced, nor
  whether multiplayer shows the same black second (see Open questions).

## Placeholders

- A placeholder is a thing whose definition has category `PLACEHOLDER`
  (in the data, `[Thing Template=Placeholder]`, e.g.
  `placeholder_settlementvillage`, or AA's `placeholder_corpsunpacked`).
  It stands for "this side's version of X", so one map serves every
  nation. Both games use them: AA's skirmish maps place each side's Corps
  HQ as `placeholder_corpsunpacked`, Kohan II's place each side's first
  settlement as `placeholder_settlementvillage`. [code, data]
- At match start every placeholder owned by a side that isn't
  independent is replaced by the thing its owner's nation names for it:
  the `[Placeholder]` entry in the nation's definition whose
  `placeholder_IDS` is that placeholder, giving `thing_IDS` (Kohan II's
  `human` nation: `placeholder_settlementvillage` becomes
  `human_center_village`). [code, data]
- Placeholders that are unowned or owned by an independent side are left
  as they are: they stay on the map as the placeholder thing. [code]
- A nation with no entry for the placeholder, or whose entry is `null`,
  leaves it as it is too (an error in the log, not a load error at that
  point). [code, data]
- The replacement keeps: [code]
  - the placeholder's owner;
  - its position and facing;
  - its thing id, so triggers and objectives naming the placeholder's id
    find the replacement;
  - properties given to the placeholder itself (in the map); properties
    it has from anywhere else are dropped, with a note in the log.
  Nothing else carries over: not the placeholder's name, health or
  state. The replacement is made as the editor would place it: complete,
  not under construction, with its own name from its name list.
  [code, guess]
- The placeholder is removed first, then its replacement created in its
  place. If the replacement can't be created there (blocked, or a rule
  below refuses it), the side gets nothing in its place and an error line
  is posted to the message log. [code]
- A replacement that is a settlement centre brings its settlement with it
  (see [settlements](settlements.md)). A replacement that needs a
  `required_object_ids` thing beneath it (Kohan II's centres need a
  `marker_settlement`) gets one created for it first, at the same spot.
  [code, data]
- When data loads, every placeholder is checked against every nation:
  [code]
  - a nation without an entry for some placeholder is a load error while
    `DataCheckingStrict` is on (it is in both games' SVars);
  - each replacement's blocking shape must contain the placeholder's (a
    load error otherwise), and some of the placeholder's flags are made
    to match its replacement's, with an error in the log where they
    differed.

## Random things

- A thing with category `RANDOM_OBJECT` or `RANDOM_ACTOR` (e.g.
  `random_settlement_camps`, the random lairs) is replaced at match start
  by one thing drawn from its list, whoever owns it, independent sides and
  unowned things included. [code, data]
- A random thing's list is made of the definitions that name it in a
  `[RandomActor]` block (`random_actor_ids`, with a `weight`):
  `bandit_settlementcamp` and three other camps each name
  `random_settlement_camps`. The draw is weighted by `weight` and uses the
  match's random seed, so every machine draws the same. [code, data]
- The replacement keeps the same things a placeholder's does (owner,
  position, facing, thing id, properties given to it). [code]
- An empty list is a load error. [code]

## Start locations

- `start_loc` (category `START_LOC`) marks where a side starts. It's
  invisible and can't be targeted. A side has at most one: giving a side
  a second removes the first. [code, data]
- When a map is prepared for play, a side with no start location gets one,
  placed at its settlement if it has one, otherwise at whichever of its
  things has the highest `starting_location_priority` (AA's
  `placeholder_corpsunpacked` has 1000), otherwise at the map's centre.
  [code, data]
- At match start each player's camera looks at their own side's start
  location; an observer's looks at the centre of the map. A side with no
  start location leaves the camera where it was. [code]
- What a start location provides is up to its data: AA's gives the side's
  building limit (`building_limit_provided = 20`, see
  [economy](economy.md)); Kohan II's has no production and only marks
  the spot. [data]

## What each side has at the start

- Nothing puts regiments, companies or heroes on the map for a side when the
  match starts. A side begins with exactly what the map gives it once
  placeholders and random things are replaced, plus its starting money.
  [code] Kohan II's sides also start with heroes in their hero pool,
  which aren't on the map: see [companies](companies.md#heroes).
- Kohan II's `(1v1)Battleground` gives each side a `start_loc` and a
  `placeholder_settlementvillage`. So each side
  starts with its nation's village centre, the settlement that comes with
  it, and the centre's denizens inside it (workers and militia, see
  [settlements](settlements.md)), and no companies on the map. [code,
  data]
- Its independent sides start with what their random lairs and camps
  become. None of those keeps a side alive, so (as in AA) they're defeated
  at the first test, and their lairs and camps carry on regardless (see
  [outcome](outcome.md)). [code, data]
- Starting money works the same way in both games: each stockpiled
  resource starts at `ResourceStockpileDefault` (the Money world rule as
  chosen for the match, see [World rules](#world-rules); its default is
  500 in both games, at most 9999 in Kohan II's world rules)
  unless the map stores the side's own stockpile. In Kohan II that's
  `gold`; its upkeep resources (`stone`, `wood`, `iron`, `mana`) aren't
  stockpiled. [code, data]

## World rules

- A match is played with one set of world rules: those in
  `Game/world_rules*.tgi`, among them AA's City Control %, Difficulty,
  Fog of War, Money, Starting Experience and Time of Day. Every map
  stores a full set of its own, each rule with its value and whether
  the map fixes it. [code, data]
- When a match starts, the rules chosen for it replace the map's stored
  set entirely. The map's stored values matter only as the Scenario Map
  dialog's starting point (below). In multiplayer the host's choice goes
  to every player. [code]
- AA's shipped scenarios all store City Control % 0, Difficulty Normal,
  Fog of War Explored, Starting Experience 0, Time of Day Default and
  Money 500, except `(2v2)Moscow`, which stores Money 1000. None fixes a
  rule. [data]

### The Scenario Map dialog (AA)

- Picking a map in the list fills the dialog's rules with the values
  that map stores. This happens every time a map is picked, including
  picking another and then coming back, and replaces whatever was shown
  or set before. So Moscow shows Money 1000 and every other shipped
  scenario 500; Starting Experience shows 0 for all of them, not the
  rule's default of 200. [code, data]
- A rule the map fixes isn't offered. [code]
- The rules shown can be changed. Load takes them as shown, and the
  match is played with them: Moscow left alone starts every side that
  uses the default stockpile at 1000; with Money changed to 800 they
  start at 800 ([economy](economy.md#starting-money)). [code]
- The map's description pane lists the stored rules whose values differ
  from the rules' defaults, such as "Starting Experience 0" (and Money
  1000 for Moscow). It shows the map's stored values, not the edited
  ones. [code, game]

### The Random Map dialog (AA)

- Choosing a type (Alaska, France and so on) fills the dialog's rules
  from that random map setup. A setup starts from every rule's default
  and then applies its own `[Rules]` list (`Templates/template_rmc_aa.tgi`
  for every shipped type). That list only sets Difficulty to Normal
  (`MEDIUM`) with `show_ui = false`, which also fixes it, so Difficulty
  isn't offered. The others start at their defaults: City Control % 0,
  Fog of War Explored, Money 500, Starting Experience 200, Time of Day
  Default. [code, data]
- Choosing another type fills the rules again from its setup, so any
  changes are lost. All shipped types have the same rules, so that
  amounts to going back to the defaults. [code, data]
- As with a scenario, the match is played with the rules as shown when
  the dialog is confirmed. [code]

## Open questions

- The Scenario Map dialog screenshot (`menu-scenario-map.jpg`, Fall of
  Berlin picked) shows Fog of War None and Starting Experience 200, not
  the Explored and 0 that Berlin stores, while its description pane
  shows "Starting Experience 0". The Random Map screenshot also shows
  Fog of War None. Either the player had changed those two rules before
  the screenshots were taken, or the dialogs keep a choice that wasn't
  found. Worth checking in the game: open Scenario Map, pick Moscow,
  then another map, then Moscow again, and see what Money, Fog of War
  and Starting Experience show each time. (analysis, 2026-11-20)

- What the screen shows in the black second between Continue and the
  map first appearing, what decides how long it lasts, and whether a
  multiplayer match shows the same. (analysis, 2026-11-22)

- How `starting_location_priority` ranks a side's loose units against the
  leaders of its regiments when placing a missing start location.
  (analysis, 2026-11-15)
