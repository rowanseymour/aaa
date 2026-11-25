# Object display

What the interface shows about things: health bars over the world, the
property icons and technology display, and the object display's portrait
box, panel, tooltips, clicks and formation buttons.

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Health bars

- A thing's health bar shows while it's selected. It also shows on the
  members of a selected regiment and on the buildings of a selected city,
  under a condition of the interface that is still open. Nothing about the
  owner matters: a selected enemy shows its bar too. [code]
- Some interface-wide states hide every bar, probably cutscenes and the
  like. [code, guess]
- Which bar: the thing's `[BodyComponent]` `health_bar_vfx_IDS` names a
  VFX; without one (the default is `NULL`) it's the `VFXHealthBar` VFX in
  `AVars.tgi`, `health_bar_vfx`. Every building from `AABuilding` in
  `template_building_aa.tgi` sets `health_bar_medium_vfx`; no shipped
  template sets `health_bar_large_vfx`. So units use the small bar and
  buildings the medium one; the large one is unused. A thing without a
  body has no bar. [code, data]
- The three VFX (`Effects/game_vfx.tgi`) are status bars showing `HEALTH`,
  with models `HealthBar.nif`, `HealthBarMedium.nif` and
  `HealthBarLarge.nif`. Each model is one flat quad 0.16 high and 1.12,
  3.35 or 4.48 long, centred on its origin, turned to face the camera
  (`billboardRigid`) and drawn without depth testing (`zMode00`), so it's
  never hidden behind anything. [data]
- Size is fixed by the model: it isn't scaled by the thing's size
  (`attached_scale` is 0) and it grows and shrinks with the zoom like the
  world (`constant_screen_size` is false). [code, data] Whether a thing's
  own model scale carries over to the bar is open.
- Placement: the bar's centre sits on the thing's own position, on the
  ground at its centre, with no offset. From the usual camera angle a
  unit's bar reads as under it and a building's as on the ground in front
  of its middle. [code, game]
- The model carries a texture animation of the bar's fill (green through
  yellow to orange, the lost part dark); the frame follows the health
  fraction. [game, guess]

## Property icons

The row of small icons above the selection panel
(`ger-division-hq-terrain-tooltip.jpg`,
`ger-division-hq-truck-guarding-tooltip.jpg`).

- It shows the properties the selected thing holds: for a regiment (an
  HQ is one), the regiment's own, not its members'. [code, game]
- A property shows if its `icon` isn't `null` and it isn't a second copy
  of one already held. [code]
- Order is the order the properties were gained, oldest first; losing one
  closes the gap. So a terrain icon comes before Guarding when the unit
  reached the terrain before it began to guard, and after it the other
  way round. [code]
- The properties a thing is made with are gained in the reverse of the
  order its template's `[PropertiesComponent]` lists them, the last
  listed first. [code] Those come before any it gains in play, its
  formation's property among them. [code, game] So
  `regiment_jpn_infantry_recon` (listing `recon` then `trailblazing`)
  shows Trailblazing (`IconPropertyMovement.png`, the boots), Recon
  (`IconPropertyMorale.png`, the bugle), then its formation's
  (`IconPropertyFormation…`, the dots). Its inherited `nation_jpn`,
  `org_infantry` and `org_unit` have no icon (`null` or `NULL`) and
  take no place. [data, game]
- The list being reversed is the whole inherited one, built as any list
  section is ([data](data.md#templates)): the furthest ancestor's
  entries first, the thing's own last. So reversed, the thing's own
  properties come first (last listed first), then its template's, then
  that template's template's, and so on up the chain. [code] A German
  medium tank shown alone (`ger_tank_medium`, from
  `AAUnitVehicleTank`, from `AAUnitVehicle`) has Tanks (`tank`) before
  Vehicles (`vehicle`), after any of its own with icons; both use
  `IconPropertyNational.png`, so only their tooltips tell them apart.
  [code, data]
- One thing comes before even the properties a regiment is made with:
  the terrain properties of where it's made. A regiment's terrain is
  first looked at as it's made, before its own properties are given, and
  it qualifies by its units ([terrain](terrain.md)). So a regiment made
  on forest, on the map at the start or in play, has its Forest icons
  (`IconPropertyJungle.png`) first, then its own properties, then its
  side's technologies, then its formation's. A unit can't qualify for a
  terrain property as it's made, so a unit's row never starts this
  way. [code, data]
- No property an AA regiment template inherits has an icon, so a
  regiment's row is its own properties reversed, then what it gains in
  play. Inherited icons show on units shown alone and on buildings:
  `infantry`, `vehicle`, `tank`, `building`, `bunker` (all
  `IconPropertyNational.png`) and the village buildings'
  `city_building_repair`. [data]
- A side's technologies are in its things' rows: every regiment, unit
  and building of a playing side holds each technology the side has
  (from research, the map's start technologies, a trigger, or a
  building's `[TechnologyProperty]`) and the handicap's property, as
  long as the thing has the technology's `required_properties`. A
  regiment has them when any of its units does, or would (an empty
  place counts by the unit template that fills it). So a tank regiment
  and its tanks show `tech_diesel_engines` (`required_properties =
  tank`), an infantry regiment doesn't, and Easy's
  `property_handicap_easy`, with none, is on every thing the side has.
  [code, data]
- The requirement is checked when the technology reaches the thing:
  when the side gains it, or when the thing joins the side. [code]
- A thing gets the side's technologies as it joins the side, in the
  order the side gained them, after the properties it's made with. A
  thing joins its side as it's made, on the map at the start or in play,
  so they come before anything it gains in play. A regiment's formation
  icon comes only with its first activity, after it's made, so it
  follows the technologies. That holds for a regiment on the map too:
  the map doesn't keep what a regiment was doing, so each starts with no
  activity and takes its formation only when it starts one. [code,
  data] One the side gains later goes to the end of the
  row of every thing it reaches; one the side loses goes from every
  row. [code]
- A thing that changes sides (a captured building) loses the old side's
  technologies and gets the new side's at the end of its row. [code]
- A technology the side holds twice (two supply-drop buildings granting
  the same one) shows once, at its first place. [code]
- A `LOCAL` research item's property is on the building that researched
  it alone, at the end of its row. [code]
- Activities' properties (an `[ActorState]`'s `property_ids`) are held
  while the thing is in that state, and need the thing to have their
  `required_properties`. Each is gained when the state starts, so it
  goes to the end of the row, and goes when the state ends; one the
  thing takes up again goes to the end again. [code, data] The ones
  with icons:
  - `actor_state_engaged` (`IconPropertyEngaged.png`): on a regiment
    while it's engaged, and on a unit while it's engaged itself
    ([combat](combat.md#engaged-and-under-siege)). [code, data]
  - `actor_state_targeted` (`IconPropertyAttack.png`): on a regiment
    while it's targeted, which it never is while engaged. [code, data]
  - `actor_state_guarding` (Guarding) while guarding, and the other
    activities' own (`actor_state_exhausted`, `actor_state_chasing`,
    `actor_state_reforming`, and so on) the same way. A regiment does
    one activity at a time, so engaging drops Guarding, and Guarding
    comes back at the end of the row when the engagement ends. Targeted
    isn't an activity and stays alongside them. [code, data]
  - `actor_state_contested` (`IconPropertyUnderSiege.png`): on a city
    centre while it's under siege, for any of the reasons a building is
    ([buildings](buildings.md#under-siege)), its city being contested
    among them. It needs `property_city`, which only the city centres
    (`AAVillageCenter` and those from it, capitals included) have, so
    other buildings under siege, the city's other buildings and units
    under siege don't show it. [code, data]
  - `actor_state_regrouping` (`IconPropertyRegrouping.png`) never shows
    in AA: no regiment ever regroups there. [code]
- When an activity starts that changes the regiment's formation
  ([movement](movement.md#a-units-top-speed): attacking, engaged and
  guarding take the combat formation), the old formation's property
  goes and the new one's is gained before the activity's own. So a
  regiment in Column that engages ends its row with Assault's icon, then
  Engaged. An activity that leaves the formation as it was moves
  nothing. [code, data]
- While a regiment has no formation (routing, Exhausted, chasing), it
  holds its layout's Broken Formation property (`BrokenProperty`,
  `IconPropertyCombat.png`) in place of its formation's: the formation's
  icon goes and Broken Formation's is gained at the end of the row. When
  a formation is taken up again, Broken Formation goes and the
  formation's icon comes back at the end. [code, data]
- A special operation's property (`[EnchantmentProperty]`) and an AT
  unit's stun (`at_stun_*`) are given to each unit they catch, never to
  a regiment ([special operations](special-ops.md)), and a unit's
  properties don't pass up to its regiment. So they show only on a unit
  shown alone, not in its regiment's row. [code, data]
- There are seven places. With more than seven, the rest are gathered
  behind one extra marker whose tooltip starts with the
  `additional_properties` line and then gives each hidden property's
  tooltip in turn. [code]
- A thing holding the same property twice (a technology from the side
  and again from its regiment, say) shows it once, at the place of the
  copy that came first, with that copy's tint. When that copy goes while
  the other stays, the icon moves to the other's place and takes its
  tint. [code]
- An icon is drawn untinted (white) unless the property came: [code, data]
  - from an ability, such as a special operation or an AT unit's stun
    (an `[EnchantmentProperty]`, which lasts its `duration`): by how the
    holder's side stands towards the side that used the ability.
    `UIColorPropertyTemporaryFriendly` (`UVars.tgi`, 0,1,0, green) when
    that side is the holder's own or an ally,
    `UIColorPropertyTemporaryEnemy` (1,0,0, red) when it's an enemy, and
    `UIColorPropertyDefault` (1,1,1, white) otherwise. So a stun on the
    player's tank, from an enemy AT gun, is red, and a special operation
    the player used on their own units is green. The property's data
    gives no colour.
  - from the side, as a technology or the handicap's property, or from
    `LOCAL` research: `UIColorPropertyTechnology` (1,1,0, yellow);
  - from a building's `[TechnologyProperty]`, or from a unit's
    `[ElementComponent]` `SupportProperty` list (which goes to its
    regiment as the unit joins; no AA unit has one):
    `UIColorPropertySupport` (0,0,1, blue).
  A property a regiment hands down to its units keeps on each unit the
  tint it has on the regiment ([A unit's row](#a-units-row)). The row
  uses none of the other `UIColorProperty*` values (`Intrinsic`,
  `StateAndFormation`, `Terrain`, `Experience`, all white).
- An icon path is read as any other path in the data: one starting with
  `/` from the data's top folder, any other from the folder of the file
  that gives it. A `%s` in it is the resolution set
  ([interface](interface.md#resolution-sets)). The file is then looked
  up as a texture: the exact path, ignoring case, with no other
  extension tried; one that doesn't exist gives the default texture,
  as for a model ([data](data.md#textures)). [code]
- So two shipped properties show the default texture in place of an
  icon: `code_breaker_damage` (on the Code Breaker tower), which sets
  no `icon` and gets `ModifierProperty`'s default
  `/Properties/PropertyIcons/IconPropertyGeneric.tga` (only a `.png` of
  that name ships), and `economic_aid_money_decay` (on the Economic Aid
  money), whose `PropertyIcons/IconPropertyHealth.png` is looked for in
  `SpecialOps/Buildings/PropertyIcons/`, which doesn't exist. Each
  still takes a place and has its tooltip. [code, data]
- The row is brought up to date about four times a second, not on the
  step a property changes. [code]
- A property's tooltip is its `name`, then a line titled
  `ui_tooltip_item_title_required_properties` ("Affects") naming its
  `required_properties` (those with a blank name, like `mobile`, add
  nothing), then one line per modifier: [code, game]
  - `TerrainSpeedModifier`: `modifier_desc_add_terrain_speed_cap`
    ("Speed limit is %s") with a word from `Game/descriptions.tgi`: the
    first `SPEED` entry in file order whose `range` holds the amount,
    lower bound included, upper excluded. Stock: 0.1 to 1.5 Very Slow,
    2 to 3 Slow, 3 to 4 Medium, 4 to 4.5 Fast, 4.5 up Very Fast. An
    amount no entry holds (1.5 up to 2, so the jungle vehicle
    property's 1.5) gets no line at all. [code, data]
  - `ResistDamageModifier`: the damage type's glyph and the amount as a
    percentage, `modifier_desc_resist_damage_positive` ("resistance")
    for a positive amount and `..._negative` ("vulnerability") for a
    negative one. [code, game]
  - `AddModifier` on `DV`: `modifier_desc_add_dv` ("%+g to Defense
    Value"). [code]
  - Lines for a change that hurts the holder (a speed limit, a
    vulnerability) are red. [game]

### A unit's row

What a regiment's unit holds, which its row shows when the display is
turned to it ([A unit out of its regiment](#a-unit-out-of-its-regiment)).
The rules above all apply; this is what they come to for a unit.

- A regiment hands down to its units every property it holds, as it
  gains it, unless the property sets `pass_down_members = false` (in AA
  only `mobile`, `oil`, `ammo` and `money`, none with an icon). Each unit
  takes it only if it has the property's `required_properties`
  itself. When the regiment loses the property, its units lose the copy
  it gave them. [code, data] That covers:
  - its own properties, the ones its template lists (Recon,
    Trailblazing);
  - its terrain properties: each unit has those of the regiment's
    terrain that fit it, so infantry the infantry ones and vehicles the
    vehicle ones ([terrain](terrain.md)). A unit inside its regiment's
    area (or its leader) goes by the regiment's terrain, not the ground
    under itself; one straggling outside it goes by its own ground;
    [code]
  - its formation's property, or Broken Formation in its place;
  - its activity's property (Guarding, Chasing, Exhausted, Reforming,
    Routing, Engaged) and Targeted;
  - entrenchment, by kind: infantry take `infantry_entrench` and
    vehicles `vehicle_entrench` ([stances](stances.md));
  - its experience level's property ([morale](morale.md#a-regiments-experience)),
    since no AA unit keeps experience of its own;
  - its side's technologies, which the unit already holds from the side
    in any case, so they add nothing to its row. [code, data]
- A unit also holds, on its own account: [code, data]
  - its template's properties, inherited ones included (`infantry`,
    `tank`, `vehicle`, all `IconPropertyNational.png`), in the order
    above (its own first, then each ancestor's);
  - its side's technologies that fit it (`tech_diesel_engines` on a tank,
    not on a rifleman);
  - its own activity's property: Engaged while it's engaged itself
    ([combat](combat.md#engaged-and-under-siege)), even when its
    regiment isn't. Its other activities' properties have no icon
    (Resting's is `null`);
  - its stance's property while kneeling or prone (`infantry_kneel`,
    `infantry_prone`, both `IconPropertyMovement.png`, untinted), gained
    when it takes the stance and lost when it leaves it
    ([stances](stances.md));
  - an ability's property that caught it, a special operation's or a
    stun, for its `duration`.
- Being under siege gives a unit no icon: `actor_state_contested` needs
  `property_city`. [data]
- Order, oldest first, for a unit made with its regiment (on the map at
  the start, recruited, or grown back as a replacement): its template's
  properties, then its side's technologies, then what its regiment holds
  at that moment in the regiment's own order (terrain, the regiment's
  own properties, then its formation, activity and so on), leaving out
  any it already has. After that, whatever it or its regiment gains in
  play goes to the end, as it comes. [code] So a German rifleman in a
  regiment on forest reads Infantry, then the side's technologies that
  fit infantry, then Forest, then the regiment's own, then its
  formation; when the regiment engages, its new formation and Engaged
  are added at the end, and the man's own engagement adds nothing more.
- Tints follow the source, as on the regiment: technologies yellow,
  an ability's property by standing (above), the rest white. [code]
- A regiment never gets an ability's property in AA, so a unit never has
  one handed down. [data] Were it to, the unit's copy would show white
  and last as long as the regiment's. [code]

## Technology display

The window listing the side's technologies, laid out by
`UI/Game/technology_display.tgi`.

- The control panel's `ButtonTechnology` opens it: a push button at the
  right of the control panel, the second of the row of four tabs
  (objectives, technology, message log, politics; 883, 40 and 45 by 52
  on the layout), drawn with the `SidePanelTabTechnology*` images (`Selected`
  while held down; it doesn't stay lit while the window is open). Its
  tooltip name is "Display Technologies"
  (`control_panel_ButtonTechnology_ToolTip`) and its text "Displays a
  list of all your technologies." (`..._tooltip_2`). F2 presses it
  (`hotkeys_custom.txt`), and its name shows the key as "(F2)Display
  Technologies" (see [Hotkeys in tooltips](#hotkeys-in-tooltips)).
  [data, code]
- It's a window of its own over the game screen, at the place and size
  its layout gives (385 by 513 at 321, 53 on the 1024 by 768 layout),
  titled "Technology Panel" (`technology_display_Child_text`), with the
  list in a scrolling box below the title. [data]
- Pressing the button with the window open closes it, so F2 opens and
  closes it. It also closes with its close button at the top right, the
  "Close" button at the bottom, Esc or F2 while it has the keyboard, and
  Backspace, which closes the objectives and technology windows (and the
  game's other display panels) together. [code, data]
- Opening it in a single-player game pauses the game,
  unless it's already paused; closing it unpauses, but only if opening it
  was what paused it. In a multiplayer game, hosted or joined, it pauses
  nothing and the game keeps running (see
  [interface](interface.md#windows-over-the-match)). [code]
- Opening it closes nothing else: the Technology Panel, the Objectives
  window and the Message Log page each open and close on their own, so
  any of them can be open together. They overlap in much the same place,
  the one opened last on top. [code, data]
- Since each window only lifts the pause it set itself, opening one in
  a single-player game and then another leaves the pause to the first:
  closing the first unpauses the game while the second is still open.
  [code]
- Neither opening nor closing it makes a panel sound; only the button
  press sounds (`push_button`). [code]
- It lists the technologies the player's own side holds, from every
  source: `GLOBAL` research, the map's start technologies, the
  `kingdom_gains_technology` trigger, the handicap's property (Easy and
  Hard show as a technology of that name), and buildings granting them
  through `[TechnologyProperty]` such as the supply drops (see
  [research](research.md#how-a-side-gets-and-loses-technologies)).
  Nothing that's only on one building or unit appears: not `LOCAL` or
  `SUPPORT` research, not an actor's own properties such as
  `tech_battleship`, not research under way or available. A technology
  with no `icon` isn't listed; every shipped one has one. [code, data]
- One line per grant, so the same technology can show more than once:
  two supply-drop buildings granting the same one, or a trigger giving a
  technology the side already has, each add a line, and each line goes
  when its own grant does. [code]
- Order is the order the grants came, oldest first; a lost one closes
  the gap. [code]
- Each line is the technology's `icon` (16 by 16) and then its `name` in
  the tiny font, wrapped onto more lines if it's too long. A line is as
  tall as its name, and never less than 20. [code, data]
- Hovering a line shows the technology's tooltip in the same place as
  the game screen's other tooltips (`anchored_tooltip_*`, the same values
  as `game_interface.tgi`). It's the technology's property tooltip (see
  [Property icons](#property-icons): name, the "Affects" line, a line per
  modifier) followed by a blank line and its `description`, when it has
  one. The research menu's tooltip is the same apart from its cost line.
  [code, data]
- With no technologies the box is empty, with no text in it. [code]
- While it's open the list is brought up to date once a second, and at
  once when it opens. So a technology gained or lost with the window open
  shows up or goes within a second. [code]
- Clicking a line does nothing; the window's only actions are closing
  it. [code]

## Portrait box

The picture in the object display (`PortraitButton` of `UnitDisplay` in
`UI/Game/template_game.tgi`). The button holds two model views, `Portrait`
and `ModelPortrait`, and shows one of them at a time.

### Which picture shows

- A thing's portrait is the `Portrait` thing its `[View]` names in
  `portrait_ids`. A thing with a portrait shows it in `Portrait`; a thing
  without one shows its own model in `ModelPortrait` (the fitted model,
  below). [code, data]
- A thing with `show_faction_portrait = true` shows its owner's general
  instead: the portrait named by `portrait_ids` of the owner's general.
  The owner is the one the viewing player takes the thing to belong to.
  [code]
  - If the owner has no general, the thing falls back to its own
    `portrait_ids`, and to its fitted model when it has none. [code]
  - If the general names no portrait, the thing's fitted model shows.
    [code]
- A single selected thing that isn't a regiment (a Corps HQ, say) uses
  its own template for the rules above. [code] A packed Corps HQ is its
  truck, which names no portrait and doesn't show the general, so its
  fitted model shows (`jpn-corps-hq-truck.jpg`). [game, data]
- A selected regiment uses the template its definition names for its
  layout's primary slot (the `PrimarySlot` of the layout, `captain` in the
  shipped layouts; the matching `[Slot]`'s `element_IDS`, such as
  `usa_infantry_co`). It is the template the regiment names, not the unit
  standing in the slot, so the picture stays when the officer is dead. A
  regiment whose layout has no primary slot shows no picture. [code, data]
  - When that template is one that packs or unpacks, the picture follows
    what the HQ is now. While it is unpacked or unpacking, the building
    the template unpacks to (`unpack_to_actor_ids`) is used when the
    template names one; otherwise the truck that packs back from it is
    used when the template is the building. So a Division HQ regiment
    shows its truck while it is a truck, and from the moment it starts
    unpacking shows what the building shows: the general, as
    `AABuildingPackableHQ` sets `show_faction_portrait`. [code, data]
    Which of the two shows while the building is packing up again is
    open.
- A selected city uses the city's own template. [code]
- The group display (several things selected) shows no officer or
  general: its small entries set `uses_icon`, so a lone thing's entry
  shows its icon, and a regiment's entry shows the regiment's banner.
  [data] See Open questions for how the banner is drawn.

### The portrait's view

- When the shown model holds a camera named `PortraitCamera`, the picture
  is that camera's view exactly: its place, direction and field of view as
  the model file gives them, moving with the model's animation if the
  camera is animated. [code]
- Only a real camera counts. A plain node named `PortraitCamera` (as some
  building models have) is ignored, and the view is set up as for a model
  with no camera. [code]
- With the model's own camera the picture fills the whole view rectangle
  and is stretched to it: a square camera window over a box wider than it
  is tall comes out slightly wide. Nothing is cropped and no bars are
  added. [code]
- The view's `position`, `look_at`, `zoom` and `fit_model` play no part
  when the model's own camera is used. [code]
- A model with no camera of its own is seen by the view's camera: [code]
  - It has the world camera's field of view (`CameraFOV`, 15 degrees
    across the width) and a picture 4 wide by 3 high, whatever the box's
    shape.
  - That 4:3 picture is fitted inside the view rectangle, centred, as
    large as it goes without stretching; the rest of the rectangle (bands
    above and below in a box taller than 4:3, at the sides in a wider
    one) is the backdrop colour.
  - With `fit_model = FALSE` the camera looks at `look_at` from the
    direction of `position`, at `zoom` times the distance between the
    two (`zoom` is held between 0 and 1), so with `zoom = 1` it stands at
    `position`.
  - With `fit_model = TRUE`, see the fitted model.
- The view's `color` is the backdrop: the picture is cleared to it before
  the model is drawn, so `0,0,0` is a black background behind the model.
  [code]
- Officer folders' `...PortraitBg.dds` files are used only by a model that
  names them as a texture. The game never asks for them by itself, so one
  that no model names (the officers') is never drawn. The generals'
  portrait models name theirs. The sky and field behind an officer is
  part of the head mesh and its `...PortraitBase.dds` (see Lighting and
  colour). [code, data]

### Terrain variants

- Variants come from the general texture rule in [data](data.md),
  Textures: on a map whose biome sets `texture_subfolder_name` (`Arctic`
  for `arctic_tundra` and `boreal_forest`, `Desert` for `desert`), a model
  or texture file is taken from that subfolder of its own folder when the
  file exists there. So on an arctic map an officer's portrait is the
  `Arctic/` portrait model where there is one, with the `Arctic/`
  textures, and on a desert map the `Desert/` textures. [code, data]
- Nothing switches the `icybreath` or `snow` quads on or off: they are
  drawn whenever the model that holds them is. They show or not by their
  textures: the plain `PortraitBreathBase.dds` is a 4 by 4 fully
  transparent texture, the `Arctic/` one a real breath image, and
  `PortraitSnowBase.dds` exists only under `Arctic/`. [code, data]

### Animation

- `UIPortraitAnimate` (stock `true`) switches it all. When off, no
  animation runs on the shown model and the picture is drawn once when it
  changes, not every frame. [code]
- The portrait box's views set `real_time_update`, so their animations
  run on real time and carry on while the game is paused. [code, data]
- With animation on, the model's `IDLE` and `FIDGET` animations alternate:
  [code]
  - The idle plays first, for a whole number of loops picked at random
    from `idle_loops_min` up to but not including `idle_loops_max` (of
    the portrait's `[View]`; stock 1 and 3, so one or two loops), each
    loop lasting the animation's `length` seconds (stock 3). The clip is
    sped up or slowed to last `length`.
  - Then one of the model's `FIDGET` animations plays once, for its
    `length`, each equally likely and the same one allowed twice
    running. Then the idle again with a fresh loop count, and so on.
  - A model with no fidget repeats its idle. Where there are several
    `IDLE` animations each idle is picked at random the same way.
  - An officer's `talk1` is a `FIDGET` like the other two, so it comes up
    at random one time in three. No selection or order triggers it. [code,
    data]
- A `SELECTED` animation (the generals' `talk1`) plays when the portrait
  is put up by the single-thing display or the city display, that is each
  time such a thing is selected: it cuts in over the idle or fidget, plays
  once for its `length`, and the idle follows. One is picked at random if
  there are several. [code] Its sound is under [sounds](sounds.md), Unit replies. It is started
  at the moment the portrait is put up only, and only if the animation
  is ready to play then: if it isn't (its model still loading), neither
  the animation nor its voice line plays, and the portrait goes straight
  to its idle. Nothing tries again later. [code]
- The general never talks on his own. No timer, chance, match-start
  step or game event plays the `SELECTED` animation or the general's
  voice line; the only way is the portrait being put up as above.
  [code, data]
  - The general's portrait otherwise does what an officer's does: idle
    and two fidgets (`AAGeneralPortrait` in
    `Templates/template_faction_aa.tgi`); its `talk1` is its only
    `SELECTED` animation and never comes up as a fidget. [code, data]
  - At match start nothing is selected: placing the camera on the start
    location selects nothing, and the object display shows its at-rest
    look ([interface](interface.md#at-rest)). [code]
  - A player who heard the general talk as a match began found the
    Corps HQ selected at the time, so it was this same talk on
    selection. What selected the HQ that early (most likely a stray
    click) wasn't found. [game]
  - So in a match a general talks when a lone thing that shows the
    general is selected: a Corps HQ (`AABuildingPackableCorpHQ`). [code,
    data]
  - The regiment display never starts it, so selecting a Division HQ
    regiment shows the general idling. [code]
  - Selecting something else while the general talks takes his portrait
    down at once, animation and all, for whatever the new selection
    shows. Whether his voice line is cut off with it or plays to its end
    is unchecked; nothing was found that stops it. [guess]
- Selecting another single thing always puts up a fresh portrait, starting
  its animation over, even when it is the same portrait. Selecting another
  regiment whose picture is the same portrait keeps the one already
  showing, mid-animation; the same holds while one regiment stays
  selected. [code]

### The fitted model

Used when there is no portrait (`ModelPortrait`, `fit_model = TRUE`):
buildings, trucks and the like.

- The model shown is always the template's plain `AspectNormal` model at
  the origin, unturned, as its file has it. It is a copy made for the box,
  not the thing on the map: the thing's facing, its damaged, packing or
  unpacking look and whatever its map model is doing don't show. (For a
  template with `aspects_as_variations`, the selected thing's own
  variation is the one shown.) [code]
- The copy runs its own animations by the rules under Animation. [code]
- The camera looks from the direction of the view's `position`, which the
  stock `ModelPortrait` leaves at the default `1,-1,1`: from the model's
  +x, -y side, 35.26 degrees above the horizontal. Only the direction of
  `position` matters; `look_at` and `zoom` aren't used. [code, data]
- It looks at the point above the model's origin at half the model's
  height, where the height is from its lowest to its highest vertex.
  [code]
- The distance is set so the model just fits top to bottom. With
  - `h` half the model's height,
  - `r` the larger of the aspect's two `ground_size` numbers or, where
    `ground_size` is 0 (the stock value), the larger of the model's reach
    along x and along y from its origin, measured over the vertices
    within 0.1 of its lowest point,
  - `e` the camera's elevation (35.26 degrees), and
  - `v = CameraFOV × 0.375` (5.625 degrees, about half the picture's
    height as an angle),

  the camera stands `sqrt(r² + h²) × sin(180° - atan(h / r) - v - e) /
  sin(v)` from the point it looks at. That puts the near bottom edge of
  the cylinder of radius `r` and height `2h` on the bottom edge of the
  picture. [code]
- The picture is the 4:3 one described under The portrait's view, so the
  model fills that picture's height, not the box's: in a box wider than
  4:3 the picture is the box's full height with backdrop at the sides.
  [code] In `jpn-corps-hq-truck.jpg` the box is wider than 4:3 and the
  truck, seen from above its front corner, takes about four fifths of the
  box's height and two thirds of its width on black. [game]
- The model is drawn in its owner's colour, as on the map (the owner as
  the viewing player knows it; the game's default colour if it has none).
  [code, game]

### Lighting and colour

What lights a shape in the box depends on the shape: skinned shapes
take the map's sun, other lit shapes take the box's own white light,
and shapes whose texture replaces the colour beneath aren't lit.

- Skinned shapes (meshes with bones) are lit by the map's sun exactly as
  skinned units on the map are ([lighting](lighting.md#models)): glow +
  material ambient × sun ambient + material diffuse × sun diffuse ×
  max(0, normal · −sun direction), each channel capped at 1, times the
  texture. The sun's colours are the ones the map shows at that moment,
  without the dimmer and without pausing's darkening. [code, game]
  - The direction is the sun's as the map has it, taken in the shown
    model's own axes (the model sits unturned at the view's origin).
    [code]
  - It follows the sun every frame: when the colours change over a match
    (Time of Day, a trigger), the portrait changes with them, smoothly,
    as the map does. [code]
  - This is when the graphics card does the skinning; otherwise see
    [lighting](lighting.md#open-questions).
  - Officers' portraits (`Units/*/*/*COPortrait.nif`) are like this.
    Their head (`IHead`) is one skinned mesh that holds both the face and
    the sky-and-field backdrop behind it, both mapped from the one
    `...PortraitBase.dds` (the face on its left half, the landscape on
    its right), so the backdrop takes the sun exactly as the face does.
    Nearly all have material ambient and diffuse 1, so under AA's white
    day sun the face shows its texture as stored, and under `sun_night`
    (ambient and diffuse 0.4, 0.4, 0.7) a face turned fully to the sun
    comes out at 0.8, 0.8, 1 times its texture and one turned away at
    0.4, 0.4, 0.7: the night's blue-violet cast, the same as on the map.
    [code, data, game]
- Other shapes that take light are lit by the box's own light: a
  directional light whose diffuse, ambient and specular are all white
  (1, 1, 1), dimmer 1, shining along the map's sun direction as it stood
  when the view first showed a model (in the model's own axes, not
  turned again afterwards). With it a shape comes out at glow + material
  ambient + material diffuse × max(0, normal · −direction), each channel
  capped at 1, times its texture. No stock portrait or fitted model holds
  lights of its own (any that did would apply as usual). [code, data]
  - The time of day and the dimmer don't reach these shapes. [code]
  - The officers' breath and snow quads (`icybreath*`, `snow*`; see
    Terrain variants) are such shapes. [data]
  - The fitted model is lit the same way from its own materials, apart
    from any skinned shape in it, which takes the sun as above: the
    truck in `jpn-corps-hq-truck.jpg` is clearly shaded, its top
    brighter than its front and side. [game, code]
- A shape whose texture replaces the colour beneath it (texture apply
  mode 0 with a base texture) isn't lit at all and shows its texture as
  stored ([lighting](lighting.md#glowing-shapes)), skinned or not.
  Every general's portrait (`Generals/*/*/*Portrait.nif`) is made like
  this, head and backdrop (`bg`, or `Plane01` in some) alike, and their
  meshes carry no normals. So a general's face and backdrop always show
  exactly their textures, a factor of 1, evenly, at any time of day: on
  `(2v2)Berlin` at night the general looks as at noon
  (`berlin-night-jpn-corps-hq-door-light.jpg`). They have no alpha
  property, so nothing is blended or doubled. The general's head is
  skinned, but a replacing texture overrides the sun's skinned lighting
  too. [code, data, game]
- A portrait model (an officer's or a general's) never takes the owner's
  colour: only a fitted model does. [code]

## Which display shows

- Several things selected: the group display, with one card per thing
  (see The group display's cards, name and owner). [code]
- One regiment selected: the regiment display (`CompanyDisplay` in
  `UI/Game/object_display.tgi`), whose banner box (`BannerButton`, at
  363,47) carries the banner with the efficiency bar above it and the
  morale bar below. Selecting any member of a regiment selects the
  regiment ([selection](selection.md)), so this is what every unit or
  building in a regiment shows. [code, data]
- A Division HQ is a regiment (`regiment_<nation>_hq_*`, holding its
  truck or building), so it gets the regiment display, banner box and
  all, in every state: a truck, unpacking, unpacked, packing. Its HQ
  places stand where an ordinary regiment's unit cards would
  ([supply](supply.toml#attaching-to-an-hq)). [code, data]
- Anything else selected alone gets the lone thing's panel (below),
  which has no banner box: the Corps HQ (a building in no regiment,
  packed or not), other buildings in no regiment, cities, and lone
  units. So the Corps HQ never shows one. [code, data]

## Object display: a lone thing's panel

What the object display shows for one selected thing that isn't a
regiment: a building, a city, a lone unit (`SimpleActorDisplay` in
`UI/Game/object_display.tgi`, built on `UnitDisplay` and
`EconomyUnitDisplay` in `UI/Game/template_game.tgi`). The portrait,
health bar, property icons and economy line are under their own
headings; this covers the rest.

- The panel has no backdrop: the space its parts leave shows the
  control panel's background, with the nation's emblem on it
  ([interface](interface.md#the-control-panel)). No AA building's
  template has a description, so for a building the area right of the
  weapon and defense values is always that background. [data, game]

### Name, owner and state lines

- The name line is the thing's own display name while the player sees
  it: the name the map gives it when it gives one (a city's, such as
  `name_3954 = "Stettin"` in the map's strings), else the template's
  `name`. For a thing only remembered it is the `name` of the template
  the player remembers there. [code]
- The owner line names the side the viewing player takes the thing to
  belong to, and is blank when that is nobody. [code]
- It is a colour chip, a space, then the side's name. The chip is in the
  side's own colour; the name is tinted by how the viewing player stands
  towards that side ([players](players.md), Relations): [code]

  | Relation | Name colour (r, g, b) |
  |---|---|
  | own side | 0, 1, 0 (green) |
  | ally | 0, 0, 1 (blue) |
  | neutral | 0.8, 0.8, 0.8 (light grey) |
  | enemy | 1, 0, 0 (red) |

  `metagame_object_display_Owner_ToolTip` words the same rule. [data]
- The chip is left out when the owner is an independent side (one of a
  map's `independent_N` sides, as opposed to its `kingdom_N` ones). The
  space and the tinted name stay. [code, game]
- The chip is the same gem glyph that marks a side in message lines
  ([messages](messages.md)), and is left out for exactly the same sides.
  [code]
- The side's name is the one [players](players.md#side-names) describes:
  in a skirmish or multiplayer match a side a player took is named after
  that player ("Kesselring AI" for a computer one), and other sides keep
  the map's name. For an independent side that is the map's own string, whole.
  "Civilian (Aggressive)" is `independent_5` in the Berlin map's strings
  and "Civilian (Neutral)" its `independent_2`: the bracketed word is
  part of the name the map's author gave, not something the game adds.
  It is red because the player's side stands as enemy towards that
  side. [code, data] An independent side the player is neutral towards
  would read in light grey. [guess]
- The state line is the name of the thing's current state, and is blank
  for a thing the player isn't seeing now. [code]
- A thing can be in several states at once (`Game/actor_states.tgi`,
  `actor_states_aa.tgi`); the line names the one with the highest
  `priority`. A state that sets no priority has 0 (the template's
  default), as `structure_normal` does. On a tie the state with the lower
  number wins, by the numbering the triggers use
  ([triggers](triggers.md), `actor_has_state`), not the order of the
  files. [code, data]
- Which states a thing can be in decides what it reads: [code, data]
  - Every thing with a regiment-style activity is always in one, and
    with nothing to do that is `mind_resting`, "Resting" (priority 4).
    That covers regiments, lone units (a truck included) and every
    building whose template comes from `AABuildingPackable`, the
    buildings that pack into a truck: the Corps HQ, the Division HQ
    buildings, the Supply Depot, Engineer and Artillery Brigades, Motor
    Pool and Airfield. In AA these are the only buildings that recruit
    or research. [code, data, game]
  - Every building is also in one structure state ([commands](commands.md#a-buildings-state-and-its-commands)):
    "Normal" (priority 0) with an empty queue, "Deploying"
    (`structure_recruiting`, 8) while a recruit of any kind is at the
    front of its queue, plain or Deploy to Location, and "Researching"
    (8) while research is. [code, data]
- So a lone building reads: [code, data, game]
  - a packable one (a Corps HQ, a depot, a brigade, a motor pool, an
    airfield): "Resting" when its queue is empty, "Deploying" or
    "Researching" while it builds something, since 8 beats Resting's 4.
    An idle Corps HQ read "Resting" and one commissioning a Division HQ
    "Deploying" (`southern-europe-corps-hq-commissioning.jpg`,
    `jpn-corps-hq-commissioning-airborne-hq.jpg`). [game]
  - any other building (a bunker, a city's factory, bank or refinery, a
    village building), captured or not: "Normal", as it has no activity
    and no queue. [code, data]
  - either kind shows a higher state over these when it's in one:
    Packing or Unpacking (14), Under Siege (12), Under Construction (10).
    Out of Supply (6) shows over Resting and Normal but not over
    Deploying or Researching; Disabled (4) shows over Normal but not
    over Resting, which ties with it and has the lower number. [code,
    data]
- A Division HQ is a regiment ([Which display shows](#which-display-shows)),
  and the regiment display's line names the regiment's own state, not
  its building's. The regiment never takes the building's queue states,
  so a Division HQ reads "Resting" while it recruits, whether by Deploy
  or Deploy to Location, and never "Deploying" or "Researching" (no
  Division HQ researches in any case). Its other states are those of
  any regiment, such as Moving while its truck drives. [code, game]

### Weapon and defense values

The small icons with a number between the portrait and the right-hand
box (`Values`, from the `DisplayValues` template; a pistol with 22 and a
shield with 5 in `ger-corps-hq-weapon-armour.jpg`).

- Only the lone thing's panel has them. The regiment display and the
  group display's cards show none, though the layout file gives the
  regiment display the same `Values` block. [code]
- They show for a thing the player sees now, whoever owns it, and are
  hidden (with the property icons and the supply stockpile bar) for one
  only remembered. [code]
- The shield is the defense value. It shows for anything with a
  `BodyComponent`, so nearly every selectable thing, a city included.
  [code]
  - Its icon is the `BodyComponent`'s `defense_icon`
    (`/Properties/AbilityIcons/IconAbilityDefense.png` in the stock
    unit and building templates). [code, data]
  - The number is the thing's defense as it stands now: the template's
    `defense` plus the `DV` amounts of the properties it holds, as
    damage uses it ([combat](combat.md), Damage). It is written as a
    whole number, rounded ("%.0f"). A stock city has `defense = 0`, so
    its shield reads 0. [code, data]
  - The number is tinted `UIColorModifierPositive` when it is above the
    template's `defense`, `UIColorModifierNegative` when below, and
    left untinted when equal. [code]
- Each weapon value is one of the thing's abilities: its own first, then
  those of its defender mount (an HQ's gun). [code]
  - The icon is the ability definition's `icon` (such as
    `/Properties/AbilityIcons/IconAbilityDamageGun.png`). [code, data]
  - The number is the ability's main damage as it stands now, with its
    modifiers, written and tinted like the defense value against the
    definition's plain damage. An ability whose plain main damage is 0
    shows its icon with no number. [code]
  - At most `max_widgets_values` (5) show. [code, data]
- A thing with neither weapons nor a body shows nothing here. [code]
- Placement comes from the layout: the block is at 144, 46 in the
  panel, 86 wide and 105 high; a weapon value is 50 by 20 at the
  block's left edge and the defense value 40 by 20 starting 52 to the
  right, each a 16 by 16 icon with the number in the `tiny` font from
  18 to the right of the icon's left edge. [data] The first weapon and
  the shield share the top row. [game] Further weapons go below, a row
  each, `display_values_separation` (1) apart. [guess]
- Hovering the shield gives: "Defense Value"
  (`ui_tooltip_defense_value`), the number (tinted as on the panel),
  then one line per resistance or vulnerability among the properties the
  thing holds (as a property's tooltip words them, see Property icons),
  then `template_game_DefenseValues_ToolTip` ("Damage from enemy
  attacks is reduced by defense value. Defense applies before
  resistances."). [code, game: `jpn-corps-hq-defense-tooltip.jpg`]
- Hovering a weapon gives the ability's name, its damage ("20 Damage",
  with the damage type's glyph), "Rate"
  (`ui_tooltip_item_title_recharge_rate`) with a word for its recharge
  time, and "Area" (`ui_tooltip_item_title_area`) with a word for its
  area when it has one; then "Bombard" (`ui_tooltip_bombard`) or
  "Vorpal" (`ui_tooltip_vorpal`) on a line of its own for an ability of
  that kind. The words come from `Game/descriptions.tgi` as a speed's
  does (see Property icons): `RECHARGE_RATE` for the rate, `AREA` or
  `AREA_VOLLEY` for the area. [code, game:
  `jpn-corps-hq-weapon-tooltip.jpg`]

### Description box

The bordered box of text on the right (`DescriptionLabel` of
`UnitDisplay`).

- Its text is the selected thing's template `description`. A city's is
  `AAVillageCenter`'s (`Templates/template_buildingvillage_aa.tgi`),
  the string `template_buildingvillage_aa_AAVillageCenter_description`.
  The layout's own text for the label
  (`template_game_DescriptionLabel_Label_text`, "replaced by code")
  never shows. [code, data]
- It shows for any lone thing whose template has a description, whoever
  owns it and whether or not the player sees it now; a thing with no
  description shows no box at all, border included. [code]
- It gives way to the commission queue: while the thing has at least one
  item queued and the viewing player owns it or is its owner's ally,
  the queue shows in that space and the box is hidden. With the queue
  empty the box is back. Other players never see the queue, so for them
  the box stays. [code]
- The regiment display and the group display never show it. [code]
- From the layout: the box is at 244, 46 in the panel, 239 by 104,
  drawn with `/UI/%s/Game/ControlPanel/DescriptionBox.png`; the text
  sits 5, 5 inside it in a 230 by 96 area, `small` font, left-aligned,
  `multiline` (wrapped to that width). The label has no tooltip. [data]

### A city

- A selected city gets this same panel: there is no separate city
  display. The layout file's `SettlementDisplay` block (and its
  `display_defense_values = false`) is not used. [code]
- What differs from an ordinary building comes from the city centre's
  own data: its name line is the city's name from the map, its
  description is the occupy line, its defense value is 0, it has no
  weapons, and it never has a queue, so the description box always
  shows. [code, data] Its economy figures count the whole city's
  buildings (see
  [economy](economy.md#the-selection-panels-economy-line)). [code]

### A unit out of its regiment

A left click on a regiment's portrait or on a living unit's card turns
the display to that one unit while the regiment stays selected (see
[The portrait](#the-portrait) and [Unit cards](#unit-cards)).

- The display then shows this same panel, for the unit, exactly as for a
  lone thing: nothing in it is laid out or filled differently because
  the unit belongs to a regiment. There is no banner box, morale or
  efficiency bar, unit cards or HQ places, and no formation buttons:
  those belong to the regiment display. [code]
- Turning to the unit puts its portrait up afresh and plays one of its
  `SELECTED` animations, as selecting a lone thing does (see Portrait
  box). [code]
- What each part shows for a unit: [code, data]
  - Name: the unit template's `name` ("Medium Tank"). Units carry no
    number, so there's no "1st"; only regiments are numbered.
  - Owner: the regiment's side, as the owner line always reads.
  - State: the unit's own state, not its regiment's activity. A unit
    has its own activity within the regiment, and the line names the
    highest-priority state among: Resting (4) when it has nothing to
    do, Moving (8) while it moves (taking its place in formation
    included), Engaged (12) while it attacks a target of its own,
    Bombarding, Joining, Reattaching or Unpacking (8) while it does
    those, Repairing (8) while it repairs, Building (8) while the site
    it works on is still under construction, Razing (8) while it razes
    (Bombarding, Unpacking, Repairing, Building and Razing read Moving
    while the unit is still on its way), Under Siege (12) for some
    seconds after an enemy hits it
    ([combat](combat.md#engaged-and-under-siege)), and Falling (10)
    while it drops. Engaged beats Under Siege on their tie. So a
    regiment reading "Guarding" or "Engaged" can have a unit reading
    "Resting", "Moving" or "Under Siege". The regiment's own states
    (Out of Supply, entrenched, Targeted, Guarding and its other
    activities) are never the unit's.
  - Portrait: the unit's own, by the rules of Portrait box.
  - Health bar: the unit's own health over its maximum.
  - Weapon and defense values: the unit's own abilities and defense,
    with the properties it holds (so a handed-down technology or
    entrenchment counts).
  - Property icons: the unit's own row ([A unit's row](#a-units-row)).
  - Economy line: blank, as the unit counts for nothing by itself
    ([economy](economy.md#the-selection-panels-economy-line)).
  - Description box: none. No AA unit template has a `description`.
    [data]
  - Supply stockpile bar: only for a unit that is a supply relay, as
    for any lone thing.
- A Division HQ's captain is its truck or building, so its portrait
  turns the display to that truck or building alone. The building then
  shows what any lone building shows, its commission queue and its
  "Deploying" state line included, which the regiment display never
  shows ([Name, owner and state lines](#name-owner-and-state-lines)).
  [code]
- Orders are unchanged: the action buttons, right-click orders and keys
  all act on the selection, which is still the regiment. Giving an
  order doesn't turn the display back. [code]
- The display turns back to what is selected (the regiment display,
  for the regiment) when: [code]
  - the selection changes in any way, even to the same regiment again:
    a click on any of its units in the world, recalling a control group
    that holds it, a double-click, a Shift- or Ctrl-click, or the
    selection dropping something. Pressing a control group's key again
    to centre the camera on it changes nothing, so the display stays
    on the unit;
  - the unit dies. It goes back at once, when the unit is struck down,
    not when its body is cleared away. A unit taken out of play some
    other way, such as a Division HQ's truck or building swapped for
    the other as it unpacks or packs, does the same.
  Nothing else does: clicking the unit's portrait (see
  [The portrait](#the-portrait)), being hit, moving, or the regiment
  changing formation or activity leaves the unit shown. To get back to
  the regiment without changing the selection there's no click on the
  panel; selecting the regiment again does it.
- When the regiment is destroyed its last unit's death has already
  turned the display back; once the regiment leaves the selection
  nothing is selected and the panel empties. [code]
- Each of these happens at the display's next update, within a frame.
  [code]

## Object display tooltips and clicks

What hovering and clicking the parts of the object display does
(`UI/Game/object_display.tgi`, `UI/Game/template_game.tgi`). None of it
has been watched in the game yet.

### Who answers a hover

- The display answers for most of its own parts, and what it gives
  replaces the part's own `ToolTip` string. So the strings that are only
  `%s` (`template_game_PortraitButton_ToolTip`, `..._ToolTip_2`,
  `object_display_CompanyDisplay_BannerButton_ToolTip`), the "Set By
  Code" ones and the bars' own strings
  (`template_game_HealthBar_ToolTip` "Health %.0f/%.0f", the morale,
  efficiency and supply stockpile bars' strings, and the queue's
  "Progress %.0f/100") are never what shows on these parts: nothing
  fills their `%` fields in. A part's own string shows only when the
  display has nothing to give, as with an empty attachment place. [code]
- What the display gives is nearly always a *thing tooltip* (below) for
  one thing, chosen by the part: [code]

  | Part hovered | Lone thing or building selected | Regiment selected |
  |---|---|---|
  | Portrait (picture, its frame) | the thing | the regiment's captain |
  | Health bar | the thing | the captain |
  | Supply stockpile bar | the thing | the captain |
  | Banner, morale bar, efficiency bar | (none there) | the regiment |
  | A unit card (picture or bar) | (none there) | that unit |
  | An attached regiment's place | (none there) | that regiment |

  - The captain is the unit in the regiment's primary slot (the one the
    portrait is chosen from, see Portrait box). The portrait's tooltip is
    the captain's own, as a unit: not the regiment's, and not the
    general's even when the picture is the general. The general is named
    in it only as any thing's tooltip names him (below).
  - While the captain is dead and not yet replaced, these parts give a
    *template tooltip* (below) for the captain's template instead.
  - A unit card whose unit is dead gives the template tooltip for that
    position's template.
- The Name, Owner, State and economy labels each give their own text as
  the tooltip, in full. [code]
- The property icons and the weapon and defense values have their own
  tooltips (see Property icons, and Weapon and defense values). [code,
  game]

### The thing tooltip

Built for one thing as the viewing player knows it. Lines are left out
when they don't apply; the order is fixed. [code]

- Title: the template's `name`, without the number a regiment's display
  name carries ("Infantry Regiment", not "1st Infantry Regiment").
- "Health" (`ui_tooltip_item_title_health`): current over maximum, whole
  numbers ("45/60"), for a thing with a body. For a thing the player
  isn't seeing now, the remembered figures. The figure is tinted
  `UIColorModifierPositive` when the maximum is above the template's
  plain maximum, `UIColorModifierNegative` when below, and
  `UIColorModifierNeutral` when equal.
- Everything from here to the general's lines shows only for a thing the
  player sees now.
- "Supply Stockpile" (`ui_tooltip_item_title_supply_stockpile`): the
  stockpile over its maximum, whole numbers, for a supply relay.
- For a regiment: "Efficiency"
  (`ui_tooltip_item_title_attack_efficiency`) with its attack efficiency
  as a whole percentage (`ui_tooltip_attack_efficiency`, "%d%%"), then
  "Morale" (`ui_tooltip_item_title_morale`) with current over maximum.
- "Experience" (`ui_tooltip_item_title_experience`): for a thing that
  gains experience, or a unit whose regiment does, the points held, over
  the next level's when there is a next level, after the level's name
  when it has one.
- "Speed" (`ui_tooltip_item_title_speed`): the speed word for its speed
  now (the `SPEED` entries of `Game/descriptions.tgi`, as for a speed
  limit under Property icons), for a thing that moves or a regiment.
  Tinted like Health, by its speed now against its plain speed.
- "Transporting" (`ui_tooltip_item_title_transport_cargo`): the name of
  what a transport is carrying.
- The economy rows (see [The economy rows](#the-economy-rows) below):
  "Production", "Upkeep", and a "Maximum" under either.
- "Recruits", "Upgrades", "Researches", "Provides", "Technology" and
  "Properties": lists of names (see [The list rows](#the-list-rows)
  below), each left out when it would be empty.
- For a template with `show_faction_portrait = true` whose owner has a
  general: "General" (`ui_tooltip_item_general`) with the general's
  `name`, and, when the owner is the player or an ally, "Experience"
  (`ui_tooltip_item_experience`) with the owner's experience as a whole
  number. These show whether or not the thing is in sight.
- Last, the template's `description`, when it has one.

A title and its value sit on one line, the value at a fixed tab; each
row starts a new line.

A regiment's unit (on its card, on the regiment's portrait, or on the
panel while the display is turned to it) gets the same tooltip as a
lone unit, built the same way, with one difference: its Experience row
gives its regiment's experience points and level, since AA's units keep
none of their own. [code, data] So it reads: the unit template's name;
Health; Experience (the regiment's); Speed, by the unit's own speed;
the regiment's economy rows (below); Provides and Technology as for any
thing; Properties, from the unit's own properties, handed-down ones
included; and the General rows for a template with
`show_faction_portrait`. It has no Efficiency or Morale (those are the
regiment's) and no description. [code, data]

#### The economy rows

For a thing whose template has an `[EconomyComponent]`, even an empty
one (every AA unit's does, through `AAUnit`). A thing in a
regiment, a unit or an HQ's truck or building, gives its regiment's
rows, not its own: the same rows the regiment's tooltip has. [code,
data]

- Two figures are worked out, each one amount per resource of
  `Game/resources.tgi`, production less upkeep: [code]
  - *Now*: what the thing counts for in its side's totals, the same
    figures as the display's economy line
    ([economy](economy.md#the-selection-panels-economy-line)), with all
    of that line's rules (a building disabled or under construction
    counts no production; a Division HQ or depot out of supply counts
    its truck's; a thing that doesn't count, nothing).
  - *Full*: what the template would give if nothing held it back: its
    own `[Production]`, with the production technologies the thing
    holds, less its own `[Upkeep]`, with `NO_UPKEEP` if it holds it.
    For a regiment, add each position's template's `[Production]` less
    its `[Upkeep]`, plain (no technologies, no `NO_UPKEEP`), whether
    the position is filled or not. Out of supply, under construction
    and disabled play no part.
- Each figure is split by sign: the amounts above zero are its
  production, those below its upkeep. A resource the thing both makes
  and pays for shows only the difference, on one side. [code]
- "Production" (`ui_tooltip_item_title_production`): the now
  production. The row shows when that has any amount, or when it
  differs from the full production in any resource; so it can show
  with nothing after its title (a building under construction).
  "Maximum" (`ui_tooltip_item_title_production_potential`) follows it,
  with the full production, only when the two differ. [code]
- "Upkeep" (`ui_tooltip_item_title_upkeep`) and its "Maximum"
  (`ui_tooltip_item_title_upkeep_potential`): the same for upkeep, after
  the production rows. [code]
- Figures are written as on the economy line (icon, then `%+g`, or a
  bare whole number for a `_consumed` resource; amounts of 0 left out,
  so upkeep reads "-20"), but with one space after each entry, not two.
  [code]
- A city's rows sum its buildings' now figures, and since the full
  figure is taken to be the same, a city never shows "Maximum". [code]
- When the two differ in AA: [code, data]
  - A building under construction, or a city building that's disabled:
    "Production" with nothing after it, then "Maximum" with its
    `[Production]`. Upkeep is counted either way, so no upkeep Maximum.
  - A Division HQ or depot out of supply: "Production" with its truck's
    rate, "Maximum" with the building's. A cut-off American oil depot
    with Oil Production reads oil +17 over a Maximum of +30 (the
    technology's +5 is in both).
  - Upkeep never differs in stock play: out of supply, construction and
    being disabled don't touch it, and a regiment pays for every
    position whether filled or not. So an upkeep Maximum shows only for
    a thing that doesn't count at all.
  - An HQ's rows are its HQ regiment's, which counts its truck or
    building, so they read as that member's (plus Military Surplus's +5
    money on the regiment), with a Maximum only out of supply.
  - A combat regiment has upkeep only and no Maximum.

#### The list rows

Each row is its title, then the names it lists joined by ", " (a comma
and a space, no "and"), all on one line. A row with no names is left
out. [code]

- "Recruits" (`ui_tooltip_item_title_recruits`), for a building (a
  template with a `[StructureComponent]`): the recruitable templates
  (`recruitable = YES`, so units, not regiments or buildings) whose
  `[ElementComponent]` `required_structures` names this building's
  template, or one it was upgraded from. It doesn't list what the
  building itself recruits. [code, data]
  - One is left out when the building's side is barred from it (a
    map's restriction group), or when its `required_properties` aren't
    met by the side's technologies: properties held only by a building
    or regiment never count here, so a requirement that isn't a
    technology always leaves it out. Its other required buildings are
    not judged. [code]
  - Each name is the template's `name`; when it needs other buildings
    as well, `ui_tooltip_other_requirements|s` (" (with %s)") follows,
    naming them, ", "-joined. [code]
  - In the order the data loads the templates that name the building.
    [guess]
  - In AA: a motor pool lists the units that need it (an American one:
    Anti-Tank Infantry, M18 Hellcat, M4A1 Sherman, and M26 Pershing once
    the side has Heavy Tank); an engineer or artillery brigade
    likewise. The airfield trucks, which need three buildings, need
    `air_support` too, which isn't a technology, so they never show and
    no AA row has a " (with …)". The Division HQ trucks' requirements
    (`mechhqproduction`, `armorhqproduction`) aren't technologies
    either, so a Division HQ has no row; nor does a Corps HQ, a depot or
    a bunker. Bunkers that need a brigade aren't recruitable, so they
    don't show on the brigade's. [code, data]
- "Upgrades" (`ui_tooltip_item_title_upgrades`): the templates named by
  the template's `[ConstructionComponent]` `[Upgrade]` list, what the
  building can be rebuilt into, all of them with no check, last listed
  first. Not upgrades it researches for units. No AA template has an
  `[Upgrade]` list, so AA never shows the row. [code, data]
- "Researches" (`ui_tooltip_item_title_researches`), for a thing with a
  `[ResearchComponent]` list: the names of its research items'
  properties, in the list's order (the research menu's), keeping those
  whose prerequisites the side meets now and that it isn't barred from,
  judged as the research menu judges them
  ([research](research.md#what-a-building-researches)). It doesn't
  leave out what's done or under way: a researched `GLOBAL` item stays
  in the row. [code]
- "Provides" (`ui_tooltip_item_title_properties_provided`): the
  properties the thing gives others, from three places, in this order:
  [code, data]
  - its template's `[ElementComponent]` `SupportProperty` list (what a
    unit gives its regiment as it joins);
  - its template's `[StructureComponent]` `SupportProperty` list;
  - for a building, the `SUPPORT` research items it has finished (the
    only part that belongs to the thing rather than its template).
  No AA template has either `SupportProperty` list and no AA research
  item is `SUPPORT`, so AA never shows the row: a supply depot, an HQ,
  a regiment and a plain unit all have none. [data]
- "Technology" (`ui_tooltip_item_title_properties_technology`), for a
  building: the technologies its template's `[StructureComponent]`
  `[TechnologyProperty]` list gives its side while it stands, in list
  order. Not the technologies the thing holds (those are in
  Properties). In AA only the special operations' supply drops have
  one: Military Gear Up's crate reads "Military Gear Up", Rapid
  Deployment's pallet "Rapid Deployment", Assembly Line's parts
  "Assembly Line". [code, data]
- "Properties" (`ui_tooltip_item_title_properties`): the properties the
  thing holds, in its icon row's order, that have an icon, aren't a
  second copy, and came from one of: [code, data]
  - its side (a technology, from research, the start, a trigger or a
    building, or the handicap's property) or `LOCAL` research;
  - a `SupportProperty`;
  - an ability (a special operation's property, an AT unit's stun).
  So its own and inherited properties, terrain, activities, formations
  and experience are left out, though their icons show in the row. Each
  name is in its icon's tint (Property icons above): yellow for a
  technology, blue for a building's or a support property, green, red
  or white for an ability's. The ", " after a tinted name takes that
  name's tint. A regiment's unit lists what it was handed down with the
  rest, in the tint it has on the regiment. [code]

### The template tooltip

For a regiment position whose unit is dead. [code]

- Title: the position's template `name`.
- "Health": the health grown back so far over the template's maximum
  (the same amount the position's bar shows), for a template with a
  body.
- "Speed": the template's speed word, for one that moves.
- "Provides" and "Technology", as above, from the template alone (so
  Provides has no finished research).
- The template's `description`.

### The regiment's banner

- Which banner a regiment carries comes from its template, not from its
  nation: the template's `[View]` names a `[Banner]` by `banner_ids`. [code,
  data] Nothing links a nation to a banner. AA's regiment templates name
  their nation's banner one by one (`AAGEROrganizationInfantry` and the
  other `AAGER…` templates give `ger_banner`, in
  `Templates/template_organization_aa.tgi`); the base `AAOrganization`
  names `default_banner`, which AA never defines, so every regiment
  template has to override it. [data]
- A template with `use_faction_banner` set carries its owner's faction
  banner instead when the faction (AA's general, Kohan II's faction) names
  one by `banner_ids`. [data] AA's generals name none, so AA's regiments
  always show their template's banner. Kohan II's companies keep
  `default_banner` (`Organizations/Banners/Default_banner.tgi`) and its
  factions each name their own (`royalist_banner` and so on), so a
  company shows its faction's banner. [data] That the faction's banner
  wins whenever there is one is from the data's own comment, not traced.
  [guess]
- A regiment template with no `banner_ids`, or one naming a banner that
  doesn't exist, is a load error (see [data](data.md)). There's no
  fallback banner. [code]
- A `[Banner]` is a model (`model`, by default `%sBanner.nif`) with planes
  of decals, each decal shown when its `function` says (`NONE`: always;
  the others by the regiment's state, see the comments in
  `Templates/template_banner.tgi`). In every texture and the model's name,
  `%s` is the banner's own `resource_prefix`, and paths are relative to
  the banner's folder (`Organizations/Banners/`). [data]
- The frame round an AA banner is one of those decals, not something the
  game adds: `AABanner` gives its decal plane an always-shown decal
  `%sBannerBase.png` first (`AABaseBannerDecal`, in
  `Templates/template_banner_aa.tgi`), so `ger_banner`
  (`resource_prefix = GER/GER`) shows `GER/GERBannerBase.png`. [data]
  Kohan II's `K2Banner` has no always-shown decal, so its banners have no
  such frame: what shows is the banner's own model (`Default/
  DefaultBanner.NIF`) and its state decals. [data]
- The picture in the banner's upper half is the regiment template's
  `banner_icon` (a `BANNER_ICON` decal): AA's infantry templates give
  `/Organizations/Banners/Icons/IconInfantry.png` (the rifle), and a
  Division HQ regiment's `IconHQ.png` (the star). [code, data]
- The emblem below it is the HQ badge (a `BADGE` decal), shown on: [code]
  - a Division HQ regiment's banner: its own badge;
  - a regiment attached to an HQ: that HQ's badge, so every regiment of
    one division carries the same emblem as its HQ, and it changes or
    goes the moment the regiment is attached elsewhere or detached;
  - anything else (an unattached regiment, an HQ that holds no badge):
    no emblem.
  The badge is the HQ's draw from its nation's list; how it's drawn is in
  [supply](supply.toml#attaching-to-an-hq). It isn't fixed by the nation's
  order of HQs, the division's number or the order HQs are raised in, so
  two matches (or two programs) needn't show the same one: Japan's red
  wreath with crossed rifles is `Nations/JPN/JPNBadge09.png`, the yellow
  flower `JPNBadge16.png`. [code, data]
- Which rank decal an AA banner shows for each experience level is in
  [morale](morale.md#a-regiments-experience). [code, data]
- A decal whose texture file doesn't exist shows the default texture, with
  a warning, as any missing texture does. [code]
- Separately, the object display's banner button draws its own border
  round the banner (`BorderUnfocusedNationBanner` and
  `BorderFocusedNationBanner`, from the interface's skin folder in
  `UI/Game/object_display.tgi`), the same for every nation. [data]
- The efficiency bar shows a regiment's attack efficiency
  ([combat](combat.md#attack-efficiency)): in the one-regiment display
  (`CompanyDisplay`'s `AttackEfficiency`, `UI/Game/object_display.tgi`,
  at 366,50, 63 × 2) and on each regiment's card in the group display
  (`CompanyMiniDisplay`, `template_game.tgi`, at 3,44, 61 × 2). Lone
  units, buildings and HQ places have none. [data]
  - The fill is continuous: its right edge is at the bar's width times
    the current value, held to 0–1, so 100% or more is a full bar. No
    steps or rounding. [code]
  - It's solid red at every value (`AttackEfficiencyBar`,
    `template_game.tgi`: middle and end colours both 1,0,0) on black;
    nothing changes its tint. [data]
  - A small marker (`UI/Game/ObjectDisplay/TargetPercentageBar.tga`,
    6 × 4) shows the value the efficiency is easing towards, held to
    0–1, its right edge at that point along the bar. [code, data; guess
    for the exact edge]
  - It shows whenever the display shows a regiment, whoever owns it. It
    follows the value as the display updates, so it moves once a second
    with the efficiency. [code]
  - A regiment whose efficiency target is full (the usual case outside
    a fight) has its marker at the bar's right end: the orange square
    at the end of the red bar. [code, game]
- The morale bar is the blue bar along the banner's bottom edge: in the
  one-regiment display (`CompanyDisplay`'s `Morale`, at 366,124, 63 × 2)
  and on each card in the group display (`CompanyMiniDisplay`'s
  `Morale`, at 3,101, 61 × 2). [data]
  - The fill is the regiment's morale over its maximum
    ([morale](morale.md#maximum-and-breakpoint)), continuous, held to
    0–1. [code]
  - Solid blue (.3,.3,.8 at every value) on black. [data]
  - Its marker (the same `TargetPercentageBar.tga`) sits at the
    regiment's breakpoint over its maximum: where the fill must stay
    above for the regiment not to break. It doesn't move with morale,
    only when the breakpoint or maximum changes; an infantry regiment's
    breakpoint of 20 puts it a little under halfway. [code, data]
- Hovering the banner, its frame, or the morale or efficiency bar under
  and over it gives the regiment's thing tooltip: the regiment template's
  name, Efficiency, Morale, Speed, its economy rows, Properties and
  description (no Health: a regiment has no body). [code]
- A left click on the banner gives Command All: with the regiment
  attached to an HQ, or itself an HQ, the selection becomes all the
  regiments of that HQ (see [supply](supply.toml#what-the-player-sees)).
  [code] A regiment with no HQ is probably left as it is. [guess]
- A right click on the banner centres the camera on the regiment. [code]

### The portrait

- A left click on a lone thing's or building's portrait selects that
  thing (for a unit shown out of its regiment, its regiment) when it
  isn't already the whole selection: the selection is emptied and the
  thing taken, with its selection reply. When it already is the whole
  selection, as it always is for a thing the panel shows, the click
  does nothing. So clicking the portrait of a unit shown out of its
  regiment leaves the display on the unit. The layout gives the button
  no pressed look. [code, data]
- A left click on a regiment's portrait, while the captain is alive,
  turns the display to the captain alone, with the regiment still
  selected ([A unit out of its regiment](#a-unit-out-of-its-regiment)).
  [code]
- A right click on either centres the camera on the thing: on the
  captain for a regiment, or on the regiment while the captain is dead;
  on the unit itself for a unit shown out of its regiment. [code]

### Unit cards

- Each card's bar is that unit's own health over its maximum; a dead
  unit's card shows its template's icon and the health its replacement
  has grown back so far. [code]
- Hovering a card's picture or bar gives the unit's thing tooltip (so
  its template name, health and speed, and whatever else applies), or
  the template tooltip while the unit is dead. [code]
- A left click on a living unit's card turns the display to that unit
  alone, as the portrait does for the captain; a dead unit's card does
  nothing on a left click. A right click centres the camera on the
  unit, or on the regiment for a dead one. [code]

### An HQ's places

- Hovering a place holding an attached regiment, anywhere on it (banner
  model, bars), gives that regiment's thing tooltip, as its own banner
  would. A place holding a transport gives the transport's. [code]
- A place held for a regiment still in the queue gives the queue item's
  tooltip (below), as the item in the queue itself does. [code]
- An empty place gives its own string, "Empty attachment slot". [code,
  data]
- A left click on a place with a regiment or transport selects it alone;
  a right click centres the camera on it. A left click on a queued one
  cancels the item (see [supply](supply.toml#what-the-player-sees)). [code]

### The commission queue

- The item being made (its button, icon and progress bar alike) gives:
  [code]
  - the item's `name`: the template's for a thing, the technology's for
    research;
  - "Cost" (`ui_tooltip_item_title_cost`) with what was paid for it,
    written as a recruit's cost is;
  - "Production" and "Upkeep" rows for what the item will add and
    cost, titled as in the thing tooltip (no "Maximum" rows);
  - "Production" (`ui_tooltip_item_title_production_percentage`) with
    the progress as a whole percentage
    (`ui_tooltip_production_percentage`, "%d%%");
  - while the queue is paused, a blank line and then "Paused"
    (`ui_tooltip_item_title_queue_paused`) with the reason: "Unit Limit"
    (`ui_tooltip_queue_paused_unit_limit`), "Out Of Supply"
    (`..._oos`) or "Disabled" (`..._disabled`);
  - last, "Click icon to cancel production."
    (`ui_tooltip_production_queue_item_cancel`).
- An item waiting behind it gives its name, Cost, the Production and
  Upkeep rows and the cancel line: no percentage and no Paused line.
  [code]
- A waiting place with nothing in it is titled "Empty Production Queue
  Slot" (`ui_tooltip_production_queue_item_empty_title`) with the text
  "This production slot is empty."
  (`ui_tooltip_production_queue_item_empty`). [code, data]
- A queued regiment shown in an HQ's place gives the first form with its
  progress, or 0% while it waits behind another item. [code]
- A left click on any queued item cancels it; a right click does
  nothing. [code] Which mouse button is which, here and for every click
  above, is inferred: a button carries a main action and a second one,
  taken to be the left and right buttons. [guess]

### What the health bars measure

- A lone thing's or building's bar: its health over its maximum, as the
  player knows them. [code]
- The bar under a regiment's portrait is the captain's alone, not the
  regiment's: the captain's health over its maximum, or, while the
  captain is dead, the health its replacement has grown back over the
  template's maximum. The Health line of the tooltip it gives is the
  captain's too. [code]
- A regiment has no summed health figure anywhere in the display. The
  only whole-regiment bar is the one in an HQ's place, which is the
  average of its positions' health fractions (see
  [supply](supply.toml#what-the-player-sees)), and its tooltip, being the
  regiment's, has no Health line. [code]
- On a regiment's card in the group display (several things selected)
  the main bar (`PrimaryHealth`) is again the captain's, and the row of
  small bars (`HealthBars`) has one per position, each that unit's own.
  Hovering the main bar gives the captain's tooltip, a small bar that
  unit's, and the morale and efficiency bars and the banner the
  regiment's; dead units give the template tooltip. A lone thing's card
  gives that thing's tooltip. [code]

### The group display's cards, name and owner

- The group display shows when the selection holds more than one thing.
  It has one card per selected thing (six at most, as a selection
  never holds more), laid out left to right with the most recently
  selected thing first: each thing that joins the selection takes the
  leftmost card and pushes the others one place right. [code]
- Above the cards are a name line and an owner line, and nothing else:
  no portrait, health bar, state line, property icons, economy line or
  weapon and defense values, though the layout data gives
  `GroupDisplay` (built on `UnitDisplay`) all of them. Only the name and
  owner lines, the formation buttons and the cards are built. [code,
  data]
- The name and owner lines show at all because `AVars_AA.tgi` sets
  `UIGroupDisplayUseNameLabel = true`; with it false (the engine-wide
  `AVars.tgi` default) the group display has neither. [code, data]
- Both lines are about one thing: the one that has been in the
  selection longest, so the one on the rightmost card. Selecting
  regiments one after another by Shift-click, the name stays the first
  one picked, and changes only when that one leaves the selection (it
  then passes to the next-oldest). It is not a count or a group name,
  whatever mix is selected. [code]
- Which of several things taken in one go counts as oldest (a box drag,
  a double-click) depends on the order they are taken in, which is
  unchecked. [guess] See Open questions.
- The name line is written exactly as that thing's own display writes
  its name, as the player sees or remembers it: a lone unit's name is
  written as on its own panel (see Name, owner and state lines), a
  regiment's as on the regiment display ("1st Infantry Regiment"). The owner line is that thing's owner line, with
  the same chip and tint by relation as the lone thing's panel (see
  Name, owner and state lines), and blank for a thing with no owner as
  the player knows it. Both follow the thing as the display updates.
  [code]
- In practice a selection mixes only things that may be selected
  together ([selection](selection.md)): regiments (and packed HQ trucks)
  with each other, or lone units with each other, never a regiment with
  a lone unit or a building. A one-unit regiment (the
  `regiment_single_unit` layout, used by the special operations' agents
  and saboteurs) counts as a regiment. [data] A selection of enemies from two different
  sides shows the owner of the oldest one only. [code]

### The card's small health bars

The `HealthBars` panel of `CompanyMiniDisplay` (`UI/Game/template_game.tgi`;
8 bars in 4 rows of 2 in `southern-europe-regiments-multi-selected.jpg`).

- Which positions get one: a position is one member of the layout,
  each `Slot` entry giving `number` of them (1 by default), in the order
  the file lists them (were a `PrimarySlot` to have a `number` above 1,
  the primary position would be its last). Only the first nine positions (the captain's
  included) are considered, and each gets a bar except the `PrimarySlot`'s
  (whose unit has the wide bar) and any position the regiment has no
  unit template for. So `regiment_infantry` (3 + 1 + 1 + 1 + 2 positions,
  then the captain) shows 8 small bars. The position left out is the primary one whatever its unit is: a
  tank regiment's lead tank has the wide bar and no small one, and its
  other tanks one each. A layout with no primary position has no wide
  bar and gives every position a small bar (nine at most). [code] With
  AA's layouts there are never more than 8: `regiment_infantry` and
  `regiment_mechanized` have 9 positions with the captain last, and
  `herd_ambient`, with no `PrimarySlot`, has 8. [data]
- The count follows the layout's positions, not the regiment template's
  `[Slot]` entries: each of the template's entries names the unit for
  every position of the layout `Slot` in the same place, so one entry
  under a `number = 3` slot fills three positions, and `element_IDS =
  null` leaves all of that slot's positions without a unit. So, for
  Japan: [code, data]
  - `regiment_jpn_infantry` names a unit for every slot: 8 small bars;
  - `regiment_jpn_infantry_recon` names `null` for the second, fourth
    and fifth slots: 3 + 1 positions with units, so 4 small bars.
- Placing them: the bars fill two to a row, left then right, then the
  next row down, in position order with the left-out positions skipped
  (no gap where they'd be). With the data's values, in the panel's own
  coordinates: [code, data]
  - the first bar's top-left is at (`horizontal_border_separation`,
    `vertical_border_separation`) = (3, 2);
  - the right-hand bar is one bar width (the `Health` view's 28) plus
    `bar_separation` (5) further right, so at x = 36;
  - each row is one bar height (the view's 2) plus `bar_separation`
    lower than the last, a pitch of 7: rows at y = 2, 9, 16, 23.
  The pitch and positions never depend on how many bars there are: a
  regiment with fewer just stops sooner, leaving the rest of the panel
  empty (6 bars fill 3 rows, 3 bars a full row and a left-hand one). The
  bars always start at the top; nothing centres them. [code]
- Anything outside the `HealthBars` panel is cut off at its edge, as
  every view is cut to the view it sits in. So a ninth bar (only
  possible with other data) would start a fifth row at y = 30, below the
  panel's 29: the bar wouldn't show at all, and only the top pixel row
  of its plate (y 28 to 29) would. [code]
- `definition_separation` and AVar `UIShouldResetGroupDisplayOffset`
  (false in `AVars_AA.tgi`) play no part in AA. With the flag true, a bar
  whose position's `Slot` has a different `property_IDS` from the bar
  before it (and doesn't directly follow the `PrimarySlot`'s position)
  starts back at the left, `definition_separation` lower. [code]
- Each bar has its own `HealthBarBorder` plate, not one per row: the
  plate's top-left is the bar's less (`horizontal_border_separation`,
  `vertical_border_separation`), held at 0 or more, so 3 left of and 2
  above the bar; it is 34 wide (the bar's width plus twice the horizontal
  separation) and 8 high (its view's `B = +8`). So the plate shows 3
  pixels either side of the bar, 2 above and 4 below, and a row's two
  plates overlap by one pixel (x 0–34 and 33–67) to span the panel's 67.
  [code, data]
- `HealthBarNarrow.tga` is an opaque 34 × 7 picture, stretched to the
  plate's 34 × 8. It is drawn behind its bar. [data; guess for the
  drawing order, from the plate being opaque and the bars showing]
- A bar and its plate show or hide together, and which positions have
  one is settled when the card is given its regiment, by the layout
  alone, not by who is alive. [code]
- Each bar's fill is its position's health fraction, updated as the
  display updates: a living unit's health over its maximum; for a dead
  or not yet recruited unit, the health its replacement has grown back
  so far over the template's maximum (so 0 until regrowth starts, then
  filling as it grows). A dead position keeps its bar and plate. [code]
- So a bright bar is a unit standing in that position, and a dim one is
  a position still empty: only the panel's dark green shows, with a
  short coloured stub as its replacement grows. A regiment whose
  template has `recruit_full = false` (most of AA's, the plain infantry
  regiment among them) arrives as its leader alone and fills its other
  positions one by one by resupply ([supply](supply.toml)), so its card
  starts all dim and brightens bar by bar; one with `recruit_full =
  true` (the recon regiment) arrives with every bar bright. [code, data]
- The small bars take the `HealthBar` template's colours unchanged
  (`StatusBar` red at empty, yellow at half, green at full, on a
  `View` colour of 0,0.3,0); only `PrimaryHealth` overrides them to
  solid red. [data]

### The card's retreat button

`RetreatRoutButton` of `CompanyMiniDisplay` (`UI/Game/template_game.tgi`,
at 16,145, 33 × 25), under each regiment's card.

- Only regiment cards have it: a lone unit's card (`SimpleActorMini`)
  has none, and neither has the regiment display (one regiment
  selected); there the same orders are only in the command grid
  ([commands](commands.md)). [code, data]
- It gives the card's regiment one of two orders, chosen afresh each
  time the display updates: Run (`retreat`) when the regiment is
  offered Run by the viewing player, otherwise Rout (`rout`) when it is
  offered Rout, otherwise the button is hidden. "Offered" is exactly as
  for the command grid: the player's own regiment, with the order among
  those it can take now ([morale](morale.md#the-players-orders)). So
  [code]:
  - an enemy's or an ally's regiment never shows it;
  - an ordinary regiment of the player's shows Run;
  - one in a fight of its own whose morale is too low for Run shows
    Rout;
  - one routing or Exhausted (it has neither order) shows nothing.
- It is never greyed or disabled, and its picture doesn't change with
  the order: it is always `IconRetreat.png`, with `IconRetreatFocused.png`
  under the pointer and `IconRetreatPressed.png` while pressed (all in
  `UI/<interface>/Game/ObjectDisplay/`). The data's separate rout
  picture is commented out. Supply plays no part beyond what it does to
  the orders on offer. [code, data]
- A left click acts on release, and acts for that card's regiment
  alone, as if that one regiment had been given the order from the
  command grid: [code]
  - Run waits for a click on the map, as the Run button does (with its
    cursor, and a facing dragged out the same way), and the order then
    goes to that regiment only, not to the rest of the selection;
  - Rout is given at once, to that regiment only.
  The selection and the display don't change. The click is refused,
  with nothing sent, only if nothing in the selection can take that
  order, which can't happen while the button shows for one of them.
  [code]
- A right click gives no order; the button only shows pressed and makes
  its click sounds. [code]
- Its tooltip isn't the data's "Set by code" string: it is built from
  the order the button gives, its name ("Run" or "Rout") as the title,
  then a blank line and that order's tooltip text ("Commands selection
  to ignore enemies and move to target area. ..." for Run, "Commands
  selection to flee automatically. It cannot be controlled." for Rout).
  So it changes when the button switches between Run and Rout. [code,
  data] The title is marked for a hotkey as the command grid's tooltips
  are, when a key maps to it ([commands](commands.md#hotkeys-in-tooltips)).
  [guess that no key maps to the card's button]

## Formation buttons

The column of square buttons at the right of the regiment display and
of the group display, one per formation, that switch the selected
regiments' formation (`FormationButtons` in `UI/Game/object_display.tgi`,
`FormationButtonsTemplate` in `UI/Game/template_game.tgi`; three, the
middle one lit, in `berlin-infantry-regiment-commands.jpg` and
`southern-europe-regiments-multi-selected.jpg`).

### When they show

- One regiment selected: the buttons show when the regiment belongs to
  the viewing player's own side, has the formation order among the
  orders it can take now ([commands](commands.md)), and its layout lists
  more than one formation. So an enemy's or an ally's regiment shows
  none, nor does a thing only remembered under fog, nor an HQ regiment
  (`regiment_hq` and `regiment_hq_mobile` list only `regiment_hq`).
  [code, data]
- Several things selected (the group display): of the first six things
  in the selection, the ones that are the player's own regiments with
  the formation order count. Their formations make the buttons; when
  that gives fewer than two buttons there are none. [code]
- A lone unit or building has no formations and shows none, alone or in
  a group, and things that don't count leave the buttons to those that
  do. [code]
- A routing or exhausted regiment keeps its formation order (it chooses
  a formation to take up later, see [morale](morale.md)), so its buttons
  show as usual. [code]
- The buttons are never greyed or disabled: a button that shows can
  always be pressed. [code]

### Which buttons, in which order

- Each counted regiment offers the first three formations its layout
  lists, in the layout's order. Regiment by regiment in selection order,
  each formation not already on a button takes the next button down, up
  to three buttons in all; the rest are left out. [code]
- AA's regiment layouts all list `regiment_aggressive`,
  `regiment_defensive`, `regiment_column` in that order, so an
  infantry, mechanized, vehicle or single-unit regiment shows Assault,
  Aggressive and Column, top to bottom. `regiment_herding` is only in
  `herd_ambient`, whose herds no player owns, so it never gets a button.
  [data]
- Each button shows its formation's `icon` inside its frame. [code,
  data]
- Placement: the panel is 46 wide and 142 tall at (444, 36) in the
  display's layout. Buttons keep the template's 46×46 size and stand at
  the panel's left edge, button *n* (from 0) with its top at *n* ×
  floor(142 / 3) = 0, 47, 94, so they are stacked with a one-unit gap.
  Buttons fill from the top; unused places are left empty at the
  bottom. [code, data]

### Which button is lit

- The lit button follows the formation the counted regiments are in
  now, not only the one the player chose: when every counted regiment
  is in the same formation, and that formation changes, its button
  lights and the others go out. A regiment that goes into its combat
  formation to attack, engage or guard lights Assault; when it leaves
  combat for the player's formation, that one lights again. [code]
- Pressing a button lights it at once, before the regiment has changed.
  It stays lit until the formation they're in next changes, so pressing
  Column during a fight keeps Column lit while the regiment fights on in
  Assault, and still Column once it forms column afterwards. [code]
- Counted regiments in different formations: no button is lit. [code]
- No formation (routing, exhausted): no new button lights; the one lit
  before stays lit. [code] When the display is opened on a regiment
  that already has no formation, the top button is lit (or the one lit
  last time the display showed formation buttons). [code]
- After a group has been in mixed formations, its buttons stay unlit if
  they come back together in the formation that was last lit, until
  their formation next changes. [code]

### Pressing one

- A left press (mouse button down, or a double-click), or the button's
  key, gives the formation order for that formation to the whole
  selection; each regiment takes it on its own
  ([movement](movement.md)), and things that can't take it ignore it.
  [code]
- Pressing the lit button gives no order. [code]
- With nothing lit (a mixed group), a press gives the order and lights
  the button; if they're still in different formations afterwards, it
  goes out again. [code]
- The press plays the button's `toggle_button` sound, the lit one's
  included ([sounds](sounds.md)). The game lighting a button by itself
  makes no sound. No message is posted. A right press does nothing.
  [code, data]

### Tooltip

- The display builds the tooltip; the button's own `ToolTip` string
  (`template_game_FormationButton_ToolTip`, "%s") never shows. [code]
- Lines, as in the thing tooltip's layout ([The thing
  tooltip](#the-thing-tooltip)): [code, data]
  - Title: the formation's `name` ("Assault"), marked with the button's
    key ([commands](commands.md#hotkeys-in-tooltips)).
  - A blank line, then "Behavior" (`ui_tooltip_item_title_behavior`)
    with "Aggressive" (`ui_tooltip_behavior_aggressive`) when the
    formation's `aggressive` is true, else "Passive"
    (`ui_tooltip_behavior_passive`).
  - When the formation has a `[Property]` with modifiers: a blank line,
    then one modifier line per modifier, worded and coloured as in
    [special operations](special-ops.md) ("-25% to Maximum Speed" in red
    for Assault).

### Keys

- The hotkey files name the buttons by place from the top:
  `CompanyFormationButtons_0` to `_2` in the regiment display and
  `GroupFormationButtons_0` to `_2` in the group display. The Standard
  set maps Q, W and E to them in both, so with the stock layouts Q is
  Assault, W Aggressive and E Column; the tooltips read "(Q)Assault",
  "(W)Aggressive" and "(E)Column". The other sets use T, G, B (Visual),
  Y, H, N (left-handed) and Y, I, X (Dvorak). [code, data]
- A key presses its button only while the button shows. [code]

### The targeted look

- `FormationIconTargeted.tga` (the `TARGETED` child) draws only while
  the button is flashing for a trigger's `flash_ui_button` effect, on
  and off as a command button flashes ([commands](commands.md)). The
  game never flashes them otherwise. [code] The effect most likely
  names them by the hotkey files' names above. [guess]

## Open questions


- The formation buttons' tooltip for Aggressive and Column: their
  `ATTACK_EFFICIENCY_DESIRED` modifier has no
  `modifier_desc_multiply_attack_efficiency_desired` string, so is its
  line blank, the bare key, or left out? None of the formation buttons'
  behaviour has been watched in the game. (analysis, 2026-11-20)

- Does a regiment with `use_faction_banner` always carry its faction's
  banner when the faction names one, as the data's comment says, and its
  template's otherwise? Only Kohan II's data depends on it. (analysis,
  2026-11-15)

- What, besides being selected, makes a regiment member's or a city
  building's bar show? Does hovering ever show a bar? (analysis, 2026-11-08)
- Does a thing's model scale apply to its health bar? (analysis, 2026-11-08)
- How is the fill frame picked from health: how many frames, and rounding
  which way? (analysis, 2026-11-08)
- Does a click outside the technology display (or the Objectives window
  or Message Log page) close it or reach the map behind it? The keys are
  in [interface](interface.md#windows-over-the-match); the clicks are
  unconfirmed. (analysis, 2026-11-12; implementer, 2026-11-13)
- Does a general's portrait play its selected animation when the general
  is chosen in the lobby or campaign screens? (analysis, 2026-11-13)
- While an HQ building is packing back into its truck, does the regiment
  display show the building's picture or the truck's? (analysis,
  2026-11-13)
- In the group display, is a regiment entry's banner drawn as a 3D model
  in the entry's `ModelPortrait` view or as a flat icon, given the entry
  sets `uses_icon`? (analysis, 2026-11-13)
- Is the fitted model scaled by its template's size settings before it
  is measured and drawn? (analysis, 2026-11-13)
- The portrait box rules are read from the code and haven't been compared
  with the running game beyond `jpn-corps-hq-truck.jpg` (a fitted truck):
  the stretch of a model camera's picture and the fitted model's exact
  framing are the ones to check against screenshots. (analysis,
  2026-11-13)
- The object display's tooltips and clicks are read from the code and
  none has been watched in the game: hover the portrait, the banner, a
  unit card and an HQ's places with a regiment selected and compare.
  (analysis, 2026-11-13)
- A unit shown out of its regiment is read from the code and hasn't been
  watched in the game: turn the display to a unit and check its state
  line against the regiment's, its property row's handed-down icons,
  and that its own portrait click does nothing. (analysis, 2026-11-24)
- What a left click does on a regiment's card in the group display
  (most likely selects that regiment alone). A right click centres the
  camera on it. (analysis, 2026-11-13)
- The exact wording of the thing tooltip's Experience row. (analysis,
  2026-11-13)
- The order of the thing tooltip's Recruits row, and whether anything
  in it, its economy rows or its Properties filter differs from the
  running game: none of it has been watched. Hover a motor pool, a
  depot cut off from supply, and a unit holding a technology to
  compare. (analysis, 2026-11-24)
- Which of a thing's abilities get a weapon value: all of them in the
  order the template lists them, or only some (attacks, say)? And how
  the rows stack when there are several. (implementer, 2026-11-14)
- The owner line's name colour when the relation is none of the four
  (a thing whose side the viewing player can't be related to, such as
  when watching without a side). (analysis, 2026-11-14)
- The exact wording of the weapon tooltip's damage row when the ability
  has extra damage effects as well as main damage. (analysis,
  2026-11-14)
- The defense value's tint, and the description box giving way to a
  queue and coming back, are read from the code and haven't been
  watched in the game. (analysis, 2026-11-14)
- In a selection taken in one go (a box drag, a double-click), which
  thing ends up oldest, and so named by the group display, and how the
  cards are ordered among them. (analysis, 2026-11-20)
