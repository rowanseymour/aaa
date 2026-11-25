# Objectives

A side's mission goals: where a map defines them, how its triggers give,
settle and take them away, the lines a side without any is shown, the
Objectives window, and the debrief's list.

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Where objectives come from

- A map defines its objectives in its OBJECTIVES section: each has a name
  (`objective_3`), a text id and whether it's primary (the editor's
  "Required" box) or secondary. Shipped maps use the name as the text
  id. [data]
- The text is the map's string of that id, from the map's own string
  table (`objective_3 = "4. Capture the port of Suda."` in
  `axis01_crete.TGI`). [code, data]
- Numbering is part of the text: the map's author writes "1.", "2." and
  so on into each string. Nothing in the game numbers objectives, and the
  name's number has nothing to do with it (Kursk's `objective_8` reads
  "1. Kursk must not fall."). [data]
- Defining an objective gives it to nobody. A side has objectives only as
  the map's triggers give them, so the window shows nothing of a map's
  objectives until they're given. [code]
- The objective list the map header keeps for each kingdom (filled in
  some developer and trailer maps) gives nothing at the start of a match:
  only the triggers below do. [code, data]

## Giving, settling and taking away

Objectives belong to a side (a kingdom). Each side has its own list, and
each entry is an objective in one of three states: active, completed or
failed. Teammates don't share lists. [code]

- `kingdom_gains_objective` (kingdom, objective) adds the objective to
  the side's list, active. [code]
  - The list is kept in alphabetical order of the objectives' texts, so
    the authors' "1.", "2." prefixes put them in order however they're
    given. A new one goes in before the first entry whose text sorts the
    same or later. This is the `SortObjectives` game variable (`true` in
    `SVars_AA.tgi`); with it false, new ones go at the end. Kohan II's
    data doesn't set it, so there it's false (the game's default) and
    objectives are listed in the order they're given. [code, data]
  - Giving an objective the side already holds adds it again, so it shows
    twice. [code]
  - An objective name the map doesn't define does nothing. [code]
- `objective_complete_for_kingdom` sets it completed,
  `objective_failed_for_kingdom` failed, and
  `objective_incomplete_for_kingdom` back to active. These only change an
  objective the side holds: on one it hasn't been given they do nothing,
  post no message and don't flash the button. [code]
- Any state can go to any other, again and again: Crete sets its
  airfields completed when captured and active again when the enemy
  takes them back. [code, data]
- `kingdom_loses_objective` takes the objective off the side's list (the
  first entry of that name, if it was given twice). It vanishes from the
  window: nothing is struck through or left behind. [code]
- The conditions `objective_succeeded` and `objective_failed` (objective,
  kingdom) are true while the side holds the objective in that state;
  for one it doesn't hold, both are false. [code]
- Each change posts a game event for the side, with the objective's text
  as its `%s`, and so a message-log line and sound
  ([messages](messages.md), [game events](game-events.md)): [code, data]

  | Change | Primary | Secondary |
  |---|---|---|
  | given | `display_when_loading_primary_objective` | `display_when_loading_secondary_objective` |
  | set active again | the same "gained" events | |
  | completed | `complete_primary_objective` | `complete_secondary_objective` |
  | failed | `failed_primary_objective` | `failed_secondary_objective` |
  | taken away | `remove_primary_objective` (no line, no sound) | `remove_secondary_objective` |

  So setting an objective active again reads "Primary Objective gained:
  ..." and plays `objective_gained` once more. [code, data]
- Each change that took (all five effects) also flashes the control
  panel's objectives button, as the `flash_ui_button` effect does. [code]
- Objectives don't end a mission by themselves: the map's triggers decide
  victory and defeat (`kingdom_mission_won`, `kingdom_mission_lost`),
  usually by testing `objective_succeeded`. What those do, and every
  other way a match ends, is in [match outcome](outcome.md). [code, data]
- A saved game keeps each side's list, in order, with each entry's
  state. [code, data]

## Matches without given objectives

- Skirmish and multiplayer maps define a few objectives (left over from
  their campaign versions: Berlin's "Capture Berlin.", Kursk's three) but
  no trigger gives them, so in practice they never show. Random maps
  have none. [data]
- A side's list in the window can carry two lines that aren't objectives
  of the map, both primary: [code, data]
  - When the `CityVictoryRatio` world rule (lobby "City Control %",
    0 to 100 in steps of 10, default 0) is above 0 and the map has
    cities: "Control %d cities (%d/%d)" (`objective_city_ratio|ddd`), with
    the cities needed (the rule's percentage of the map's cities, rounded
    up), the cities the side's team holds (the side's own if it has no
    team) and the map's total. It shows completed once the side has won
    by holding that share, active until then. It's always the first
    line.
  - When the side holds no objectives and there's no city line: "Defeat
    all enemies" (`objective_defeat_all_enemies`), always active, never
    ticked.
- So a skirmish or custom game shows "Defeat all enemies" (or the city
  line), and so does a campaign mission in the seconds before its
  triggers hand out the first objectives. The window is never empty.
  [code, data]
- These two lines exist only in the window: no messages, no flashing,
  and the debrief doesn't list them. [code]

## The Objectives window

Laid out by `UI/Game/objectives_display.tgi` (`ObjectivesDisplay`), the
same kind of window as the Technology Panel
([object display](object-display.md#technology-display)).

- The control panel's `ButtonObjectives` opens it: a push button, the
  first of the row of four tabs (838, 40 and 45 by 52 on the layout),
  drawn with the `SidePanelTabObjectives*` images. Its tooltip name is
  "Display Objectives" (`control_panel_ButtonObjectives_ToolTip`) and its
  text "Displays a list of mission objectives." (`..._tooltip_2`). F1
  presses it (`hotkeys_custom.txt`), so the name shows as "(F1)Display
  Objectives". [data, code]
- 385 by 513 at 321, 53 on the 1024 by 768 layout, titled "Objectives"
  (`objectives_display_Child_text`). Below the
  title, the "Primary" heading (`objectives_display_Child_text_2`, tiny
  font, at 43, 73) over a list box (13, 88, 362 by 179), then the
  "Secondary" heading (`..._text_3`, at 43, 265) over a second list box
  (13, 280, same size). Each box scrolls on its own. A close button at
  the top right and a "Close" button (`..._SecondaryButtonClose_...`) at
  the bottom. [data, game]
- Primary objectives go in the top box and secondary ones in the bottom,
  each in the side's list order. [code]
- Each line (`ObjectiveWidget`): a 20 by 20 box at x 14, the text at x 37
  in the tiny font, 290 wide and wrapping onto more lines when it's long.
  A line is as tall as its text, and never under 20. Lines stack from 15
  below the top of their box (`vertical_offset`) with `separation` (2)
  between them. [code, data]
- Colour and box by state: [code, data, game]

  | State | Text (`text_color_*`) | Box (`icon_*`) |
  |---|---|---|
  | active primary | `required`: white | `Unselected.png`: empty |
  | active secondary | `optional`: yellow | `Unselected.png`: empty |
  | completed (either) | `completed`: green | `Selected.png`: ticked |
  | failed (either) | `failed`: red | `failed.png`: red cross |

  So the yellow in the Crete shot only means secondary and not yet
  settled. No other colours appear.
- The list shows the side the player is viewing: their own side, or,
  when watching, the one being watched. It refreshes at least once a
  second and whenever the window opens, so it changes as triggers fire
  while it's open. [code]
- Opening and closing work as for the Technology Panel: pressing the
  button (or F1) again closes it; so do the close button, the "Close"
  button, F1 or Esc while it has the keyboard, and Backspace, which
  closes it together with the other display panels. [code, data]
- Opening it in a single-player game pauses the game unless it's already
  paused, and closing it unpauses only if opening it paused it. In a
  multiplayer game, hosted or joined, nothing pauses. The details (pausing
  while it's up, other windows, keys) are in
  [interface](interface.md#windows-over-the-match). [code]
- Only the button press sounds (`push_button`); opening and closing the
  window make no sound of their own. [code]
- It can be open together with the Technology Panel and the Message Log
  page; opening one doesn't close the others
  ([object display](object-display.md#technology-display)). [code]
- Clicking the icon of an objective line in the message log opens it
  (`ui_tooltip_message_quest`, "Open objectives panel";
  [messages](messages.md)). [code, data]

## Mission debrief

- The debrief's "Primary Objectives" and "Secondary Objectives" boxes
  (`UI/Menus/debriefing.tgi`) list the player's side's objectives as they
  stand when the mission ends: the same list as the window, in the same
  order, primaries in one box and secondaries in the other. [code, data]
- Objectives taken away aren't listed, nor are "Defeat all enemies" and
  the city line. [code]
- Each is its text after a status box: `Selected.png` (ticked) if
  completed, `Failed.png` (red cross) otherwise, so one still active at
  the end shows as failed. Text is in the panel's own colour, not the
  window's state colours. [code, data]

## Open questions

- Loading a saved game gives each side its saved objectives again the
  same way a trigger does, which would post a "gained" line and sound for
  each; does the player see and hear these after loading? (analysis,
  2026-11-13)
- How long, and how, does the objectives button flash after a change,
  and does opening the window stop it? (analysis, 2026-11-13)
- Does clicking an objective line's icon while the window is open close
  it, as the button does? (analysis, 2026-11-13)
- Does AA's lobby offer the City Control % rule? It's defined, but none
  of the lobby screenshots shows it. (analysis, 2026-11-13)
