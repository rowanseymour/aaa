# Players

The sides (kingdoms) of a match: how two of them stand towards each other,
what a side is called, and the Easy / Medium / Hard handicap a side can
be given.

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Relations

- Each player stands to another as `SELF`, `ALLY`, `NEUTRAL` or `ENEMY`.
  [code]
- Relations are one-way: each player holds its own relation towards every
  other player and towards every team, and asking how A stands to B reads
  only A's side. A can be B's ally while B is A's enemy. [code]
- How A stands to B: [code]
  - `SELF` when they're the same player.
  - `NEUTRAL` when B has no player (things belonging to nobody); likewise
    a thing with no player stands `NEUTRAL` to everyone.
  - If A is in a team, the team's relation is used instead of A's own:
    towards B's team if B is in one, else towards B.
  - Otherwise A's own relation: towards B's team if B is in one, else
    towards B.
- Anything the map doesn't set is `ENEMY`: every player and team starts
  as an enemy of every other. [code]
- A team stands `ALLY` to itself: a newly made team is set so, and the
  stock team maps set it in their data ((2v2)Berlin's two teams are each
  allied to themselves and enemies of each other). [code, data]
- At launch each side a player takes joins the team of its lobby slot,
  so teammates are allies from the start. [code]

## Side names

- A side's name is what the selection panel's owner line and the message
  lines that name a side show (see [object display](object-display.md)
  and [messages](messages.md)). [code]
- Each side starts with the name the map's strings give it (`kingdom_N`,
  `independent_N`). [code, data]
- In a skirmish or multiplayer match (anything but a campaign mission),
  each side that a player takes at launch is renamed to that player's
  name, person or computer. Sides nobody takes, the independent ones
  among them, keep the map's name ("Civilian (Aggressive)"). [code]
- That is the player's name as the session holds it, number included, so
  a second Bob's side is "Bob (1)" (see Player names in
  [multiplayer](multiplayer.md)). A single-player skirmish is a session
  too, and the person's side there takes their LAN nickname, "Player" for
  someone who has never set one. [code, data]
- A computer player is named after its AI profile's `name` in `SAI/`: a
  Kesselring computer player is "Kesselring AI"
  (`Germany_Kesselring_SAI_kesselring_sai_name`), never the general's
  "Field Marshal Kesselring". So a computer side's lines and panel read
  "Kesselring AI". [code, data] A computer slot whose profile is left to
  chance shows "Random" (`staging_and_session_random_sai_name`) in the
  lobby until launch picks one (see Choosing a profile at launch in
  [multiplayer](multiplayer.md)). [code]
- A person renaming themselves during a skirmish or multiplayer match
  renames their side too ("%s changed name to %s",
  `networking_player_changed_name`). [code, guess]
- When a computer player takes over a side mid-match, it is named after
  the side with `sai_midgame_name_postfix` (" AI") added, so Bob's side
  becomes "Bob AI" ([multiplayer](multiplayer.md)). [code, data]
- In a campaign mission every side keeps the map's name. In a battle
  fought from the WWII campaign's map each side that a player takes is
  named from the campaign's own data for it rather than after the player.
  [code, guess]

## Handicaps

The Easy / Medium / Hard setting a side can be given in the custom-game
lobby (`Game/handicaps.tgi`): which sides have one, what it gives them at
the start of a match, and what that changes in play.

### Who has a handicap

- A handicap belongs to a side (a map player), not to whoever plays it.
  Whatever handicap a side holds when the match starts is applied to it,
  whether a person or the computer plays it. [code]
- The match uses the side's handicap as the lobby leaves it. The map's
  own per-player `handicap` (in MAP_HEADER) is only where that starts:
  the lobby's choice replaces it. A side the lobby never touches, an
  independent one included, keeps the map's value. [code]
- No shipped map sets one: the field is empty for every player in all 80
  maps, campaign maps included. So a side starts with no handicap unless
  the lobby gives it one, and campaign missions have none. [data]
- No handicap plays exactly like Medium, which gives nothing. [code, data]
- The lobby's handicap choice is meant for computer-played sides: the
  setting is named for the AI, and the stock descriptions all speak of
  "the AI" ("EASY: Gives a military and economic penalty to the AI.").
  It's offered only on slots the local player may change, only when
  handicaps are defined, and not in some game modes (the campaign's,
  probably). Whether a slot a person holds can be given one is unsettled.
  [code, data, guess]

### What a handicap gives

- A handicap (`[Handicap Template=Handicap]`) can give two things, both
  once, as the match starts: [code]
  - the technology its `property_ids` names, given to the side like a
    start technology;
  - its `ExtraResources`, added to the side's starting money (see
    [economy](economy.md#starting-money)).
- Stock handicaps: [data]
  - Easy (`handicap_easy`) gives `property_handicap_easy`: `RECRUIT_COST`
    and `health` each multiplied by −0.25.
  - Medium (`handicap_medium`) gives nothing.
  - Hard (`handicap_hard`) gives `property_handicap_hard`: `RECRUIT_COST`
    and `health` each multiplied by +0.25.
  - None has `ExtraResources`.
- A handicap does nothing else. It isn't consulted after the start: not by
  the computer player (its aggression, what it builds, any cheating), not
  for production, upkeep, sight or anything else. How hard the computer
  plays is the separate Difficulty option. [code]
- The technology is given only if the side doesn't already hold it. A
  restriction group that bars it from the side keeps it from being given
  (see [research](research.md#scenario-restrictions)); no shipped group
  does. [code, data]
- The handicap technologies have no `required_properties`, so they apply
  to every thing the side owns that has a `[PropertiesComponent]`, which
  in the stock data is every unit, vehicle, truck, regiment and building.
  [code, data]

### What the multipliers do

- A multiplier of amount a makes the value × (1 + a), added together
  with any other multipliers of the same attribute the thing has, not
  compounded, and never below `ModifierAttributeClampMultiply` (0.05).
  So Easy is × 0.75 and Hard × 1.25; Easy with a +0.10 health technology
  is × 0.85. [code]
- Attribute names are matched without regard to case: the data's
  `health` is `HEALTH`. [code, game]
- `RECRUIT_COST` scales what recruiting costs (see
  [economy](economy.md#paying-for-commissions)): regiments, Division HQ
  and other trucks, and anything else a building or HQ recruits. It
  doesn't touch: [code, data]
  - things built in place by a builder, such as bunkers and supply depots,
    which go by `STRUCTURE_COST` instead;
  - research costs;
  - upkeep, production, or the regiment and building limits a thing uses.
  Shipped costs are all money, so only money is affected. [data]
- `HEALTH` scales the maximum health of every unit, vehicle and building
  the side owns (in a regiment, each man and vehicle). [code]
  - Things the side already has when the handicap is given, the starting
    forces and buildings, keep the share of health they had: at full
    health they're at full scaled health. [code]
  - Things that join the side later (recruited, unpacked, captured, given)
    take on its technologies as they join, and keep their share of health
    the same way, so a new unit starts at full scaled health. [code]
  - A thing that leaves the side, captured by another say, loses the
    multiplier, keeping its share of health again. [guess]
- So on Easy a side pays three quarters for its recruits but its things
  have three quarters of their health; on Hard it pays a quarter more but
  everything is a quarter tougher. That cost goes the other way to the
  descriptions, which call Easy a penalty and Hard a bonus in both.
  [data]

### Seeing it

- The side gets "Easy technology gained" or "Hard technology gained" in
  its message log as the match starts (see
  [messages](messages.md)); a computer side's message reaches no one.
  [code, data]
- The technology display lists it as a technology named Easy or Hard,
  with the economic icon (`IconPropertyEconomic`); see
  [object display](object-display.md#technology-display). [code, data]
- Costs shown to the side's player, in tooltips and in "Insufficient funds
  for X. Requires Y.", are the scaled ones, rounded up to a whole number.
  [code]

### Losing it

- Nothing in normal play takes it away. Only the
  `kingdom_loses_technology` trigger effect naming it would, and no
  shipped map does. [code, data]
- A saved game keeps it; it isn't given again on loading. [guess]

## Open questions

- What makes a player independent, as the `independent` target type in
  [combat](combat.md) uses it? (analysis, 2026-11-08)
- Can the lobby give a handicap to a slot a person holds, and in which
  game modes is the setting offered? (analysis, 2026-11-12)
- What does the lobby show for a computer side whose map value is empty,
  and what does a new computer slot start at? (analysis, 2026-11-12)
