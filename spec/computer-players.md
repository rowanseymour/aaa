# Computer players

How a computer player plays a match: the profile and moods that steer
it, how often it thinks, the goals it weighs and how it picks among
them, and what each kind of goal makes it do. Who runs a computer player,
and on which machine, is in
[multiplayer](multiplayer.md#computer-players); how its profile is
chosen is in [multiplayer](multiplayer.md#choosing-a-profile-at-launch);
how it takes team commands and gives to allies is in
[team commands](team-commands.md).

Games: traced in AA's engine and data. Kohan II's computer player is the
same apart from its data; its differences are noted where they're known
([Kohan II](#kohan-ii)), and its goals weren't traced one by one.

Most of what a computer player does is set by numbers in its AI data:
the engine-wide template `Templates/template_sai.tgi` (whose comments
describe each field), AA's moods in `Templates/template_sai_aa*.tgi` and
the profiles in `SAI/`. The rules below say how those numbers are used,
and where the game does something other than the template's comments
say. A field this spec doesn't mention is used as its comment says
[guess], unless it's listed under [Unused data](#unused-data).

## Who has one

- Every major side that no person plays gets one computer player, and no
  side gets two. Independent sides never get one: they have only their
  things' own behaviour. A side a person plays never has a computer
  player helping it. [code]
- A computer player that takes over a side mid-match, when its player
  leaves or is dropped, is an ordinary one: a profile as at launch, a
  fresh plan, all the side's things to command. It doesn't carry on what
  the earlier player was doing; what belongs to the side (its things,
  queues, flares) stays. Which sides get one, its name and profile:
  Taking over a side in [multiplayer](multiplayer.md). [code]
- It thinks only on the machine it belongs to, and its decisions go out
  as ordinary orders for its side, through the same checks as a
  person's (owner, the order offered, cost). [code]

## Profiles and moods

### The profile

- A profile (`[SAI template = SAI]` in `SAI/`) names its moods:
  `initial_ego` and a list of `[Ego]`. In AA every profile has three, a
  low-income, a main-game and an end-game mood. [code, data]
- AA's profiles come in two kinds: [data]
  - one per general (`rommel_sai` in `Germany_Rommel_SAI.tgi`), `public`,
    built from one of eight styles in `Templates/template_sai_aa_*.tgi`:
    armour, artillery, balanced, infantry, infantry and planes,
    mechanised, planes, and (for the campaign profiles below) campaign.
    Rommel, Kesselring, Wavell, Zhukov and Patton play armour; Manstein,
    Nagumo and Arnold planes; Rundstedt and Chuikov artillery;
    Montgomery balanced; Mountbatten, Yamamoto, Konev and Nimitz
    infantry and planes; Wingate, Mikawa and Rokossovski infantry;
    Kuribayashi and Eisenhower mechanised.
  - one per nation (`ger_sai` in `germany_sai.tgi`), `public = false`, on
    the campaign moods. These are what the campaign maps name for their
    computer sides (a side's `ai` in the map setup) and what the World
    War II campaign map names per nation. Two missions name `example_sai`,
    which doesn't exist, so those sides get a random profile as when none
    is named ([multiplayer](multiplayer.md#choosing-a-profile-at-launch)).
    [data, code]
- The styles differ in their numbers, not in kind: chiefly what they like
  to recruit and build (`[ActorPriority]`), what they research, how much
  strength they want before attacking, and how much they explore and
  attack from the air. [data]
- The profile's own fields: `difficulty_level` and `public` only matter
  when a profile is chosen; `[Nation]` and `[Faction]` likewise (nothing
  reads them during a match); `ego_fuzziness` (below); and how its orders
  go out (`command_max_delay`, `command_max_total_delay`,
  `max_ge_commands`, `max_rai_commands`, in [Orders](#orders)). [code]

### Moods

- A mood (`[Ego]`) holds everything else: how often to think, every
  goal's weights, what to recruit, build and research, how to fight,
  explore, use special operations and obey flares. [code, data]
- The computer player starts in its profile's first mood, the
  `initial_ego`. [code; guess: that the initial mood is always first]
- At the start of each planning think it checks its current mood's
  `[Filters]`: every item's `stat` must lie between its `min` and `max`
  (inclusive). While they hold it keeps the mood. Only when they fail
  does it choose again, among the profile's moods whose filters hold:
  each scores its `priority` + a random fraction of the profile's
  `ego_fuzziness`, and the highest wins. If no mood's filters hold it
  takes the first mood, and posts once the line "No ego matches game
  state for SAI profile (%s)." [code]
- A filter's `stat` is one of the scoring statistics
  (`Game/Scoring.tgi`). AA's moods all use `money_rate`, the side's net
  money per minute, the same figure the resource bar shows. [code, data]
- AA's three moods all have priority 2000, with overlapping bands: low
  income up to 40, main game 30 to 120, end game from 100. Because the
  current mood is kept while it fits, the overlaps act as hysteresis: it
  moves from low income to main game only once income passes 40, on to
  end game past 120, back to main game below 100 and back to low income
  below 30. Where two moods fit when it must choose (income 35 on leaving
  the end game, say), it's an even chance. [code, data]
- On a change of mood its exploring and special-operation plans are
  remade from the new mood's numbers; every other goal simply uses the
  new mood's numbers from then on. [code]

## When it thinks

- It thinks on two schedules, both in game time, so pausing stops it and
  game speed scales it: [code, data]
  - planning (the mood's `[GoalEngine]` `[GeneralSettings]`
    `think_frequency`, 30 seconds in AA): everything below except what
    [Reacting](#reacting) does;
  - reacting (`[ReactiveAI]` `think_frequency`, 2 seconds): retreats,
    picking targets near a fight, formations.
- A new computer player plans and reacts straight away. After that its
  thinks are spread out so that computer players don't think together:
  the second think of the i-th of n computer players comes i ÷ (n + 1)
  of a period early, and from then on every period. And no two planning
  thinks of the computer players one machine runs come closer than
  `GoalEngineThinkFrequencyMinOffset` (SVars, 2 seconds), nor two
  reacting thinks closer than `RAIThinkFrequencyMinOffset` (0.2): a
  think due sooner waits. The spacing is per machine: computer players
  thinking on different machines of a multiplayer match don't hold
  each other back. [code, data]
- Early in a match it also plans at once, out of turn, when one of its
  buildings is finished and when its units first see some ground, as
  long as it has at most `immediate_think_on_build_max_settlements` (2)
  settlements and the game time is under
  `immediate_think_on_build_max_time` (600 seconds). (The template says
  "fewer than"; the game allows equal.) [code; guess: whether a finished
  builder is a third such event, as the template says]
- Its picture of strength across the map ([Regions and
  strength](#regions-and-strength)) is refreshed every
  `dynamic_map_analysis_think_frequency` (SVars, 10 seconds), for all
  computer players at once. [code, data]
- It doesn't think while the match is frozen or a cutscene plays, before
  the map has been analysed for it (a moment after play starts), while
  its side has computer control switched off ([Switched
  off](#switched-off-by-a-trigger)), and only in matches and the
  editor's test play. [code]
- In AA a defeated computer player keeps thinking (`DisableSAIWhenGameLost`
  false in `SVars_AA.tgi`); with it true (Kohan II) it stops once its
  side has lost or has nothing left. [code, data]
- Thinking can take several frames of real time; that changes only when
  its orders go out, not what it decides, so an implementation can think
  in one go. [code]

## Orders

- A think's orders go out spread over real time, not game time: the k-th
  of n leaves `reaction_time` (the mood's, for planning or reacting; 0
  in all stock data) + k × the smaller of `command_max_delay` (0.05
  seconds) and `command_max_total_delay` (5 seconds) ÷ n after the
  think. So at a higher game speed it answers more slowly in game time.
  [code, data]
- A think gives at most `max_ge_commands` (100) orders when planning and
  `max_rai_commands` (40) when reacting; the rest are dropped, not kept
  for later. [code, data]
- Each order is sent as a person's would be: it selects the things (with
  group movement switched on when there are several), then gives the
  order. Planning gives moves, explore and recon orders, recruit and
  research, unpack and pack, building and repair, attaching, air attacks,
  bombardment, special operations and gifts; reacting gives attacks
  (kill), stop, formation changes and sally forth and recall for
  garrisons. [code]

## Regions and strength

- The computer player sees the map as regions: patches of ground of one
  kind, each with a centre, its neighbours and the distance to each, and
  the island it's on. The regions are worked out when the map is saved
  and stored with it (worked out afresh for a map saved without them,
  and for a random map when it's made), the same for every computer
  player. Its main island is the largest land island; exploring,
  special operations on the fog and landings use only that island's
  regions. How they're cut is below. [code, data]

### How the map is cut

All sizes are in tiles, from SVars (stock values given). [code, data]

- Kind: every tile is land, water or blocked. A tile no land unit can
  enter but a water unit can is water; one only land units can enter is
  land; one neither can enter is blocked; one both can enter takes its
  terrain's `type` (`LAND`, `WATER`, or `MOUNTAIN` for blocked).
  Things standing on the map don't count, only the ground.
- Rectangles: the map is covered with rectangles of one kind each, by
  squares of shrinking size. The first size is
  `StaticMapStartingSquareSize` (10); each next size is the last ×
  `StaticMapFastSquareReductionMult` ÷ `…Div` (3 ÷ 4), rounded down
  (with `StaticMapUseFastSquareReduction` off, one less), so 10, 7, 5,
  3, 2, 1. For each size, while its square has no more tiles than are
  still uncovered, every tile is tried in turn, column by column from
  the left (top to bottom in each column): where the size's square
  starting at that tile (its top-left corner) lies on the map, is all
  uncovered and all of that tile's kind, it's grown into a rectangle,
  adding one column on the right, then one row at the bottom, in turn,
  each only while the new column or row stays on the map, uncovered and
  of the same kind, until neither can grow.
- Each grown rectangle is cut into even pieces no wider or taller than
  the largest width W = `StaticMapMaxRectangleWidth` (12) × (the map's
  mean side ÷ `StaticMapBaseMapWidth` (64))^`StaticMapMaxRectangleWidthExp`
  (0.3333), rounded: ceil(width ÷ W) by ceil(height ÷ W) pieces, the
  k-th of n ending at k × width ÷ n (rounded down). Every piece is a
  rectangle of its own from then on. A piece of at least
  `StaticMapMinRegionSize` (9) tiles, with both sides at least
  `StaticMapMinRegionWidth` (3), becomes a region on its own; a smaller
  one waits.
- Islands: rectangles touching along an edge and of the same kind
  belong to one island, waiting pieces included. So there are land
  islands, bodies of water and blocked masses.
- Waiting pieces then join regions, taking each in turn and going round
  them again until none is left: a piece joins the touching region of
  its own kind with which it shares the longest border (the smaller
  region on a tie), leaving out regions of more than
  `StaticMapMaxRegionSizeForExpansion` (1000) tiles. With none to join,
  it becomes a region on its own if it touched a region of its kind
  that was too big, or if its island has no region yet; otherwise it
  waits for a later round.
- Centre: the mean of the centres of its rectangles, each weighted by its
  area, in map coordinates.
- Neighbours: regions with rectangles touching along an edge, whatever
  their kind; the distance to each is the straight line between the two
  centres.
- Bottleneck: a region is one when two of its neighbours on its own
  island can't reach each other within `StaticMapBottleneckDetectionDepth`
  (5) region steps over that island without passing through it. This is
  worked out when play starts.
- Region distances are counted two ways: steps from region to region
  (for "within n regions"), and travel distance summed along the way.
  [code]
- For every region it keeps each side's strength there, its influence,
  in three parts, each a spread of things' combat value
  ([combat](combat.md#combat-value)) out from the region a thing is in:
  mobile influence from units, static and border influence from
  buildings. [code]
- A spread gives the thing's own region its whole combat value V
  (wherever in the region it stands), and each other region whose
  travel distance d from it, along the cheapest way through neighbouring
  regions, is less than a reach R:

  V × (1 − (1 − m) × d ÷ R)^`cv_influence_exponent`

  with m = `min_cv_propogation_factor` (SVars, 0.2) and the exponent
  1.5. So the share falls from 1 to m^1.5 (about 0.09) at the edge of the
  reach; halfway it's about 0.46. A side's influence in a region is the
  sum of all its things' shares there. [code]
- The spread only passes into regions whose ground is land: no strength
  reaches across or into water, though a thing on water still counts in
  its own region. [code]
- The reach: [code, data]
  - for a unit, its speed (capped at `DynamicMapAnalysisSpeedCap`, 10
    m/s) × `cv_influence_max_travel_time` (45 seconds);
  - for a building, the spread stops at its range r, its defence's
    `control_range` (its regiment's for a regiment), else its garrison's
    `guard_range`, else 0 (its own region only), but the share is
    worked out with R = r × `settlement_cv_range_fudge_factor` (1.7), so
    the farthest region it reaches still gets (1 − 0.8 ÷ 1.7)^1.5, about
    0.39. A building in a settlement spreads from the settlement's
    centre. With the stock ranges (10 to 30) a building's static
    influence seldom leaves its own region [guess];
  - for border influence, each building spreads again with R =
    `border_influence_range` (300) and no other limit.
- Friendly influence is the sum over itself and its allies, enemy
  influence over the sides it's at war with. A unit's combat value is
  scaled by what it has learnt of that kind of unit
  ([Learning](#learning)); things with no combat value spread nothing.
  [code]
- This picture counts every live thing on the map, seen or not: the
  computer player always knows where the strength lies
  ([What it knows](#what-it-knows-and-gets)). [code]

## Goals

The computer player decides by keeping a set of goals, giving each a
priority every planning think, and carrying out those that come out
best. Its kinds of goal, with the id a mood's `[GoalPriority]`
`goal_IDS` names it by (AA's data gives entries for only some of
them; a kind with none takes the defaults): [code, data]

| goal | one for | what it does |
|---|---|---|
| attack a region (`ATTACK_REGION`) | every region | sends units at the best enemy target there |
| defend a region (`DEFEND_REGION`) | every region | gathers units where its or its allies' things are threatened |
| attack a thing (`ATTACK_ACTOR`) | each attack flare on a thing | sends units at it |
| defend a thing (`DEFEND_ACTOR`) | each defend flare on a mobile thing | sends units to it |
| air attack (`AIR_ATTACK`) | every thing not its own (when SVars `UseAirAttacks`) | has an airfield send its attack at it |
| bombard (`BOMBARD`) | every thing not its own | has a bombarder in range shell it |
| recover (`RECOVER`) | each of its units | takes a damaged or shaken unit back to supply |
| repair (`REPAIR`) | every building | sends a builder to repair one of its own or an ally's |
| explore (`EXPLORE`) | as many as the mood says | scouts a region |
| build | each thing a builder can build | |
| construct (`CONSTRUCT`) | every region | builds a building there |
| upgrade | each upgrade a thing offers | |
| pack (`PACK`), unpack (`UNPACK`) | its mobile buildings | packs one out of supply, unpacks one somewhere useful |
| recruit (`RECRUIT`) | made afresh each think | recruits a regiment or a thing |
| research (`RESEARCH`) | each technology it could start | researches it |
| attach (`ATTACH`) | each unattached company | attaches it to a headquarters |
| destroy (`DESTROY`) | each thing it could sell or raze | sells or razes it |
| give money, give a building | each other side; each building asked for | [team commands](team-commands.md#computer-allies-giving) |
| special operations (`GENERAL_POWER`) | one | uses the general's operations |
| attach or move a hero | each hero (Kohan II) | |

- A goal goes on from think to think. A goal whose priority comes out at
  0 or less is idle; when a goal it was carrying out goes idle, its units
  are let go. [code]

### Weighing goals

Each planning think, for every goal: [code]

1. The goal works out its own priority from the situation (each kind's
   rules are below). At 0 or less it's idle and nothing further applies.
2. The mood's `[GoalPriority]` entry for its kind changes it: × its
   `multiplier`, + its `bonus`, and to 0 if that's below its `minimum`
   (defaults 1, 0, 0).
3. A goal not carried out last think gets a random amount from 0 to the
   fuzziness, drawn afresh each think. A goal carried out last think
   keeps the random amount it had and gets the inertia on top. The
   fuzziness and inertia are the entry's `fuzziness` and `inertia` where
   they're 0 or more, else the mood's `[GeneralSettings]` `fuzziness`
   (200) and `goal_inertia` (250). Gifts don't take part in this
   ([team commands](team-commands.md#computer-allies-giving)).
4. For a goal that uses units, the units are then chosen
   ([Choosing](#choosing)), and its worth with a set of units is

   travel factor × (V + S) × M

   - V is the priority from steps 1 to 3 as it applies to those units:
     for fighting goals, how well they match the strength wanted
     ([How much strength it sends](#how-much-strength-it-sends)); for
     other goals the priority itself.
   - S and M come from the `[ActorPreference]` entries of the kind's
     `[GoalPriority]`. For each entry, a company matches if it or any of
     its members has the entry's `property_IDS` or is its `actor_IDS`.
     Its `value` is added to S once for a matching company with
     `count_value_once_per_company` (the default), else once for the
     company if it matches plus once for each matching member. Its
     `multiplier` multiplies M once for the whole goal with
     `count_multiplier_once_per_goal` (the default), else once per
     matching company with `count_multiplier_once_per_company`, else once
     per matching company and member. S is added even when V is 0.
   - The travel factor is (max(0.2, 1 − 0.8 × t ÷
     `max_travel_time`))^`travel_time_exponent`, t being the average
     travel time to the goal (distance ÷ speed) of its units that can
     move (300 seconds and 2 in stock data).
   [code]
5. A goal it can't afford ([Money](#money)) is worth 0.

AA's base mood boosts some kinds: repairing × 10 + 10000, unpacking
× 5, recovering × 5, building × 2.7, attacking a region + 750, defending
one + 500, attaching + 500, recruiting × 0.4; each mood sets research's
own multiplier and fuzziness. [data]

### Choosing

- Goals are taken in order of priority, highest first, after the random
  amounts are added; there's no weighted lottery among goals (the
  template's "ping-pong ball" choice applies only to special operations).
  [code]
- A unit serves one goal at a time. Which goals a thing can serve
  depends on what it is (a repair goal takes only builders, a bombard
  goal only bombarders, and so on), and each of an airfield's groups of
  planes
  ([airfields](airfields.md#stock-and-groups)) counts as a unit of its
  own: its attack group serves air attack goals, its recon group air
  recon. So one thing can be on the list for several kinds, and taking
  it for one takes it from the others. [code]
- Each goal keeps a stored worth. It's set at the start of the think,
  with the units the goal held then, and set again when the goal itself
  is settled (below), with the units it ends up with;
  nothing else changes it during the think. A unit's gain to a goal is
  the goal's worth with the units it holds at that moment plus this one,
  less the stored worth; its loss to a goal holding it is the stored
  worth less the worth with the units that goal holds at that moment
  minus this one. So for a goal not yet settled this think, both are
  measured against its start-of-think worth, however many units it has
  already gained or lost since. For a fighting goal the gain and loss
  count across the goals linked to it
  ([How much strength it sends](#how-much-strength-it-sends)). [code]
- A goal that concerns one particular unit (that unit's recover goal,
  say) takes it, from another goal if need be, when its own worth beats
  the unit's loss there. [code]
- Every other goal, in its turn, fills up in three steps, stopping as
  soon as it wants no more (a fighting goal once its strength reaches the
  strength wanted, T; it doesn't go on towards the "enough" M; an air
  attack, bombard or repair goal once it has one unit): [code]
  1. free units, in no particular order, taking any with a gain above 0
     (there's no minimum gain, and the first that gains anything is
     taken, not the best);
  2. units of goals ranked below it, from the lowest up, taking any with
     a gain above 0 whatever they were worth there;
  3. units of goals ranked above it, from the nearest up, taking a unit
     only when its gain is more than its loss there; at the first unit of
     a goal that isn't, it moves on to the next goal up.
  A unit whose way to an attack is blocked ([How much strength it
  sends](#how-much-strength-it-sends)) isn't taken.
- As each goal is settled, its place in the order moves to suit its new
  worth; one that drops is met again later in the same think. [code]
- A goal whose worth comes out above 0 is carried out this think: it
  gives its orders, and what it costs is set against the money left
  ([Money](#money)). The rest go back to waiting. [code]
- Orders are given goal by goal in the order chosen. [code]

### Money

- Goals are paid for in order of priority, out of the stockpile left
  after the goals above them. A goal can also count on income: it's
  affordable if, for each resource that's stockpiled, what's left plus
  the side's net income of it over `saving_time_limit` seconds (300 in
  AA's moods) covers its cost. Resources that aren't stockpiled (unit
  limit, oil, ammo) aren't counted here. [code]
- When the best goal not yet paid for can't be paid now but could be
  from that income, the computer player saves for it: for the rest of the
  think it pays for nothing below it, though goals that cost nothing
  still go ahead. A goal it can't afford even then is passed over and
  the goals below it get their turn. [code]
- `settlement_reserved_resources_*` would hold money back for
  settlements early in a match; AA sets them to 0. [data]

## Building up

### Recruiting

Each planning think it throws away last think's recruit goals and makes
new ones from requests of three kinds: [code]

- one for each `[SpecificRecruitRequest]` in the mood, for each thing it
  could recruit that matches (`number_to_request` of each): the thing
  named by `recruit_ids`, or anything with the property `property_ids`,
  or a regiment with such a thing in it. A thing matches by its own
  properties only: a truck by the truck's, not those of the building it
  unpacks into. (AA's engineer brigade building has `oil` and `ammo`,
  but its truck has only `engineer_brigade`, so it answers neither the
  oil nor the ammo request.) This is how AA's computer players get their
  headquarters, depots and other mobile buildings: AA's base mood asks
  for oil, ammo, a corps headquarters and the regiment headquarters each
  mood allows;
- `num_military_goals` requests for fighting strength, each for 10 combat
  value;
- one for a builder whenever a construct goal ([Building](#building))
  finds a building worth building that no builder it has can build. This
  is how it gets engineers.

What can be recruited is whatever its recruiting buildings (not full,
not dead) offer now, as a person would see it. [code]

- A specific request's priority is its `base_priority` + the thing's
  economic worth ([Worth](#worth)) × `economic_value_multiplier`,
  − `repeat_penalty` for each it already has, + `one_time_bonus` if it
  has none, + `hq_type_not_available_bonus` if the thing is a
  headquarters and the side has none of that kind with a free place.
  [code, data]
- Requests are taken in order of priority (the fighting-strength ones
  count as 0). For each, the computer player picks the best thing to
  recruit: [code]
  - for a specific request only what it names; for a builder request a
    builder that can build what's wanted (an engineer, or a truck that
    unpacks into it); for fighting strength, anything with combat value;
  - a candidate is worth its combat value × `core_cv_multiplier` (for
    fighting strength) or `support_cv_multiplier` (otherwise), plus its
    combat value ÷ its cost × the matching `…_cv_ratio_multiplier`, plus
    its economic worth × `[Recruiting]` `economic_value_multiplier`
    (0.2), plus its `[ActorPriority]` ([Liking](#liking)); a regiment
    adds up its members;
  - minus `insufficient_income_level`'s `insufficient_income_penalty`
    (350) if recruiting it would take its money income below that level,
    else minus `decreased_income_penalty` (20) if it would lower it at
    all (oil and ammo it would use count at what buying them costs when
    it's short of them);
  - builders and trucks lose `mismatching_builder_penalty` (500) as
    fighting strength, and gain `matching_builder_bonus` (500) for a
    builder request they answer;
  - then a random amount up to `candidate_fuzziness` (50).
  The best above 0 wins; with none above 0 the request gives nothing.
  So a thing whose `repeat_penalty` outweighs its value (AA's corps
  headquarters: 1000, less 10000 per one owned) is never recruited twice.
- A request for fighting strength goes on making recruit goals until
  they add up to its 10 combat value, which in AA is one regiment; and
  once one such request has made its goals, the others are skipped. So
  in effect it recruits at most one fighting regiment per think, and
  `num_military_goals` only sets how many tries it gets. [code]
- No kind is picked more often in one think than its
  `max_recruited_per_think`, or `default_max_recruited_per_think` (1)
  when that's 0. [code]
- Of the goals made, only the best `max_goals_per_think` are kept, and
  only those that fit the side's limits ([recruiting](recruiting.md)),
  unless it makes room by disbanding something (below). [code]
- Making room: a recruit goal that doesn't fit the limits looks for one
  of its side's things to disband in its place. It takes the one with
  the lowest combat value among those that: [code, data]
  - can be disbanded at that moment;
  - have a combat value below `replace_company_cv_mult` (0.6) × the
    combat value of what it would recruit (all its members);
  - aren't builders, repairers or recruiters, and aren't already held
    for another recruit;
  - would free, once gone, at least as much of every limit as the
    recruit needs.
  With one found, the goal is kept and holds that thing (taking it off
  whatever goal it served). When the recruit goal is carried out, the
  thing is ordered to disband, if it still can, and the recruit is
  ordered in the same think. If the recruit goal isn't carried out, the
  thing is let go again like any unit of an idle goal. A goal born of a
  request (a builder or a specific request) makes room only when what
  it recruits can build a settlement centre, so never in AA, whose data
  has none. [code, data]
- A recruit goal's priority is the request's plus the winner's worth,
  then the mood's `RECRUIT` weights. A thing that would come ashore by
  boat where its side is weaker than `non_combat_landing_min_influence`
  (0.75 of the enemy) is worth × `non_combat_landing_high_risk_multiplier`
  (0.001). [code, data]
- Where: among its recruiting buildings that can recruit it now, each
  scores `base_value` + (`building_bonus` + any `SpecificBuildingBonus`)
  per building in the recruiter's settlement + a random amount up to
  `location_fuzziness`. In AA that's in effect a random choice. The
  template's "purely on the basis of distance" isn't so: distance plays
  no part. [code]
- It never has more than `build_queue_max_items` of its own items queued
  at one building (1 in AA), so each recruiter works on one thing at a
  time for it. [code, data]
- Recruits that can be delivered to a spot ([deliveries](deliveries.md))
  are dropped: [code]
  - in a region where its units are fighting, or one it's attacking,
    scored by that region's attack or defend priority × a factor that is
    highest for an even fight: the friendly ÷ enemy ratio r squared below
    1 (`recruit_by_air_ratio_under_one_*`), 1 ÷ r above
    (`…_over_one_*`), + a random amount up to `recruit_by_air_fuzziness`
    (200); a side with no strength there counts as
    `recruit_by_air_min_ratio` or `…_max_ratio`;
  - then on a region within `recruit_by_air_target_search_depth` (2) of
    that one, scored by its ratio held between
    `recruit_by_air_target_search_min_ratio` and `…_max_ratio`, ×
    `…_ratio_multiplier` + a random amount up to `…_fuzziness`; on a free
    spot there, else the region's centre;
  - with no such region, when the recruiter is on another island, on
    the main island where it costs least: distance from the recruiter,
    + 10000 if it can't be reached, + (enemy − friendly influence) ×
    `landing_location_influence_multiplier` held between −50 and 100,
    − `landing_location_friendly_buildings_distance_modifier` (150) where
    it has a building.

### Liking

What a mood's `[ActorPriority]` entries add to a kind of thing, used in
recruiting, building, attaching and attack and defence worth: [code,
data]

- Each entry that applies gives `value` − `repeat_penalty` × the number
  the side owns, + `one_time_bonus` if it owns none. The number counted
  is of that very kind for an `actor_IDS` entry, and for a
  `property_IDS` entry of everything with the property when
  `count_all_property_actors` (the default), else of that kind.
- An entry applies through the thing's own properties only, as with
  requests (a truck's, not its building's).
- A thing takes one entry's worth, not the sum: the best of those that
  apply (an `actor_IDS` entry for it counts among them). With none, 0.
- In some uses besides choosing what to recruit or build (valuing a
  thing that already exists), one fewer is counted and there's no
  `one_time_bonus`. [code; guess: which uses]

### Research

- Each think there's a research goal for each technology one of its
  research buildings could start now. Its priority is the mood's
  `[Research]` `TechPriority` for it, or `base_priority` when it isn't
  listed; then the mood's `RESEARCH` weights. Nothing else counts (the
  template's "not yet implemented" note holds). [code]
- AA's styles give research low priorities (around 20 before weights),
  so research takes what money is left over. [data]

### Worth

How much a thing is worth to the computer player (the template's
`[ActorValue]`), used for recruiting, building, targets and selling:
[code, data]

- Its economic worth: for each resource it makes or uses, a need per
  point, from the side's net income of that resource: `short_need` for
  the part that takes income below 0, `enough_need` up to `plenty_value`,
  `plenty_need` above (never plenty when `plenty_value` is 0 or less).
  Money uses the `stockpilable_*` numbers, other resources the `other_*`
  ones, unless a `[ResourceNeed]` names the resource. A packed mobile
  building counts the building it unpacks into, if that's worth more.
- Plus the worth of its side's buildings that it supplies, plus its
  military worth (below). [code]
- × a factor for enemy strength where it stands, as the template's
  formula with `enemy_influence_mult`, `min_influence_factor` and
  `influence_exponent`; × a factor between `distance_mult_min` and
  `distance_mult_max`, higher the nearer it is to the side's centre (the
  average position of its buildings); × `scaling_mult` (0.3).

Military worth, all × `military_value_scaling_mult` (1): [code, data]

- A thing that can move (or a spot off the map): its combat value ×
  `military_cv_mult` (5).
- A thing that can't move: nothing unless it has strength, its combat
  value plus that of its garrison stock (each `[Inventory]` entry's
  count × that kind's combat value), and a reach r above 0: its
  garrison's `guard_range` if it has a garrison, else its defence's
  `control_range`. Then, with the ratio q = the enemies' influence in its
  region ÷ its own side's (mobile and static, allies not counted;
  [Regions and strength](#regions-and-strength)):
  - q is 0 when below `military_value_influence_ratio_min` (0.5); above
    `…_max` (2) it's held at 2 for a building that stands, and 0 for one
    only being considered for building; then f = q^`…_exponent` (2).
    So a new defence is worth nothing where the enemy is weak, or more
    than twice as strong;
  - + f × the worth of each of its side's things within r ×
    `military_value_radius_mult` (0.6) of it, apart from itself and the
    rest of its own regiment or settlement [guess: that their worth is
    taken without their own military part];
  - + f × `military_value_marker_value` (200) for each marker in its
    region (the whole region, not only within reach) that nothing is
    built on, other than one at its own spot;
  - + `military_value_enemy_traffic_value` (20) for each time an enemy
    unit left its region within the last `defend_duration` (300 seconds,
    from `[Learning]`), without f;
  - + f × `military_value_bottleneck_value` (200) if the region is a
    bottleneck, one that must be crossed to go between two of its
    neighbours ([How the map is cut](#how-the-map-is-cut)).

### Unpacking and packing

- Each packed mobile building (a truck that unpacks into a building) has
  an unpack goal. It weighs the regions next to the truck's
  (`adjacent_region_depth`, 1), up to `num_random_regions` (20) random
  regions of its island (repeats skipped) and the one it chose before.
  In each it looks for a spot for the building it unpacks into
  ([Finding a spot](#finding-a-spot)); a region with none drops out.
  At that spot a region is worth: [code]
  - `base_priority` (500),
  - − enemy mobile influence × `enemy_dynamic_influence_mult` (3) −
    enemy static influence × `enemy_static_influence_mult` (3),
  - + for each of its own buildings (its side's things that can't move,
    allies' not counted) less than `max_distance` (30) from the spot,
    in a region whose centre is also within that,
    `max_friendly_building_bonus_per_building` (200) × (distance ÷
    `max_distance`)^`distance_exponent` (1), the sum capped at
    `max_friendly_building_bonus` (500). So a building right beside the
    spot adds almost nothing and one nearly 30 away nearly 200, the
    reverse of what the template's comment intends;
  - − (straight-line distance from the truck to the spot ×
    `travel_distance_mult` (5))^`travel_distance_exponent` (1),
  - + a random amount up to `fuzzy_factor` (150), + `inertia` (300) for
    the region chosen before.
- The best region wins (the first of equals); the goal's priority is its
  worth, raised to 0.001 if below 0. The truck is given one unpack order
  at that region's spot, and drives there as a person's unpack-at-a-spot
  does ([buildings](buildings.md#packing-and-unpacking)). [code]
- It packs a building only to bring it back into supply: a packable
  building that needs supply and stands out of its side's supply, while
  the side has supply somewhere, has priority
  `pack_out_of_supply_priority` (1000). It never packs a building just
  to move it somewhere better. [code]

### Building

- Each region has a construct goal. With `construct_in_all_regions` off
  (AA), it builds only in regions holding one of its things that
  researches, recruits or is a city (`construct_to_defend_research`,
  `…_recruiters`, `…_cities`), and with `construct_in_supply_only` (AA)
  only in supply. [code, data]
- It weighs each building it could build there: its worth + its
  `[ActorPriority]`, and for a defence building +
  `construct_supply_defense_bonus` (400) in a supply zone and +
  `construct_city_defense_max_bonus` (200) × (1 ÷ n)^`construct_city_defense_exponent`
  in a city's zone, n being its defence buildings there counting the new
  one; + build flares. Each building is weighed at the one spot it finds
  for it in the region ([Finding a spot](#finding-a-spot)); one with no
  spot there isn't weighed. Once it has settled on a building it keeps
  that spot on later thinks rather than looking again. [code; guess:
  when it gives the spot up] The best one a builder of its can build is the
  goal's priority; when the best of all needs a builder it hasn't got, it
  asks for one ([Recruiting](#recruiting)). A building under way gets
  `construct_in_progress_bonus` (2000). [code, data]

### Repairing

- Every building has a repair goal. It has a priority only while the
  building belongs to the computer player's side or an ally's and is
  below full health, by any amount: (its worth ([Worth](#worth)) + its
  `[ActorPriority]`) × (1 − its health share), + repair flares on it
  (a build flare on a damaged thing). AA's mood then makes it × 10 +
  10000, so a repair outranks nearly everything else. [code, data]
- It takes one builder: a thing of its side that can repair (in AA,
  engineers). The builder is given the repair order on the building,
  unless its own recover goal outbids the repair. [code]
- Units aren't repaired by this goal: a damaged unit goes back to supply
  by its recover goal ([Recovering](#recovering)), and a repair flare on
  a unit adds to that. [code]

### Finding a spot

Building, unpacking and the spot for a delivered recruit all find a
place inside a region the same way: [code, data]

- It tries points in rings round the region's centre. The step s is the
  size of the building's placement shape (its blocking shape grown by
  `BuildingPaddingMeters`: a circle's radius, or a square or rectangle's
  smaller half-side), at least one tile (2 m). Ring k has radius k × s
  and max(1, 6k) points evenly round it (ring 0 is the centre). There are
  ceil(0.75 × W ÷ s) rings, W being the widest region patch the map
  allows: `StaticMapMaxRectangleWidth` (12) × (the map's mean side in
  tiles ÷ `StaticMapBaseMapWidth` (64))^`StaticMapMaxRectangleWidthExp`
  (0.3333), rounded, taken as metres.
- It starts at a random ring and works outwards, wrapping round to the
  centre, every ring from the same random angle, and takes the first
  point that passes, unscored. With none, it tries once more from a new
  random ring and angle; then there's no spot.
- A point passes when it's on the map; in its side's supply if the goal
  builds only in supply or the building needs supply (`supply_required`);
  with no ground under the placement shape in an enemy's guard zone (or
  other zone [guess]); passing the player's placement check
  ([objects](objects.md), [buildings](buildings.md)); and in the goal's
  own region.
- A building that must stand on a given object (`required_object_ids`)
  takes instead the first free one of that kind in the region that its
  side may use; AA's data hardly uses this. [code, data]
- To build, its builders are sent to a site of that building already
  standing on the spot, if there is one, else ordered to build it there.
  [code]

### Attaching

- Each unattached company that isn't fighting (`attach_when_engaged`
  false) has an attach goal for the first headquarters of its side that
  can take it; its priority is its combat value × `cv_multiplier` (5) +
  its `[ActorPriority]`, then the mood's `ATTACH` weights. [code]

### Selling and razing

- Every thing of its side has a destroy goal; it counts only for a
  thing it could sell, raze or disband at that moment (in that order of
  preference when it acts). For any other thing the priority is just
  `base_priority` + the random amount below, so it stays idle. AA's moods
  keep the template's `[Destroy]` numbers. [code, data]
- Priority = `base_priority` (−300) + a random amount from 0 to
  `fuzzy_factor` (100), drawn every think, + the economic term (with the
  sale term when it applies) + the denial term. [code]
- Economy in trouble: for any stockpiled resource (money in AA), the
  side's net income of it is below `min_stockpilable_income` (−9999),
  or its stockpile is below `min_stockpile` (1), or the stockpile plus
  the net income over `min_seconds_solvency` (60) seconds is below 0.
  [code, data]
- Economic term: −(the thing's economic worth ([Worth](#worth), without
  its `[ActorPriority]`)) × `economic_value_mult` (0.85). A thing worth
  more than nothing gives a negative term, which always counts; a
  positive one (a thing that costs more than it brings) counts only while
  the economy is in trouble, else it's 0. [code]
- Sale term, added to the economic term only while the economy is in
  trouble, for a building that can be sold and has production or upkeep
  of its own: [code]
  - it's skipped if selling would leave the side's net income of any
    non-stockpiled resource (net income less what the building
    produces of it, net of its upkeep) below
    `sale_resource_income_min` (−5), or if the building's worth (its
    cost, with its members' for a thing made of several) is 0 or less in
    any stockpiled resource;
  - else it's `sale_base_value` (50) + its worth in each stockpiled
    resource × `sale_price_multiplier` (2) + its combat value ×
    `sale_cv_mult` (0.2; added, though the template's comment says
    subtracted) + for each non-stockpiled resource, its net production
    of it, less however far the side's net income of it is above
    `sale_resource_income_max` (10), × `sale_resource_income_mult` (3)
    where that's above 0; and never below 0.
- Denial term, from the strength picture ([Regions and
  strength](#regions-and-strength)) in the thing's region: its own
  side's influence there (mobile and static, allies not counted) and
  every enemy's. For a thing it can sell, with the `denial_sale_*`
  numbers, else for one it can raze or disband, with the `denial_raze_*`
  ones: when the enemy's influence is more than its own ×
  `…_min_cv_advantage` (0.8 for selling, 1.2 for razing), and the time
  since the thing came to the computer player (built, recruited or
  captured) is at most `…_max_time_owned` (300 seconds), the term is
  ((max time − time owned) ÷ max time)^`…_exponent` (1) ×
  `…_max_priority` (200) − its combat value × `…_cv_mult` (5), and never
  below 0; otherwise 0. [code, data]
- So with AA's numbers it destroys something only when its money runs
  out (or will within a minute) and the thing costs more than it brings,
  or the sale value lifts it over the base, or a thing with no combat
  value is newly taken where the enemy is stronger. A unit that can be
  disbanded falls under the same rules; nothing here looks at the unit
  limit. [code, guess: what that comes to in play]
- The goal doesn't keep the inertia of a goal carried out last think:
  each think it weighs the thing afresh. It's given the order (sell,
  else raze, else disband) when carried out. [code]
- When it captures a building it could raze, it decides once whether to
  raze it (scorched earth): with chance `se_base_probability` (0.2) ×
  ((enemy influence × `se_influence_mult`) ÷ its own)^`se_exponent`, the
  factor held between `se_influence_floor` and `se_influence_ceiling`.
  [code]

### How an AA match opens

Derived from the rules and the armour style's low-income mood, not seen
in the game: [guess]

- It starts with its corps headquarters and money, and its income puts it
  in the low-income mood.
- Nothing its corps headquarters recruits has combat value, so there's no
  fighting recruit at first. It recruits a mechanised and an infantry
  headquarters, then oil and ammo depots, one at a time, saving up when
  it must. Engineers are ruled out (−10000) in this mood, and armour
  headquarters aren't asked for until the main game.
- Each truck drives out and unpacks near its base, away from the enemy.
- Once a regiment headquarters stands, it recruits a regiment a think
  (tanks first, for the armour style), attaches companies to
  headquarters, and spends what's left on research. Its exploring units
  head out from the start.
- With more income come the main game's airfield, engineer brigade,
  motor pool, artillery brigade, one engineer and bunkers.

## Fighting

### Attacking

- A region's attack goal targets the most valuable enemy thing it knows
  of there: a unit or building it can see, or one it remembers seeing (a
  remembered one is attacked at the region's centre). A thing counts if
  its side is at war with the computer player's, it's alive and it can
  be attacked. A target in a city makes the city the target. [code]
- Its priority is the target's worth ([Worth](#worth)) + its
  `[ActorPriority]` ([Liking](#liking)), so it likes
  to hit what it likes to build; + attack flares within
  `linked_region_distance` (2) regions, but only when there's something
  to attack. [code]
- A thing's attack goal (from an attack flare on it) has the flares'
  priority + the thing's worth while it's seen or remembered; it ends
  when no flare is left. [code]

### Defending

- A region's defend goal looks at its own and its allies' things there.
  For the most valuable, priority = (its worth)^`economic_value_exponent`
  (0.5) × `economic_value_multiplier` (2) + enemy influence^…exponent ×
  `enemy_influence_multiplier` (0.5) + enemy border influence × 0.2 +
  friendly border influence × 0; × `enemy_forces_bonus` (3) when an
  enemy unit it can see is within `enemy_forces_threat_distance` (80) of
  the region's centre; × `capture_risk_bonus` (2) when the enemy is at
  least `capture_risk_ratio` (1) times as strong there (or it has no
  strength there); × what it has learnt of losses there
  ([Learning](#learning)); × `defend_friendly_kingdom_actor_multiplier`
  (0.2) for an ally's thing. Then + defend flares. [code; guess: the
  capture-risk test's exact form]
- A thing's defend goal (from a defend flare on a unit) has the flares'
  priority + the thing's defend worth, and ends with its last flare.
  [code]

### How much strength it sends

For each attack and defend goal: [code, data]

- Strength wanted T = the larger of `min_cv_to_assign` and the enemy's
  influence in the region (mobile and static, for defending too, though
  the template says otherwise) × `cv_advantage_target`; enough M =
  T × `cv_advantage_max` (not the enemy's × it, as the template
  suggests). Attacks use `[OffensiveCV]`, defence `[DefensiveCV]`.
- Strength it has, C: the combat value of the units on the goal and on
  same-kind goals within `linked_region_distance` (2) regions (so nearby
  targets share forces), builders counting at `penalty_to_builder_cv`
  (0.01 in AA); for defence its buildings' static influence there too
  when `count_static_cv`; plus all its allies' influence there, people's
  included, at full (`attack_/defend_allied_influence_cv_multiplier` are
  ignored). With `attack_/defend_allied_influence_cv_can_exceed_max`
  (true in AA) allies' strength first fills up to M and what's beyond
  raises M.
- The goal's worth with those units: its priority × (C ÷ T)^`cv_exponent_under_target`
  below T; × 1 at T; rising in a straight line to ×
  `cv_max_mult_over_target` at M and no more above. Attacks are then
  × `congruency_min_multiplier` + (1 − it) × (how much the units come
  from the same direction)^`congruency_power`, and + `no_primary_supply_bonus`
  (500) when the side has no supply source left. Then the travel factor
  and unit preferences ([Weighing goals](#weighing-goals)). [code;
  guess: how the direction is averaged]
- AA's moods set, low income / main game / end game: attack
  `cv_advantage_target` 1.2 / 1 / 0.8 with exponent 3 / 2 / 1.5; defence
  target 1, `cv_advantage_max` 2.5, `min_cv_to_assign` 150 / 150 / 250,
  `count_static_cv` only in the low-income mood. So it attacks only once
  it can match the enemy there, and less carefully as it grows richer.
  AA's base mood prefers infantry for defence (+50 each, −25 per vehicle
  or tank) and not for attack (−25), and keeps engineers out of fights
  (−5000). [data]
- So it doesn't wait to gather strength: an attack with too little is
  still worth something above 0, and takes what it can get. One company
  goes at a city that needs five if nothing ranked above wants it, and
  more join as later thinks free them; each unit sets off on its own,
  with no gathering point. What holds units back is only other goals:
  those ranked above take free units first, and a goal ranked below can
  take a unit from an under-strength attack easily, as the unit adds
  little there. [code]
- `min_cv_to_assign` only sets the least strength wanted; it doesn't
  stop a weaker goal being carried out. [code]
- A unit locks onto an attack once it's within `attack_commit_distance`
  (3) regions, unless it's a builder. [code]
- Before a unit is put on an attack, its way there is checked: the
  chain of regions from its region to the target's, and every region
  within `occluded_target_search_depth` (2) regions of each of them. The
  first of those (nearest the unit first) holding an enemy building
  with combat value above 0 that the computer player sees or remembers
  blocks the way [guess: the order the regions are tried in]: the unit
  isn't sent, and the computer player flares
  that region for attack ([Its own flares](#its-own-flares)). Whether
  anyone is fighting there doesn't matter; enemy units don't block. The
  check is made once per unit per think. [code]

### Moving and fighting

- Planning only moves units: to the target, to a remembered target's
  region centre, into the region to defend, to the thing to defend. The
  fighting is done by reacting and by the units' own engagement
  ([combat](combat.md#engagement)). [code]
- A building is taken by fighting it down: attack targets include enemy
  buildings, and a capturable one falls to whoever brings it to nothing
  ([buildings](buildings.md)). There's no separate order to capture.
  [code, guess]
- A unit whose recover goal outbids its place on a defence goes to
  recover first. [code]

### Reacting

Every reacting think it decides retreats, sends recovered units back,
picks targets near its goals, answers bombardment and sets formations.
AA changes only the formations from the template's `[ReactiveAI]`
numbers. [code, data]

- Retreat: a regiment that is fighting, not already retreating, below
  `unit_retreat_max_health` (0.3) average health and able to recover
  (morale below `critical_morale`, or supply to go to) looks at the
  fight around it: itself and those fighting it, out to
  `unit_retreat_depth` (1) steps of who fights whom, own side against
  theirs by combat value. If the enemy has any and the ratio is below
  `unit_retreat_cv_ratio` (0.9), it retreats with chance
  `unit_retreat_probability` (1), which means it goes over to its
  recover goal. There's no retreat of a whole region and no halting a
  retreat: those template fields aren't in AA. [code]
- A recovering unit goes back to the fight (to its region's goal) once
  its morale isn't critical and its health is at least
  `unit_retreat_max_health`, or when recovering has become impossible.
  [code]
- Targets: for a region goal, every enemy thing within `attack_distance`
  (2) regions is a candidate. If its units there are weaker than
  `min_attack_advantage` (1) times the candidates, it takes on only
  enemies already fighting. Each target gets the nearest free units until
  it faces `min_attack_advantage` times its combat value, then again up
  to `max_attack_advantage` (1.5); units already fighting are used only
  when no free ones are left. Chosen units are told to attack; the rest
  move to the region. A thing goal's units within `attack_distance` are
  set on the thing (on its occupants if it holds others) and nearby
  enemies the same way; those further off close in. [code]
- The candidates aren't limited to what its side sees: every enemy unit
  or building within `attack_distance` regions that is alive, can be
  targeted, isn't invulnerable and isn't inside something counts, seen
  or not, and its units are told to attack it all the same. What a unit
  then does with an attack order on a thing it can't see is the attack
  order's business ([combat](combat.md)). [code]
- Bombarded: it flares the bombarders for attack, and a garrisoned
  building sallies forth when strong enough against the enemy around it
  and recalls its garrison when not; a bombarded unit that can't get at
  its attacker goes to recover. [code, guess]
- Formations: each regiment takes its goal's `[GoalFormation]` (or
  `[DefaultFormation]`): `combat_formation` when an enemy is fighting it
  in its region, `threatened_formation` when the enemy is at least
  `threatened_region_influence_ratio` (0.3) of its own strength there,
  else `unthreatened_formation`; it's changed only when different. AA:
  column by default, defensive on attack and defend goals, aggressive in
  combat. [code, data]

### Recovering

- Each unit has a recover goal: priority the larger of
  `base_priority_hp` (500) × (1 − health)^`exponent_hp` (3), once health
  is below `max_hp` (0.4), and `base_priority_morale` (500) ×
  (1 − morale share)^`exponent_morale` (3), once below `max_morale`
  (0.5); either keeps counting once the goal is under way, until the
  unit has healed. Then + repair flares on the unit, and AA's × 5. [code,
  data]
- Where it goes: [code, data]
  - a unit that nothing is fighting and that stands in its side's supply
    stays where it is;
  - a place it has chosen is kept while nothing is fighting the unit and
    the place is still in supply; otherwise it chooses again, so a unit
    under attack chooses afresh each time its orders are given;
  - to choose, it takes `RecoverSupplyZoneCandidates` (5) points round
    each supply provider of its own or a friendly side that is giving
    supply, each in a random direction at a random distance from 0 to 0.9
    of the provider's supply range, moved to the nearest spot within 4
    where the unit fits; it keeps those whose ground is in its side's
    supply ([supply](supply.toml)) and scores them as a routing regiment
    scores where to run ([morale](morale.md#routing)), with the
    `Recover…` SVars in place of the `Rout…` ones: base 201, the angle
    away from its attackers × 1, less the distance, up to 200, × 1,
    500 off in an enemy's control zone and 500 in its guard zone, nothing
    for supply, and a random 0 to 40. When it's leaving a fighting goal,
    the distance is measured from that goal's region centre rather than
    from the unit;
  - the best point wins if it scores above 0, which rules out any point
    in an enemy's zone.
- It's ordered to retreat there, and to stop once it stands on the spot;
  it then waits, healing as supply allows. With no place found it's
  ordered to rout if something is fighting it (and runs by the routing
  rules), else to stop. When the provider it chose is gone, it chooses
  again. [code]

### Exploring

- There's no exploring when the match has fog switched off. [code]
- The mood sets how many explore goals of each kind (`[Exploration]`):
  close (near its buildings), deep (far afield) and air recon (by
  aircraft; ground explore goals don't take aircraft). AA: no close
  ones; deep 1 / 2 / 3 and air recon 0 / 1 / 1 in the low-income / main
  / end-game moods. [code, data]
- Priority: `base_priority` − `repeat_penalty` per earlier goal of the
  kind, sliding towards `min_priority` as more of the main island's
  regions count as explored (at least `percent_to_count_region_explored`,
  0.8, of a region seen); + the strongest explore flare for its side.
  In AA base and minimum are equal, so exploring never tails off. [code,
  data]
- Which units: those the `EXPLORE` preferences favour (AA: recon units
  +500, engineers and trucks −5000). The template's actor scores by
  speed, sight and combat value aren't used. [code, data]
- Which region, each think: candidates are the explorer's region
  (`actor_region_bonus`), the one it explored last (`current_region_bonus`),
  for close goals each region with its buildings
  (`building_region_bonus`), each explore flare's region
  (`flare_region_bonus`), `num_random_candidates` random regions
  (`random_candidate_bonus`), and each region of the main island with
  chance `intuition_probability` (within `intuition_distance` regions of
  its buildings when that's 0 or more), scored `intuition_region_bonus`
  × (the mines, settlement spots, settlements and enemy buildings it has
  no picture of there, as the `intuition_count_*` flags say)^`intuition_region_exponent`.
  A region takes the largest of its bonuses, not their sum, and a bonus
  of 0 or less doesn't make it a candidate. "No picture of" means its
  side neither sees nor remembers it: intuition counts the markers and
  enemy buildings in a region that are hidden from it, so it's drawn
  towards enemy buildings it has never seen. [code]
- Score: the bonus + a fog score from `black_fog_priority` down to
  `grey_fog_priority` by how many regions (up to `fog_search_depth`)
  away the nearest unexplored region is; × the distance factors from its
  centre, its last region and the explorer, each between its `_floor`
  and `_ceil` by (distance ÷ `_max`)^`_exp`, reversed when
  `…_prefer_close`, and 0 beyond `_cap`; × the time since it last saw an
  explored region, capped at `time_since_last_scouted_max`, when
  `use_time_since_last_scouted`; + a random amount up to `fuzzy_factor`.
  The best wins. [code; guess: how the factors are scaled]
- Ground explorers get the Explore order to the region's centre,
  aircraft air recon there ([airfields](airfields.md)). [code]

### From the air

- With SVars `UseAirAttacks` (true in AA, false in Kohan II), any enemy
  thing it can see or remembers, on a region of the map, can be
  air-attacked, unless it sent planes at it less than
  `min_time_between_attacks` (120 seconds; 90 for the planes styles)
  ago. That time is game time, kept per target, from the think in which
  it gave the air attack order; it's counted from the order, whether or
  not the airfield then sends anything. [code, data]
- Priority: the target's attack worth, exactly as a region's attack goal
  values it ([Attacking](#attacking): its worth + its `[ActorPriority]`),
  then: [code, data; guess: the resupplying test]
  - × `engaged_bonus` (2) if it's fighting;
  - × `target_damage_bonus` (2) at health from `target_damage_floor` up
    to, not including, `target_damage_ceiling` (0.5–0.8);
  - below `absolute_min_health` (0.3): × 0 if it's fighting, × 5 if not;
  - × `resupplying_mult` (0) while it resupplies;
  - × `support_ground_attack_bonus` (2) when the computer player's
    attack goal for the target's region was carried out at its last
    planning think; else × `no_bonus_penalty` (0) if none of the
    factors above applied.
  So it strikes only what's fighting, damaged, nearly dead and alone,
  or in a region it attacked last think. Then, if above 0, + attack
  flares on the target.
- What carries it out is an airfield: each airfield's attack group (its
  `AIR_OFFENSIVE` group, [airfields](airfields.md#stock-and-groups)) is
  one unit for this, and an air attack goal takes one, never more. It
  chooses as any goal does ([Choosing](#choosing)): the first free
  airfield that gains anything, else one taken from another air attack
  goal. An airfield whose attack is still out (its planes not yet home)
  stays with the goal it went for and can't be taken by another. Whether
  the group's planes are ready isn't looked at; an airfield that can't
  send, or whose side can't pay `[AirAttackCost]`, refuses the order as
  it would a person's ([airfields](airfields.md#air-attack)). [code;
  guess: the refusal]
- Its worth with the airfield is its priority × the travel factor
  ([Weighing goals](#weighing-goals)), the airfield's travel time being
  its distance to the target at its attack planes' speed [guess: whose
  speed]. With no `[GoalPriority]` entry for `AIR_ATTACK` in AA, the
  mood's weights are the defaults and no preferences apply. The
  `extra_planes_multiplier_*` fields scale the worth by the number of
  airfields on the goal (× 1 for one, × 1.5 for two, × 1.75 for three
  with the stock 2 and 0.5), but since it takes only one they change
  nothing. [code, data]
- It costs nothing in the computer player's money check
  ([Money](#money)): it's never saved for and never passed over for
  want of money; the airfield's own cost is paid, or the send refused,
  when the order arrives. [code]
- Orders: the airfield's Escort is switched on first if it's off,
  every time, whether or not the airfield has escort fighters ready (it
  never switches it off); then the air attack order on the target, on
  its remembered spot when it isn't seen. [code]

### Bombardment

- Any enemy thing it sees or remembers, on a region of the map, can be
  bombarded; a unit only while it's fighting, a building at any time.
  Priority: its attack worth (as for air attacks), ×
  `unengaged_target_multiplier` (0.1) when it isn't fighting and the
  computer player's attack goal for its region wasn't carried out at its
  last planning think; + attack flares on it. [code, data]
- A bombarder is any thing of its side with a weapon that can bombard:
  a unit, a regiment one of whose members has one, or a building, at sea
  as on land. A bombard goal takes one bombarder, and only one already
  within its bombard range of the target: the computer player never
  moves anything to bombard. [code]
- Its worth with the bombarder: priority × the bombarder's combat value
  ÷ `balancing_constant` (0.001), then the travel factor and
  preferences of `BOMBARD` (none in AA's data). [code, data]
- A bombard goal competes for units like any other: a bombarder on an
  attack or defend goal can be taken by a bombard goal, and the other
  way round, by the rules of [Choosing](#choosing). [code]
- The bombarder is given the bombard order on the target, on its
  remembered spot when it isn't seen ([combat](combat.md)). [code]

### At sea

- AA's computer player has no ships to command; the sea matters to it
  only for recruits landed by boat ([Recruiting](#recruiting)). [data,
  guess]

## Special operations

### The goal

- It has one goal for its general's operations ([special
  operations](special-ops.md)), covering each operation that its mood
  lists in `[GeneralPowers]` (matched by `ability_ids`); operations its
  general has that aren't listed are never used. [code]
- The goal is weighed like any other ([Weighing goals](#weighing-goals)):
  its priority × the `GENERAL_POWER` `[GoalPriority]` entry, with
  fuzziness and inertia. AA's moods have no such entry, so the defaults
  apply (× 1, + 0, fuzziness 200, inertia 250). It holds no units and
  costs no money, so it's carried out in every think in which its
  priority after those comes out above 0; nothing else can take that
  from it. [code, data]
- Its priority is the sum, over the operations it weighs that think, of
  each one's best location score (below). Every operation that found a
  location is used that think, each at its own best location, so one
  think can use several. [code]

### Which operations it weighs

- The listed operations are kept in an order drawn at random, an
  operation's chance of coming early being its `[Priority]` `base`
  squared over the sum of that for those still to place (a weighted draw
  without replacement). The first order is drawn when the goal is made.
  [code]
- Each think it goes down that order with the side's experience as a
  budget. An operation costing more than what's left of the budget
  (`cost_faction_experience`; equal is enough) ends the walk: it and
  every operation after it are skipped this think. Otherwise its cost
  comes off the budget and it's weighed, whether or not it then finds a
  location. [code]
- Whether an operation is still recharging isn't looked at here. A
  recharging operation is weighed, can win a location, adds to the
  priority and takes its cost off the budget like any other, and its
  order is then refused (below). [code]
- At the start of the next think, if any operation found a location in
  the think before, the order is drawn again from the first such
  operation on: the operations before it (weighed but with no location)
  keep their places, and that one and all after it are drawn afresh by
  the same weighting. A think in which none found a location leaves the
  order as it was. This happens whether or not the goal was carried
  out. [code]

### Where it looks

- Things are looked at one by one: a regiment's or settlement's members
  each count on their own, never the regiment as a whole. Dead things
  are never counted. [code]
- `[ActorRequirements]` pick the things a location is found from (and
  `[Category]` entries below test things the same way). A thing passes
  when: [code]
  - its own health ÷ its full health is at most `max_percent_health`
    (default 100%; a unit's or building's own, not its regiment's);
  - with `must_be_engaged`, it's fighting (attacking or being
    attacked); with `must_not_be_engaged`, it isn't;
  - with `must_be_friendly`, its side is the computer player's own or
    an ally; with `must_be_enemy`, a side it's at war with; with
    `must_be_owned`, its own side. Without any of the three, only the
    things of no side are left out: but see the next point;
  - with any `[RequiredProperty]` entries, it has at least one of those
    properties.
- The things tried come only from the sides that the requirements'
  `must_be_friendly` (own and allies), `must_be_enemy` and
  `must_be_owned` select, so an entry setting none of the three never
  finds a location. [code]
- A thing a location is found from must also be one its side sees or
  remembers ([What it knows](#what-it-knows-and-gets)). [code]
- `[Location]` `type`: [code]
  - `ON_ACTOR`: each such thing is a candidate, scored at its own
    position.
  - `NEAR_ACTOR`: for each such thing, `num_candidates` spots, each in a
    random direction at a random distance from 0 up to `[Placement]`
    `radius` (evenly by distance, not by area) from where the thing
    really is. A `radius` of −1 (the template's default) puts the spots
    within one tile of the thing, on the opposite side of the drawn
    direction; it's not the operation's area. No AA entry uses it. If
    the operation summons something, each spot is moved to the nearest
    clear place for that thing ([summons](special-ops.md#where-and-how-it-arrives)), and a spot
    with none is dropped; other operations use the spot as drawn. A spot
    must lie on the map.
  - `FOG`: the centre of each region on the main island (the largest
    land island, [Regions and strength](#regions-and-strength)).
- `[GeneralRequirements]` (`no_friendly_actors`, `no_enemy_actors`,
  `no_owned_actors`, `requires_supply`) are read but never used: no
  candidate is ruled out by them. The operation's own rules still apply
  when its order is carried out (below). [code]

### Scoring a location

- A location's score is `base` + each `[Priority]` factor below. If any
  factor is below its own `minimum`, the location is ruled out. If the
  total is below the `[Priority]` `minimum`, it's ruled out; otherwise a
  random amount from 0 up to `fuzzy_factor` is added. An operation's
  best location is the one scoring highest, and it counts only if that
  score is above 0. [code]
- Each factor, with `maximum` a cap applied when it's 0 or more (a
  negative `maximum`, such as −1, means none), then `minimum` compared
  with the capped value: [code]
  - **`[CV]`** and **`[ActorValue]`**: each thing counted adds its
    combat value ([combat](combat.md#combat-value)) for `[CV]`, its
    worth ([Worth](#worth)) for `[ActorValue]`, × its multiplier. A
    thing's multiplier is that of a `[Category]` it matches (tested as
    for `[ActorRequirements]`, above), else the factor's own
    `multiplier`; a thing whose multiplier is 0 isn't counted. When a
    thing matches several categories, the one listed last wins. Then
    the cap, then the minimum, on the sum.
    - For `NEAR_ACTOR`, the things counted are those within the
      operation's own `area` of the spot ([what an operation
      reaches](special-ops.md#what-an-operation-reaches)), measured
      centre to centre here, without the edge allowance, from
      the sides that `consider_friendly` (own and allies),
      `consider_enemy` and `consider_owned` select (none: nothing
      counted). Seen or not, every thing in reach counts. An enemy
      thing's worth is reckoned for its own side.
    - For `ON_ACTOR`, only the target itself counts, the `consider_*`
      flags aside (a category's own side test still applies); its worth
      is reckoned for the computer player's side.
    - For `FOG` these two factors are skipped altogether: they add
      nothing and their `minimum` isn't tested.
  - **`[EnemyInfluence]`**: the enemy influence in the region the
    location is in (mobile and static, not border; [Regions and
    strength](#regions-and-strength)), raised to `exponent` (0 when
    there's none), × `multiplier`.
  - **`[InfluenceRatio]`**: friendly influence (its own and its allies';
    its own only with `count_owned_only`) ÷ enemy influence in that
    region, both mobile and static. With no enemy influence the ratio
    is the friendly influence × 1000 (0 if that's 0 too). A ratio above
    1 is raised to `exponent_over_one`; one of 1 or less to
    `exponent_under_one`; a ratio of 0 gives 0. Then × `multiplier`.
  - **`[TimeSinceExplored]`**: the seconds since its side last saw at
    least `ResetWhiteFogTimerPecent` (SVars, 10%) of that region; 0 while
    it sees that much ([Exploring](#exploring)). A region it hasn't
    seen that much of since the match began counts from the match's
    start (game time 0). × `multiplier`. [code] These times aren't
    kept in a saved game, but the game clock is: on loading, every
    region's time is 0 again, so a region its side then sees less than
    10% of counts from the match's start, however recently it was
    looked at before the save. [code]
  - **`[EconomyGenerated]`**: the listed `[Resource]` amounts valued the
    way a thing's economic worth values what it makes ([Worth](#worth):
    a need per point from the side's net income, a capped resource
    counted only up to its cap), × `multiplier`. It has no `maximum`.
  - **`[ActorEconomy]`** isn't read, so it never rules a location out
    (industrial sabotage goes on any enemy building that passes its
    requirements). [code]
- So a factor whose multiplier is 0 still rules out a location when its
  `minimum` is above 0. [code]

### What it orders

- For each operation that found a location it gives the same order a
  person's aimed use gives: on the target thing for `ON_ACTOR` (if it's
  still alive; else nothing), on the point for the others. The order is
  carried out as a person's would be ([using an
  operation](special-ops.md#using-an-operation)), with the same checks on readiness, cost
  and target, but a refused order shows no message and spends nothing.
  [code]
- Nothing else is done for any operation: Reorganization's template
  only sets where it looks and how it scores. [code, data]
- `[Followup]` would add an attack flare on the target (or point) of
  each use, but only when both `attack_flare_priority` and a positive
  `attack_flare_duration` are set; AA's data gives no duration, so none
  is placed. [code, data]

## Flares and allies

- Every goal looks at the flares for its side: team commands from allies
  ([team commands](team-commands.md)), flares placed by triggers, and its
  own. Attack flares feed attack, air attack and bombard goals; defend
  flares defend goals; build flares on a damaged thing repair and that
  unit's recover goal, other build flares build and construct goals;
  explore flares explore goals. [code]
- A flare adds its own priority if it has one, else the mood's
  `obey_flare_priority` (99999 in AA, so an ally's request all but
  commands it); trigger flares come at `SAITriggerTeamCommandPriority`
  (20000). Of several flares on one goal only the strongest counts, apart
  from additive ones, which all add. [code, data]
- A team-command attack flare on a thing gives allied computer players an
  attack goal on it, and a defend flare on a mobile thing a defend goal;
  a flare for one side reaches only that side's computer player. [code]
- A defend flare on a building, its own or an ally's, makes no goal of
  its own: it adds to the defend goal of the region the flare is in,
  like any defend flare there. That's where its own "defend it" flares
  on its buildings go ([Its own flares](#its-own-flares)). [code]
- Allies' strength counts towards its own in attack and defence
  ([How much strength](#how-much-strength-it-sends)), and it defends
  allies' things at 0.2 of its own. That's all its teamwork with other
  computer players: they don't plan together. [code]

### Its own flares

It flares for itself when (stock durations and priorities, which AA
doesn't change): [code, data]

| when | flare | seconds / priority |
|---|---|---|
| one of its buildings is attacked | defend it | 240 / 500 |
| one of its regiments beats an enemy regiment | attack there (additive) | 45 / 200 |
| one of its things is captured | attack it, to take it back | 90 / 500 |
| it captures a building, a settlement | defend it | 180 / 500, 750 |
| it finishes a building, a settlement | defend it | 120 / 500 |
| its units are bombarded | attack the bombarders | 90 / 1000 |
| an attack's way is blocked | attack the blocking region | 45 / ¼ of that attack's priority |

A new flare doesn't replace one that already lasts as long with at least
its priority, and pushes out weaker, shorter ones. [code]

## Learning

- `[Learning]` is used: from recent wins and losses it scales how much it
  likes recruiting a kind of unit (`recruit_*`), how strong it rates an
  enemy kind (`cv_*`, the combat value in its strength picture) and how
  much it wants to defend a region (`defend_*`). `learning_factor` moves
  each floor and ceiling towards 1 (0 switches learning off). [code;
  guess: the formulas, as the template gives them]

## What it knows and gets

- It gets nothing a person doesn't: no extra money, no cheaper or faster
  recruiting, building or research, no different limits or sight. The
  rules of play never ask whether a computer plays a side. [code]
- Much of what it does follows its side's fog: exploring by its own
  explored and seen ground; planning's targets (attacks, air attacks,
  bombardment, blocked ways) and the things special operations are aimed
  at are among what its side sees or remembers, a remembered target
  being attacked where it was last seen. "Sees" is the same test the
  side's own map uses. [code]
- But it also knows what its side can't see: [code]
  - its picture of strength ([Regions and
    strength](#regions-and-strength)) counts every thing on the map, so
    it always knows how strong the enemy is in each region; that steers
    where it defends, attacks, unpacks and lands, and how much it sends;
  - reacting picks targets near its goals among all enemies, seen or
    not ([Reacting](#reacting));
  - special operations aim round a remembered thing's real position,
    and weigh what's in reach whether seen or not ([Special
    operations](#special-operations));
  - exploring's intuition is drawn to enemy buildings hidden from it
    ([Exploring](#exploring)).
- Neither the handicap ([players](players.md#handicaps)) nor the
  mission's difficulty changes how it plays, and a profile's
  `difficulty_level` only matters when a profile is picked at random
  (every AA profile is hard). Its only knobs are in its data: thinking
  less often, `reaction_time` (0 in stock data), the order limits. [code,
  data]

## Switched off by a trigger

The effects are in [triggers](triggers.md#computer-control).

- With computer control off for its side, it neither plans nor reacts,
  but forgets nothing: its goals, choices and units stay as they were,
  and when control comes back on it carries on at its next think. Orders
  from its last think that haven't gone out yet still go, up to
  `command_max_total_delay` (5 seconds) later. [code; guess: whether
  that's ever seen]
- A thing with computer control off is let go from its goal and is
  neither given to a goal nor sent an order. Switched back on, it's just
  free, and a later think may use it. [code]

## Kohan II

- Kohan II's computer player plays by the same rules; the differences
  are in its data: [code, data]
  - its moods are built around recruiting companies (with
    `[RecruitPreference]`, `[CompanyTemplate]`), settlements and what to
    build in them (`[SettlementTemplate]`), research, wall strength
    (`[WallStrength]`) and heroes (`[Heroes]`), none of which AA's data
    uses;
  - its template adds `attack_camps_only_until_time`,
    `…_num_settlements` and `…_priority_multiplier`: early on it attacks
    nothing but camps [guess: as the names say, not traced];
  - its learning holds on longer (`cv_duration` 600, `cv_exponent` 1);
  - it has no air attacks (`UseAirAttacks` false) and stops once
    defeated (`DisableSAIWhenGameLost` true);
  - its profiles go by race, with an easy profile (`general_easy`, with
    its own easy mood) beside the hard ones, so in Kohan II a difficulty
    is a choice of profile.
- AA's World War II campaign map has a computer player of its own;
  that isn't traced.

## Unused data

Loaded but never read, so they change nothing: [code]
- `attack_allied_influence_cv_multiplier`,
  `defend_allied_influence_cv_multiplier`, `max_defense_depth`,
  `static_defensive_cv_mult`, `unit_retreat_cycles_til_reconsider`;
- the explore actor scores `move_rate_*`, `sight_*`, `subtract_cv`,
  `cv_mult`, `cv_exp`, `unit_inertia`;
- the special operations' `[GeneralRequirements]`.
Not loaded at all: `region_retreat_*`, `halt_retreat_cv_ratio` and the
special operations' `[ActorEconomy]`.

## Open questions

Ordered by what a first computer player needs.

- How an AA match actually opens: the order of the first recruits
  should be checked in the game. A computer player can only be added in
  the lobby, so it takes a skirmish started by hand; the game's
  `GoalPriorityCSV` user var (a file name, with `SAIDebug` on) then
  writes every goal's priority at each think, and
  `SinglePlayerAutoPause` off keeps it running unfocused. (analysis,
  2026-11-23)
- Kohan II's goals one by one, and the computer player of AA's World
  War II campaign map. (analysis, 2026-11-23)
