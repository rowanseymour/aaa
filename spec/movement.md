# Movement

How a ground unit drives a route once it has one: turning, speed, corners,
what it does about a spot it can't turn onto, when it counts as arrived,
the Explore order, and how a truck's trailer follows. Where a route's points go isn't covered,
only when there's no route to be had ([Moves that can't be
made](#moves-that-cant-be-made)). What a mind saves about
a move is in [saved state](saved-state.md).

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Turn types

- A mover's `turn_type` is `INSTANT`, `IN_PLACE` or `WHILE_MOVING`.
  `INSTANT` units face any new heading at once and their `turn_radius` is
  ignored (treated as 0 everywhere below). [code]
- `IN_PLACE` and `WHILE_MOVING` move alike: nothing in how the unit turns,
  drives or arrives tells them apart. Both turn at a fixed rate of
  `speed_max` / `turn_radius` radians a second, whatever speed the unit is
  doing at the time. [code] Stock HQ and supply trucks (`speed_max` 3,
  `turn_radius` 3) turn at 1 radian a second. [data]
- So an `IN_PLACE` unit does not stop to turn: it drives while turning,
  like a `WHILE_MOVING` one. [code]
- The root mover template sets `turn_type = INSTANT`, so a unit with a
  `turn_radius` and no `turn_type` anywhere up its template chain turns
  instantly and its radius does nothing. A unit that sets only
  `turn_radius` keeps its template's `turn_type`: stock halftracks set 1.5
  over their template's `WHILE_MOVING`. [code, data]
- `turn_radians_per_sec` is never used. [code]

## Driving a leg

- A route is a list of points, each with a heading: the direction of
  travel along the leg that ends at it. A point made for a move straight
  from where the unit stands has the direction from the unit to the point.
  [code]
- For each leg the unit has a heading to hold (the leg's) and an aim point:
  the leg's point, pulled back along the heading by the corner lead (see
  [Corners](#corners)). The heading stays as it was when the leg was made;
  it is not re-aimed at the point as the unit moves. [code]
- Each simulation step, in this order: [code]
  1. It turns toward the leg's heading by its turn rate times the step
     length, the shorter way round. If that is enough to reach the heading
     it takes the heading exactly.
  2. Its speed changes (see [Speed](#speed)).
  3. It moves speed times the step length along the way it now faces.
- It only ever moves the way it faces: never sideways or backwards. [code]
- So the unit doesn't steer at its aim point. It swings onto the leg's
  heading and drives; if the swing has carried it off the leg's line it
  stays off until the correction below. [code]
- The leg ends when the distance left to the aim point, measured along the
  leg's heading (not straight-line), is no more than one step's travel.
  That includes a unit already level with or past the aim point, however
  far to the side it is and whichever way it faces. On that step, a unit
  facing exactly along the heading moves just the distance left, ending
  level with the aim point; any other takes a full step. [code]
- A leg also ends at once when the unit is within 0.01 of the aim point,
  and a leg whose aim point is within `TAIReachedDestinationThreshold`
  (stock 0.1) of the unit when it's set ends without moving. [code]
- A point the unit is already level with or past (measured along that
  point's heading) when it comes to steer for it is skipped. [code]

## Speed

- A unit drives at its travel speed: its top speed (`speed_max` as its
  properties change it, [below](#a-units-top-speed)) unless its order or
  its regiment sets a lower one ([a regiment's
  speed](#a-regiments-speed)). [code]
- Speeding up, it gains `acceleration` a second until it reaches the
  travel speed; `acceleration` 0 means it's at travel speed at once. [code]
- Slowing down is immediate: a lower travel speed is taken at once, and a
  unit that stops, stops dead on the step it arrives. `acceleration` plays
  no part in stopping. [code]
- It never slows, stops or reverses to make a turn. [code]
- Terrain sets a speed limit. Each terrain has at most one, for everyone
  on it: [code]
  - It's the `TerrainSpeedModifier` amount of the last property the
    terrain lists that has one above 0. Stock terrains list the infantry
    property and then the vehicle one, so the vehicle figure is the
    terrain's limit: city 3, forest 2, jungle 1.5, sand and snow 3.5.
    The infantry figures (forest 2.5, jungle, sand and snow 2) are never
    used for movement. [code, data]
  - A terrain that lists no such property has no limit. [code]
  - The limit used is never below `TerrainMinimumMoveSpeed`, which the
    data files don't set; the game's own default is 0.5. [code]
  - A terrain of type `MOUNTAIN` is given a limit of 0.0001 whatever it
    lists, which that floor then raises: on a mountain the limit in play
    is `TerrainMinimumMoveSpeed` itself, 0.5 with stock data. It doesn't
    depend on what any other terrain lists. [code]
  - It seldom comes up. A terrain says who may cross it with `land` and
    `water`; `MountainTerrain` sets both false and no stock mountain
    changes them, so mountain tiles are closed to everything that moves
    on land or water. [code, data] Aircraft, which do cross them, have no
    terrain looked at and so no limit ([terrain](terrain.md)). The
    mountain limit only bites on a ground unit whose terrain is a
    mountain anyway: one a map placed there, or a regiment member whose
    leader's tile is one. [guess]
  - Each step, after the unit's speed has moved towards its travel
    speed, a speed above the limit is cut to it at once (a regiment
    member keeping its place then stretches its step, so it can go
    faster: [keeping place](#keeping-place)). It's an absolute
    speed, not a share of `speed_max`: a unit slower than the limit isn't
    affected. [code]
  - Whether the unit holds the terrain's property plays no part, nor
    whether it's infantry or a vehicle: foot soldiers in forest are held
    to 2 like the tanks. [code]
  - The terrain is the unit's terrain as [terrain](terrain.md) has it: a
    member with its regiment (the leader, or a member inside the
    regiment's control zone, `control_range`) is limited by the terrain
    under the regiment's position, not by the tile it's on, so one
    lagging behind in forest while the regiment's position is on open
    ground has no limit at all. Whether a member is with its regiment is
    settled when the regiment's zone is laid again or the member ends a
    move, not continuously. so the
    body of a regiment shares one terrain limit, on top of moving at its
    slowest member's top speed ([a regiment's speed](#a-regiments-speed)). A straggler is limited by its own tile. [code]
  - A unit with the `TRAILBLAZING` flag (property `trailblazing`, held by
    the stock recon regiments) ignores the limit. [guess, data]
- Speed carries through from one leg to the next. After a stop (the end of
  a move, or between tries at a missed destination) it starts from 0
  again. [code]
- Because the turn rate is fixed, a unit below its top speed (still
  accelerating, or held back by its regiment) turns in a tighter circle
  than `turn_radius`: the circle's radius is `turn_radius` times its speed
  over its top speed. [code]

### A unit's top speed

- A unit's top speed is its `speed_max` times (1 + the sum of the
  `SPEED_MAX` `MultiplyModifier` amounts of every property it holds),
  never below 0.05 times `speed_max` (`ModifierAttributeClampMultiply`).
  The amounts add; they don't multiply each other. [code]
  - Example: an infantry unit in Assault formation (−0.25), engaged
    (+0.25) and prone (−0.6) is at 1 − 0.25 + 0.25 − 0.6 = 0.4 of its
    `speed_max`. [code, data]
  - The same property held twice (from two sources, or the same
    operation cast on it again) counts once. Different properties add up
    even when they come from the same kind of source: Courage and
    Forced March together are +0.7. [code]
- Each modifier works on the unit that holds the property, on that
  unit's own top speed. A property the regiment holds is handed down to
  each member, so it reaches every unit in the regiment; see below for
  which properties those are. [code]
- A property with `required_properties`, handed down from a regiment or
  given by a technology, reaches only the units that have them (a
  special operation gives its property without checking, [special
  operations](special-ops.md)). A technology is given to every unit of
  its side that qualifies:
  `tech_diesel_engines` (`AATechnologyTank`, `required_properties =
  tank`) speeds up only the tanks of a mixed regiment, and
  `tech_superchargers` (`AATechnologyAirplane`, `aircraft`) only
  aircraft, delivery planes included, which fly at their top speed
  ([deliveries](deliveries.md#the-air-transport)). Handing down from the regiment checks each member the same
  way. [code, data]
- A change takes effect at once: the top speed is recomputed whenever a
  property arrives or goes; a unit faster than its new top speed drops to
  it at once, and one below it speeds up at its `acceleration`. [code]
- The turn rate follows the top speed: it's the top speed over
  `turn_radius`, recomputed with it, so a faster unit also turns faster
  and the circle it draws at top speed keeps the radius `turn_radius`.
  [code]
- The terrain limit is applied after all this, as an absolute speed
  (above). [code]

Where the stock `SPEED_MAX` modifiers come from, and who holds them:
[code, data]

| Source | Held by | Amount | When |
|---|---|---|---|
| Formation `regiment_aggressive` (Assault, the layouts' combat formation) | regiment, handed down | −0.25 | while the regiment is in it |
| Formation `regiment_column` (Column) | regiment, handed down | +0.25 | while in it |
| Formation `regiment_defensive` (Aggressive) | — | none | |
| Layout's Broken Formation property | regiment, handed down | +0.25 | while the regiment has no formation (routing, Exhausted) |
| State `actor_state_engaged` | regiment, handed down; also a unit on its own | +0.25 | while engaged |
| State `actor_state_targeted` | regiment, handed down | −0.10 | while targeted (below) |
| Stance `infantry_kneel` | the unit | −0.2 | while kneeling ([stances](stances.md)) |
| Stance `infantry_prone` | the unit | −0.6 | while prone ([stances](stances.md)) |
| Technologies (`tech_diesel_engines` +0.15, `tech_superchargers` +0.25) | each qualifying unit | | once researched |
| Special operations | each unit reached | | while the enchantment lasts ([special operations](special-ops.md#what-the-modifiers-do)) |

- Formations: the formation the regiment is in decides, moving or
  standing: [code, data]
  - Moving or standing on the player's orders, it's in the formation the
    player chose, so Column is +0.25 and Assault −0.25 whether or not it's
    moving.
  - Attacking (the approach of an attack order or attack-ground),
    engaged, and guarding are in the combat formation, Assault, whatever
    the player chose ([combat](combat.md#attack-orders)): −0.25.
  - Engaged also gives `actor_state_engaged` +0.25, which cancels
    Assault's −0.25: an engaged regiment's units are at their full
    `speed_max`, stances aside. On the approach of an attack order the
    regiment isn't engaged yet, so it closes in at 0.75.
  - Routing and Exhausted regiments have no formation and hold the
    Broken Formation property instead: +0.25.
- Engaged: the regiment holds `actor_state_engaged` while it's engaged,
  and hands it to its members; a unit that is itself engaged holds it
  too, which counts once ([combat](combat.md#engaged-and-under-siege)).
  A unit standing alone and engaged gets the +0.25 with no Assault
  penalty. [code, data]
- Targeted: a regiment is targeted while another regiment is engaging it
  and it isn't engaged itself (it hasn't engaged back, say while moving
  or routing away). The regiment checks once a second and whenever its
  activity changes. Its units then move at −0.10. [code, data]
- Stances: an infantry unit kneels while it is both engaged and under
  siege ([combat](combat.md#engaged-and-under-siege)); those whose
  template also lists `StanceProne` go prone instead at half health or
  less. The speed changes when the man's stance does, which can wait
  for the animation he's playing to end. When stances apply, and the
  rest of what they do: [stances](stances.md). [code, data]

### A regiment's speed

- A regiment moves at the top speed of its slowest member: the lowest of
  the members' top speeds, each with its own modifiers. Of a mixed
  regiment with `tech_diesel_engines`, only the tanks are faster, so the
  regiment only gains if a tank was its slowest member. [code]
- It's kept up to date: a member slowing below it lowers it at once; when
  the slowest member speeds up, joins or leaves, it's worked out afresh
  from all the members. [code]
- A move order hands the regiment's speed (or a lower one, if the order
  carries a speed) to every member, and the leader drives the route at
  it. A change to the regiment's speed during a move is handed to the
  members again. [code]
- An order's lower speed lasts for that move: through the fights that
  interrupt it, since the move takes up again at the same speed, until
  it arrives or another order replaces it. A trigger's move can give
  one ([triggers](triggers.md#effects-the-shipped-multiplayer-maps-use),
  `actor_moves_to_location`); a person's moves and the moves a regiment
  makes for itself go at top speed. [code]
- A regiment speed below 0.1 is no speed: a member handed one stops.
  [code]

#### Members' routes

- A regiment's places face the way the regiment faces. Each member's
  place is its position in the layout, set out from the regiment's
  position (which follows the leader's) and turned to the regiment's
  facing. [code]
- When a regiment sets off on a route it turns at once to face along
  the route's first leg, and its places turn with it. From then on it
  follows its leader: each time the leader passes a point of the route,
  the regiment drops that point from its own copy of the route and turns
  at once to face along the next leg, places and all. So during a move
  the regiment always faces along the leg its leader is on. [code]
- In a formation that is `organized` (all of AA's regiment formations
  are), the leader is handed the route itself, and every other member,
  with the regiment or away from it, is handed a copy of the route
  fitted to its place, with the regiment's speed: [code, data]
  - Each point of the copy is the leader's point moved by the member's
    place: its place's distance ahead of or behind the leader, and to
    the leader's left or right, as the regiment faces at set-off, laid
    out along that point's own heading instead. A place 2 to the
    leader's left puts every point 2 to the left of the leader's point,
    square to the leg that ends there; a place 3 behind puts it 3 back
    along that leg. [code]
  - Every point but the last is then slid along the leg that ends at
    it to where that leg's line meets the next leg's line, so the
    member's legs run parallel to the leader's, each at the same
    offset, and meet in clean corners: a member on the inside of a turn
    turns before the leader's corner, one on the outside after it.
    Where two legs run the same way the point stays where it was moved
    to. [code]
  - The points keep the leader's headings. The last point, the member's
    destination, is its place at the end of the route as the regiment
    will face along the last leg. [code]
  - Nothing tests the moved points against the ground when the copy is
    made. A member finds a point it can't reach as it drives: when, at
    a look, the way to its next point is blocked, it pulls that point
    back towards the leader's point it was made from, a short step at a
    time, until it reaches a spot it can drive to in a straight line,
    and goes there instead. If none is found short of the leader's
    point, that counts as a try at the point. A point it can't reach
    after three tries is given up and counts as passed; but when that
    point is its last and it isn't within 0.5 of it, its move fails.
    [code]
- A member that joins the regiment, or comes back to it (back inside
  its control zone after being cut off), while the regiment is moving
  is handed a copy at the regiment's next look, about once a second,
  made the same way from what is left of the route and the regiment's
  facing then. [code]
- A regiment with no `organized` formation (one routing or exhausted
  has no formation at all) doesn't keep place. Each member with the
  regiment is handed the route moved bodily by where the member stands
  from the leader, unturned and with no corner fitting, and drives it at
  the regiment's speed like the leader; a member away from the regiment
  is sent to the move's destination on its own. [code; guess: where the
  member away is sent] Kohan II's `herding` formations aren't covered.
- **Getting onto its route.** Every point keeps the leader's heading, so
  the first leg holds that heading rather than pointing from where the
  member stands to its first point. A member off its line (just ordered,
  or pushed off it) swings onto the heading and drives parallel to its
  line, and at its next look the correction under
  [Second thoughts](#second-thoughts) (Getting back on line) brings it
  back: 1 or more off the line, it turns in by up to 45 degrees for a
  point on the line ahead, or, where that turn would meet the line
  beyond its next point, faces that point at once and dashes straight
  at it. That straight dash is the member's **detour**: it's on it
  until it reaches the point (or it's taken off the route) and then
  carries on along its route. The turn-in point is an ordinary route
  point, which puts the member one point further behind the leader
  until it passes it. [code]
- **At the end.** A member's move ends when it has no route left and is
  within 0.5 of its destination; if it isn't, it plans its own way
  there, as under [Arriving](#arriving). It can also stop short (well
  ahead of its place when the leader has stopped, below). When its move
  ends it faces the regiment's facing at once. [code]
- When the regiment arrives (within 0.5 of where it was sent), it faces
  along the route's last leg. If the move order carried a facing (the
  player drags one out as the order is given,
  [commands](commands.md#targeted-commands)), the regiment turns to it at once instead, its places turning with it, and
  the order's facing is then forgotten. Then every member is sent to its
  place, each on a move of its own from where it stands, and faces the
  regiment's facing when it gets there. [code]
- **A new order while moving** takes effect for the whole regiment at
  its first look after the order: the route is planned and every member
  is handed its new route then, dropping the old one, wherever it is.
  The same fresh hand-out happens during a move when the regiment's
  formation changes, when the thing it's following moves far enough
  for it to plan again, and when a long route moves on to its next
  stage. [code; guess: that the first look comes straight after the
  order]

#### Keeping place

The leader drives the route at the regiment's speed R. Every other member
heading for its place on its route (above) sets its own pace v each time
it steers (when it takes up a leg, and on each look along the way, about
once a second): [code]

- Mostly v = R + d ÷ 2, where d is how far its place is ahead of it:
  - While it has as many route points left as the leader (it's on the
    same leg as the leader), d is the distance from the unit to its
    place, measured along the heading of the leg the unit is on:
    positive when its place is ahead of it (it has fallen behind),
    negative when it's ahead of its place. Its place here is the
    regiment's position plus its place in the formation, with the
    regiment facing along the leader's leg.
  - Within 6 of the corner at the end of its own leg (its distance to
    its own next route point, when that point isn't its last), its
    place's sideways offset from the leader (its left or right distance,
    as the regiment faces) is counted in as well, from none 6 away to
    all of it at the corner: a place on the outside of the turn counts
    as that much further ahead, one on the inside as that much nearer,
    so the outer file speeds up and the inner slows down round the
    turn. The turn's direction is from its own next leg's heading.
  - With a different number of route points left from the leader, d is
    how much further it has to go than the leader along the leader's
    route, measured from each of them; but if that disagrees in sign
    with how many route points it is behind the leader (negative when
    it's ahead), d is half that number of points instead.
- While it's on a detour (above), its pace instead rises evenly from R,
  when it has no more route left than the regiment, to
  `MoverCatchupSpeed` × its top speed when it has 4 or more further to
  go. It never drops below R on a detour. [code, data]
- v is never above `MoverCatchupSpeed` (1.75 in AA) times the unit's own
  top speed. [code, data]
- If v comes out at 0.2 or less (it's well ahead of its place), it stops.
  If the leader is still moving it waits a second and looks again; if
  the leader has stopped, its move ends there. [code]
- It drives at the regiment's speed R (accelerating to it as usual) with
  each step stretched by v ÷ R, so once up to speed it covers v a second.
  The terrain limit cuts R, before the stretch, so a member catching up
  in forest goes faster than the forest limit by the same proportion.
  [code]
- So a member behind its place moves at the lower of R and its
  terrain limit, times v ÷ R, with v at most `MoverCatchupSpeed` × its
  own top speed. Where R is under the limit the terrain plays no part;
  where the limit is under R, the member's catching up is cut in the
  same proportion as the leader's own speed. [code]
- The leader never slows down or waits for members behind their
  places: it drives at R, the regiment's speed (the slowest member's top
  speed, members away from the regiment included), however far behind
  any member is. The only waiting is a group's, below. [code]
- A member no longer in a regiment drives at its own top speed. [code]
- A unit making its way back to its regiment (returning after being cut
  off, not yet with it) while the regiment is moving drives at
  `MoverCatchupSpeed` × its own top speed instead of R, stretched as
  above when it's heading for its place. It keeps that speed until the
  regiment next hands it a speed. [code]

#### Several regiments ordered together

- Giving one order to a selection of several regiments that isn't a
  group with Group Behavior on (below) gives each regiment its own
  order. Nothing ties them: each moves at its own speed and arrives
  when it arrives, whether the order is a move, an attack, a guard or
  an attack-ground. [code]
- A move or guard on a place sends each regiment to the clicked point
  plus its own offset from the selection's centre (the average of their
  positions), cut to at most `GroupZoneRangeMin` (18) long, so they
  keep roughly how they stood. An attack, attack-ground or bombard
  sends every regiment to the same thing or point. Which orders spread
  is the command's `apply_group_offset` (true by default; false for
  `kill`, `bombard` and `air_attack`). [code, data]
- Only an order on a place spreads. A move or guard on a thing (a
  right-click on a regiment, unit or building) gives every regiment the
  same order on that thing, with no offset, whatever
  `apply_group_offset` says. So regiments moved onto a thing they
  can't attack don't keep how they stood: each heads for the thing
  itself and stops as one alone would, within 0.6 × its own
  `control_range` of it ([combat](combat.md#attack-efficiency),
  Resting), at the first point its own route comes that close. They
  end up spread round the side of the thing each came from, regiments
  with a smaller `control_range` closer in, and nothing keeps one from
  stopping where another stands. [code]
  - When the thing moves off, each follows it on its own, re-aiming and
    stopping by its own distance, just as one regiment does. Nothing
    keeps their places relative to each other. [code]
  - Each regiment decides for itself whether the order is a move or an
    attack: in a mixed selection, those with an ability that can attack
    the thing attack it and the rest move onto it. [code]
#### Groups

With Group Behavior on, an order to the selection goes to it as one
group, whose regiments are timed to arrive together. [code, data]

- Group Behavior is a switch on the selection's current group
  ([selection](selection.md#control-groups)): a saved group selected
  exactly, or the player's one stand-in for any other selection of two
  or more. The stand-in starts off, and since it's one switch for all
  of them, turning it on for one unsaved selection leaves it on for
  every later one until it's turned off. A newly saved group takes the
  switch of the current group it was saved from. [code]
- The button is in command grid slot 4 (row 1, second column), hotkey V
  in AA (H in Kohan II): `group_behavior_enable` while the switch is
  off, `group_behavior_disable` while it's on, one at a time. It shows
  whenever two or more of the player's own things are selected, saved
  group or not. Pressing it only flips the switch; nothing is sent to
  the regiments until the next order. [code, data]
- An order counts as given to the group as one whenever two or more
  things are selected and the current group's switch is on, except
  `set_formation`, which always goes to each on its own. Each such order
  (not queued with Shift) makes a fresh group of what's selected, and
  each regiment leaves any group it was in for the new one, keeping the
  old one's facing; an order queued with Shift goes to the group it
  already has. [code]
- A regiment leaves its group (a group left empty goes) when: it accepts
  an order given to it rather than to the group (Group Behavior off, or
  the regiment ordered alone), apart from `set_formation`; it starts
  routing or becomes exhausted; it joins another group; it changes side;
  or it's removed. Orders the group itself hands its regiments don't
  take them out. [code; guess: that routing and exhausted are what end
  it there]
- Then it no longer times its arrival with the rest.

The group's orders and timing: [code, data]

- An order on a thing reaches each of its regiments as the same order
  on that thing, as it does without Group Behavior (above): a move onto
  a thing they can't attack still has each regiment head for the thing
  and stop within its own distance of it, and follow it on its own when
  it moves. What the group adds is the timing below, so they arrive
  together rather than each when it gets there, and each again when
  they follow. [code]

- Once a second, for each of its regiments that is moving and takes
  part (below), it works out
  the route the regiment has left and the time that takes at the
  regiment's own speed. Each regiment is then given the speed that
  makes its time equal the longest: its own speed × its time ÷ the
  longest time. The regiment furthest out in time goes at its own speed
  and the others slow down to match; none is ever sped up. [code]
- That speed is the regiment's speed R from then on, as an order's
  speed is ([A regiment's speed](#a-regiments-speed)): it goes no higher
  than the slowest member's top speed, is handed to every member, and
  the members keep place against it. Worked out again each second.
  [code]
- Each regiment has a place in the group, an offset from the group's
  centre in the group's own frame (sideways, and ahead along the way
  the group faces): [code, data]
  - When all its regiments are in the same formation, regiment by
    regiment in group order they take the positions of that formation's
    `group_formation_ids` (`Game/group_formations.tgi`: AA's
    `group_aggressive` sets (0, 6), (±13.5, 6), (0, −6), (±13.5, −6));
  - otherwise each keeps where it stands from the group's centre, turned
    into the group's frame; one not taking part (below) is put at
    `GroupZoneRangeMin` from the centre in its own direction.
  - The places are then recentred so their average is the centre.
- When the group is heading for an enemy thing that has a supply zone,
  the regiments time their arrival at the zone's edge plus
  `GroupSupplyZoneBuffer` (5) plus how far ahead its place in the group
  is (the ahead part only, so a place behind the centre counts
  negative), taken off its route, rather than at the end of their
  routes (never less than 0). When the order carried a facing, the
  ahead part is instead the place's y coordinate once turned onto
  the map. [code, data]
- A regiment handed a speed below `GroupSpeedMinimum` (1.0 in AA)
  doesn't crawl: its members stop, and the regiment looks again each
  second without that counting as a try at the move. So a regiment
  nearly there waits for the rest. [code, data]
- Which regiments take part: [code]
  - The group's zone is a circle round its centre (the average of its
    regiments' positions) whose radius is the largest control zone
    radius among its regiments (`control_range`), never less than
    `GroupZoneRangeMin` (18 in AA).
  - Each time the group gives a move, and each time it finds itself in
    a fight (below), it marks the regiments then inside its zone as taking part and the
    rest as not. A regiment joining the group starts out taking part.
    Until the next marking, a regiment left outside goes at its own
    speed and doesn't hold the others back.
  - Whether the group is in a fight is decided by the first member
    taking part, in the group's order, that is engaged or under siege;
    the rest aren't looked at. It's in a fight if that member is:
    [code, data]
    - engaged ([combat](combat.md#engaged-and-under-siege)) with
      something other than what it was sent to attack. It counts unless
      it has an attack target and is engaging that target, or the
      regiment that target belongs to. Its attack target is the target
      of its attack order; while besieging a city, a defender of the
      city or the besieged target; failing those, while engaged, the
      first attack order in its queue whose target it can attack. So a
      group whose regiments are fighting what they were sent to attack
      goes on timing their arrival, while one drawn into fights on the
      way stops. A member fighting its own target ahead of one drawn
      into another fight keeps the group out of a fight;
    - or under siege. Only a lone unit in the group can be: a regiment
      as a whole never is.
  - While the group is in a fight it times nothing: it marks again and
    looks again at once, and timing resumes once it's out of the fight.
    Meanwhile it sends its regiments that aren't fighting to guard one
    that is, and gives its own order again when the fight ends
    ([When a move fails](#when-a-move-fails)). [code]
  - A computer player switches Group Behavior on in a mode where the
    group times every moving regiment, marked or not. [code; guess that
    a human player's Group Behavior button never does]

## Corners

- Before each corner the unit starts its turn early by the lead distance:
  `turn_radius` times the tangent of half the angle between the route's
  next two legs. The lead is 0 when only one point is left. [code]
- It drives the leg until it is level with the point lead short of the
  corner, then takes up the next leg at once: the heading becomes the next
  leg's and the aim point the next point, less that point's own lead.
  Nothing else changes: it keeps its speed and turns at its rate as it
  goes. At its top speed that draws an arc of radius `turn_radius` touching
  both legs. [code]
- When it finishes a turn (takes the heading exactly) the lead for the
  corner ahead is worked out afresh. [code]

## Second thoughts

- While it drives, a unit looks at its move again every 1.0 to 1.3 seconds
  (uniform), and every 0.3 to 0.6 seconds on a leg it was snapped onto
  (below). Between looks it just carries on as under Driving a leg. [code]
- **Snapping onto the last leg.** Picture two circles of radius
  `turn_radius` touching the unit, one centred `turn_radius` to its left
  and one to its right: the tightest turns it can make. When only the last
  point is left and that point lies inside either circle, the unit can't
  turn onto it, so it gives up turning for that leg: it faces the leg's
  heading at once and drives. This is tested when the leg is set and at
  each look. [code]
- **Getting back on line.** At a look, a unit that has finished turning
  (faces the leg's heading exactly) and is 1 or more to the side of the
  leg's line (the line through the leg's point along its heading) puts a
  detour in ahead of the point: it turns toward the line by 45 degrees,
  drives to meet it, and turns back along it, all at its usual turn rate.
  [code] When the offset is small enough that 45 degrees would overshoot, a
  shallower angle is used. [code, guess: the exact angle]
- If that detour would meet the line beyond the leg's point, the unit
  instead faces the point at once and drives straight at it. [code]
- No detour is made if something is in its way. [code]

## A destination close by

These apply when a unit is given a destination of its own, not when it's
handed a regiment's route (a regiment's leader gets the route, see
[Arriving](#arriving)).

- On a move that needs exact arrival (see [Arriving](#arriving)), a
  destination less than 3 away gets a single straight leg from where the
  unit stands, with no route planned. The unit turns onto it at its turn
  rate unless the snap above applies, which it does at once for a spot
  inside a turning circle. [code]
- Otherwise, if the destination is ahead of the unit (not behind its
  facing) and inside the turning circle on the destination's side, the
  unit faces it at once and drives straight there. [code]
- Otherwise a route is planned. [code]
- So a spot close beside a unit makes it jump to face the spot. A spot
  straight behind, on an exact move, is outside both circles: the unit
  starts a U-turn, driving away as it turns; partway round the spot falls
  inside a turning circle and the unit snaps onto the leg's heading,
  usually ending up level with the spot but off to one side. It has then
  missed, and tries again as below. [code, guess: read off the rules, not
  watched]

## Arriving

- When a unit has no route left it stops dead and checks where it is. It
  has arrived if it is within 0.5 of its destination, or within 0.1 on a
  move that needs exact arrival (going to another actor, or to a spot of
  its own to unpack). It is not moved onto the spot. [code]
- If it hasn't arrived it plans again from where it stands and drives
  again. [code]
- A move is planned at most 3 times; when a fourth would be needed the
  move is given up. The count starts again each time the unit finishes a
  stage of a long route, and when an actor it follows moves. [code]
- The unit does not turn to an ordered facing when it stops. A unit in a
  regiment faces the way its regiment faces, at once, when its move ends;
  one on its own stays facing the way it arrived. [code] Which way the
  regiment faces: [Members' routes](#members-routes).
- A regiment's leader is one of its members: [code, data]
  - The member in its layout's `PrimarySlot` (the captain, in AA's
    infantry and mechanized layouts) is the leader whenever that slot is
    filled, including when a replacement fills it again.
  - Otherwise the first member to join an empty-handed regiment becomes
    leader, so a layout with no `PrimarySlot` (AA's `herd_ambient`) has
    one too.
  - When the leader leaves the regiment (it dies, say), the next leader
    is taken at once from the members left: the first, in the order of
    the layout's positions, that is with the regiment (inside its
    control zone, not straggling); failing that, the first member at
    all. The positions run in the order the layout lists its `Slot`s,
    each `Slot` giving `number` of them in turn. [code]
  - So a regiment is without a leader only when it has no members,
    which ends it. A map or saved game stores which member leads.
    [code]
- A regiment's own position follows its leader's. A regiment sent
  somewhere plans a route and hands it to its leader, who drives it as
  above, counting as arrived within 0.5 of the route's last point. [code]
- The regiment has arrived when its position is within 0.5 of where it
  was sent. If its leader has stopped and it hasn't arrived, it sends the
  leader again. Each plan or re-send counts as a try; from the third try
  on, the regiment gives up the move if `CAIMoveMinSecondsBeforeFail`
  (stock 5) seconds have passed since the order. [code]

## Moves that can't be made

What happens when a route is planned to somewhere the unit can't get
to. A regiment plans from its leader's position, by what its leader can
cross; a unit planning its own way (a lone unit, or a member on a move
of its own, such as to its place) plans the same way. A member handed a
copy of its regiment's route plans nothing ([Members'
routes](#members-routes)), and a destination close by may get a
straight leg with no plan ([A destination close by](#a-destination-close-by)).
[code]

### Where the move is aimed

- Before planning, the destination is made a spot the unit could stand
  on: on the map, on a tile open to it (for a land unit, not water and
  not ground closed to everything), and not on a tile a thing blocks
  ([objects](objects.md#blocked-tiles)). A destination that isn't one
  is pulled back along the straight line towards the unit, a tile (2)
  at a time, until it is. At each spot on the way that falls on a
  thing, the spot one tile out from that thing along its facing axis,
  on the unit's side, is tried as well. [code]
- So an order into a lake, onto ground closed to the unit, or onto a
  building heads for the first spot short of it on the line from the
  unit: the near shore, the edge of that ground, the building's near
  side. Only the
  route is aimed there: arriving is still judged against the spot the
  unit was sent to. So once there it hasn't arrived, plans again
  (reaching the same spot) and gives up as under [Arriving](#arriving):
  a lone unit after its third plan, a regiment from its third try once
  `CAIMoveMinSecondsBeforeFail` (5 s) has passed since the order. It
  then comes to rest where it stopped as a failed move does (below).
  [code]
- If that pulling back reaches the unit before it finds a spot, or the
  spot it finds is where the unit already stands, there's no route and
  the move fails (below). [code]

### Areas

- The map is divided into areas, each made of tiles of one kind: tiles
  land things may stand on, water, or ground closed to everything.
  Areas of the same kind join where they share a tile edge; areas of
  different kinds never join. Things on the map, buildings included,
  play no part. The areas are worked out when a map is prepared and
  kept with it. [code]
- If no chain of joined areas of the kind the unit stands in leads from
  its area to its (moved) destination's, there's no route and the move
  fails. The one exception: a destination joined to the unit by a
  straight line that crosses no tile closed to it is never failed this
  way.
  [code]
- So for a land regiment the move fails when its destination is land
  it can't reach overland: another island, or ground cut off by water
  or ground closed to everything. A destination in the water between
  doesn't fail: it's pulled back to the near shore first (above). [code]
- A chain longer than 254 areas is cut short at the area reached that
  is nearest the destination, and the move heads there instead. [code]
- The area step is switched by `TAIUseRegionPathPlanner` (SVars, stock
  `true`); things flagged `ethereal` skip it. Without it, nothing but
  the destination's spot (above) fails a move. [code, data]

### Closed in by things

- Things that block tiles (buildings, mostly) don't split areas, so a
  destination closed in by them, or a unit closed in, still has a
  route at the area level. The route over tiles then finds no way
  through, and heads for the tile it could reach that is nearest the
  destination instead (nearest as the crow flies, a diagonal tile step
  counting 1.414). [code]
- A route over tiles is at most 254 tile steps long, a diagonal step
  counting as one: once the search reaches tiles 255 steps from the
  start it stops and settles the same way, for the nearest tile reached
  so far. On open ground that cuts only a route longer than 254 tiles
  (508 units) along its longer axis; one that winds round obstacles
  reaches it sooner. The unit then plans the rest from where the cut
  route ends, and each such plan counts as a plan or try under
  [Arriving](#arriving). [code]
- Either way it ends up short, so it gives up there as a pulled-back
  destination does (above). Where the nearest tile is the one it stands
  on, it doesn't move at all before giving up. [code]
- So a move to somewhere it can't reach fails at once only for the
  cases under [Where the move is aimed](#where-the-move-is-aimed) and
  [Areas](#areas); anything else sets off, ending entrenchment, and is
  given up later. [code]

### When a move fails

- A regiment plans at its first look after the order, before any member
  moves, and again whenever it has no route left short of its
  destination (a long route's next stage, a target that moved). When
  that plan fails: [code]
  - nothing is handed to the leader, so no member sets off, and the
    regiment doesn't start out: its position, facing, formation and
    entrenchment stay as they were
    ([stances](stances.md#what-ends-it));
  - a move order, or an attack order whose way to the target fails, is
    dropped. The order it replaced was already dropped when it was
    given, so the regiment doesn't go back to it: it rests where it is,
    showing Resting, and its next queued order (Shift), if any, starts;
  - if the order carried a facing, the regiment turns to it at once as
    it comes to rest, its places with it, as it would on arriving;
  - its members are then sent to their places round where it stands,
    as at any rest; members already in place stay put;
  - a guard order keeps its post: the regiment guards from where it
    stands without trying again, and tries for the post again only when
    it returns to it after a fight ([combat](combat.md#guard-orders));
  - an unpack order shows its "Cannot unpack" line
    ([Unpacking after a move](#unpacking-after-a-move)). [code]
- Nothing else tells the player: no line, no sound, no buzzer. The
  order's reply (`WhenCommanded`, [sounds](sounds.md#unit-replies)) has
  already played when the order was given, since the route is only
  planned afterwards. [code]
- The cursor doesn't show it either. An armed command shows its
  command's `cursor_ids` cursor wherever it points. The one exception is
  Bombard: it shows its `cursor_invalid_ids` cursor (`bombard_invalid`,
  "Bombard Out of Range") over a spot none of the selection can
  bombard. No other command sets `cursor_invalid_ids`, and no cursor
  tests whether a spot can be reached. [code, data]
- A unit planning its own way fails the same way: its move is dropped
  and it stops where it is, keeping its facing (it doesn't turn to its
  regiment's facing, as it would on arriving). A lone unit then stands
  idle; a member's regiment, at rest, sends members not yet in their
  places once more, and leaves them where they are after that. A lone
  truck with an unpack order shows "Cannot unpack". [code]
- Several regiments ordered together each plan their own route, with or
  without Group Behavior: those that can't get there fail as above and
  the rest carry on. A failed regiment stays in its group, but it isn't
  moving, so the group leaves it out of its timing
  ([Groups](#groups)) and doesn't wait for it. [code]
- A group sends its order to all its regiments again, the failed one
  included, at two moments, and the failed one then plans afresh
  (failing again if nothing has changed): [code]
  - when a fight the group was in ends. While it's in a fight, once a
    second, it goes through its regiments in group order. Each one
    taking part that isn't engaged or under siege itself is given a
    guard order on a regiment of the group that is (the last such in
    group order, not the nearest), or a stop order if it won't take
    that guard ([combat](combat.md#guard-orders)). When a regiment's
    turn comes and no other regiment of the group is engaged or under
    siege, it and the rest are told to stop. The group then gives its
    own order to all of them again and goes back to timing
    ([Groups](#groups)). So a failed regiment taking part joins in its
    group's fights like the others, then gets the group's order again;
    [code]
  - during a move or guard order, when its regiments come to share a
    formation they didn't share before (say all of them are switched to
    the same new one), since that changes their places in the group.
  It sends nothing again at any other time. [code]

## Unpacking after a move

- An unpack order to a regiment sends the regiment to the ordered spot
  first, as any move, with no facing asked of it. [code]
- Once the regiment has arrived (within 0.5, above), its leader unpacks
  where it stands: the building goes at the truck's own position, not the
  ordered spot, facing as the order says (see
  [buildings](buildings.md#packing-and-unpacking)). [code]
- Nothing about the truck's facing, or its trailer, is tested. The only
  test is room for the building. [code]
- If the building has no room, the regiment asks once more on its next
  look; after two refusals the player is told "Cannot unpack" with the
  truck's name (string `tai_error_unpack`) and the order is dropped. [code]
- If the regiment gives up the move (above), the same message is shown,
  nothing unpacks, and the truck stays where it stopped. A truck that
  keeps missing a spot too close to turn onto ends this way. [code]
- A truck in no regiment unpacks where it stands at once, without moving;
  with no room, the same message. [code]

## Explore orders

Explore (command `explore`) sends a regiment off on its own to uncover
the map around a place the player picks, and keeps it at that until it's
given another order. [code]

### Who can explore

- Every unit that can move and belongs to a regiment can explore, and a
  regiment can when any of its units can. Nothing else counts: not
  `recon`, `trailblazing` or any other property, nor the kind of unit, so
  plain infantry, tank and other ground regiments get the button at slot
  7 as the recon regiments do. A thing outside a regiment, and a unit
  that can't move, never has it. [code, data]
- With the fog setting None ([fog](fog.md#the-match-setting)), nobody has
  it. [code] So a match with fog of war off shows no Explore even for a
  plain infantry regiment, whose slot 7 stays empty
  (`berlin-infantry-regiment-commands`, from a session with fog off),
  while a recon regiment in a fogged match has it
  (`berlin-recon-regiment-hq-unpacking`). A plain regiment in a fogged
  match hasn't been seen yet. [code, game]
- It's lost with the regiment's other orders while it routs, is
  Exhausted or is at 0 morale ([morale](morale.md#the-players-orders)).
  Fighting doesn't take it away. [code]
- With several selected, the button shows when any of them can explore
  (the usual rule, [commands](commands.md#command-grid)), and each
  selected regiment that can explore takes the order. [code]

### Giving it

- It takes a place, never a thing: a click on a thing is a click on the
  ground under it ([commands](commands.md#targeted-commands)). Any place
  on the map is taken, unexplored ground and water included. [code]
- A right click never explores: neither of its `can_hot_command_*`
  switches is set, so the only ways to give it are the button and its
  hotkey. [code, data]
- The clicked point becomes the regiment's explore point, which stays
  fixed for the whole order (pulled in to at least 1 from the map's low
  edges, and onto the map if it's ever off it). [code]
- Several regiments given it together without Group Behavior each get
  their own order, the explore point spread by their offsets from the
  selection's centre as for a Move
  ([Several regiments ordered together](#several-regiments-ordered-together));
  each then explores on its own. [code, data]
- With Group Behavior on ([Groups](#groups)), the group's leading
  regiment chooses where to go and every other regiment of the group
  heads for the same spot as soon as the leader picks it, with the
  group's arrival timing. When the leader has nowhere to go, they stand.
  [code]
- Given to a regiment already exploring, it starts over from the new
  point. [code]
- A Shift- or Ctrl-click queues it like any targeted order
  ([commands](commands.md#queued-orders)). But exploring never
  finishes by itself (below), so an order queued behind Explore is never
  reached; only a fresh order, or the regiment routing, ends it. [code]

### Where it goes

- It first heads for the clicked point itself, as a move would. If no
  member could stand within 2 of that point (water, a building), it
  skips that and chooses a destination at once. [code]
- Whenever it isn't moving, and whenever it comes within
  `ExploreInertiaCloseEnough` (3) of its destination, it chooses a new
  one. Until then it keeps going, whatever else comes into view. A move
  that fails ([When a move fails](#when-a-move-fails)) leaves it not
  moving, so it chooses again. [code, data]
- Choosing: it makes two sets of candidate places, and takes the best.
  The destination it's just had isn't one of them. [code, data]
  - `ExploreNumActorLocations` (20) around the regiment: each in a random
    direction, at a random distance from `ExploreActorRadiusMin` (20) to
    `ExploreActorRadiusMax` (30). One is only kept when a member could
    stand within 4 of it.
  - `ExploreNumWaypointLocations` (20) around the explore point: each in
    a random direction, the game looks along that direction from the
    explore point for ground its side has never seen, on the same
    stretch of land as the regiment, and takes a spot up to
    `ExploreWaypointTilesIntoFog` (8) tiles further into that unseen
    ground. A direction that only reaches the map's edge gives nothing.
    [code; guess: the exact search along the line]
  - Off the map, or where no member could stand within 2, a candidate is
    dropped.
- Best: the less the side knows of a place, the better, before anything
  else: never seen beats seen before but not in view now, which beats in
  view now. This is judged for the regiment's side alone. Among places
  known equally well, the higher value wins: [code, data]
  - `ExploreActorBaseValue` (40) for one around the regiment,
    `ExploreWaypointBaseValue` (0) for one around the explore point;
  - plus `ExploreDistanceMultiplier` (−1) × its distance from the explore
    point, so it keeps to the area the player picked;
  - plus `ExploreDotProductMultiplier` (20) × how nearly it lies the same
    way from the regiment as its last destination did (the cosine of the
    angle between the two, from −1 to 1), so it tends to keep heading the
    same way;
  - plus `ExploreFinishAreaBonus` (20) for a never-seen place in a part
    of the map whose neighbouring parts the side has wholly explored, to
    finish it off [code; guess: what counts as a part of the map];
  - plus a random amount from 0 to `ExploreFuzzyFactor` (20).
- So it works outwards from the clicked point into unexplored ground,
  nearest first; with nothing unseen left within reach it still wanders,
  among places around itself and the explore point it has seen. [code]
- If every candidate is dropped, it stands where it is, still exploring,
  and keeps trying. [code]
- On the way it moves as for a move order, in the player's chosen
  formation (`formation_required = DESIRED`), and at full speed. [code,
  data]
- Fog plays no part in its routes: the `PathingExplore*Fog` values in
  `SVars.tgi` are never read by AA. [code]

### On the way

- An engagement interrupts it as it does any order
  ([combat](combat.md#engagement)): the regiment fights by the usual
  rules for its formation's aggressiveness, then goes back to exploring,
  choosing a new destination. Meeting an enemy doesn't end it. [code]
- It counts as moving for stances
  ([stances](stances.md)) and for supply it allows resupply
  ([supply](supply.toml#what-supply-gives)); it doesn't regain morale
  (`mind_exploring` has `morale_replenish = false`). [code, data]
- It never ends by itself, at any distance or once nothing is left to
  uncover. [code]

### What the player sees

- The regiment's state line reads "Exploring" (`mind_exploring`) for the
  whole order, while it walks and while it stands with nowhere to go.
  [code, data]
- The order's marker is `waypoint_explore_actor`, shown at the clicked
  point while the regiment is selected, as for any order
  ([commands](commands.md#order-markers)). [code, data]
- Giving it plays the selection's `WhenCommanded` reply, as any order
  on a place does ([sounds](sounds.md#unit-replies)). Nothing is posted
  to the message log, and the regiment says nothing as it picks new
  destinations. [code]
- The armed cursor is `explore` (`cursor_ids`). [data]

## Trailers

- A truck with a `trailer_actor` tows it by the truck's `TrailerHitch`
  `node_offset` (x to the truck's right, y ahead, so stock trucks' 0, -2.2
  is behind the cab). The trailer has a `TrailerHitch` offset of its own
  (stock 0, 1.4: ahead of its centre). [code, data]
- Whenever the truck moves, the trailer turns to face from where it was
  toward the truck's hitch point, then is placed so that its own hitch
  point lies on the truck's. A trailer's own trailer follows it the same
  way. [code]
- The trailer is only ever dragged. It never limits or blocks the truck's
  turning or movement, and it is no obstacle to unpacking (see
  [objects](objects.md#room-to-stand)). [code]
- When the truck faces a new way at once (a snap, above), its hitch point
  jumps and the trailer is left pointing across or through the truck until
  driving straightens it: the tangled look. It changes nothing in play.
  [code, guess: that this is the look players see]

## Open questions

- Does `IN_PLACE` differ from `WHILE_MOVING` in anything shown (animation,
  tracks)? In movement they're the same. (analysis, 2026-11-09)
- Where does the route planner put its points, and are a leader's legs
  always straight from point to point? (analysis, 2026-11-09)
- Is it the `TRAILBLAZING` flag that lets a unit ignore the terrain speed
  limit? A per-unit switch does; its tie to the flag is taken from the
  flag's description in the data. (analysis, 2026-11-11)
- Can a ground unit's centre ever be on a mountain tile in play (a map
  placing it there, a member pushed off its route, a regiment leader's
  tile), so that the mountain limit is seen? Routes are taken to keep
  off tiles closed to the unit. (implementer, 2026-11-11)
- The close-destination and missed-arrival sequences are read from the
  rules; do they match what a truck does on screen, step for step?
  (analysis, 2026-11-09)
- Does the `explore` cursor change over places the order can't use, such
  as water? Every place on the map is accepted. (implementer, 2026-11-22)
