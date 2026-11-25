# Deliveries

How something is brought onto the map by a transport instead of walking out
of a building: a recruit whose template names a `transport_actor` (Deploy to
Location), and the things special operations and triggers have flown in.
The button and its menu are in [commands](commands.md#deploy-menus); what
a transport stores in a saved game is in
[saved-state](saved-state.md#deliveries).

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

Distances are in the units of `speed_max` and ranges; "the plane's line" is
the direction the transport is facing.

## What is delivered, and by what

- A template's `transport_actor` names the unit that brings it. In shipped
  data: [data]
  - `<nation>_transport_air` (the nation's bomber airframe, `speed_max` 7,
    `operational_height` 17, `deployment_delay` 0.4) brings the paratrooper
    regiments, the special operations' regiments (saboteurs, secret
    agents, S.A.S.) and their crates (`provisions`, `economic_aid`,
    `infantry_supplies`).
  - `<nation>_transport_water` (a landing craft, `speed_max` 3,
    `deployment_delay` 0.5) brings the amphibious variants of the
    infantry, anti-tank and engineer regiments
    (`regiment_<nation>_infantry_amphib`, `..._infantry_at_amphibious`,
    `..._engineer_amphibious`) and the USA Marine regiment. A regiment's
    plain variant, and `regiment_rus_paratrooper_ground`, name none and
    come out of the building.
  - `<nation>_transport_water_hq` (the same craft, loaded with a truck)
    brings `<nation>_truck_corps_amphibious` and its `_dynamic` twin.
- A delivery is either a regiment (the template and its layout: one unit
  per filled slot) or a single thing. [code]
- Three things start one: a building carrying out a Deploy to Location
  order, a special operation that summons something with a transport, and
  the trigger effect `actor_create_transported_named`. All three hand the
  transport the same job, so the flight, the unloading and the landing
  below are the same for all. What differs is where the transport comes
  from, who pays, and whether an HQ is waiting
  ([Special operations and triggers](#special-operations-and-triggers)).
  [code]

## Aiming

- Pressing an entry in the Deploy to Location menu closes the menu (the
  grid goes back to the commands) and starts aiming. Nothing is ordered or
  paid yet. [code]
- While aiming: [code, game]
  - the cursor is the command's, `move`;
  - a preview follows the mouse over the map: the regiment's banner at the
    mouse's place and each member of its layout at its formation place
    around it, drawn as the real models with no tint and no division
    badge (`jpn-airborne-hq-recruit-preview.jpg`). A single thing previews
    as itself. The preview faces the map's default direction (0, −1);
  - the resource bar previews the recruit's cost, as hovering its button
    does ([economy](economy.md)).
- A place is valid when every piece of the preview (the banner and each
  member, each at its own spot) passes all of these: [code]
  - every tile under its shape has been explored by the player (seen now
    or remembered). One unexplored tile makes the place invalid. With fog
    switched off by the cheat, this test is skipped;
  - it has room to stand there, by the check in
    [objects](objects.md#room-to-stand): on the map, every tile under it
    passable for it, and no blocking thing in the way. Where none of its
    tiles is in sight right now (remembered ground), things there don't
    count against it, only the tiles do, so the preview doesn't give away
    what the fog hides.
- Nothing else limits the place: not distance from the recruiter, supply,
  whose ground it is, or what stands near. It's the same test for an air
  and a water transport; a water transport's own limits come later, when
  the boat is sent ([Water transports](#water-transports)). [code]
- An invalid place puts the invalid-placement effect on every piece of
  the preview, the same one Unpack's preview uses [code], which draws the
  piece flat red (`jpn-airborne-hq-truck-unpack-invalid.jpg`). [game]
- A left click on a valid place gives the order for that place and ends
  aiming: one click, one order. To send another, the menu is opened
  again. The order carries the place only, no facing. [code] Whether the
  order goes on the press or on the release is an open question.
- A left click on an invalid place does nothing: no order, no message,
  and aiming goes on. [code]
- A right click ends aiming with nothing ordered. So does choosing another
  command. [code]
- Only the map view aims. The preview stays where it was while the mouse
  is over anything else, and a click on the minimap is the minimap's own
  (it moves the camera), not an order. [code]

## The order

- The order is taken like any recruit order
  ([economy](economy.md), [supply](supply.toml)): an entry that isn't
  offered is refused silently; a regiment for an HQ with no free place is
  refused with the HQ-full message; one the side can't afford is refused
  with the funds message. Otherwise the whole cost is taken at once and
  the item joins the building's queue, remembering the place. [code]
- It waits and builds in the queue like any other item: the template's
  `recruit_time` for a regiment (15 seconds for the paratrooper regiments,
  which use the infantry organization), the usual single-unit time
  otherwise; limits are checked when it reaches the front; it pauses as
  the queue pauses; and cancelling it there refunds the whole cost and
  frees the HQ place.
  While queued, its place on the HQ's panel shows the regiment's icon and
  the build's progress. [code]
- When the build finishes the transport is sent
  ([The air transport](#the-air-transport),
  [Water transports](#water-transports)). At that moment: [code]
  - "<template name> recruited" (`company_recruited`) is posted to the
    owner, located at the transport, for regiments and single things
    alike. [code, game] An ordinary regiment gets "commissioned" instead
    ([messages](messages.md#production-lines));
  - the transport takes over the HQ place the queue item held; the panel
    slot now shows the transport's icon and its health bar, and a click on
    it selects the transport ([supply](supply.toml)). The slot becomes the
    regiment's when the regiment appears (below);
  - the transport carries what the cargo cost and the cargo's upkeep, and
    stands in for it in the side's accounts until it has unloaded
    ([saved-state](saved-state.md#deliveries)).
- Between the click and the transport being sent, nothing marks the
  aimed place: `recruit_location` names no `command_vfx_ids_location`,
  unlike Move, so no marker or effect appears on the map, and the
  preview goes when aiming ends. [code, data] No message is posted for
  the order itself. What the player has to go on is what any recruit
  order gives: the cost leaving the stockpile, the item in the
  building's queue and in its HQ place with a progress bar
  ([object display](object-display.md#the-commission-queue)). [code] An Airborne
  Division HQ with a paratrooper regiment building showed just that: the
  place held the regiment's icon (`IconParatrooper.png`, the picture of
  parachutes) over a bar most of the way along, which is the build's
  progress, the plane not yet sent. [game, data] Its state line read
  "Resting" all the while, not "Deploying", as a Division HQ's always
  does while it recruits: see [object
  display](object-display.md#name-owner-and-state-lines). [game, code] Whether a
  sound plays is an open question.
- If the transport can't be made, "Could not create <template name>"
  (`recruit_error_creating`) is posted and the cost is refunded. [code]
- A building can have any number of deliveries under way: once sent, a
  transport is on its own, and the queue moves on to its next item. HQ
  places and the side's limits are the only cap. [code]
- Once the transport is sent there's no cancelling it. [code]
- The building's rally point plays no part: a delivered regiment isn't
  sent on to it. [code]

## The air transport

Where it comes in, for a building's delivery:

- Take the recruiter R and the aimed place T. The far point is
  F = m × (R − T), where m is the larger of the map's width and height,
  taken as a position (not an offset from T). The plane appears where the
  straight segment from T to F crosses the map's edge. The four sides are
  tried in the order y = 0, x = width, y = height, x = 0 and the first
  crossed wins. With m in the hundreds, F is far outside the map in the
  direction from T towards R, so in effect the plane comes in over the
  recruiter: T, R and the entry point lie nearly in a line, R between the
  other two. [code] In the Japanese drop the HQ stood south-east of the
  drop point and the plane came from the south-east. [game]
- If the segment crosses no side (R and T the same place can do this), the
  entry point is the map's (0, 0) corner. [guess]
- Each coordinate of the entry point is then pulled inside the map, to at
  least one tile from the low edge and just under one tile from the high
  edge. [code]
- This entry point is also where the plane leaves the map afterwards.
  [code]
- A recruiter with a runway that is asked for an air transport launches it
  from the runway instead, and the plane comes home to it. No shipped
  recruiter with a runway offers a transported recruit. [code, data]

The flight:

- The plane is an ordinary unit of the recruiter's side, made at the entry
  point at its `operational_height`, facing T, with no taxi or climb. It
  is made at the moment "recruited" is posted and is given its job at
  once. It flies straight, its full speed being its top speed: its
  `speed_max` as its properties change it, as for a ground unit
  ([movement](movement.md#a-units-top-speed)), so `tech_superchargers`
  (+0.25 for `aircraft`) makes the stock plane's 7 into 8.75. [code, data]
  - The plane's only type property is `aircraft`, so of the stock speed
    modifiers only `tech_superchargers` is sure to reach it. Operations
    that require tanks, infantry or vehicles can't; those with no such
    requirement (Fighting Spirit, Forced March, Smoke Barrage,
    Propaganda War) can: an operation's area catches a plane in flight
    ([combat](combat.md#what-can-hit-a-plane)). [code, data]
  - The top speed feeds the lead and the drop's timing below (v is half
    the modified top speed; the marks use the plain `speed_max`). With
    superchargers (8.75, v 4.375) a nine-man drop's lead is about 3.64
    and its release point 10.64 short of T; the second man goes 0.44
    seconds and 2.8 after the first, the rest 0.6 seconds and 2.625
    apart (2.1 for the stock plane). Only a top speed above about 9.33
    would make the 2.8 marks come before the 0.6 seconds. [code]
  - A change of top speed during the flight takes effect from the next
    stretch it starts. [guess]
- A plane's speed changes with distance, not time. Each stretch it flies
  (here: entry point to release point, then each stretch of the drop
  run, then the way home) starts at the speed the plane has and ends at
  the speed it is aiming for, the speed in between rising or falling
  evenly with the distance covered, and never under 0.1. A stretch
  longer than 20.1 is cut into ⌊length ÷ 20⌋ equal pieces: the whole
  change happens over the first piece and the rest are flown at the
  speed aimed for. [code]
- A plane made at the map's edge starts from rest, aiming for full
  speed. So it creeps in (0.1 at first) and is at full speed one piece
  later, 20 to 40 in from the edge. With a first piece of length p and
  full speed s that piece takes p ÷ s × (1 + ln(s ÷ 0.1)) seconds: about
  0.75 × p for the stock plane, 15 seconds for a piece of 20 where full
  speed would take under 3. The rest of the way in is at full speed.
  [code] A drop timed at 38 seconds from "recruited" to the release
  fits a run in of about 180. [game]
- It shows on the minimap like any unit (a small dot in its side's colour
  in `jpn-airborne-drop-1-plane.jpg`). [game] It plays its template's
  usual effects: the nation's bomber "deployed" sound when it appears,
  the contrail and wing lights when it starts moving. [data]
- It doesn't fly to T but to a release point short of it, so that the
  drop falls around T. With n things to put down, v half the plane's
  top speed (3.5 for the stock plane), h its `operational_height`, and d its
  `deployment_delay`: [code]
  - lead(template) = max(0, √(2h ÷ 49.05) × v − `fall_drag`), 49.05 being
    the size of the game variable `Gravity`. For a regiment the lead is
    the largest over its members;
  - the release point is back from T along the plane's line by
    lead + d × v × ⌊(n − 1) ÷ 2⌋.
  For the stock plane and a paratrooper regiment of nine (`fall_drag` 0)
  that's 2.91 + 0.4 × 3.5 × 4 = 8.51 short of T; for a crate
  (`fall_drag` 10, lead 0, n = 1) it's T itself.
- It reaches the release point still at full speed, and lets the first
  thing go there. Things leave in layout slot order, empty slots
  skipped. [code]
- The releases after the first are timed, not placed. At each release
  the plane aims for half its top speed and for a mark d × `speed_max`
  (2.8; the plain `speed_max`, which speed modifiers don't change)
  beyond the last mark, the first mark being the release point. The next
  thing goes when the plane reaches its mark or 0.6 seconds after the
  last release, whichever comes first. [code] With the stock plane:
  - it slows from 7 to 3.5 over the 2.8 to the first mark, which takes
    0.55 seconds, so the second thing goes at that mark, 2.8 after the
    first;
  - from then on it's at 3.5 and the marks (2.8 apart) get away from it
    (2.1 flown in 0.6 seconds), so every later thing goes 0.6 seconds
    and 2.1 after the one before.
  Release times are to the nearest simulation tick. `deployment_delay`
  itself is never used as a time by a plane. [code]
- So nine men leave at 0, 0.55, 1.15, 1.75 … 4.75 seconds, the plane
  then being 8.51, 5.71, 3.61, 1.51 short of T and 0.59, 2.69, 4.79,
  6.89, 8.99 past it. It passes over T 2.2 seconds after the first
  release with four men out. [code] A shot of the Russian drop with the
  plane about over the target shows three parachutes clear behind it
  (a fourth could be behind the plane), evenly strung out and
  overlapping. [game]
- After the last one it aims for full speed again, turns round and flies
  to its entry point, where it's taken off the map. [code] A plane
  launched from a runway returns to its building instead. [code]

Losing it:

- The plane can be shot down on its way. It's `air`, so only
  anti-aircraft weapons (`+air`) and some operations can hit it. It
  carries on its run whatever happens to it, never fighting back:
  [combat](combat.md#things-in-the-air). [code, data]
- A transport destroyed before it puts anything down: its cargo never
  appears, nothing is refunded, its HQ place is freed, and its owner gets
  the ordinary "lost" line for it. [code]
- One destroyed part-way: what is already down stays, the rest never
  arrives, and the regiment is under strength. Nothing is refunded. The
  regiment stops counting as arriving at its next once-a-second check
  ([supply](supply.toml)). [code]
- A stock bomber airframe that dies sets off its usual death blast
  (`OnDeathAbility`: 150 blast damage within 5). [data]

## Coming down

- The regiment itself appears with the first release: its banner is at T
  (or the nearest open spot the game finds within 4 of T, looking first
  along the line back towards the plane), facing the plane's line. From
  then on it's a regiment like any other: it takes its HQ place from the
  transport, is attached to that HQ, and can be selected and ordered.
  [code, game] That is while the plane is still 8.5 short of T, 2.2
  seconds before it passes over: the banner is up before any man has
  landed, and in the game it reads as appearing when the plane gets to
  the target. [code, game] If no spot is open, "Could not create <regiment name>" is
  posted and nothing is delivered; the plane flies home and the cost is
  not refunded. [code]
- Each thing is made at the plane's position, its own `height` below the
  plane, facing the plane's line, in the `falling` state. It's there and
  drawn from that moment, under its open parachute: nothing of it is
  hidden or held back. The gap seen between the plane and the newest
  parachute is the plane's own travel since that release (up to 2.1)
  and the man starting his `height` below it. [code]
- Its landing point is ahead of where it left the plane, along the
  plane's line, by max(0, √(2a ÷ 49.05) × s − `fall_drag`), where a is
  its height above the ground as it leaves and s the plane's speed at
  that moment: full speed for the first thing out, half speed for the
  rest. With the stock values the first paratrooper lands 5.5 ahead of
  where he left, the others 2.7 ahead, and a crate (first and only)
  straight below. [code]
- So a regiment's men come down in a line along the plane's path, but
  not an even one, and not centred on T (the centring allows 1.4 a man).
  For the nine paratroopers the landing points, measured along the line
  from T, are −3.0, −3.0, −0.9, +1.2, +3.3, +5.4, +7.5, +9.6, +11.7:
  the first two on the same spot, the rest 2.1 apart, the third about
  on T. [code] In the air the parachutes hang closer than they land,
  1.2 to 1.8 apart along the line against a canopy about 2.9 across,
  since each man is carried forward as he falls and the earlier ones
  have been carried further. [code, data] A line along the plane's path
  with the canopies overlapping is what the screenshots show
  (`jpn-airborne-drop-3-landing.jpg`, `jpn-airborne-drop-4-landed.jpg`,
  and the Russian drop). [game]
- The landing point is pulled inside the map, then checked for room: the
  thing's blocking shape must overlap no blocking thing and only tiles
  passable for a land unit. If it's blocked, the game tries rings round
  the point, radius 1, 2, … 16: eight points a ring, the first one along
  the plane's line and the rest in 45° steps clockwise (seen from above);
  the first clear point wins. If all 129 are blocked the thing is simply
  removed: no message, no refund. [code]
- It then moves in a straight line from where it left the plane to the
  landing point, with no drift. Its height drops by 49.05 × t² every
  simulation tick of length t; at 16 ticks a second that's a steady 3.07
  a second, so a man leaving 15 above the ground takes about 4.9 seconds.
  It keeps the facing it was made with all the way down and on the
  ground. [code]
- `fall_drag` only shortens the lead. It doesn't slow the fall. [code]
- When it reaches the ground the `falling` state ends. A unit with a
  character then plays its `SUMMON` animation (`summon1`, the
  `Landing.kf` of its character component) and is in the `summoning`
  state while it plays. [code, data]
- While it falls a member doesn't count as with its regiment
  ([supply](supply.toml)). [code]
- Nothing is random from man to man: every one leaves from the plane's
  height less its own `height`, falls at the same rate and keeps the
  same facing, and no landing point or facing is randomised. From the
  second man on the parachutes hang in an even diagonal; the first man,
  thrown further, hangs close behind the second, and ground of
  different heights and blocked landing spots break the line too.
  [code]
- With the stock values the first man touches down 4.9 seconds after
  he left, just after the ninth has left the plane. [code]

After landing:

- The men don't stay in the line they came down in. A regiment keeps its
  members in formation, and that applies from the moment each one joins
  it, which here is the moment he leaves the plane: he is ordered to his
  place, the regiment's position plus his slot's offset in the regiment's
  current formation (the layout's `default_formation_ids`,
  `regiment_defensive` for the infantry layout), turned to the regiment's
  facing. [code]
- He can't act on that while he falls: a falling unit doesn't move. Once
  down he runs to his place, and on arriving turns at once to the
  regiment's facing, like any member ending a move
  ([movement](movement.md)). [code] In
  `jpn-airborne-drop-3-landing.jpg` the first men down are already
  running, closer together than they landed. [game] Whether he waits for
  his landing animation to finish first is an open question.
- Nor can he fight while he falls, though he can be shot; the men
  already down fight as usual, whether or not the rest have landed
  ([combat](combat.md#what-can-hit-a-falling-man)). [code]
- The regiment's position follows its leader ([movement](movement.md)),
  the member in the layout's `PrimarySlot`. Until he has left the plane
  the regiment, and so every place in its formation, is where it
  appeared, at T. Once he has, the regiment's position moves towards his
  at its usual catch-up pace, tracking him as he comes down, and the
  formation's places move with it. So the regiment ends up formed around
  where its leader landed, not exactly on T. [code]
- An idle regiment looks its formation over every 2 seconds and sends
  any member who isn't in his place there again, twice at most, then
  leaves them be. [code] That is why the regiment can still look loose
  some seconds after the last man is down
  (`jpn-airborne-drop-4-landed.jpg`): men are on their way to places
  that have themselves just moved. [guess]
- The regiment itself is given no order, by the delivery or the
  building: it's idle from the start. [code]
- The number of men is the number of filled places in the regiment's
  layout, and a layout slot with `number` above 1 is that many places:
  the infantry layout (`regiment_infantry`) has 3 + 1 + 1 + 1 + 2 + 1 = 9.
  [data]

What it looks and sounds like, from the data: [data]

- Falling things use their `falling` stance and aspect: the model
  `<resource_prefix>Falling.nif` (the figure under an open parachute),
  with animations switched off.
- Leaving the `falling` state creates the closing-parachute effect:
  `parachute_close_<nation>` at the `neck` for paratroopers,
  `parachute_close_saboteur` and `parachute_close_agent` for those two,
  `parachute_close_generic` for crates.
- The effect is a temporary model effect with no `duration` and the
  default attachment, so by the rules in [effects](effects.md) it stays
  on the man's `neck` and goes with him as he moves off, and lasts the
  length of its model's animation. It takes the neck's position but not
  the bone's orientation: it stands upright, turned once when it's made
  ([effects](effects.md#where-an-effect-appears)), and doesn't turn with
  the man after that; the tipping seen as it collapses is the model's
  own animation.
- Which way that turns it, from the standing model
  (`<resource_prefix>.nif`) as it rests: the `neck` bone (`Bip01 Neck`)
  has its x up the spine, its y along the man's forward and its z out to
  his side, the whole bone leaning 4.5° forward. The command's default
  `orientation` taken through it comes out pointing behind the man and
  4.5° above level. So the canopy's heading is the opposite of the man's
  facing, and it stands upright but for that 4.5° tilt. The landing
  animation's first pose, not the rest pose, is what the game reads, so
  the tilt may differ a little. [data, guess]
- The falling model's `neck` rests leaning much further (the same
  heading comes out 53° above level), which would tip the canopy over;
  the upright canopy in the game says the standing model is the one
  read, the swap having happened first. [data, game] `critical = true` keeps it from being dropped.
- Its model (`Parachute/<NATION>/<NATION>ParachuteClose.nif`) is a
  canopy alone, no figure, and does all the work itself: its bones fold
  the canopy down over about the first 2.3 seconds, and its material's
  alpha is keyed 1 until 1.17 seconds and then down to 0 at 1.97. So the
  canopy is seen collapsing for a little over a second and has faded
  away by two; nothing else fades it. The model's keys run on to 6
  seconds, with nothing left to see. A half-faded canopy over a man
  already on the ground is what the Russian drop shows. [game]
- The swap is at touchdown, in one step: the falling model (figure and
  open canopy together) gives way to the unit's ordinary model starting
  its landing animation, and the closing canopy appears at the same
  moment. [code]
- The crates, the saboteur and the secret agent play `parachute_drop_sfx`
  when created. The paratroopers name no sound of their own for the drop.
- `parachute_open` and `parachute_travel` are defined in
  `Effects/parachute_effect.tgi` but nothing uses them.
- The stance's `airborne` property does nothing: its target flag is
  commented out in `properties_stance.tgi`.

## Water transports

- A water transport is launched from the recruiter when the recruiter is
  itself `water_based` (a battleship). The boat is made at a clear spot by
  the ship: the ship's own position first, then the four rings of eight
  points described for a recruit without a `DeploymentInfo` block in
  [recruiting](recruiting.md#a-single-unit), the rings'
  radius being the ship's. Here the result is used the right way round:
  the first clear point is where the boat appears. No clear point: "Could
  not create <boat name>", and the cost is refunded. [code]
- The boat doesn't sail to the aimed place T. Its landing point is found
  on the straight line from the boat to T: the last tile on that line
  that belongs to the boat's own body of water before the line enters the
  connected area of land T stands on. The landing point is that tile's
  centre. [code]
- The landing point is good only if it's on the map and it, or one of the
  eight tiles round it, belongs to T's area. Otherwise (the line never
  reaches T's land from the boat's water, say across another island)
  "Invalid deploy location" (`recruit_invalid_landing_location`) is
  posted, no boat is made, and the cost is refunded. This is the only
  place a water transport's target is tested for shore; the aiming check
  doesn't. [code]
- "<template name> recruited" and the HQ place work as for a plane. [code]
- The boat sails to the landing point as an ordinary unit of its side
  (while idle it shows its cargo as the attached `<nation>_infantry_solid`
  or `<nation>_truck_solid` effect on its `hitch`). On arriving it enters
  the `unloading` state: its `Unload` animation plays (9 seconds for the
  infantry boat, 3 for the truck boat) and the cargo effect is swapped
  for the `_fade` one. [code, data]
- Unloading puts one thing out at once and one more every
  `deployment_delay` (0.5 seconds), in layout slot order. Each is made at
  the boat's `creation_offset` (0, 2.5: just ahead of the boat) and walks
  straight to its `deployment_offset` (0, 8), as for a building with a
  `DeploymentInfo` block. If the spot it arrives at is blocked it moves
  to a free spot nearby; the sixth time that happens it dies. [code]
- The regiment appears with the first one, its banner at the boat's
  `deployment_offset` point (or the nearest open spot within 4), and
  takes its HQ place then. No open spot: "Could not create <regiment
  name>", nothing is delivered. [code]
- As soon as the regiment's leader is ashore the regiment is ordered to
  move to T. [code]
- When all are out the boat sails back to its ship and is taken off the
  map there. A boat that gets home without having put anything out posts
  "Could not unload <template name> - funds returned"
  (`transport_error_unloading`) and refunds the cost. [code]
- A water transport ordered from a recruiter that isn't on water comes in
  from the map's edge like a plane, at ground level. No shipped recruiter
  on land offers one that isn't hidden. [code, data]

## Special operations and triggers

- A special operation that summons something with a `transport_actor`
  (Supply Drop and the other crates, the saboteur and secret agent
  regiments, S.A.S. Commandos) uses the same delivery as above, aimed at
  the operation's target point. What differs: [code]
  - nothing is queued or paid for through a building. The transport
    carries no cost, only the cargo's upkeep;
  - no HQ is involved: a delivered regiment arrives unattached;
  - no "recruited" line is posted;
  - the plane's entry point doesn't depend on any building. It comes
    straight along one map axis from whichever edge is farthest from the
    target T = (x, y): with dx the larger of x and width − x, and dy the
    larger of y and height − y, if dx > dy it flies along x, from x = 0
    when x is the larger and from x = width otherwise, at T's y; if
    dx ≤ dy it flies along y, from y = 0 when y is the larger and from
    y = height otherwise, at T's x. The point is pulled inside the map
    as above.
- A single thing (a crate) is one release: the plane's release point is
  its lead short of T, the thing is made below the plane and comes down
  as in [Coming down](#coming-down), blocked landing spots included.
  [code]
- The trigger effect `actor_create_transported_named` starts the same
  delivery and names what it delivers
  ([saved-state](saved-state.md#deliveries)). Its transport appears at
  the effect's first place (at its flying height if it flies, else on the
  nearest clear ground) and delivers to the second; a boat lands at the
  shore point nearest the second place, and the effect fails if there's
  none ([triggers](triggers.md#making-and-removing-things)). [code]

## Open questions

- Does the 15 second build really come before the plane? The code queues a
  Deploy to Location order like any recruit, but the screenshots can't
  show the wait. To check in the game: time from the click to "recruited".
  (implementer, 2026-11-13)
- Is the order given when the button goes down or when it comes up? For
  a template that may be turned when placed, the press starts a drag that
  turns the preview about the pressed place (as Unpack's preview may; see
  the same question in [buildings](buildings.md)), and what ends the drag
  wasn't followed. The facing is dropped from this order either way.
  (analysis, 2026-11-13)
- Does Escape also end aiming? Only the right click and choosing another
  command were found. (implementer, 2026-11-13)
- Can the transport be selected by clicking it on the map and given
  orders? Known: it's an ordinary unit of its side, the HQ panel's slot
  selects it, and it's skipped by the selection-cycling keys. Not
  traced: map clicks, and what orders it accepts. Who can hit it is in
  [combat](combat.md#things-in-the-air). (implementer, 2026-11-13)
- Does the transport give sight? Its template's detection would say so;
  not checked. (implementer, 2026-11-13)
- How long is each release's 0.6 seconds in ticks: is a wait of 0.6
  nine ticks, ten, or sometimes either? It moves the 2.1 spacing by up
  to 0.2. (analysis, 2026-11-14)
- Do the first two men, whose landing points are the same to within
  0.1, end up on one spot, or does the second get moved to a ring point
  because the first is in the way? The first is still in the air, well
  short of the spot, when the second's is chosen. (analysis, 2026-11-14)
- Does a landed man wait for his landing animation before running to
  his place? (implementer, 2026-11-13)
- Does any sound confirm the order? A unit ordered about plays its
  `WhenCommanded` reply ([sounds](sounds.md#unit-replies)); whether
  that covers this order to a building, and which HQs name one, wasn't
  checked. (implementer, 2026-11-13)
- Is the leader the last man out? The `PrimarySlot` is last in the
  infantry layout, and things leave in slot order, but the order the
  slots are kept in wasn't confirmed. (analysis, 2026-11-13)
- Does the computer player order a battleship's amphibious Corps truck
  without a place, which would take the plain recruit path in
  [recruiting](recruiting.md#a-single-unit)? (analysis,
  2026-11-13)
