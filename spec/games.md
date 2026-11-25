# Games

Axis & Allies and Kohan II: Kings of War run on one engine. What the engine
fixes for every game, what each game's data decides, and where the two
games' rules differ. Kohan II here is its retail 1.2.3 release.

Games: compares both.

## One engine, two data sets

- Both games read the same file formats, the same kinds of definition and
  the same kinds of component, and nearly all of their rules are the same.
  What makes each game is its data. [code, data]
- Of the top-level definition kinds in the shipped `.tgi` files (`[Name
  ...]` blocks outside any other), AA uses 276 and Kohan II 251, with 241
  in both, interface layouts included. Nested section kinds: 1201 and
  1087, with 1036 in both. [data]
- AA's own part is the campaign map (the dynamic campaign: its map,
  territories, turns, divisions, purchasing and the opponent that plays
  it), with its own definition kinds (`DynamicCampaign`, `MetaGame*`,
  `MG*`) and its own keyword lists. [code, data]
- Kohan II's own parts in data are settlement layouts and templates, wall
  sets, heroes, spells and lairs. These are kinds the engine supports in
  both games: AA simply doesn't use them, and the shipped AA data even
  keeps a stray lair and settlement section. [code, data]
- Kohan II 1.2.3 was patched after AA was built, so it has a few later
  additions AA lacks (Rule differences, below). [code]

## Words for the same things

The specs call things by the names players know, and the two games name
some of the same engine pieces differently. A rule written with one
game's word applies to the other's. [code, data]

| Specs say | AA | Kohan II | In the data |
|-----------|----|----------|-------------|
| side | player | kingdom | a map's kingdoms, each of a `[Nation]` |
| regiment | regiment | company | a thing with an `OrganizationComponent`, members in its layout's slots |
| unit | unit | unit | a thing with an `ElementComponent` (`[Thing]` in AA, `[Unit]` in Kohan II) |
| leader | the regiment's leading unit | the captain | the layout's `PrimarySlot` |
| faction | the general a side picks | the side's faction (Council, Royalist and so on) | `[Faction]` |
| purchase resource | `money` | `gold` | `[Resource]` (economy.md) |
| upkeep resources | `ammo`, `oil` | `stone`, `wood`, `iron`, `mana` | `[Resource]` |
| recruiter | HQs and other recruiting buildings, battleships | the settlement | `RecruitComponent` (recruiting.md) |
| attaching | a regiment to a Division HQ ([supply](supply.toml)) | a hero to a company ([companies](companies.md#heroes)) | different rules: only the word is shared |

Things only one game's data uses have no counterpart: AA's cities,
trucks and special operations, Kohan II's settlements, denizens, walls,
heroes and spells.

## What the data defines

- Each top-level definition adds an entry under its `IDS`. The game has no
  built-in list of nations, factions, resources, damage types, target
  types, terrains, biomes, formations, ranks, difficulty levels, handicaps,
  game types, statistics, kingdom colours or the like: they're whatever the
  data defines. Data refers to them by `IDS`. [code, data]
- Naming an `IDS` that no loaded definition of the expected kind has is a
  load error, handled like every other load error (data.md). [code]
- Some kinds have more than one name. `[Unit ...]`, which Kohan II writes
  for its soldiers and heroes where AA writes `[Thing ...]`, is the same
  kind as `[Thing ...]`, and so is `[Projectile ...]`: one set of ids,
  the same fields, the same templates. Both games accept all three names.
  [code, data]
- `[Ego ...]` definitions belong to the computer player: each is one of
  its plans (a priority and what to build and recruit), used by the
  per-nation `SAI/*.tgi` files. Nothing outside the computer player reads
  them; [match start](match-start.md) and [outcome](outcome.md) don't
  need them. [code, data]
- Placeholders and random things, which turn a map's generic things into
  each side's own at match start, are used by both games: see [match
  start](match-start.md). [data]
- Resources are an example. The kinds of resource, and each one's role,
  come from `[Resource]` definitions: a purchase resource (AA's `money`,
  Kohan II's `gold`), upkeep resources with their `[PurchaseCost]` in
  purchase resources (AA: `ammo`, `oil`; Kohan II: `stone`, `wood`,
  `iron`, `mana`), and limits (`unit_limit_provided`, and
  `unit_limit_consumed` with `limited_by`). A thing's cost and upkeep name
  resources by `IDS`. The three roles are a fixed list (below); the
  resources aren't. [code, data]
- Components: a thing is whichever components its definition has. Both
  games have every kind of component, AA's (cities, HQs, defenders,
  mobile deployment, transports, aircraft) and Kohan II's (walls, portals,
  lairs, settlements) alike; a game's data uses the ones it wants. [code,
  data] Whether each works as well in the other game hasn't been tried.
  [guess]
- Game-wide switches and constants come from data: `SVars` and `AVars`,
  each a file shared by both games plus a per-game file that overrides
  it (`SVars_AA.tgi`, `SVars_k2.tgi`, `AVars_AA.tgi`, `AVars_K2.tgi`).
  [data]
  - Both games' code reads every one of them, except the few under Rule
    differences. The same rules then run with each game's numbers and
    switches: AA's data turns on unit supply (`UnitSupplyEnabled`); Kohan
    II's turns off air attacks (`UseAirAttacks = false`) and sets its own
    morale, retreat and routing values (`MoraleEngagementLoss`,
    `RoutingTime` and the rest). [code, data]
  - Interface choices are switches too: AA sets `UIUseSidePanel = false`
    and `UIShowFactionExperience = true`; Kohan II sets
    `UIUseDualColumnWorldRules`, `UIShouldDisplayRMCHerosInMapPreview`
    and the like. [code, data]
- A field that a definition kind doesn't read is ignored: AA's shipped data
  carries many keys the game never reads, and it loads cleanly.
  [data, guess]

## What the game fixes

- Wherever data picks a value from a keyword list (`attribute = SPEED_MAX`,
  an animation's `type`, a section such as `[WhenStart]`), the list is fixed
  by the game, not the data. A keyword outside the list is a load error.
  [code]
- The fixed lists are the same in both games, apart from AA's campaign
  lists and the additions under Rule differences. They include: [code]
  - component kinds (31), unit commands (39), team commands (10);
  - trigger conditions and effects (115), their comparisons (`less_than`
    to `greater_than`) and value kinds;
  - modifier kinds (`FLAG`, `ADD`, `MULTIPLY` and the rest) and the 25
    attributes a modifier can change (`AV`, `AV_MELEE`, `AV_RANGED`, `DV`,
    `SPEED_MAX`, `MORALE`, `HEALTH`, `RECHARGE_TIME` and so on);
  - ability flags (`VORPAL`, `SPELL`, `ENCHANTMENT`, `BOMBARD`,
    `SUMMON_UNIQUE` ...), ability effects (`SUMMON`, `HEAL`,
    `MAIN_DAMAGE` ...) and status effects (`PARALYZE`, `ENTANGLE`,
    `BERSERK` ...);
  - game events (62), message kinds, statistics on the score screen (62);
  - the points where effect commands fire (`WhenCreated`, `WhenDestroyed`,
    `WhenSelected`, `WhenCommanded`, `WhenCommandedAggressive`,
    `WhenConstructed`; `WhenStart`, `WhenDo`, `WhenEnd`, `WhenDoPerTarget`,
    `WhenDoPerSummoned`; `WhenAdded`, `WhenRemoved`, `WhenResearched`) and
    the effect commands themselves (`CREATE_VFX`, `PLAY_SFX` ...);
  - animation kinds (`CONST`, `RANDOM`, `IDLE`, `FIDGET`, `OPEN_GATE`,
    `CLOSE_GATE`, `CUSTOMPLAY`, `SELECTED`), resource roles (`STOCKPILED`,
    `UPKEEP`, `LIMITED_CONSUMED`), stances, relations (`SELF`, `ALLY`,
    `NEUTRAL`, `ENEMY`), terrain kinds (`LAND`, `MOUNTAIN`, `WATER`) and
    the computer player's goal kinds (23).
- Some of these lists also have a definition per entry in data, which
  describes it: unit commands (`[ActorCommand]`, 39 in each game, one per
  command), message kinds (`[MessageType]`), game events (`[GameEvent]`)
  and team commands (`[TeamCommand]`). The game knows the entries by name;
  the data gives each its text, icon, sound and the like. [code, data]
  The four team commands beyond AA's ten (`team_attack_this` and the
  like) are commented out of the data and unknown to the game ([team
  commands](team-commands.md)). [code, data]
- Data names a keyword, never its position in the list, so the two games
  numbering a list differently (Kohan II's message kinds put two more
  before `KINGDOM_DEFEATED`) makes no difference to the data. [code]

## Rule differences

A difference in one subject's rules is stated in that subject's spec,
marked as the game it belongs to. Here are only the differences that
span the engine, and where the others are.

- AA adds the campaign map and everything under it, which Kohan II doesn't
  have. Its message kinds (`METAGAME_EVENT` and the like) are in both games'
  lists. [code]
- Kohan II 1.2.3 adds two entries to the fixed lists: the message kinds
  `TECH_POSITIVE` and `TECH_NEGATIVE` ([messages](messages.md)) and the
  ability flag `SUMMON_REQUIRED`, which no shipped data uses. [code, data]
- Game constants only one game's data sets: Kohan II's
  `TargetedStateTimer` ([combat](combat.md), Engaged and under siege) and
  AA's `SortObjectives` ([objectives](objectives.md)). [code, data]
- Apart from these, the two games' shared rules haven't been compared in
  detail. Where they differ, it's mostly in the unit and company AI
  (engaging, targeting, retreating), in how a company's members are
  managed, and in parts of cities, transports, defenders and mobile
  deployment. [code, guess]

## Open questions

- What differs between the two games inside the rules they share,
  starting with the unit and company AI. (analysis, 2026-11-15)
- Whether a field a definition kind doesn't read is ignored in every
  case, or some kinds reject unknown fields. (analysis, 2026-11-15)
- Where Kohan II's `AllowSettlements` switch takes effect: neither
  game's code names it directly. (analysis, 2026-11-15)
