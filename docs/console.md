# Console commands

<!-- Written from the verbs in crates/game/src/console.rs by its docs_list_the_verbs test; run it with UPDATE_DOCS=1 to rewrite. -->

How lines are written, and how scripts and a port feed them in, is in [console.rs](../crates/game/src/console.rs). Words in [brackets] are optional, and (a | b) means either.

- `help [verb]`: lists the verbs, or says how to use one; also ?
- `echo text`: prints the text
- `exec file`: runs the commands in a file, a line at a time
- `quit`: ends the program
- `screenshot file.png`: saves what the window shows to a file, answering once it's written
- `ui`: lists the ids of the buttons on screen that can be pressed, and which are chosen
- `click button`: presses a button, by its id; ui lists them
- `press key`: presses a key as a hotkey file writes it, e.g. esc, f1 or ctrl-s
- `wait seconds`: lets that long go by before the next command
- `lookat [x y]`: points the camera at a place on the map, in metres; without one, says where it points
- `camera x y [height] [pitch]`: points the camera at x y from a height above the ground in metres, looking a pitch in degrees down
- `zoom [0-1]`: zooms from 0, all the way out, to 1, all the way in; without a level, says it
- `yaw [degrees]`: turns the camera to look that far counter-clockwise from the map's x axis; without, back to the game's
- `pitch [degrees]`: tilts the camera that far down from level; without, back to the game's
- `defaultangles`: puts the camera back to the game's own view
- `create template [x y]`: raises a regiment for the placement kingdom where the camera points, or at x y, and says its id
- `placementkingdom [kingdom]`: sets, by name or id, who owns what create raises; without one, lists the kingdoms
- `playerkingdom [kingdom]`: plays as a kingdom, by name or id; without one, lists them
- `move regiment x y`: sends a regiment, by its id, to x y
- `attack regiment (x y | enemy)`: sends a regiment, by its id, to x y in its combat formation, taking on enemies that come near on the way, or after an enemy regiment, by its id
- `guard regiment (x y | other)`: posts a regiment, by its id, at x y or beside another regiment, by its id, to come back to after every fight
- `select [id ...]`: selects the player's regiments, or one of its buildings, by their ids; without any, says what's selected
- `commission template [x y]`: asks the selected building to commission a template it lists, or with x y, one that's flown in, to deliver it there
- `research technology`: asks the selected building to research a technology it lists
- `pack`: packs up the selected building
- `sell`: sells the selected building
- `disband regiment`: disbands a regiment, by its id
- `attach regiment`: attaches a regiment, by its id, to an HQ with a place for it, or moves it on to the next
- `detach regiment`: detaches a regiment, by its id, from its HQ
- `unpack x y [degrees]`: sends the selected packed building to unpack at x y, facing that far counter-clockwise from the map's x axis
- `destroyselected`: destroys the selected regiments
- `advance seconds`: plays that much of the game at once, paused or not
- `fog`: lifts the fog of war from the screen, or lays it again
- `gamespeed [speed]`: runs the game that many times as fast, 0.05 to 16; without, says how fast
- `gamepause [1/0]`: pauses or resumes the game; without, switches
- `toggleshowzonesupply`: shows or hides the supply border
- `uiscale [scale]`: draws the interface that many times as big, 0.5 to 2, though smaller if the window's too narrow for it; without, says how big it's asked to be and is
- `uitint [r g b [r g b]]`: shades the interface's backgrounds, borders and text from a dark colour toward a light one, each part 0 to 1; without, says them
- `resolution [width height]`: resizes the window; without, says its size
