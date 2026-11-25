# Fog of war

What each side can see of the map: the three fog states, what reveals
ground and for whom, what shows of things under each state, the match
setting and cheat, and what fog does and doesn't change in play.

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Fog states

- Every tile (2 × 2 units) is in one of three states for each side:
  [code]
  - **visible** ("white fog" in the data): something of the side, or of a
    side it shares sight with (Shared sight), sees the tile now;
  - **explored** ("grey fog"): seen at some time, not now;
  - **unexplored** ("black fog"): never seen.
- A side keeps two facts per tile, "seen now" and "ever seen"; seeing a
  tile sets both, losing sight clears only "seen now". Nothing makes a
  tile unexplored again except the trigger effect below. [code]
- A tile goes back from visible to explored the moment the last thing
  seeing it stops seeing it (moves away, dies, loses its sight): there's
  no delay or fade-out in the rules. [code]
- The trigger effect "Game - Fog restored in Area" (`fog_restored_in_area`)
  makes every tile in its area unexplored again for every side that
  doesn't see it at that moment. [code, data]
- The neutral side (and any side flagged to see everything) counts every
  tile and thing as visible. [code]

## What reveals the map

- Only things with a `[DetectionComponent]` see. It has two fields,
  `detection_range` and `remote_detection_range`, both in world units
  (2 to a tile). A thing whose template has no detection component sees
  nothing. [code, data]
- A thing sees for its owner. A thing with no owner (one whose template
  is `ownable = false`, such as `objective_compass`) shows its disc to
  no one, whatever its ranges. [code, data]
- A thing sees a disc around itself of radius `detection_range`: every
  tile whose centre lies within that radius of the centre of the tile
  under the thing's centre (whole tiles; the disc is centred on that
  tile, not on the exact position). A radius that is a whole number of
  tiles, or less than a quarter tile over one, is first pushed out to a
  quarter tile past it (18 → 18.5, 26 → 26.5; 65 stays 65). [code]
- The disc is a plain circle: hills, forests, buildings and water don't
  block it, and it doesn't depend on height or on the viewer flying.
  [code]
- The disc follows its owner as it moves, re-laid around the tile now
  under it, and grows or shrinks when its range changes. [code]
- `detection_range` can be changed in play by modifiers on the
  `DETECTION_RANGE` attribute (the `recon` property,
  `tech_improved_recon`, `tech_improved_logistics`). [code, data]
- Load-time corrections, per template: [code]
  - a `detection_range` smaller than the thing's `control_range`,
    `supply_range`, `guard_range` or `guard_operational_range` is raised
    to the larger of its `control_range` and `supply_range` (with an
    error in the log);
  - a thing whose `remote_detection_range` is 0 or less, and that has no
    defender `control_range` above 0, has no detection at all, whatever
    its `detection_range` (the minefield and beach mine poles: 0 and 0);
  - for a regiment (anything with an `[OrganizationComponent]`) the
    `remote_detection_range` given is replaced by its `control_range`.
- Who doesn't see even with a detection component: [code]
  - a unit that is with its regiment (inside the regiment's control zone,
    or its leader): the regiment's own disc stands for it. A unit that has
    strayed outside its regiment's control zone sees with its own range;
  - a thing carried inside another (a transport). [guess]
- Stock ranges (`detection_range` / `remote_detection_range`, before the
  corrections above): [data]

  | Thing | Sight | Remote |
  | --- | --- | --- |
  | Infantry and Engineer regiments | 26 | 12 |
  | Paratrooper regiments | 30 | 12 |
  | Mobile regiments | 22 | 12 |
  | Artillery regiments | 20 | 12 |
  | Armor regiments | 18 | 12 |
  | Recon and commando regiments | 27 | 12 |
  | HQ regiments (`AAOrganizationHQ`) | 65 | 18 |
  | Units on their own (`AAUnit`) | 18 | 10 |
  | Division and Corps HQ buildings (`HQDetectionComponent`) | 65 | 20 |
  | Supply, ammo and oil depots (`AABuildingPackableDepot`) | 55 | 20 |
  | Other buildings (`AABuilding`), resources | 16 | 12 |
  | Bunkers | 20 | 12 |
  | AA bunkers | 25 | template |
  | Artillery bunkers | 30 | template |
  | Airfields, carriers | 48 | template |
  | Battleships | 35 | 35 |
  | City centres | 55 | 30 |
  | Other city buildings | 16 | 12 |
  | Campaign airfields | 60 | 20 |
  | Searchlight (campaign) | 500 | template |
  | Projectiles | 2 | 2 |

  "template" means the value comes from the template the thing builds on.
  Regiments' remote ranges are replaced by their `control_range` (above).

### Remote detection

- `remote_detection_range` is a second disc around a thing, shown not to
  its own side but to enemies who are fighting it. Tiles in it become
  visible (not explored, so they go straight back to unexplored-looking
  if never seen otherwise; see Open questions) to: [code]
  - every side with a regiment engaging it;
  - every side that has bombarded it, for `BombardTargetTimeout` (1 s)
    plus the bombarding weapon's recharge time after the last hit;
  - the side of whatever it is itself attacking (AA sets
    `EngagementShowReverseRZOD` true), so an attacker is revealed to its
    victim;
  - the sides that a defending building's weapons are aimed at;
  - every enemy side holding the `REVEAL_ENEMY_RZOD` flag;
  - for a regiment, the sides each of its attached units is revealed to.
- Its own side never gets it. The disc follows the thing and the list of
  sides is re-checked every second. [code]
- So a regiment shelled or attacked from out of sight sees its attacker
  and a little ground round it, and keeps seeing it while the fight goes
  on. [code]

## Shared sight

- A side sees whatever is visible to any side it counts as an ally (its
  relation towards that side is ALLY), and itself; explored ground is
  shared the same way. The relation is read from the viewing side, so
  one-sided alliances share one way only. It follows changes of relation.
  [code]
- Teammates in a team game are allies from the start
  ([players](players.md)), so they share sight from the first moment of
  the match. [code, data]
- A side's sight comes only from its things: when they die, what they saw
  turns explored. An eliminated ally adds nothing more, but ground it
  explored stays explored for its allies. [code]

## What shows under each state

- Terrain, water and the sky are always drawn; fog only shades the
  ground. Unexplored ground is drawn darker than explored ground, and
  explored ground darker than visible ground. [code, game] How dark, and
  how soft the edge between states is, are open.
- Units and buildings of other sides show only while visible, apart from
  the remembered pictures below. [code]
- Things drawn on explored ground (scenery, remembered pictures) are
  drawn dimmed; nothing is drawn on unexplored ground. [code] How dim is
  open.
- A regiment counts as visible when any of its units is; a unit with its
  regiment counts as visible when its regiment does, so the whole
  regiment shows once one man of it is seen. [code]
- Buildings and other placed things are visible when any tile under
  their footprint is. [code]
- Remembered things: when a thing that a side has explored drops out of
  its sight, the side keeps a picture of it as last seen, owner included,
  and shows that under the explored fog.
  When the side sees the spot again the picture is dropped and the real
  state shows: a building destroyed or captured meanwhile is then gone
  or shows its new owner. [code]
  - Pictures are never kept for `invisible` things or dead ones. [code]
    They're kept for buildings and cities; whether moving units leave
    one is open. [guess]
  - Indestructible scenery (trees, rocks: `invulnerable`, not
    `ownable`) shows on any explored ground as it really is. [code]
- Things flagged `revealed` (cities and villages, roads, bridges, Monte
  Cassino) count as explored by every side from the start when the
  Explored setting is in force; the data's comment calls them "seen with
  gray fog world rule". [code, data]

## The match setting

- `FogOfWar` (`SVars.tgi`, overridden by the world rule of the same name
  chosen in the lobby or the random and scenario map dialogs) has three
  values, in the order the rule lists them: [code, data]
  - 0, **None**: every tile is visible to every side for the whole match,
    and every fog test passes;
  - 1, **Explored**: as Full, but at the start every tile is explored by
    every side, along with `revealed` things and indestructible scenery,
    so the whole map shows under grey fog;
  - 2, **Full**: everything starts unexplored.
- AA's world rule defaults to Explored; `SVars.tgi` alone would give
  Full. [data]
- At the start of a Full match nothing is pre-revealed: each side sees
  only what its starting things see. [code]

## The fog cheat

- The developer hotkey bound to `Fog` (Ctrl+W in the dev key map)
  toggles fog off and on for the screen, if cheats are allowed
  (`AllowCheats`) and the game is single player. [code, data]
- It changes only what's shown: the map draws unfogged, the minimap
  unfogged, all things show and every temporary effect plays. The fog
  itself carries on underneath, and nothing in play (AI, orders,
  targeting) changes. [code]

## Minimap

- The minimap shades its ground by the local side's fog and marks only
  the things that side sees or remembers: the shades, when they're left
  off and which things show are in [minimap](minimap.md). [code]

## What fog changes in play

- Fog doesn't stop combat: regiments engage and units target enemies
  that come within their ranges whether or not their side sees them, and
  an attack order is not dropped when its target goes into fog. [code]
- Bombarding is the exception: a bombard target that moves and can no
  longer be seen ends the order with "lost bombard target in fog"
  (`bombard_target_lost_in_fog`); one that doesn't move is shelled where
  it was when the order was given, until it's seen again
  ([combat](combat.md#bombard-orders)). [code]
- Temporary visual effects aren't created where the local side can't
  see ([effects](effects.md)). [code]
- World sounds flagged `FOGGED` aren't heard by a player for whom their
  source is fogged. [data] Whether "fogged" means unexplored or anything
  not visible is open.
- `SVars.tgi` lists fog costs for exploring routes
  (`PathingExploreWhiteFog` 9, `...GreyFog` 3, `...BlackFog` 1), but AA
  never reads them: routes don't depend on fog. [code, data]

## Open questions

- How dark is explored ground, and unexplored ground, on the terrain,
  and how soft are the edges between states? Check in the game with fog
  Full. (analysis, 2026-11-10)
- Can the player order a move or attack onto unexplored ground, and does
  the cursor or hover treat fogged ground or fogged enemies differently?
  (implementer, 2026-11-10)
- Do `FOGGED` world sounds go quiet under explored ground, or only under
  unexplored? (implementer, 2026-11-10)
- Remote detection marks tiles visible without exploring them; does the
  revealed patch show as visible ground in the middle of black fog, and
  black again once it's gone? (analysis, 2026-11-10)
- Do units that move out of sight leave a remembered picture, or only
  buildings and other things that stay put? (analysis, 2026-11-10)
- Some things lay their sight disc with their remote range in place of
  their detection range in a special state not identified yet.
  (analysis, 2026-11-10)
- When a remembered building is destroyed, does its picture stay until
  the spot is seen again, or vanish once the remains are cleared away?
  (analysis, 2026-11-10)
