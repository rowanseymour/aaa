//! A map as the editor and the game work with it, built from the stored form
//! that [`tgck::Map`] reads and the definitions in [`aaa_data`].

mod buildings;
#[cfg(test)]
mod fixtures;
mod ground;
pub mod pathing;
mod units;

pub use aaa_data::{Damage, DeployPoints, Fighter, Id, Requires, Shape, Targets, Weapon};
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
        for thing in &mut things {
            snap(thing, bodies);
            for member in members_mut(thing) {
                snap(member, bodies);
            }
        }
        let obstacles = obstacles(&things, bodies);
        let pathing = Pathing::new(
            &terrain,
            &variations,
            obstacles.iter().map(|(_, o)| o.clone()),
        );
        let fighters = &recruits.fighters;
        let (mut players, teams) = read_sides(game, &mut map).map_err(err)?;
        for player in &mut players {
            if let Some(n) = nations.get(&player.id) {
                player.nation = Some(n.id.to_string());
            }
        }
        let buildings = buildings::buildings(
            &things,
            bodies,
            fighters,
            &game.city_zones(),
            &obstacles.iter().cloned().collect(),
            &game.deploy_points(),
        );
        let ids: HashSet<u32> = buildings.iter().map(|b| b.id).collect();
        let scenery = obstacles
            .into_iter()
            .filter(|(id, _)| !ids.contains(id))
            .map(|(_, o)| o)
            .collect();
        let mut regiments = units::regiments(
            &things,
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
    use tgck::map::{Component, Detail, State};
    if let Detail::Actor(actor) = &mut thing.detail
        && let State::Decoded { components, .. } = &mut actor.state
        && let Some(o) = components.iter_mut().find_map(|c| match c {
            Component::Organization(o) => Some(o),
            _ => None,
        })
    {
        return &mut o.members;
    }
    &mut []
}

/// What blocks the ground, by id, of the objects standing on the map.
/// Movers don't block the ground they start on.
fn obstacles(things: &[Thing], bodies: &HashMap<Id, aaa_data::Body>) -> Vec<(u32, Obstacle)> {
    buildings::standing(things)
        .filter_map(|(t, _)| {
            let body = bodies.get(&Id::from(t.template.as_str()))?;
            let shape = body.blocking.filter(|_| body.speed.is_none())?;
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
    Ok(units::sides(
        &map.kingdoms()?,
        &map.teams()?,
        &game.kingdom_colors(),
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
        );
        let [b] = &buildings[..] else {
            panic!("{buildings:?}");
        };
        assert_eq!((b.id, b.owner, b.position), (11, 2, [11.0, 0.0]));
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
