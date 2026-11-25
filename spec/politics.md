# Politics panel

The list of a match's sides with how the viewing player stands to each:
AA's politics panel (`PoliticsDisplay` in `UI/Game/politics_display.tgi`)
and Kohan II's politics page of its side panel (`SidePanelPolitics`).

Games: traced in AA, whose program also holds the Kohan II page's code.
Kohan II's own program wasn't checked.

## Opening and closing

- It's closed at the start of every match, like AA's other windows
  ([interface](interface.md#at-rest)). It opens only when the player asks:
  the control panel's politics button (`ButtonPolitics`, which runs
  `PoliticsDisplayToggle`), or F4, which presses that button
  (`hotkeys_custom.txt`). A second press closes it, and Backspace closes
  it too (`CloseSideOrDisplayPanel`). Screenshots that show it open are
  from matches where the player had opened it. [code, data]
- Unlike the objectives, technology and message log windows, it doesn't
  pause a single-player game, and it doesn't close or hide them either.
  It's a panel that stays up during play, sharing the screen with
  whatever else is open. [code]
- It opens and closes without a sound of its own: its layout gives it
  none. The button sounds as a toggle button does
  ([sounds](sounds.md)). [code, data]
- The button is a toggle button: each press flips it between lit and
  unlit, and the panel opens or closes with it. Nothing ties the two
  together otherwise, so closing the panel with Backspace leaves the
  button lit, and the next press unlights it and opens the panel.
  [code; guess: that F4 lights and unlights the button just as a
  click does]
- It's closed whenever a match ends. [code]

## Rows

- One row per side that a player holds in the match, person or
  computer. The map's independent sides get no row, and nor do slots that
  nobody took at launch. A player watching without a side gets no row of
  their own. [code]
- Rows come in side order (the map's `kingdom_0`, `kingdom_1` and so on),
  the same for every player: the viewer's own row isn't moved to the top.
  [code]
- A side knocked out of the match keeps its row, with its name written in
  grey (`UIColorDefeatedKingdom` in `UVars.tgi`, 0.5, 0.5, 0.5); its gem
  keeps its side's colour. A side that has won is written as usual.
  [code, data]
- A row reads: the gem in the side's colour, a space, the side's name in
  the row's own text colour, then the relation marker. That's the gem
  and name a message line gives a side ([messages](messages.md#names-in-lines)),
  and the name follows the same rules ([players](players.md#side-names)):
  the player's name for a side a player took ("Player", a nickname),
  "Kesselring AI" for a computer player, a " (1)" for a repeated name
  from the session's numbering, and in a campaign mission the map's own
  side names ("Kesselring's Force"). [code, data, game]
- The relation marker is how the viewing player stands to that row's
  side ([players](players.md#relations)): `glyph_ally` (`ch_ally`, a blue
  circle), `glyph_neutral` (`ch_neutral`) or `glyph_enemy` (`ch_enemy`, a
  red square). They're special glyphs from `Fonts/special_glyphs_AA.tgi`
  drawn in the row's text, so their pictures are the glyphs' own, not
  tinted. The viewer's own row has no marker. [code, data, game]
- "The viewing player" is the side the player is playing or, while
  observing, the side being viewed. With neither (watching without a
  side), no row has a marker or a tooltip. [code]
- A side is neutral to another only where the match sets it so, as the
  map's relations or the teams do; anything unset is an enemy
  ([players](players.md#relations)). [code; guess: that no shipped AA
  match has neutral sides]

## Keeping up to date

- While it's open, the panel checks every frame and follows any change at
  once: a relation changing, a side being knocked out (turning grey), a
  side being renamed (a computer player taking over a leaver's side
  becomes "Bob AI", [multiplayer](multiplayer.md)). A row appears or goes
  as sides join or leave the list, and the panel's height follows the
  number of rows. [code]
- A player who leaves a multiplayer match doesn't lose their side's row:
  the side stays in the match under its new name. [code, guess]

## Layout

All in layout units ([interface](interface.md#layout-units-and-placement)).
[code, data]

- The panel runs from x 800 to the screen's right edge, from the top of
  the screen down. Its height is `offset_top` + `offset_bottom` + the rows'
  heights + `separation` between each two rows: with AA's 20, 20, 3 and
  15-tall rows, two sides make it 20 + 15 + 3 + 15 + 20 = 73 tall. It
  grows downward as rows are added; the top stays put.
- Row n (from 0) starts `offset_top` + n × (row height + `separation`)
  below the panel's top. Each row is the `Label` in the layout: from 23 in
  from the panel's left, 188 wide and 15 tall, in the tiny font, left
  justified.
- The relation marker sits `offset_relation` (161) across from the start
  of the row's text, whatever the name's length, so the markers line up
  in a column.
- Behind the rows is the panel's background, sized to the whole panel:
  a tooltip-style frame (the tooltip box's corner, edge and centre
  textures, `SharedTooltipChildPanelVE`) over a black fill whose opacity
  is `transparency` (0 in AA, so no fill shows), clamped to 0..1.

## Tooltips and clicks

- Each row has its own tooltip, built for the viewer and that row's side.
  The layout's `politics_display_Label_tooltip` ("set by code") is never
  shown. [code, data]
  - The title: the side as the row shows it, gem then name.
  - If the side is in a team, a "Team" line with the team's name
    (`ui_tooltip_politics_team`).
  - A "Relation" line (`ui_tooltip_politics_relation`): "Self", "Ally",
    "Neutral" or "Enemy" (`ui_tooltip_politics_relation_self` and so on).
  - A blank line, then a description: "Your Side." for the viewer's own
    row (`ui_tooltip_politics_display_self`), and for the others
    `ui_tooltip_politics_display_ally_generic` ("Supply and visibility
    are shared with allied sides. ..."),
    `ui_tooltip_politics_display_neutral_generic` or
    `ui_tooltip_politics_display_enemy_generic`.
- Clicking a row does nothing in AA. [code]

## Kohan II's politics page

- It lists the same sides in the same order, each row written the same
  way (gem, name, then the relation marker at `offset_relation`, 150 in
  its layout), and keeps up to date the same way; so one model of rows
  fits both games. Its markers are `ch_shield`,
  `ch_sword` and `ch_eye` (ally, enemy, neutral). [code, data] The row
  tooltips are presumably the same too. [guess]
- What differs: its rows are items of a list box (`ListBoxKingdoms`, up
  to 16 visible, several selectable at once) on a side panel page rather
  than a panel of its own, and it adds a Send Tribute button
  (`ButtonSendTribute`). [code, data] It's enabled only while the page
  has a viewing player and something it needs is set. [code; guess: that
  it needs a side selected]
- Selecting a row of an allied side opens the command grid's team
  commands for that side, as choosing it on the ally page does ([team
  commands](team-commands.md)). [code]
- Send Tribute opens the same number box as Give Resources, with no
  text, capped by the player's own money, and on OK gives that amount of
  money to each selected side that is an ally, as Give Resources does.
  [code; guess: one gift per selected ally]

## Open questions

- What a team's name is in the tooltip's Team line, where the map or the
  lobby gives one. (analysis, 2026-11-22)
- Can anything change a relation during an AA match (a trigger effect,
  a player's choice), or are they fixed at launch? (analysis, 2026-11-22)
