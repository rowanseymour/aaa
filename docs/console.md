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
- `create template [x y]`: raises an organization for the placement kingdom where the camera points, or at x y, and says its id
- `placementkingdom [kingdom]`: sets, by name or id, who owns what create raises; without one, lists the kingdoms
- `playerkingdom [kingdom]`: plays as a kingdom, by name or id; without one, lists them
- `outcome`: says whether the match is over, and how it's come out for each kingdom
- `missionwon kingdom`: has a kingdom, by name or id, win the match
- `missionlost kingdom`: has a kingdom, by name or id, lose the match
- `move organization x y`: sends an organization, by its id, to x y
- `attack organization (x y | enemy)`: sends an organization, by its id, to x y in its combat formation, taking on enemies that come near on the way, or after an enemy organization, by its id
- `guard organization (x y | other)`: posts an organization, by its id, at x y or beside another organization, by its id, to come back to after every fight
- `heroes [kingdom]`: lists the heroes a kingdom, by name or id, holds, and what each is doing; without one, the kingdom played
- `settlements`: lists the settlements, with each one's side and the id of its centre
- `select [id ...]`: selects organizations, or one building, of any side by their ids; without any, says what's selected
- `commission template [x y | hero]`: asks the selected building to commission a template it lists, or with x y, one that's flown in, to deliver it there; a settlement's company can be led by a hero from the pool
- `research technology`: asks the selected building to research a technology it lists
- `usefactionability n x y`: uses the nth of the faction's special operations, from 0 as the panel lists them, on the ground at x y
- `pack`: packs up the selected building
- `sell`: sells the selected building
- `disband organization`: disbands an organization, by its id
- `attach organization`: attaches an organization, by its id, to an HQ with a place for it, or moves it on to the next
- `detach organization`: detaches an organization, by its id, from its HQ
- `attachhero organization hero`: puts a hero from its side's pool at the head of a company, by the company's id
- `detachhero organization`: sends the hero leading a company, by its id, back to its side's pool
- `unpack x y [degrees]`: sends the selected packed building to unpack at x y, facing that far counter-clockwise from the map's x axis
- `destroyselected`: destroys the selected organizations
- `advance seconds`: plays that much of the game at once, paused or not
- `record [file]`: says how many ticks the match has played, how many orders it's carried out and its hash, which copies of a match share while they've played out the same way; given a file, keeps the match so far there as a film, for aaa-server to play again
- `fog`: lifts the fog of war from the screen, or lays it again
- `gamespeed [speed]`: runs the game that many times as fast, 0.05 to 16; without, says how fast
- `gamepause [1/0]`: pauses or resumes the game; without, switches
- `toggleshowzonesupply`: shows or hides the supply border
- `uiscale [scale]`: draws the interface that many times as big, 0.5 to 2, though smaller if the window's too narrow for it; without, says how big it's asked to be and is
- `uitint [r g b [r g b]]`: shades the interface's backgrounds, borders and text from a dark colour toward a light one, each part 0 to 1; without, says them
- `resolution [width height]`: resizes the window; without, says its size
