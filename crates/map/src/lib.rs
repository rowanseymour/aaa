//! A map as the editor and the game work with it, built from the stored form
//! that [`tgck::Map`] reads and the definitions in [`aaa_data`].

mod buildings;
#[cfg(test)]
mod fixtures;
mod ground;
pub mod pathing;
mod units;

pub use aaa_data::{
    Amounts, Damage, DeployPoints, Economy, EconomyRules, Fighter, Id, Requires, Shape, Targets,
    Weapon,
};
pub use buildings::Building;
pub use ground::{Ground, Tilt};
pub use pathing::{Domain, Obstacle, Pathing};
pub use tgck::map::Relation;
pub use units::{
    Division, Formation, Member, Packs, Player, Recruits, Regiment, Team, Trailer, UNPACKED_FACING,
    Unpacks, divisions, owner, relation,
};

use std::collections::{HashMap, HashSet};

use aaa_data::{GameData, Nation};
use tgck::map::{Sun, Terrain, Thing};

pub struct Map {
    /// The file name without its extension, e.g. `(2v2)Berlin`.
    pub title: String,
    pub ground: Ground,
    pub terrain: Terrain,
    pub things: Vec<Thing>,
    /// Which organization members' records depend on.
    pub world_version: u32,
    pub sun: Sun,
    pub players: Vec<Player>,
    pub teams: Vec<Team>,
    pub regiments: Vec<Regiment>,
    pub buildings: Vec<Building>,
    /// The obstacles that aren't buildings, such as trees and rocks.
    pub scenery: Vec<Obstacle>,
    pub pathing: Pathing,
    /// The ids of the things among [`Map::things`] with no room where the
    /// map has them, for the ground under them or something stored before
    /// them, which the game leaves out and the editor keeps.
    pub crowded: HashSet<u32>,
    /// What the map holds that the game can't place, as it loads.
    pub warnings: Vec<String>,
}

impl Map {
    /// Loads the map at an archive path, as [`GameData::find_map`] gives it.
    pub fn load(game: &mut GameData, name: &str) -> Result<Map, String> {
        Map::load_as(game, name, &HashMap::new())
    }

    /// [`Map::load`]s a map with sides playing as `nations`, by player id,
    /// each putting its own templates in place of the placeholders it starts
    /// with, such as its Corps HQ.
    pub fn load_as(
        game: &mut GameData,
        name: &str,
        nations: &HashMap<u32, Nation>,
    ) -> Result<Map, String> {
        let mut map = game.open_map(name)?;
        let err = |e: tgck::Error| format!("{name}: {e}");
        let hm = map.height_map().map_err(err)?;
        let mountain = map.mountain_height_map().map_err(err)?;
        let terrain = map.terrain().map_err(err)?;
        let variations = game.terrain_variations();
        let ground = Ground::new(&hm, &mountain, &terrain, &variations)
            .map_err(|e| format!("{name}: {e}"))?;
        let mut things = map.things().map_err(err)?;
        for thing in &mut things {
            let nation = owner(thing).and_then(|o| nations.get(&o));
            let template = Id::from(thing.template.as_str());
            if let Some(t) = nation.and_then(|n| n.placeholders.get(&template)) {
                thing.template = t.to_string();
            }
        }
        let world_version = map.world_version().map_err(err)?;
        let recruits = Recruits::new(game);
        let bodies = &recruits.bodies;
        let mut warnings = Vec::new();
        let defined = |t: &Thing| bodies.contains_key(&Id::from(t.template.as_str()));
        retain(&mut things, |t| {
            let keep = defined(t);
            if !keep {
                warnings.push(format!("{} {}: no such template", t.id, t.template));
            }
            keep
        });
        for thing in &mut things {
            snap(thing, bodies);
            for member in members_mut(thing) {
                snap(member, bodies);
            }
        }
        let mut pathing = Pathing::new(&terrain, &variations, []);
        let (off_map, crowded) = placed(&things, bodies, &pathing);
        retain(&mut things, |t| !off_map.contains(&t.id));
        warnings.extend(
            standing_ids(&things, &crowded).map(|t| format!("{} {}: no room", t.id, t.template)),
        );
        warnings.extend(off_map.iter().map(|id| format!("{id}: off the map")));
        let standing = without(&things, &crowded);
        let obstacles = obstacles(&standing, bodies);
        for (_, o) in &obstacles {
            pathing.block(o);
        }
        let fighters = &recruits.fighters;
        let (mut players, teams) = read_sides(game, &mut map).map_err(err)?;
        for player in &mut players {
            if let Some(n) = nations.get(&player.id) {
                player.nation = Some(n.id.to_string());
            }
        }
        let buildings = buildings::buildings(
            &standing,
            bodies,
            fighters,
            &game.city_zones(),
            &obstacles.iter().cloned().collect(),
            &game.deploy_points(),
            &recruits.ranges,
        );
        let ids: HashSet<u32> = buildings.iter().map(|b| b.id).collect();
        let scenery = obstacles
            .into_iter()
            .filter(|(id, _)| !ids.contains(id))
            .map(|(_, o)| o)
            .collect();
        let mut regiments = units::regiments(
            &standing,
            bodies,
            fighters,
            &recruits.layouts,
            &recruits.ranges,
        );
        for r in &mut regiments {
            recruits.give_unpacks(r);
        }
        Ok(Map {
            title: aaa_data::map_title(name).to_string(),
            ground,
            regiments,
            buildings,
            scenery,
            players,
            teams,
            pathing,
            terrain,
            things,
            world_version,
            sun: map.sun().map_err(err)?,
            crowded,
            warnings,
        })
    }
}

impl Map {
    /// Regroups the map's sides into `teams`, each a list of player ids, for
    /// a match: sides on a team are allies, at war with the other teams, and
    /// sides on none leave the match with everything they own. Independent
    /// players keep the stance they had toward each side.
    pub fn arrange(&mut self, teams: &[Vec<u32>]) {
        let kept = |id: u32| teams.iter().any(|t| t.contains(&id));
        let leaving: HashSet<u32> = sides(&self.players)
            .map(|p| p.id)
            .filter(|&id| !kept(id))
            .collect();
        let (players, teams) = arranged(&self.players, &self.teams, teams);
        self.players = players;
        self.teams = teams;
        let owned_by_leaver = |t: &Thing| owner(t).is_some_and(|o| leaving.contains(&o));
        self.things.retain(|t| !owned_by_leaver(t));
        self.regiments.retain(|r| !leaving.contains(&r.owner));
        take_buildings(&mut self.buildings, &mut self.pathing, |b| {
            leaving.contains(&b.owner)
        });
    }
}

/// Takes the buildings that `gone` picks off the map, giving back the
/// ground they kept movers off.
fn take_buildings(
    buildings: &mut Vec<Building>,
    pathing: &mut Pathing,
    gone: impl Fn(&Building) -> bool,
) {
    buildings.retain(|b| {
        if !gone(b) {
            return true;
        }
        if let Some(o) = &b.obstacle {
            pathing.unblock(o);
        }
        false
    });
}

/// Moves a thing whose template keeps it to the half-tile grid onto it.
fn snap(thing: &mut Thing, bodies: &HashMap<Id, aaa_data::Body>) {
    if bodies
        .get(&Id::from(thing.template.as_str()))
        .is_some_and(|b| b.tile_locked)
    {
        let step = pathing::CELL_SIZE / 2.0;
        for c in &mut thing.position[..2] {
            *c = (*c / step).round() * step;
        }
    }
}

/// The members of a thing that's an organization.
fn members_mut(thing: &mut Thing) -> &mut [Thing] {
    organization_members(thing).map_or(&mut [], |m| &mut m[..])
}

fn organization_members(thing: &mut Thing) -> Option<&mut Vec<Thing>> {
    use tgck::map::{Component, Detail, State};
    let Detail::Actor(actor) = &mut thing.detail else {
        return None;
    };
    let State::Decoded { components, .. } = &mut actor.state else {
        return None;
    };
    components.iter_mut().find_map(|c| match c {
        Component::Organization(o) => Some(&mut o.members),
        _ => None,
    })
}

/// Keeps the things, organizations' members among them, that `keep` picks.
fn retain(things: &mut Vec<Thing>, mut keep: impl FnMut(&Thing) -> bool) {
    things.retain(|t| keep(t));
    for t in things {
        if let Some(members) = organization_members(t) {
            members.retain(|m| keep(m));
        }
    }
}

/// The things among those standing whose ids are `ids`.
fn standing_ids<'a>(things: &'a [Thing], ids: &'a HashSet<u32>) -> impl Iterator<Item = &'a Thing> {
    buildings::standing(things)
        .map(|(t, _)| t)
        .filter(|t| ids.contains(&t.id))
}

/// The things, organizations' members among them, without those `gone`.
fn without(things: &[Thing], gone: &HashSet<u32>) -> Vec<Thing> {
    let mut out = things.to_vec();
    retain(&mut out, |t| !gone.contains(&t.id));
    out
}

/// Places the things in the order stored, each with its plain blocking
/// shape, and gives the ids of those off the map, and of those without
/// room otherwise: on ground they can't stand on, or overlapping a static
/// blocking thing placed before them. Things that don't block, and
/// ethereal ones, need only be on the map.
fn placed(
    things: &[Thing],
    bodies: &HashMap<Id, aaa_data::Body>,
    pathing: &Pathing,
) -> (HashSet<u32>, HashSet<u32>) {
    let (mut off_map, mut crowded) = (HashSet::new(), HashSet::new());
    let mut fixed: Vec<Obstacle> = Vec::new();
    // The static blocking things by the tiles their boxes span, to test
    // only those nearby.
    let mut near: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (t, _) in buildings::standing(things) {
        let Some(body) = bodies.get(&Id::from(t.template.as_str())) else {
            continue;
        };
        let [x, y, _] = t.position;
        let o = Obstacle {
            position: [x, y],
            facing: units::facing(t),
            shape: body.shape,
            soft: body.soft,
        };
        if !pathing.on_map(&o) {
            off_map.insert(t.id);
            continue;
        }
        if body.blocking.is_none() || body.ethereal {
            continue;
        }
        let domain = match body.water_based {
            true => Domain::Naval,
            false => Domain::Land,
        };
        let tiles: Vec<(i32, i32)> = pathing::spanned(o.bounds(o.shape)).collect();
        let mut others: Vec<usize> = (tiles.iter())
            .flat_map(|tile| near.get(tile).into_iter().flatten().copied())
            .collect();
        others.sort_unstable();
        others.dedup();
        if !pathing.ground_for(&o, domain) || others.iter().any(|&i| fixed[i].overlaps(&o)) {
            crowded.insert(t.id);
            continue;
        }
        if body.fixed {
            for tile in tiles {
                near.entry(tile).or_default().push(fixed.len());
            }
            fixed.push(o);
        }
    }
    (off_map, crowded)
}

/// What blocks the ground, by id, of the objects standing on the map:
/// those that never move.
fn obstacles(things: &[Thing], bodies: &HashMap<Id, aaa_data::Body>) -> Vec<(u32, Obstacle)> {
    buildings::standing(things)
        .filter_map(|(t, _)| {
            let body = bodies.get(&Id::from(t.template.as_str()))?;
            let shape = body.blocking.filter(|_| body.fixed)?;
            let [x, y, _] = t.position;
            let obstacle = Obstacle {
                position: [x, y],
                facing: units::facing(t),
                shape,
                soft: body.soft,
            };
            Some((t.id, obstacle))
        })
        .collect()
}

/// The players and teams of [`Map::arrange`].
fn arranged(
    players: &[Player],
    old_teams: &[Team],
    teams: &[Vec<u32>],
) -> (Vec<Player>, Vec<Team>) {
    let team_of = |id: u32| teams.iter().position(|t| t.contains(&id));
    let before = |from: u32, to: u32| relation(players, old_teams, from, to);
    let kept: Vec<&Player> = players
        .iter()
        .filter(|p| p.independent || team_of(p.id).is_some())
        .collect();
    let between = |a: Option<usize>, b: Option<usize>| match a == b {
        true => Relation::Ally,
        false => Relation::Enemy,
    };
    // How a side or a team stands toward a player, the independent keeping
    // the stance taken toward the first of a team's sides.
    let toward = |from: u32, side: Option<usize>, to: &Player| match (side, team_of(to.id)) {
        (Some(_), Some(_)) if from == to.id => Relation::Itself,
        (Some(a), Some(b)) => between(Some(a), Some(b)),
        _ => before(from, to.id),
    };
    let new_players = kept
        .iter()
        .map(|p| {
            let team = team_of(p.id);
            let team_relations = teams
                .iter()
                .enumerate()
                .map(|(t, members)| match team {
                    Some(own) => between(Some(own), Some(t)),
                    None => members
                        .first()
                        .map_or(Relation::Neutral, |&m| before(p.id, m)),
                })
                .collect();
            Player {
                team,
                relations: kept.iter().map(|q| toward(p.id, team, q)).collect(),
                team_relations,
                ..(*p).clone()
            }
        })
        .collect();
    let new_teams = teams
        .iter()
        .enumerate()
        .map(|(t, members)| Team {
            id: t as u32,
            name: format!("team_{t}"),
            relations: kept
                .iter()
                .map(|q| match members.first() {
                    Some(&m) => toward(m, Some(t), q),
                    None => Relation::Neutral,
                })
                .map(|r| match r {
                    Relation::Itself => Relation::Ally,
                    r => r,
                })
                .collect(),
            team_relations: (0..teams.len())
                .map(|u| between(Some(t), Some(u)))
                .collect(),
        })
        .collect();
    (new_players, new_teams)
}

/// What a map's lobby shows of it, read without loading the rest.
pub struct Brief {
    /// Its name as the game shows it, e.g. `Fall of Berlin`; the file's
    /// where the map has none.
    pub name: String,
    pub description: String,
    /// Width and height in world units, where the map records them.
    pub size: Option<[f32; 2]>,
    pub players: Vec<Player>,
    pub teams: Vec<Team>,
}

impl Brief {
    /// Reads the brief of the map at an archive path, as
    /// [`GameData::find_map`] gives it.
    pub fn load(game: &mut GameData, name: &str) -> Result<Brief, String> {
        let mut map = game.open_map(name)?;
        let err = |e: tgck::Error| format!("{name}: {e}");
        let header = map.header().map_err(err)?;
        let (players, teams) = read_sides(game, &mut map).map_err(err)?;
        Ok(Brief {
            name: map
                .string(&header.name)
                .map_err(err)?
                .unwrap_or_else(|| aaa_data::map_title(name).to_string()),
            description: map
                .string(&header.description)
                .map_err(err)?
                .unwrap_or_default(),
            size: header.size,
            players,
            teams,
        })
    }

    /// The players that play to win, as the lobby numbers its sides from 1;
    /// the rest are independent.
    pub fn sides(&self) -> impl Iterator<Item = &Player> {
        sides(&self.players)
    }
}

/// The players that play to win, in the order the lobby numbers its sides.
pub fn sides(players: &[Player]) -> impl Iterator<Item = &Player> {
    players.iter().filter(|p| !p.independent)
}

fn read_sides<R: std::io::Read + std::io::Seek>(
    game: &mut GameData,
    map: &mut tgck::Map<R>,
) -> tgck::Result<(Vec<Player>, Vec<Team>)> {
    let rules = game.economy_rules();
    let default = map
        .rules()?
        .rules
        .iter()
        .find(|r| r.name == "ResourceStockpileDefault")
        .and_then(|r| match r.value {
            tgck::map::RuleValue::Float(v) => Some(v),
            tgck::map::RuleValue::Int(v) => Some(v as f32),
            _ => None,
        })
        .unwrap_or(rules.stockpile_default)
        .clamp(0.0, rules.stockpile_max);
    Ok(units::sides(
        &map.kingdoms()?,
        &map.teams()?,
        &game.kingdom_colors(),
        default,
    ))
}

/// The point `right` and `ahead` of `origin` when facing `facing`, a unit
/// vector.
pub fn from_local(origin: [f32; 2], [fx, fy]: [f32; 2], [right, ahead]: [f32; 2]) -> [f32; 2] {
    let [x, y] = origin;
    [x + right * fy + ahead * fx, y - right * fx + ahead * fy]
}

/// How far `point` is to the right of `origin` and ahead of it when facing
/// `facing`, a unit vector; the inverse of [`from_local`].
pub fn to_local(origin: [f32; 2], [fx, fy]: [f32; 2], point: [f32; 2]) -> [f32; 2] {
    let (dx, dy) = (point[0] - origin[0], point[1] - origin[1]);
    [dx * fy - dy * fx, dx * fx + dy * fy]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{organization, placed};

    /// Sides 1 and 2 on the map's first team, 3 and 4 on its second, and an
    /// independent 9 at war with 1 and neutral to the rest.
    fn sides() -> (Vec<Player>, Vec<Team>) {
        use Relation::*;
        let side = |id, team| Player {
            id,
            name: format!("kingdom_{id}"),
            color: "blue".into(),
            rgb: [1.0; 3],
            team: Some(team),
            independent: false,
            playable: true,
            nation: None,
            relations: vec![Enemy; 5],
            team_relations: vec![Enemy; 2],
            stockpile: 0.0,
        };
        let mut players = vec![side(1, 0), side(2, 0), side(3, 1), side(4, 1)];
        players.push(Player {
            id: 9,
            team: None,
            independent: true,
            relations: vec![Enemy, Neutral, Neutral, Neutral, Itself],
            team_relations: vec![Enemy, Neutral],
            ..players[0].clone()
        });
        let team = |id, own: usize| Team {
            id,
            name: format!("team_{id}"),
            relations: vec![Neutral; 5],
            team_relations: (0..2)
                .map(|t| if t == own { Ally } else { Enemy })
                .collect(),
        };
        let mut teams = vec![team(0, 0), team(1, 1)];
        teams[0].relations[4] = Enemy;
        (players, teams)
    }

    fn moves(trailer: Option<&str>) -> aaa_data::Body {
        aaa_data::Body {
            speed: Some(3.0),
            trailer: trailer.map(Id::from),
            ..aaa_data::Body::default()
        }
    }

    #[test]
    fn things_without_room_are_crowded_out_as_the_map_loads() {
        let circle = |radius| Shape::Circle { radius };
        let body = |blocking: bool, fixed: bool| aaa_data::Body {
            blocking: blocking.then_some(circle(1.0)),
            shape: circle(1.0),
            soft: circle(1.0),
            fixed,
            ..aaa_data::Body::default()
        };
        let bodies = HashMap::from([
            (Id::from("hut"), body(true, true)),
            (Id::from("tank"), body(true, false)),
            (Id::from("ripple"), body(false, true)),
        ]);
        let at = |id, template, [x, y]: [f32; 2]| {
            let mut t = placed(id, template, 0, None, None);
            t.position = [x, y, 0.0];
            t
        };
        let things = [
            // A unit stored before the hut it stands in is kept, and so is
            // the hut, as units are in nothing's way.
            at(1, "tank", [5.0, 5.0]),
            at(2, "hut", [5.5, 5.0]),
            at(3, "tank", [6.0, 5.0]),
            at(4, "hut", [6.5, 5.5]),
            // Blocking things need land, the others only the map.
            at(5, "hut", [13.0, 3.0]),
            at(6, "ripple", [13.0, 3.0]),
            at(7, "ripple", [0.5, 3.0]),
            // Members are placed like any other thing.
            placed(
                8,
                "squad",
                0,
                None,
                Some(organization("line", vec![at(9, "hut", [5.0, 6.0])])),
            ),
        ];
        let mut pathing = Pathing::uniform(8, Domain::Land);
        pathing.tiles[6 + 8] = 2;
        let (off_map, crowded) = super::placed(&things, &bodies, &pathing);
        assert_eq!(off_map, HashSet::from([7]));
        assert_eq!(crowded, HashSet::from([3, 4, 5, 9]));
    }

    #[test]
    fn tile_locked_things_snap_to_the_half_tile_grid() {
        let locked = aaa_data::Body {
            tile_locked: true,
            ..aaa_data::Body::default()
        };
        let bodies = HashMap::from([(Id::from("wall"), locked)]);
        let at = |template, [x, y]: [f32; 2]| {
            let mut t = placed(1, template, 0, None, None);
            t.position = [x, y, 3.0];
            snap(&mut t, &bodies);
            t.position
        };
        assert_eq!(at("wall", [10.4, 7.6]), [10.0, 8.0, 3.0]);
        assert_eq!(at("wall", [10.6, -0.4]), [11.0, 0.0, 3.0]);
        assert_eq!(at("tree", [10.4, 7.6]), [10.4, 7.6, 3.0]);
    }

    #[test]
    fn owned_units_of_no_organization_are_regiments_of_their_own() {
        let things = [
            placed(1, "truck", 2, Some(2), None),
            placed(2, "gun", 2, None, None),
            placed(3, "cow", 0, None, None),
            placed(4, "rock", 2, None, None),
        ];
        let bodies = HashMap::from([
            (Id::from("truck"), moves(Some("gun"))),
            (Id::from("gun"), moves(None)),
            (Id::from("cow"), moves(None)),
        ]);
        let none = HashMap::new();
        let regiments = units::regiments(&things, &bodies, &HashMap::new(), &none, &HashMap::new());
        // The gun goes with the truck, and the cow is nobody's.
        let [truck] = &regiments[..] else {
            panic!("{regiments:?}");
        };
        assert!(truck.lone());
        assert_eq!((truck.id, truck.owner, truck.primary), (1, 2, Some(0)));
        assert_eq!(truck.members[0].health, 1.0);
        let trailer = truck.members[0].trailer.as_ref().unwrap();
        assert_eq!(trailer.placed.map(|p| p.0), Some(2));
    }

    #[test]
    fn members_keep_the_health_the_map_has_them_at() {
        let things = [crate::fixtures::hurt(placed(1, "gun", 2, None, None), 0.4)];
        let bodies = HashMap::from([(Id::from("gun"), moves(None))]);
        let none = HashMap::new();
        let regiments = units::regiments(&things, &bodies, &HashMap::new(), &none, &HashMap::new());
        assert_eq!(regiments[0].members[0].health, 0.4);
    }

    #[test]
    fn buildings_standing_in_organizations_are_buildings() {
        // A Division HQ standing unpacked: its building and a bunker that
        // can't be hurt, both owned through the HQ.
        let hq = placed(
            10,
            "hq_unpacked",
            2,
            None,
            Some(organization(
                "regiment_hq",
                vec![
                    placed(11, "hq", 0, None, None),
                    placed(12, "bunker", 0, None, None),
                ],
            )),
        );
        let things = [hq];
        let bodies = HashMap::from([(
            Id::from("hq"),
            aaa_data::Body {
                blocking: Some(Shape::Rect {
                    width: 8.0,
                    height: 8.0,
                }),
                fixed: true,
                ..aaa_data::Body::default()
            },
        )]);
        let obstacles = obstacles(&things, &bodies);
        assert_eq!(
            obstacles.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [11]
        );
        let fighters = HashMap::from([(
            Id::from("hq"),
            Fighter {
                health: 2000.0,
                ..Fighter::default()
            },
        )]);
        let buildings = buildings::buildings(
            &things,
            &bodies,
            &fighters,
            &HashMap::new(),
            &obstacles.into_iter().collect(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let [b] = &buildings[..] else {
            panic!("{buildings:?}");
        };
        assert_eq!((b.id, b.owner, b.position), (11, 2, [11.0, 0.0]));
        // It packs into its place in the organization, which keeps its id.
        let o = b.organization.as_ref().unwrap();
        assert_eq!((o.id, o.template.as_str(), o.owner), (10, "hq_unpacked", 2));
        assert!(o.members.is_empty());
        assert!(b.obstacle.is_some());
        assert!(
            units::regiments(
                &things,
                &bodies,
                &fighters,
                &HashMap::new(),
                &HashMap::new()
            )
            .is_empty()
        );
    }

    #[test]
    fn buildings_taken_off_the_map_stop_blocking() {
        let wall = |id, owner, x| Building {
            id,
            template: "wall".into(),
            owner,
            position: [x, 8.0],
            facing: [0.0, -1.0],
            fighter: Fighter::default(),
            health: 1.0,
            zone: None,
            members: Vec::new(),
            obstacle: Some(Obstacle {
                position: [x, 8.0],
                facing: [0.0, -1.0],
                shape: Shape::Circle { radius: 1.0 },
                soft: Shape::Circle { radius: 1.0 },
            }),
            deploy: None,
            organization: None,
        };
        let mut buildings = vec![wall(1, 2, 4.0), wall(2, 3, 12.0)];
        let mut pathing = Pathing::uniform(8, Domain::Land);
        for b in &buildings {
            pathing.block(b.obstacle.as_ref().unwrap());
        }
        take_buildings(&mut buildings, &mut pathing, |b| b.owner == 3);
        assert_eq!(buildings.iter().map(|b| b.id).collect::<Vec<_>>(), [1]);
        assert!(!pathing.passable(2, 4, Domain::Land));
        assert!(pathing.passable(6, 4, Domain::Land));
    }

    #[test]
    fn arranging_makes_teammates_allies_and_the_rest_enemies() {
        let (players, teams) = sides();
        // A free-for-all of 1 and 3, who were on different teams, and 2,
        // who was 1's ally.
        let (players, teams) = arranged(&players, &teams, &[vec![1], vec![2], vec![3, 4]]);
        let ids: Vec<u32> = players.iter().map(|p| p.id).collect();
        assert_eq!(ids, [1, 2, 3, 4, 9]);
        let r = |a, b| relation(&players, &teams, a, b);
        assert_eq!(r(1, 2), Relation::Enemy);
        assert_eq!(r(3, 4), Relation::Ally);
        assert_eq!(r(1, 3), Relation::Enemy);
        assert_eq!(r(1, 1), Relation::Itself);
        // The independent keeps its stances, either way round.
        assert_eq!(r(9, 1), Relation::Enemy);
        assert_eq!(r(1, 9), Relation::Enemy);
        assert_eq!(r(9, 3), Relation::Neutral);
        assert_eq!(r(3, 9), Relation::Neutral);
    }

    #[test]
    fn arranging_leaves_out_sides_on_no_team() {
        let (players, teams) = sides();
        let (players, teams) = arranged(&players, &teams, &[vec![1], vec![4]]);
        let ids: Vec<u32> = players.iter().map(|p| p.id).collect();
        assert_eq!(ids, [1, 4, 9]);
        assert_eq!(teams.len(), 2);
        assert_eq!(relation(&players, &teams, 1, 4), Relation::Enemy);
        assert_eq!(relation(&players, &teams, 1, 2), Relation::Neutral);
    }
}
