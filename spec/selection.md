# Selection

What the player can select, which things can be selected together, and how
clicks, Shift and Ctrl, box-dragging and double-clicks change the selection.

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Selection

What the player can pick on the map, and how a selection holds together.
Any side's things can be selected, not only the player's own; what the
player can then do with them is under "With someone else's thing
selected" below.

### What a click picks

- A left click on the map picks the thing under the pointer, of any
  owner: the player's own, an ally's, an enemy's or a neutral side's.
  [code]
- The thing must be one the player can see now, or one shown as
  remembered under the fog (see [fog](fog.md#what-shows-under-each-state)).
  Something on unexplored ground, or in the fog without a remembered
  picture (a unit that walked out of sight), can't be picked, and the
  pointer doesn't highlight it either. [code]
- What gets selected for what was clicked:
  - A unit of a regiment selects the regiment. If the player can't see
    the regiment (a remembered unit picture, say), the unit itself is
    taken instead. [code]
  - One of a city's buildings selects the city, not the building. [code]
  - Anything else selects itself. [code]
- Only `targetable` things stay selected (see Keeping a selection), so
  scenery and other things without the flag can't be selected in
  practice: at most they're picked for a moment and dropped again.
  [code] Whether wrecks, projectiles and the like are ever under the
  pointer at all is unchecked. [guess]
- A plain left click replaces the selection with what was picked. A
  click on a spot where nothing can be picked clears the selection.
  [code]
- Where several things overlap under the pointer and one of them is the
  whole selection, clicking there again picks another of them, so repeated
  clicks step through the pile. [code]

### Who goes together

A selection holds things of one standing towards the player and one
kind, and at most six of them.

- Standing ranks: the player's own things above enemies', enemies' above
  allies', allies' above neutral sides'. [code]
- A thing's standing is how the player's side stands towards the thing's
  owner, from the map's relations (team relations included): self,
  enemy, ally or neutral. Whether the owner
  is a kingdom or one of the map's independent sides plays no part.
  [code]
  - An independent side the player is at war with (Berlin's
    `independent_5`, "Civilian (Aggressive)") ranks as an enemy, so its
    regiments can be selected together with an enemy kingdom's. [code]
  - A kingdom the player stands neutral towards ranks with neutral
    independents, lowest; their regiments can be selected together.
    [code]
  - A thing with no owner, or any thing when the player has no side
    (watching), ranks as neutral. [code]
  - Relations can be one-sided; it's the player's side's view of the
    owner that counts, not the owner's view of the player. [code]
- Kinds within one standing, highest first:
  1. Regiments, and other things that think for themselves and can move
     (an HQ truck while packed). Many can be selected together. [code]
  2. Everything else that can't move or has no will of its own: a
     building, a city, an unpacked HQ, an emplacement. Only one at a
     time. [code]
  3. Things that move but stand outside any regiment and aren't
     buildings in motion. Many together. [code]
- Adding a thing to a selection (by any of the ways below):
  - if it ranks above what's there, by standing or by kind, the
    selection is emptied first and it alone is taken;
  - if it ranks below, it isn't added;
  - if it's of the same standing and kind it joins, for the kinds that
    allow many; for the one-at-a-time kind it isn't added. [code]
  So enemies of two different enemy sides can be selected together
  (both are enemies), but never an enemy with an ally or with one's own.
- A selection never takes a seventh thing: with six selected, nothing
  more is added. [code]
- "Standing" is fixed when a thing joins: a thing that changes owner
  while selected keeps its place and still counts as it did for what may
  join it later. [code]

### Shift and Ctrl clicks

- A left click with Shift or Ctrl held toggles the thing under the
  pointer: one already selected is taken out; one not selected is added
  by the rule above. [code]
- So with own things selected, a Shift-click on an enemy does nothing;
  with an enemy regiment selected, a Shift-click on another enemy
  regiment adds it, a Shift-click on an enemy building does nothing, and
  a Shift-click on an own regiment replaces the selection with it.
  [code]

### Box-dragging

- Dragging a box takes in every thing drawn inside it that a click could
  pick (seen now or remembered, standing and kind as for a click,
  regiment members counting as their regiment). The things it would take
  are highlighted while dragging. [code]
- The box applies the same ranking as adding: of what's inside, only the
  highest-ranked group is taken. A box over own and enemy things takes
  only the own ones; a box over enemies alone takes enemy regiments; a
  box over only buildings takes one. [code]
- The six-thing cap: when what the box takes, with what's already
  selected, comes to more than six, the ones taken are those nearest the
  middle of the existing selection, or of what the box covers when
  nothing was selected, filling up to six. [code]
- Whether releasing the box replaces the selection or adds to it (with
  Shift, say) is unchecked; replacing is the likely plain case. [guess]

### Double-clicks

- A double-click with an own thing selected (the first click selects
  it) selects every thing on screen of the same template, by the adding
  rule, so up to six, nearest first. Others' things of that template
  aren't taken, since own things outrank them. [code]
- A double-click on someone else's thing does nothing more than a
  single click: there's no select-all-of-a-kind for others' things.
  [code]
- With Shift or Ctrl held, a double-click on a selected thing adds every
  thing on screen of its template the same way; on a thing not selected,
  it takes every selected thing of that template out. [code]

### Keeping a selection

The game checks the selection continually and drops what no longer
qualifies.

- A thing stays selected while it is `targetable` and the player can see
  it or has it shown as remembered. [code]
- So someone else's unit or regiment that goes out of sight is dropped
  once nothing of it is seen (a regiment stays while any of its men is
  seen). A building or city that drops into the fog stays selected as its
  remembered picture. [code]
- A destroyed thing leaves the selection when it's removed from the
  world. [code]
- A change of owner doesn't touch the selection: an enemy building the
  player captures stays selected, and so does the player's own building
  when someone captures it, now as someone else's. [code]

### With someone else's thing selected

- No command buttons: only the player's own selected things contribute
  commands (see [commands](commands.md), Command grid). [code]
- A right click on the map gives no order: orders are only sent for a
  selection the player may command, and are otherwise dropped without a
  word or a reply. [code]
- Saving a control group keeps only the player's own selected things.
  Saving with only others' things selected therefore leaves that group
  empty, wiping what it held before. [code]
- Selecting someone else's thing is silent (see [sounds](sounds.md), Unit replies). [code]

### Control groups

- Ctrl with a digit saves the selection as that digit's group (Ctrl+1
  to Ctrl+9 and Ctrl+0: ten groups, `Localization/Hotkeys/hotkeys_groups.txt`); the
  digit alone recalls it. The groups are shown as "Group 1" and so on.
  The number of groups is the keymap's: the game takes any group name.
  [code, data]
- Saving empties the group of that name, then puts in the player's own
  selected things, so a group holds whatever a selection can: up to six,
  each regiment counted once (regiments and lone movers), or one
  building or city on its own. [code]
- A thing is in at most one group: saving it into another takes it out
  of the old one, and a group left with nothing in it is deleted. So is
  the group saved over with none of the player's things selected. [code]
- Recalling clears the selection and selects what's left of the group,
  under the usual rules, with the selection reply. Pressing the digit
  again while the group is still exactly the selection, within
  `UICenterGroupTimeMS` (1000 ms) of the last recall, centres the camera
  on the group (the average of its things' positions) instead, selecting
  nothing new. [code, data]
- The selection's **current group** is the saved group whose things are
  exactly what's selected, if there is one. Any other selection of two
  or more of the player's things shares one stand-in group of the
  player's own. One thing or none has no current group. Group Behavior
  ([movement](movement.md#groups)) is a switch on the current group.
  [code]

### The group strip

The row of group buttons along the top of the screen (`GroupFloater` in
`UI/Game/floaters.tgi`).

- Always ten buttons, one per name in the `Groups` block's
  `groups_shown` (`group_1` to `group_10`, the names the keymap's
  digits use), in that order from left to right, each in a fixed place
  whether its group exists or not: nothing is packed or hidden. The ten
  places divide the `Groups` block's width (339) evenly, a tenth each
  (33.9), from its left edge, each button 32 by 32. [code, data]
- A button whose group doesn't exist (never saved, or emptied) shows
  `empty_group_texture` (`UI/Game/Floaters/ButtonGroupUnassigned.tga`).
  [code, data]
- A group's button shows the `icon` of one of its things, chosen
  afresh every frame, so it follows deaths and saving over at once:
  [code]
  - Each thing in the group puts forward a picture. A lone unit or a
    building puts forward its own template's `icon`. A regiment puts
    forward the `icon` of one of its unit types, taken from its layout
    and template rather than from whoever is alive: the unit type of its
    first `locked` slot, in layout order; failing that, its leader's
    slot (`PrimarySlot`); failing that, its first slot with a unit type.
    So an infantry regiment shows its riflemen, never the regiment's
    own `icon`.
  - Going through the group's things in the group's order, the first
    picture put forward by at least half of them (rounded up: 2 of 3,
    3 of 6) is shown. With no such picture, one of those put forward is
    shown. A thing with no `icon` puts nothing forward but still counts
    towards the half.
- Nothing else is drawn on a button: no digit, count or health, and no
  different look for the selection's current group, for Group Behavior
  being on or for things in combat. Pointing at or pressing a button
  doesn't change its look either: the button layout has only the
  picture. [code, data]
- A left click on a group's button recalls the group, as its digit
  does. A double-click recalls it twice, so the second recall centres
  the camera on it under the rule above (the group still the selection,
  within `UICenterGroupTimeMS`). A left click on a button without a
  group does nothing. [code, data]
- A right click saves the selection as that button's group, as Ctrl
  with its digit does, whether or not the group exists. [code, data]
- Ctrl, Shift and Alt change nothing about a click on a group button.
  [code]
- The tooltip, the same for every button bar its keys: [code, data]
  - Title "Groups" (`groups_tooltip_title`).
  - For a group that exists, a line with the key that recalls it, then
    " Select Group" (`hotkey_select_group|s`, "%s Select Group").
  - Then, whether or not the group exists, a line with the key that
    saves it, then " Save Group" (`hotkey_save_group|s`).
  - Then "Right-Click on a group button to save.  Double-Click on a
    group button to center." (`groups_tooltip_description`).
  - Each key is written as the bracketed form of [hotkeys in
    tooltips](commands.md#hotkeys-in-tooltips), coloured the same way:
    "(1) Select Group", "(Ctrl-1) Save Group" for the first button. The
    keys are the ones the keymap gives to `SelectGroup group_1` and
    `SaveGroup group_1` and so on (`hotkeys_groups.txt`).
- The strip is open at the start of a match when `UIGroupFloaterIsOpen`
  is true, as `UVars.tgi` ships it. Its toggle on the left
  (`ShowButton`, tooltip "Groups" / "Opens/closes the group buttons.")
  shows its selected look while the strip is open; clicking it hides or
  shows the ten buttons, together with the tab at the strip's right end
  (`EndDecal`, `GroupsTab.png`, drawn because `AVars_AA.tgi` sets
  `UIFloatersDisplayEndCap = true`). The strip's background with the
  toggle (`GroupsFloater.png`) stays either way. [code, data]
- Toggling sets `UIGroupFloaterIsOpen`, so a later match in the same
  run starts the strip as the player left it. [code]

### Cycling idle trucks and engineers

AA's Cycle Idle Trucks & Engineers (F6, `CycleSelection 1` in the
shipped hotkeys, or the floating button `IdleButtonBuilder`, which does
the same thing) works as Cycle Detached Regiments does
([supply](supply.toml#what-the-player-sees)) with its own test.
Kohan II has the same control (F6 and its "Idle Builder" button,
"Focuses on next idle builder company."). [code, data]

- What counts: your own things that aren't members of a regiment, so
  regiments (companies in Kohan II) and lone things, never a unit inside
  a regiment. One counts when it's idle and it is, or has a member that
  is, either: [code, data]
  - a builder, a template with a `BuilderComponent`: in AA the five
    nations' engineers, so an engineer regiment counts; in Kohan II a
    company holding one of its builders (engineers, settlers and the
    like);
  - a truck: a thing with a `MobileDeploymentComponent` that no truck
    packs into, i.e. the packed side of a pack pair. In AA that's every
    `<nation>_truck_*`: depot trucks (supply, ammo, oil), the Corps HQ
    truck, the airfield and motor pool trucks, the brigade trucks, and
    the Division HQ trucks. So a lone truck counts, and so does a packed
    Division HQ regiment. An unpacked building never counts, being the
    side a truck packs into. There may be one more condition on trucks
    that wasn't pinned down; it's believed to hold for any truck on the
    map. [guess]
  - a regiment's other members (its captain, fighting units) don't
    matter.
- Idle means the regiment, or the lone thing, is doing nothing at all:
  no order being carried out (moving, building, repairing, guarding,
  packing, unpacking, attacking), not fighting, not routing, reforming
  or regrouping. Being given any order makes it not idle at once; it's
  idle again as soon as it finishes or gives up. A regiment's own state
  is what counts, not its members'. A thing that has no mind at all
  counts as idle. [code]
- Skipped by the cycle: things that can't be selected, as for the
  detached cycle. Cargo isn't a thing until it's unloaded, so nothing
  aboard a transport is ever in it. [code]
- Each step, as for the detached cycle: the order is your side's list
  of things (probably oldest first [guess]), starting after what's
  selected now and wrapping round; the step replaces the selection with
  that one thing and moves the camera to it. If the only candidate is
  already selected, or there's none, nothing happens (the camera stays).
  No reverse key is shipped. [code, data]
- The button shows while at least one of your things passes the test
  above (idle, and a builder or truck or holding one); it doesn't check
  selectability. It's re-checked about once a second, so it can lag a
  change by up to a second. With no side of your own (watching only),
  it's hidden. [code]
- Kohan II: the button is at 10, 493, 36 by 38, a border
  (`ButtonWorkerIdleBorderUnfocused.png` / `...Focused.png`) round
  `ButtonWorkerIdleDecal.png`. Its `IdleButtonDetachedOrganization` is
  zero-sized with no image, so it's never seen, and its hotkeys map only
  F6. [data]

## Open questions

- Does every idle lone truck on the map make the idle-trucks button
  show and get a turn in its cycle, packed Division HQs included? A
  check in the real game would settle the truck rule's last condition.
  (analysis, 2026-11-22)

- Is `UIGroupFloaterIsOpen` kept when the game is quit, so the strip's
  open or closed state carries over to the next run? And are the group
  tooltip's lines separated by blank lines? (implementer, 2026-11-22)
- When no picture is put forward by half a group's things, which one is
  shown? (implementer, 2026-11-22)

- Selection rules are read from the code and haven't been tried in the
  game: check the six-thing cap, box-dragging over own and enemy
  regiments, and Shift-clicking an enemy with own things selected.
  (analysis, 2026-11-14)
- Does releasing a drag box replace the selection, and does Shift or
  Ctrl make it add instead? (analysis, 2026-11-14)
- Which things outside regiments rank in the lowest kind (moving, not in
  a regiment): aircraft, lone vehicles? And can wrecks, projectiles or
  scenery ever be under the pointer? (analysis, 2026-11-14)
