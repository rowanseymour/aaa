# Team commands

What the command grid's Team Commands button opens: the choice of ally,
the ten team commands, the flares they place, giving and asking for
money and buildings, and the chat and economy report sent to allies.
When the button itself shows is in [commands](commands.md#a-buildings-state-and-its-commands).

Games: traced in AA. Kohan II runs the same code with the same ten
commands; its data and its computer allies weren't checked.

## The ally page

- Pressing Team Commands (`GoToTeamCommands`, key T) always shows a page
  for choosing whom the commands are for first, even with a single ally.
  The grid's buttons are replaced by: [code, data]
  - slot 0: all allies (`SelectTeamKingdomAlly`: the
    `IconAlliedTeamCommand.png` icon, tooltip "Opens the Team Commands
    menu for all allies."; key A, `SelectTeamKingdom`);
  - slots 1 to 10: one button per allied side, in the order the map
    lists its sides, each with the `SelectTeamKingdom` look (the
    `IconSelectTeam.tga` icon), its tooltip the side's name and its icon
    tinted the side's colour. Keys 1 to 9 press the buttons in slots 1
    to 9 (`SelectTeamKingdom N`). With more than ten allies the rest get
    no button;
  - slot 11: Cancel (`CancelTeamCommands`, "Cancels the current action.",
    key C).
- The sides offered are the other sides your side stands `ALLY` to
  ([players](players.md#relations)), computer and human alike, but never
  a map's independent sides, even ones allied to you: an independent ally
  makes the Team Commands button show but gets no button here. Whether
  an ally is still playing isn't checked. [code]
- Like other grid buttons, these show only their tooltip text, which
  carries the key marking ([commands](commands.md#hotkeys-in-tooltips)).
  [code]

## The team commands menu

- Choosing a side or all allies replaces the page with the team commands
  for them, each in the slot its `preferred_position` gives
  (`Game/team_commands.tgi`), and Cancel in slot 11: [code, data]

  | Slot | Entry (`IDS`) | Name | Key | Shown for |
  |---|---|---|---|---|
  | 0 | `team_attack` | Attack | A | all allies, any one ally |
  | 1 | `team_defend` | Defend | D | all allies, any one ally |
  | 2 | `team_build` | Build | B | all allies, any one ally |
  | 3 | `team_explore` | Explore | E | all allies, any one ally |
  | 4 | `team_give_resources` | Give Resources | R | one ally |
  | 5 | `team_give_actor` | Give Building | G | one ally |
  | 6 | `team_request_resources` | Request Resources | S | one ally |
  | 7 | `team_request_actor` | Request Building | Q | one ally |
  | 9 | `team_report` | Report | T | all allies, any one ally |
  | 10 | `team_chat` | Chat | S | all allies, one ally a person plays |

  Slot 8 stays empty. The keys are the standard set's
  (`hotkeys_standard_aa.txt`); each works only while its button shows.
- So with all allies the four giving and asking entries are left out, and
  with one computer-run ally Chat is left out: a computer ally gets nine
  entries, a human ally ten, all allies six. Entries are left out, never
  greyed, and nothing else (money, what you own) hides one. [code]
- Each entry's tooltip is its `name` on the first line and its
  `tooltip` below, as a command's is, and its icon is the entry's `icon`.
  [code, data]
- A data entry's `preferred_position` must be 0 to 11 and no two entries
  may share one; the game refuses to load otherwise. [code]
- Cancel (or C) goes back to the selection's commands, not to the ally
  page. [code]
- Both pages stay up while the selection stays the same. Selecting
  something else puts the new selection's commands back in place of
  either page; clearing the selection leaves the page as it is. [code]
  Once the team menu has given way to a selection that way, clearing the
  selection again brings the team menu back for the same allies. [code,
  guess]
- Using an entry goes back to the selection's commands as soon as it's
  done: after the click that places a flare or names a building (or is
  refused), after Chat opens the chat bar, after Report, and after the
  amount box is accepted. While the amount box is open the menu stays,
  and cancelling the box, or accepting it with nothing above 0, leaves
  the menu up. [code]

## Flares: Attack, Defend, Build, Explore

- These four arm the cursor (`cursor_ids = move` for each) and are given
  with a left click in the world, like a targeted command
  ([commands](commands.md#targeted-commands)). [code, data]
- What each accepts: [code]

  | Entry | A place | A thing | The thing must be |
  |---|---|---|---|
  | Attack | yes | yes | owned by a side that is your enemy |
  | Defend | yes | yes | yours or an ally's |
  | Build | yes | yes | yours or nobody's; or, whoever owns it, of the 33rd category listed in `Game/thing_categories.tgi` (`placeholder`) [guess: meant for markers] |
  | Explore | yes | no | |

  A thing counts only if it's `targetable`. Clicking a thing that fails
  posts "%s is not a valid target" (`ui_error_team_command_target_invalid`)
  at the thing, with its name, and ends the targeting; it doesn't fall
  back to the place beneath. A click on a place has to be on the map;
  one off it does nothing and the cursor stays armed. Explore ignores
  things and always takes the place. [code]
- The flare is a thing of its own, owned by the side that placed it:
  [code, data]

  | Entry | On a thing | On a place |
  |---|---|---|
  | Attack | `attack_actor_flare` ("Attack Target Flare") | `attack_location_flare` ("Attack Location Flare") |
  | Defend | `defend_actor_flare` ("Defend Target Flare") | `defend_location_flare` ("Defend Location Flare") |
  | Build | `build_actor_flare` ("Build Target Flare") | `build_location_flare` ("Build Location Flare") |
  | Explore | | `explore_flare` ("Explore Flare") |

  (`Flares/*.tgi`). It's for the ally chosen, or for all allies.
- It lasts its `[FlareComponent]` `default_duration` (180 seconds in
  every shipped flare), then goes. A flare on a thing sits over it at
  `actor_offset` (0, 0, 7) and follows it as it moves; it goes when the
  thing does, and when the thing no longer suits the flare's order (an
  attack flare's target no longer an enemy of the placer's, a defend
  flare's no longer an ally). A flare for one side goes when that side
  is gone. [code, data; guess: the order-suits test is the order's own
  target rule]
- A side keeps at most `MaxFlaresPerKingdom` (5, `Game/SVars.tgi`)
  flares: placing a sixth takes away one of the others, the oldest
  [guess]. [code, data]
- Who sees it: the side that placed it and the side or sides it's for.
  Anyone else, enemies included, never sees it. Of those, a flare on a
  place is always visible, fog or not; a flare on a thing is visible
  only while the thing is. [code]
- A flare for all allies is drawn in `all_ally_color` (white);
  which colour one for a single ally takes is unchecked. [data; guess]
- It can be selected (`flare_selection_vfx`, `flare_focus_vfx` on
  mouse-over) and its owner gets Cancel (`cancel_flare`, slot 11) to take
  it away ([commands](commands.md)). [data]
- On placing, the placer gets `my_flare_created` ("%s placed", the flare's
  name) or `my_flare_created_on_actor` ("%s placed on %s", the flare and
  the thing), and each player it's for gets `ally_flare_created` ("%s
  placed by %s", the flare and the placer's side) or
  `ally_flare_created_on_actor` ("%s placed on %s by %s"). All four blink
  `TeamCommandMiniMapIcon.png` on the minimap at the flare and play
  `team_command_sfx` (`Game/game_events.tgi`, [game
  events](game-events.md)); they're shown even where the player can't see
  the spot. [code, data]
- Computer allies treat a flare meant for them as a request: its
  `[TeamCommands]` `obey_flare_priority` (99999 in
  `Templates/template_sai.tgi`) is added to the priority of whatever
  serves it. The data's own notes say what that is: exploring the
  flare's region; the attack or defend goal at the place, and goals
  linked to it; for a thing, attacking or defending its region if it's a
  building and the thing itself if it moves; for Build, building at a
  marker, a defensive building on open ground, repairing a damaged
  building, upgrading a healthy one, building in a settlement, or
  recovering a damaged unit. [code, data; guess: the notes' list]
  Flares from triggers are the same flares with a priority of their own
  ([triggers](triggers.md)).

## Giving and asking for money

- Give Resources and Request Resources open a message box with a number
  box: text "Resource Amount" (`resource_transfer_dialog_name`), a
  number field starting at 0 and at most 10 characters, a "+25" and a "-25" button (`TeamCommandTransferResourceIncrementValue`,
  25), OK and Cancel. [code, data]
  - "+25" adds 25 but never goes above the cap: for Give, your money
    stockpile, rounded down; for Request, the ally's money stockpile,
    rounded down. "-25" subtracts 25, not below 0. Typing can enter any
    number. [code]
  - OK with an amount above 0 sends it; 0 or less sends nothing. Cancel
    closes the box. [code]
- The resource is money, the stockpiled one (`resources.tgi`). [code,
  data]
- Give: when it's carried out, the amount is cut to what you have, taken
  from your stockpile and added to the ally's, within its stockpile
  maximum as any stockpile is ([economy](economy.md)). You get
  `resources_given` ("%i%s given to %s", the amount, money's glyph, the
  ally's name), the ally `resources_received` ("%i%s received from %s",
  your name). Nothing is given to a side that has lost. [code, data]
- Request moves nothing. You and the ally both get
  `resource_request_received` ("%s is requesting %i%s", your side's name,
  the amount, money's glyph). A computer ally weighs it as below
  (Computer allies giving). [code, data]
- Neither puts anything in chat: "Please give me %d gold." and "Please
  give me %s." (`team_command_request_cash`, `team_command_request_actor`)
  are never used. [code, data]

## Giving and asking for buildings

- Give Building and Request Building arm the cursor (`move`) and take a
  thing; clicking a place does nothing and the cursor stays armed.
  [code, data]
- Give Building takes a thing of yours that is `tributable` (in the
  shipped data, buildings, `Templates/template_building*.tgi`). Refusals,
  posted at the thing with its name, end the targeting: [code, data]
  - "%s is not tributable" (`ui_error_tribute_not_tributable`): the
    thing isn't yours, isn't targetable or isn't tributable;
  - "%s is not currently tributable"
    (`ui_error_tribute_not_tributable_currently`): it's under siege,
    under construction, dead, upgrading or being razed, or it's the last
    tributable thing you have [guess: that the list counted is your
    buildings];
  - "Insufficient unit limit to tribute %s"
    (`ui_error_tribute_not_tributable_resource_production`): the
    building provides unit limit (`unit_limit_provided`) and giving it
    away would leave you, or the ally, using more unit limit
    (`unit_limit_consumed`) than provided. The regiments attached to it
    count with it. [code; guess: which things count as attached]
- With "Confirmation Dialogs" on (`UIMessageBoxConfirmation`), a valid
  click first asks "Tribute %s to %s" (`ui_message_tribute_actor_to_kingdom`,
  the building and the ally) with an OK button; otherwise it's sent at
  once. [code, data]
- When it's carried out, the building becomes the ally's, and you get
  `actor_given` ("%s given to %s"), the ally `actor_received` ("%s
  received from %s"). Nothing is given to a side that has lost. [code,
  data]
- Request Building takes a thing that belongs to the chosen ally (any
  targetable thing of theirs; tributability isn't checked). It moves
  nothing: you and the ally both get `actor_request_received` ("%s is
  requesting %s", your side's name and the thing). A computer ally weighs
  it as below. [code, data]

## Computer allies giving

A computer player decides by giving each of its possible goals a
priority and taking those that come out best; giving is one such goal.
The weights are the `[Give]` block of its AI data
(`Templates/template_sai.tgi`; no shipped AI file overrides them). In
what follows "stockpile" and "income" are of money, and income is money
produced less money spent on upkeep [guess]. How a gift is weighed
against its other goals is in [computer
players](computer-players.md#weighing-goals).

- Money: from the start of the match a computer player has a goal to
  give money to every other side that isn't independent. Its priority is
  0 unless that side counts the computer's side as an ally; otherwise it
  is [code, data]

  `resource_without_request_base_priority` (−200)
  + my stockpile × `resource_without_request_my_stockpile_mult` (1)
  + their stockpile × `resource_without_request_their_stockpile_mult` (−1)
  + my income × `resource_without_request_my_income_mult` (5)
  + their income × `resource_without_request_their_income_mult` (−5).

  So unasked, it only comes out above 0 when the computer has well over
  200 more money than the ally, or earns more. [code, data]
- Taken unasked, it gives (my stockpile − their stockpile) ×
  (`resource_without_request_give_amount_base_mult` (0.1) + a random
  fraction of `resource_without_request_give_amount_fuzzy_factor` (0.2)),
  rounded, never below 0. [code, data]
- Asked for an amount, the base becomes
  `resource_with_request_base_priority` (−100) and this is added:
  ((their stockpile − my stockpile) × (0.1 + 0.2 ÷ 2) − amount asked) ×
  `resource_with_request_amount_mult` (2). The data's own note says this
  term should use what the computer would give unasked (my stockpile
  less theirs); the game has the two stockpiles the other way round. So
  the term is above 0 only when 0.2 × (their stockpile − my stockpile)
  is more than the amount asked, which needs the side asking to be the
  richer one; otherwise it takes away, more the more is asked. Against the unasked goal, a request still gains the 100 of the
  higher base. [code, data]
- Taken after a request, it gives exactly the amount asked. Either way
  the gift is cut to the computer's own money, rounded down, and nothing
  is sent when that leaves 0 or the computer has no money. A gift goes
  out as a Give Resources of its own, with the same lines and checks.
  [code]
- A building: the goal exists only once someone has asked for that
  building, one goal per building and side asking. It lasts until the
  building is no longer the computer's or the side asking is gone. Its
  priority is 0 unless the side asking counts the computer's side as an
  ally; otherwise it is `actor_base_priority` (−100) + my stockpile ×
  `actor_my_stockpile_mult` (1) + their stockpile ×
  `actor_their_stockpile_mult` (−1) + my income × `actor_my_income_mult`
  (5) + their income × `actor_their_income_mult` (−5) + my building count
  × `actor_my_building_count_mult` (10) + their building count ×
  `actor_their_building_count_mult` (−10), plus the building's worth to
  it × `actor_building_value_mult` (0, so unused). [code, data; guess:
  which buildings the counts take in]
- Taken, it gives the building as Give Building does, if the building
  still passes Give Building's checks. [code]
- Goal inertia (the bonus an AI goal gets for being under way) never
  applies to a give goal (the data's note). [data]

## Chat and Report

- Chat opens the chat bar ([multiplayer](multiplayer.md#the-chat-bar))
  with its filter set: to Allies for all allies, to the ally's name for
  one ally. It shows for one ally only when a person plays that side.
  [code]
- Report sends your economy, the same line Espionage shows of an enemy's
  ("<side> Economy: " and the side's resource bar,
  `ability_effect_describe_economy`, [special
  operations](special-ops.md)), as a `GENERIC_EVENT` line: [code]
  - to one ally a person plays: that ally gets your report, and so do
    you;
  - to one computer-run ally: nothing is sent; instead you are shown that
    ally's report;
  - to all allies: you're shown the report of each computer-run ally, and
    if at least one ally is a person's, your report goes to you and to
    every ally.

## How they travel

- Placing a flare, giving, requesting and reporting are orders that go
  through the host's step like any other ([multiplayer](multiplayer.md)),
  so they're carried out on every machine at once; the checks above are
  made again then, and an order that fails them is dropped. [code]
- Only the host's machine thinks for computer players, so that's where
  their requests are noted ([multiplayer](multiplayer.md#computer-players)).
  [code]
- The data has four more entries commented out ("Removed Par Bug 2267":
  `team_attack_this`, `team_defend_this`, `team_give_this`,
  `team_request_this`, acting on the selection instead of a click). The
  game knows only the ten above: a data entry with any other `IDS` stops
  the data loading with an error, and the standard set's keys
  for two of them (U, I) do nothing. [code, data]

## Open questions

- Which colour a flare for a single ally is drawn in, and whether flares
  show on the minimap beyond the placing blink. (analysis, 2026-11-22)
- What a right click or Escape does while a team command's cursor is
  armed, and where the grid goes then. (analysis, 2026-11-22)
- With one human ally the menu shows both Request Resources and Chat,
  which share the key S in the standard set: which one S presses.
  (analysis, 2026-11-22)
