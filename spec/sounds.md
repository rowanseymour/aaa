# Sounds

Sounds that aren't music or part of a visual effect: what the interface's
widgets play, units' spoken replies, game events' sounds, the Audio options'
sound switches, and how any audio entry picks its file, pitch and volume.
Music is in [music](music.md); sounds played by effects are in
[effects](effects.md), Sounds.

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Interface sounds

Every interface sound is a widget's own setting, named by an `IDS` from
`Audio/sounds_ui.tgi`. The kinds of widget and their defaults are the
templates in `Templates/UI/template_widget.tgi`, which every screen's
widgets inherit from; a widget or template in the UI files can override
them, with `NULL` (any case) meaning silence.

- A button has two sounds: `set_sound`, played when it's pressed, and
  `unset_sound`, played when it's let go. Push buttons
  (`PushButtonWidget`) default to `push_button` and toggle buttons
  (`ToggleButtonWidget`) to `toggle_button`; `unset_sound` is `NULL` for
  both. [code, data]
- Nothing in the shipped UI overrides a button's sound, so every push
  button in the menus and the match plays `push_button`, and every toggle
  button plays `toggle_button`. The main menu's buttons (Single Player,
  Options, Quit and the rest) are push buttons and play `push_button`;
  so do the in-match push buttons such as the control panel's menu,
  objectives, technology and message log buttons. [code, data]
- `menu_button`, `radio_button` and `popup` are never played: no widget
  names them and the game asks for none of them itself. [code, data]
- A push button sounds the moment the mouse button goes down on it, not
  on release, and since its `unset_sound` is silent, nothing plays on
  release. The left button does it, and so does the right one. Each click
  of a double-click sounds. A press on a button already held down makes no
  second sound. [code]
- A toggle button sounds when the left button goes down on it, and it
  plays the same `set_sound` whether that press turns it on or off. A
  right press does nothing to it. [code]
- The control panel's round buttons that stay lit (politics and the
  supply border toggle) are toggle buttons and play `toggle_button`, as do
  the options screen's checkboxes and switches. [data]
- The command panel's buttons are the exception: their layout makes them
  toggle buttons, so their sound is `toggle_button`, but they're pressed
  like push buttons, sounding when either mouse button goes down on one.
  [code, data]
- The favourites bar's buttons are push buttons and play `push_button` on
  press the same way. [code, data]
- A toggle button in a group, where choosing one clears the others (the
  tab-like choices), is still a toggle button and plays `toggle_button`.
  [code, data]
- A button whose on or off state is set by the game rather than by a
  click (a button lit because something happened) plays its `set_sound`
  when that turns it on and its `unset_sound` when that turns it off, and
  nothing if the state didn't change. [code]
- `ButtonWidget`, the template both kinds of button come from, names
  `button_push`, a sound that doesn't exist. No shipped widget uses it
  directly, so it never matters; a widget that did would fail to load as
  an unknown sound. [code, data]
- A slider plays `sound_clicked` when the left button goes down on its
  grip or its track (each click of a double-click too); dragging and
  letting go are silent. Its default is `slider_bar` (which is
  `RadioButton.wav`). The options screen's four volume sliders set it to
  `null`, so they're silent. [code, data]
- A slider's two end arrows are ordinary push buttons and play
  `push_button`. [data]
- A dropdown list plays `sound_open` when a left click opens it and
  `sound_close` when it closes, by a second click or by choosing an
  entry. They default to `tiled_button` and `NULL`, so opening a dropdown
  sounds and closing it doesn't. [code, data]
- The menus in a menu bar (`MenuWidget`: File and Options on the main
  menu) default to the same pair: `tiled_button` when one opens, silent
  when it closes. Moving along the bar from one open menu to the next
  closes one and opens the other, so each move sounds. [code, data]
- Panels (`PanelWidget`, which every screen and dialog is) also carry
  `set_sound` and `unset_sound`: the first plays when the panel is shown
  and the second when it's hidden. Both default to `NULL` and no screen
  or dialog sets them, so screens and dialogs open and close silently.
  [code, data]
- A panel's show or hide sound plays only if no other panel's has played
  in the last 250 ms, so a screen whose panels all open at once makes one
  sound, not many. Button presses aren't held back by this. [code]
- Two panels in the multiplayer staging screen, the team slot and the
  player slot, set `set_sound = push_button`. Since a panel's sound plays
  as it's shown, they sound when they appear; whether clicking them makes
  a sound too is open. [code, data]
- The minimap is a panel with no sound of its own (`NULL`), so clicking
  it plays nothing. [data]
- Nothing plays on hover: a widget lighting up under the mouse, and its
  tooltip appearing, are silent. [code, data]
- An interface sound plays as its `AudioInterfaceSound` entry says:
  heard by the local player only, not placed in the world, at 90% volume
  by default, with up to 2 playing at once and a new one taking over the
  oldest past that. [data]
- An interface sound's pitch and volume are the same on every play. The
  audio fields that vary them (`pitch_adjust`, `volume_adjust`, see
  Picking a sound's file) are both zero for `AudioInterfaceSound` and no
  entry in `sounds_ui.tgi` sets them, and `delay_ms` is 0, so a click
  starts at once, at the file's own pitch, at 90%. [code, data]

## Unit replies

Units, regiments and buildings answer the player when selected or given
an order: a short spoken line, such as an infantry regiment's "select",
"move" and "attack" call-outs. The options screen's Unit
Acknowledgement switch turns them off.

- A thing's replies are three effect entries in its data, each an
  `FXC_PlaySFX` naming an audio entry: `WhenSelected` when it's selected,
  `WhenCommanded` when it's given an order, and `WhenCommandedAggressive`
  when the order is aimed at an enemy. Regiments carry most of them;
  some units (ships, aircraft) and buildings carry their own. A thing
  with no entry for the moment says nothing. [code, data]
- Selecting plays the selected thing's `WhenSelected`: picking it on the
  map, an HQ's Command All button (its first regiment answers), or
  calling up a saved group (its first member answers). A unit that
  belongs to a regiment answers with its regiment's line rather than its
  own. One reply per selection, however many things it takes in. [code,
  data] Whether every way of selecting (box-dragging, cycling idle
  units) replies is unchecked. [guess]
- Giving an order plays one reply, from the selection's lead thing:
  `WhenCommandedAggressive` if the order is aimed at a thing (not a
  place) that is an enemy of the player, `WhenCommanded` for anything
  else (moves, orders on a place, orders on own or allied things). An
  attack on a place, such as a bombardment of ground, gets
  `WhenCommanded`. [code]
- An order the game refuses before sending it (nothing selected that can
  take it) gets no reply. [code]
- Only the player's own orders and selections make replies, and only
  that player hears them: every reply entry is an `AudioCallout`, whose
  `who_flags = PLAYER` plays it only for the thing's owner. Selecting an
  ally's or enemy's thing is silent, and nobody hears another player's
  units answer them. [code, data]
- Every shipped reply is an `AudioCallout` (`Templates/template_audio_aa.tgi`,
  no entry overrides it): not a `WORLD` sound, so it plays flat at the
  same loudness wherever the thing is and wherever the camera looks;
  `volume = 70%`; `priority = HIGH`; `simultaneous_limit = 1`, so one
  copy of a given line at a time; and the `VOICE` flag, one voice line
  at a time per kind of thing (below). A line picks at random among its
  `files`. [code, data]
- A reply asked for while the same line is still playing, or is waiting
  to start, is dropped; the line playing carries on to its end, and
  nothing waits to play after it. `AudioCallout` lacks the `INTERRUPT`
  flag that would let the new copy restart the line. [code, data]
- A reply is also dropped while any line with the `VOICE` flag is
  playing, or waiting to start, from a thing of the same kind (the same
  template) as the one answering, whichever of that kind it came from.
  So clicking a regiment again while it's still talking is silent, as is
  selecting a second regiment of the same kind; a regiment of another
  kind answers at once, over the first. The kind is free again once its
  line ends. [code]
- Replies, like event sounds, never take the place of a playing sound
  except an interface sound, and only when every flat channel is busy
  (see Event sounds). [code, data]
- Its category is `SOUND`, not `SPEECH`, so a reply is heard at Main
  volume × FX volume, not under the Speech slider, and it doesn't duck the
  music. [code, data]
- The generals' talking portraits use the same switch: a general
  portrait's `ObjectAnimationSelected` animation (in `Generals/`, voice
  lines such as `usa_patton_select`, `AudioCalloutGeneral`) still plays
  with the switch off, but silently. [code, data] When it plays is under
  [object display](object-display.md#animation): only when a lone thing
  showing the general is put up, never on a timer or by chance. [code]
- A general's line is not a reply entry: its `AudioCalloutGeneral` entry
  (`Templates/template_audio_aa.tgi`, no general's entry overrides it)
  plays it for the local player only (`who_flags = LOCAL`), not placed in
  the world, at `volume = 90%`, `priority = LOW`, one copy of a line at a
  time (`simultaneous_limit = 1`), with the `VOICE` flag. Its category is
  `SOUND`, so Main volume × FX volume applies. It is `SEQUENTIAL`: each
  time the general talks, the next of his `files` plays, in listed order,
  wrapping after the last (see Picking a sound's file). [code, data]
- With Unit Acknowledgement off, none of the above plays; nothing else
  changes. Units' other sounds (weapons, engines, deaths, a building
  finishing, game event sounds) aren't replies and still play. [code]
- The switch is looked at only when a reply would start: turning it off
  doesn't cut a line already playing, and turning it on doesn't bring
  back one that was skipped. [code]

## Event sounds

- Every event sound in `sounds_game_events_aa.tgi` is an `AudioFeedback`
  sound unless it says otherwise. `AudioFeedback` (in
  `Templates/template_audio_aa.tgi`) isn't a `WORLD` sound, so it plays
  flat, like an interface sound: the same volume wherever the building is
  and wherever the camera is looking, never cut off by distance. [code,
  data] It's never panned either: every flat sound plays centred, equally
  in both speakers. [code]
- Fog and the camera play no part in the sound itself; fog matters only
  through the event being dropped when its thing can't be seen (Who is
  told, in [game events](game-events.md)). [code]
- `AudioFeedback`'s `who_flags = LOCAL` means the sound plays for the
  player here without asking whose it is; the event already decided that.
  [code, data]
- `simultaneous_limit = 1`: only one copy of a given event sound plays at
  a time. [code, data] Its other settings: `priority = HIGH`, `volume =
  90%`, pitch varied at random by up to 1% either way. [data]
- When an event sound is asked for while the same sound is still playing,
  or already asked for and not yet started, the new one is dropped: the
  first carries on to its end, and nothing waits to play after it. So
  two buildings taken close together are heard once, and of several
  asked for in one tick the first wins. `AudioFeedback` lacks the
  `INTERRUPT` flag that would let the new copy restart the sound. Only
  the sound is dropped; the event's message line and blip still come.
  [code, data]
- Different event sounds don't hold each other back: each plays as soon
  as it's asked for, overlapping any other. [code, data]
- Flat sounds (event sounds, interface sounds, units' replies) share
  `Miles2DChannels` (8, in `UVars.tgi`). With all of them busy, a new
  event sound takes the place of the oldest playing sound that has
  `INTERRUPT` (an interface sound), which stops for it; with none of
  those playing, it's dropped. An event sound never takes the place of
  another event sound or a reply, which lack `INTERRUPT`. [code, data]

## Audio options: Sounds

The Audio tab's "Sounds" column holds two switches. Each is a toggle
button showing on or off, and each is saved like the sliders beside it.

- Chat Notification sets `UIEventFilterChatSound` (stock `true` in
  `UVars.tgi`): the sound that greets a chat message in a match. See
  [messages](messages.md), Chat. [code, data]
- Unit Acknowledgement sets `UnitAcknowledgement` (stock `true`): units'
  replies, above. [code, data]
- Both start from `UVars.tgi`; a value the player saved, or one given on
  the command line (`UnitAcknowledgement=false`), overrides it. Nothing in
  the game changes either of them by itself. [code, data]
- Like the sliders, the switches take effect when the dialog's Ok is
  pressed, all together; Cancel leaves the old values. [code, guess]
- The FX slider sets both the flat and the positioned sound volumes
  (`Audio2DVolume` and `Audio3DVolume`), so it governs the chat sound and
  units' replies as well as world sounds. [code]

## Picking a sound's file

This holds for every audio entry, not just the interface's.

- When an entry's `files` lists several files, each play picks one of
  them at random, every listed file equally likely and each pick
  independent of the one before, so the same file can play twice running.
  There's no limit on how many files take part: every listed file that
  exists can play. [code]
- By that rule all four of `push_button`'s files play, about a quarter
  of the clicks each. This is read from the code and disagrees with what
  was heard in the game (see Open questions), so don't build on it until
  that's settled. [code]
- The four files, all 22,050 Hz, 16-bit mono, are recordings of a button
  being pressed and let go, so most hold two clicks: [data]

  | File | Length | What's in it |
  |---|---|---|
  | `PushButton01.wav` | 0.54 s | a click 0.03 s in, a quieter one at 0.14 s |
  | `PushButton02.wav` | 0.51 s | a click at the start, a louder one at 0.12 s |
  | `PushButton04.wav` | 0.36 s | a higher-pitched click at the start, quieter ones at 0.13-0.15 s |
  | `PushButton05.wav` | 0.24 s | the first click of `PushButton02` alone, at about 60% of the others' peak |

  So `PushButton05` sounds like `PushButton02` cut short, and only 01, 02
  and 04 differ from the first instant. [data]
- `control_flags` changes the pick: with `SEQUENTIAL` the files play in
  listed order, one per play, wrapping to the first after the last.
  `ALL` doesn't make a play run through every file, whatever its comment
  in `template_audio.tgi` says: a play still plays one file from `files`
  (music, which loops, is in `music.md`). [code, data]
- The picks come from a random stream seeded from the clock when the
  game starts, shared with visual effects and animations, so the order of
  clicks differs from run to run. [code]
- `leadin` and `fallout` lists are picked from at random too, except that
  with more than one file listed the last one is never chosen. [code]
- `pitch_adjust = low, high` plays the file at a rate picked evenly
  between 100% + low and 100% + high, afresh each play (stock default 0%,
  0%: no change). [code, data]
- `volume_adjust` (zero or negative) plays it at `volume` times a factor
  picked evenly between 100% + `volume_adjust` and 100%, afresh each play
  (stock default 0%: always `volume`). [code, data]
- `delay_ms = low, high` holds the start back by a time picked evenly in
  that range. [code, data]

## Open questions

- Which `push_button` file plays? Heard in the game: the main menu's
  buttons (Single Player and the rest) make the same click every time,
  and across the menus only two distinct clicks were made out. Read from
  the code: every press picks afresh among all four files, and all four
  load. The two don't fit, and nothing found so far (the widgets' sound
  setting, other definitions of `push_button`, missing files, reuse of an
  earlier pick) explains a constant click. To settle it, replace the four
  files with four different tones in the game's loose `data/` folder and
  listen. (implementer, 2026-11-11)
- Does clicking one of the staging screen's team or player slots make a
  sound, or only its appearing? (analysis, 2026-11-10)
- A dropdown's open button is a push button inside it: does opening a
  dropdown with it play `push_button` as well as `tiled_button`?
  (analysis, 2026-11-10)
- Selecting in one kind of session is always silent, probably watching a
  recorded game; confirm which. (analysis, 2026-11-13)
- Are the two Sounds switches saved to the player's settings file when
  the game exits, like the sliders? (implementer, 2026-11-13)
