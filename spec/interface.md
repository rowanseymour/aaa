# Interface

The in-match screen as the games' data lays it out: which panels make it
up, what each is placed within, when each is on screen, how the layout
fits the window, and which images it draws with (nation skins and the
resolution sets). What the panels show is in the subject specs:
[object display](object-display.md), [commands](commands.md),
[messages](messages.md), [economy](economy.md) (the resource bar's cells),
[objectives](objectives.md).

Games: traced in AA, whose program also holds Kohan II's layout choices
(they're switched by data flags, below). Kohan II's own program was
checked for its side panel only.

## Layout units and placement

- Every position in the interface data is in a 1024 by 768 layout, set
  by `InterfacePositioningWidth` and `InterfacePositioningHeight` in
  `AVars.tgi` (1024 and 768 in both games). [code, data]
- A widget's `L`, `T`, `R` and `B` are measured within its parent's
  bounds: a plain number from the parent's left or top edge, `Nr` and `Nb`
  that far in from its right or bottom edge, `N%` that fraction of its
  width or height, and `+N` for `R` or `B` that far past `L` or `T`.
  [data, game]
- So a `b` or `r` value only looks screen-relative when the parent
  reaches the screen's bottom or right edge, as the control panel does:
  AA's `ObjectDisplay` at `T = 192b` sits 192 above the control panel's
  bottom, which is the screen's, at 576; its `GameMiniMap` (`T = 42`)
  and `ActionButtons` (`T = 108`) count down from the control panel's top
  at 511. [data, game]
- The layout is stretched to the window: x scales by the window's width
  over 1024 and y by its height over 768, independently. Nothing is
  re-laid out for another size or shape. [code]
- So the game runs at any aspect ratio, but everything is drawn for 4:3:
  in a wider window the interface and the map view (below) are both the
  4:3 picture stretched sideways, never widened to show more. Nothing
  is letterboxed or pillarboxed. [code, game]
- The options' resolution list offers every display mode the system
  reports that is at least `MinimumGameResolutionX` (800) wide, as
  width x height x colour depth (`1280x720x32`), whatever its shape. Only
  with `Requires43Resolutions` (false in both games' `AVars.tgi`) does it
  keep just the 4:3 ones, so a widescreen display offers wide modes.
  [code, data, game]
- `UI/resolution.tgi` lists 4:3 sizes only (800x600, 1024x768,
  1280x960), but it isn't the options' list. [data, code]

## The map view

AA traced; Kohan II unchecked.

- The 3D map is drawn over the whole window, every frame, and the
  interface is drawn over it. Nothing cuts the view off above the
  control panel: the panel covers the map where its images are opaque,
  and the map shows through wherever they aren't, as in the gaps of its
  upper fringe around the minimap ring and the top bars. [code, game]
- The camera's picture is framed on the whole window, not on the part
  above the panel. Its projection is centred (not off-centre) with a
  fixed 4:3 shape: `CameraFOV` (15 degrees) is the full horizontal
  angle, and the vertical angle is the one that gives 4:3,
  2·atan(0.75·tan 7.5°), about 11.3 degrees. [code]
- So the point the camera looks at is the middle of the whole screen
  (512, 384 on the 1024 by 768 layout), 127 above the control panel's
  top edge, not the middle of the visible map above it (about y 256).
  Centring the camera on a place (a unit, a recalled group) puts that
  place there, and the match starts with the start position there.
  [code, game]
- None of the other interface pieces change the framing: the resource
  bar, the floaters down the left and the opened windows are drawn over
  the map like the control panel. [code]
- Other window sizes: the picture is always the 4:3 one stretched to the
  window, like the layout. At 800x600 or 1280x960 it's the same picture,
  scaled; in a wider window (16:9) everything in the map is stretched
  sideways, about a third wider than true at 16:9. The `UI/800` and
  `UI/1280` sets swap images only and don't affect the view. [code, game]

### Pointer and the view

- Clicking or hovering on the map picks what's drawn under the pointer,
  through the same whole-window projection, wherever on screen it is
  and no interface piece is there to take it. [code]
- Where the pointer goes to the interface instead: a widget takes it
  within its bounds. The control panel's view is marked `transparent`,
  which makes it take the pointer only within its client area
  (`client_T = 210b`, from y 558 down), not its whole bounds (from 511).
  In the band above it the map gets the pointer, even over the panel's
  own pieces: the minimap and the buttons take it only where they lie
  inside that client area. [code; guess: whether that leaves the top
  few rows of the minimap and the top buttons dead in play]
- A box-drag selection runs from where the button went down to where the
  pointer is, in screen terms; its corners are carried onto the map
  through the whole-window projection, and it's neither clipped to nor
  stopped by the control panel. [code]
- Edge scrolling starts only with the pointer on the window's outermost
  row or column of pixels (it can't go further), at any edge, the bottom
  one under the control panel included. [code]
- `CameraScrollEdgePadding` (10) isn't the width of an edge-scroll zone:
  it keeps the point the camera looks at inside the map, half of it in
  from each edge. [code; guess: in the map's own units]
- The minimap's outline of the camera's view is a trapezoid. With the
  match's start position in the middle of the screen, it sits about in
  the middle of the outline, which fits the outline being the whole
  window's view. [game; guess]

## What the screen is made of

The screen has one full-screen layer for the match (`Game` in
`UI/Game/game_interface.tgi`), and every widget below is placed within it
unless it's listed under the control panel. Each comes from the file and
top-level name given, and sits at its own bounds against the 1024 by 768
screen. [code]

Which optional pieces exist is set by flags in `AVars.tgi` (with each
game's own file, `AVars_AA.tgi` or `AVars_K2.tgi`, overriding): [code,
data]

| Flag | AA | Kohan II | Gives |
| --- | --- | --- | --- |
| `UIUseSidePanel` | false | true | Kohan II's side panel and its tabs, in place of AA's separate windows and their buttons |
| `UIUseEconomicImpact` | true | true | the preview resource bar |
| `UIUseRecruitItemFloater` | false | true | the recruit column (`RecruitItemFloater`) |
| `UIShowFactionExperience` | true | false | the commander experience label (`FactionExperience`) |
| `UIUsePauseButton` | true | true | the control panel's pause button |

The widgets placed directly in the screen layer for the whole match: [code]

- `EconomyBar` and `EconomicImpact` (`economy_bar.tgi`): the resource bar
  and its preview.
- `GameMessageLog` (`message_log.tgi`): the message lines on screen
  ([messages](messages.md)).
- `FactionExperience` (AA) and `UpkeepShortageIcon`, top-level in
  `game_interface.tgi`.
- `GroupFloater`, `FactionAbilitiesFloater` and Kohan II's
  `RecruitItemFloater` (`floaters.tgi`).
- `IdleButtonBuilder` and `IdleButtonDetachedOrganization`
  (`idle_buttons.tgi`).
- `KingdomDropDown` (`game_interface.tgi`), `TimeMessage`
  (`time_message.tgi`), `DeveloperModeLabel`
  (`UI/Shared/dev_mode_label.tgi`) and `CameraInformationLabel`
  (`UI/Shared/camera_info_label.tgi`).
- `DisplayBannerPanel` and `DisplayPause` (`UI/Shared/display_banner.tgi`).
- `Subtitles`, `ControlPanelTop` (children of `Game` in
  `game_interface.tgi`) and `ControlPanel` (`control_panel.tgi`).
- Kohan II's `SidePanel` (`side_panel.tgi`).

Opened on demand, each placed against the whole screen: [code]

- AA only, each opened and closed by its control panel button or key
  (the `ObjectivesDisplayToggle`, `TechnologyDisplayToggle`,
  `PoliticsDisplayToggle` and `MessageLogToggle` commands; a second press
  closes it): `ObjectivesDisplay`, `TechnologyDisplay` and
  `PoliticsDisplay` from their files, and `SideMessageLogPanel` (a child
  of `Game`), the whole message log in a window. The politics panel is
  in [politics](politics.md).
- `GameChatPanel` (`chat_panel.tgi`, `visible = false`): the chat line,
  opened by the chat button or key.
- `Counter` (`time_message.tgi`): a trigger's countdown, on screen only
  while one is set ([the time labels](#the-time-labels)). [code]
- `GameMenuResignDialog` (`game_menu.tgi`), and the game menu itself.
- `FavoritesDisplay` (`favorites_display.tgi`) is made with the control
  panel but isn't on screen at the start.
- `SAIDebug` (`SAIDebug.tgi`) is made but never put on screen in normal
  play.
- Kohan II's `CityManagement`, objectives and politics are pages of its
  side panel, below, not windows.

### The control panel

- `ControlPanel` spans the screen's width and runs from `T = 257b` (AA,
  y 511) or `256b` (Kohan II, y 512) to the bottom. [data]
- Behind everything in it, the panel draws
  `/UI/%s/Game/ControlPanel/Background.tga` from 170 above the screen's
  bottom down to it, the full width. The object display and the command
  grid draw no backdrop of their own, so wherever they draw nothing this
  image shows. [data, game]
- Each AA skin's `Background.tga` has the nation's emblem painted into
  the object display's area: a stencilled star over "U.S.A." (USA), an
  RAF roundel (GBR), a red star (RUS), an embossed "Wehrmacht" plate
  (GER); Japan's has none. It shows whole with nothing selected
  (`menu-pause.jpg`) and in part around whatever the selection's panel
  draws (`ger-corps-hq-weapon-armour.jpg`). [data, game]
- Placed within it: `ObjectDisplay` (`object_display.tgi`),
  `ActionButtons` (`action_buttons.tgi`), Kohan II's `SidePanelTabs`, the
  buttons defined in `control_panel.tgi`, and last `GameMiniMap`
  (`minimap.tgi`). [code]
- Its buttons: the menu, supply border, speed down and up, chat, pause
  (with `UIUsePauseButton`), and in AA objectives, technology, politics
  and message log. AA's data also defines `SidePanelTabs`, which AA never
  shows. [code, data]
- So Kohan II's `ActionButtons` at `L = 800`, `T = 154b` sits at y 614,
  counted from the screen's bottom through the control panel's. [data]

### At rest

What's on screen when the match starts and nothing has been touched:
[code, game]

- The control panel with the minimap, the command grid (empty with
  nothing selected but for the team commands) and the object display
  (with nothing selected it draws nothing, so the background's emblem
  shows); Kohan II's side panel
  tabs. Nothing is selected for the player at the start, so the general
  doesn't show or talk until the player selects the Corps HQ
  ([object display](object-display.md#animation)). [code]
- The resource bar, the message lines as they arrive, the group strip
  along the top (`GroupFloater`, [selection](selection.md#the-group-strip)) and the ability column down the left
  (`FactionAbilitiesFloater`), AA's commander experience at the top left,
  Kohan II's recruit column.
- `ControlPanelTop`: in AA it has no image and no size, so it shows
  nothing; in Kohan II it draws `UI/%s/Game/ControlPanel/Backgroundtop.png`
  at 101, 480, 64 by 32. [data]
- Hidden at the start: `DisplayBannerPanel` (`visible = false`; a
  trigger's banner text), `DisplayPause` (while the game is paused, see
  below) and `Subtitles` (while a line of speech is subtitled). [code;
  guess: when the other two show]
- `UpkeepShortageIcon` shows only while the player's side is short of
  upkeep ([economy](economy.md)); it's checked as the match comes up.
  [code]
- No side window is open: AA's four windows are closed whenever a match
  ends, and Kohan II's side panel shows no page. [code]
- `TimeMessage` shows the elapsed game time with the Elapsed Game Time
  option ([the time labels](#the-time-labels)). [code, data]
- The idle buttons show while the side has an idle truck or engineer
  ([selection](selection.md#cycling-idle-trucks-and-engineers)) or a
  detached regiment an HQ of yours could take
  ([supply](supply.toml#what-the-player-sees)), each re-checked about once
  a second. Kohan II's detached button is never seen. [code]
- `KingdomDropDown` (with its `observer_text`) is for observing other
  sides and isn't seen in normal play. [guess]
- `DeveloperModeLabel` and `CameraInformationLabel` are developer
  readouts, not seen in normal play. [guess]

### While paused

The same in both games: [code, data]

- `DisplayPause` (red, large, centred, "#display_banner_Label_text_2")
  blinks while the game is paused: it is simply shown or hidden, with no
  fade, change of colour or size, flipping every second (one second on,
  one off). The second is real time, so it blinks at the same rate
  whatever the game speed.
- It goes with the game's paused state alone, whatever paused it: the
  pause key or button, `gamepause`, another player's pause in
  multiplayer, or one of AA's windows (objectives, technology, message
  log; not the politics panel) pausing a single-player game as it opens.
- While the game menu is open it's hidden, paused or not. Opening the
  game menu in a single-player game pauses it (unless already paused;
  closing it unpauses only if opening it paused it, as the windows do),
  so in single player the menu shows over a darkened view
  ([lighting](lighting.md)) with no label. The menu's own paused line
  is blank in AA.
- It's hidden at once when the game unpauses. As the game pauses, or as
  the menu closes over a paused game, it normally appears at once
  (within a second at most) and then blinks.
- Nothing else marks a pause: no tint over the screen beyond the
  darkened light, and no interface sound of its own. Opening and
  closing AA's windows make no sound either. In multiplayer, pausing
  posts its line in the message log ([multiplayer](multiplayer.md)).
- A single-player game that stops because the game window lost focus
  (`SinglePlayerAutoPause` in `UVars.tgi`) doesn't count as paused:
  game time stands still, but there's no label and no darkening.
  [guess: that this is what that setting does]

### Windows over the match

What happens to the match while one of the in-match windows is up: the
objectives, technology and message log windows, the politics panel, the
game menu, and the options opened from it.

- Which games pause at all: only a match that isn't a multiplayer game.
  A match counts as multiplayer when it was set up as one, hosted on this
  machine or joined on another's, and a game hosted for multiplayer
  counts even when every other side is a computer player. Skirmishes and
  campaign missions started from the single-player screens are not
  multiplayer. [code]
- Single player: [code]
  - Opening the objectives, technology or message log window, or the
    game menu, pauses the match if it's running. Each remembers whether
    its own opening was what paused it.
  - Closing one unpauses the match only if its own opening paused it. So
    a match the player had already paused before opening a window stays
    paused when the window closes.
  - The remembered pause belongs to the window alone: with two windows
    open, the one opened first holds it, and closing that one unpauses
    the match while the other is still up (the other closes later
    without touching the pause).
  - The player can still pause and unpause with the pause key or button
    while one of the windows (not the game menu) is up: pausing is the
    ordinary toggle, and the window is not told. So if the player
    unpauses while a window that paused the match is up, the match runs
    behind it, and closing the window unpauses nothing further. If they
    then pause again before closing it, closing it unpauses the match:
    the window still remembers that it paused. And if the match was
    already paused when the window opened and the player unpauses while
    it's up, closing it leaves the match running. [code; guess: that the
    pause key reaches the match while one of these windows is up, see
    keys below]
  - The politics panel never pauses or unpauses anything, in any game:
    it's a panel that stays up during play.
  - The options are opened from the game menu's Options button and come
    up over the menu, which stays open beneath them. The options don't
    pause or unpause anything themselves: the match stays as the game
    menu left it (paused, in single player) until the game menu closes.
- Multiplayer, hosting: opening any of these windows, the game menu or
  the options pauses nothing, for anyone. The match runs on, for the host
  as for everyone else. Since nothing pauses, nothing is posted, no
  "paused" line or vote line appears to the others, and nothing counts
  towards the pause rules in [multiplayer](multiplayer.md) (the 5-minute
  refusal after being voted down, the unpause votes). The host pauses
  for everyone only by the pause key or button, under those rules. [code]
- Multiplayer, joined: the same. Opening a window pauses nothing, locally
  or for anyone; the match carries on behind it at full speed. There's no
  local-only pause in a multiplayer game. [code]
- The windows are the same in all of this, apart from the politics panel
  (never pauses) and the options (rely on the game menu's pause). [code]
- Keys and orders while a window is up: [code; guess: the parts noted]
  - The objectives, technology and message log windows and the politics
    panel don't take over the keyboard or the map. The map's keys keep
    working while one is up: the pause key, game speed, the chat line
    (Enter), cycling the message lines (Space), camera keys, and the
    control panel's keys such as F1 to F4, which open and close the
    windows. Backspace closes them all. [guess: that the map takes
    clicks outside the window as it does with no window open]
  - So in a multiplayer game the player can go on giving orders with a
    window up, and they reach the match as usual. In single player the
    match is paused by the window, and orders given then are as orders
    given in any pause.
  - Esc opens the game menu, and with the menu open Esc returns to the
    game (`hotkeys_ui.txt`). [data]

### The time labels

`TimeMessage` and `Counter` (`time_message.tgi`) are the same kind of
label, one counting up, the other down. [code]

- Both write a time as hours, minutes and seconds separated by colons:
  hours with no leading zero and always shown, even under an hour;
  minutes and seconds two digits each. So 12 minutes 34 seconds is
  `0:12:34`, and 25 hours is `25:00:00`. Fractions of a second are
  dropped, not rounded. [code]
- Both count game time in seconds: it runs faster at higher game speeds,
  stands still while the game is paused (or stopped because the window
  lost focus), and moves smoothly between the game's steps rather than
  in jumps. [code]
- `TimeMessage` shows the game time since the match began: `0:00:00`
  as a new match starts. [code; guess: that every new match starts its
  clock at 0, and that a loaded game carries on from its saved time]
- `TimeMessage` is on screen exactly while the Elapsed Game Time option
  (`ViewShowElapsedGameTime` in `UVars.tgi`, off by default) is on. It's
  checked every frame, so turning the option on or off mid-match shows
  or hides it at once. [code, data]
- `Counter` shows the seconds given to `game_counter_show` less the game
  time since it was shown ([triggers](triggers.md#counter)). It starts at
  the full time (`game_counter_show` with 1200 shows `0:20:00`), then
  `0:19:59` as soon as any time has passed. [code]
- At 0 the counter stops at `0:00:00` and stays on screen until
  `game_counter_hide` or another `game_counter_show`; it never goes away
  by itself. [code]
- The counter belongs to the match, not to a side: once shown, every
  player in a multiplayer match sees it, whichever side's trigger showed
  it. It's on screen exactly while one is set, so it appears and goes
  away on the frame after `game_counter_show` or `game_counter_hide`.
  [code]
- Neither label changes its own colour, font or size: both draw as their
  label template in `time_message.tgi` gives, the counter as it nears 0
  included. [code]

### The resource bars

- A bar with `hidden_offset` rests that far from its bounds while no
  preview is showing, and slides to its bounds over `slide_time` seconds
  when one starts, and back when it ends. [code, data]
- AA: `EconomyBar` has bounds (320, 520) to (779, 545) and
  `hidden_offset = 0,25`, so it rests at y 545 to 570. `EconomicImpact`
  (`slide_economy_bar = true`) at y 545 to 570 doesn't slide itself: when
  a preview starts, the main bar slides up to 520 and the preview appears
  in the place it left. [code, data, game]
- Kohan II: `EconomyBar` sits fixed at the top right (419 wide, y 0 to
  25). `EconomicImpact` (`hidden_offset = 0,-25`, `slide_economy_bar =
  false`) rests behind it and slides down to y 25 to 50 for a preview.
  [data, code]
- What starts a preview is in [economy](economy.md).

### Kohan II's side panel

- `SidePanel` sits at the top right under the resource bar (`L = 419r`,
  `T = 25`, 425 wide, to y 576). It shows one page at a time
  (`SidePanelMessageLogPanel`, `SidePanelCityManagement`,
  `SidePanelPolitics`, `SidePanelObjectives`, `SidePanelInfo`), picked
  with `SidePanelTabs` in the control panel, or none. [code, data]
- It shows no page when a match starts. [code]

### Tooltips

- In a match, tooltips are anchored rather than following the mouse
  (`anchored_tooltips` on `Game`): AA puts the box's bottom-right corner
  at 1024, 521 (`anchored_tooltip_x`, `anchored_tooltip_y`), the screen's
  right edge just above the control panel, 346 wide
  (`anchored_tooltip_width`), as tall as its text. [data, game]

## Images

### Resolution sets

- An image path with `%s` in it (`/UI/%s/Game/ControlPanel/...`) first
  tries `%s` replaced by the window's width in pixels: `UI/800/...` at
  800 wide, `UI/1280/...` at 1280 wide. [code]
- If no such file exists, the `%s/` is dropped and the base file is used
  (`UI/Game/...`). So 1024 wide, and any width without a set, uses the
  base images. [code]
- Only the image changes: the widget keeps its place in the 1024 by 768
  layout, stretched as above. [code]
- The base data ships `UI/800` and `UI/1280` sets, and so do the skins.
  [data]

### Nation skins

- In a match the interface draws with the local player's nation's skin:
  the depots its nation's `depot_name` lists (`Skins/RUS.rwd,Skins/RUS/`:
  the archive, then a folder of loose files) are added to the files the
  game reads. [code, data]
- It's `depot_name`, not `resource_prefix`, that names the skin, though
  the stock nations name both alike. Kohan II's nations are its races
  (`Skins/Human.rwd`). [data]
- The skin's files stand in for the same paths in the base data: the HUD
  frame is themed by nation (brass for Japan, green for the USSR and USA,
  camouflage for Britain). [game]
- The skin is put on when the match's interface comes up, and in AA also
  for the campaign map; it's taken off for everything else (menus, lobby,
  editor), and when the local player has no side. Menus and the lobby use
  the base images. [code; guess: which of the interfaces that keep it is
  the campaign map]
- The skins hold `UI/Game/ControlPanel`, `UI/Game/Panel` and (AA)
  `UI/Game/ListBox` images, with their `UI/800` and `UI/1280` versions;
  AA's also hold the campaign map's (`MetaGame/UI/...`), and Kohan II's
  `UI/Default/Briefing`. [data]

## Open questions

- How the minimap's view outline is worked out: the whole window's view
  of the ground, or something else? (analysis, 2026-11-20)
- Do the control panel's minimap and top buttons really ignore the
  pointer above y 558? (analysis, 2026-11-20)
- Does a box drag carry on while the pointer is over the control panel?
  (implementer, 2026-11-20)
- Does pausing a single-player game post a line to the message log, as
  it does in multiplayer? (analysis, 2026-11-21)
- While the game menu (or the options over it) is open, do the map's
  keys, such as the pause key and game speed, still reach the match, and
  can the map be clicked? (analysis, 2026-11-22)
- `ref/aa/screenshots` notes the campaign's windows over a battle with
  "the game keeps running behind"; the rules above say a campaign
  mission pauses. A check in the real game (does the paused label blink
  behind the objectives window in a campaign mission?) would settle it.
  (analysis, 2026-11-22)

- Which of two overlapping screen widgets draws on top: does AA's
  resource bar draw over the control panel's frame, or show through it?
  (analysis, 2026-11-16)
- When exactly `KingdomDropDown`,
  `DisplayBannerPanel` and `Subtitles` show.
  (analysis, 2026-11-16)
