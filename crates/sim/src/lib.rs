//! The rules of the game, played out with no rendering so that it can run
//! headless.
//!
//! A *match* is one play of a map: a *match setup* (the map, its players and
//! their nations, teams and rules) once it's running. Each *player* is a side
//! in it, controlled by a person or an AI.

pub mod path;

use std::collections::HashMap;

use aaa_map::pathing::tile_at;
use aaa_map::{Domain, Map, Pathing};

pub use aaa_map::{Player, Relation, Team};

/// Seconds of play one [`World::tick`] advances.
pub const TICK: f32 = 0.1;

/// A match in progress.
pub struct World {
    pub sides: Sides,
    /// The owning [`Player::id`] of each owned object that isn't a
    /// regiment, such as a building, by object id.
    pub owners: HashMap<u32, u32>,
    pub pathing: Pathing,
    pub regiments: Vec<Regiment>,
    pub units: Vec<Unit>,
    /// Ticks played.
    pub ticks: u64,
}

pub struct Regiment {
    /// Its object's id in the map.
    pub id: u32,
    pub template: String,
    /// The owning [`Player::id`], the only one it takes orders from.
    pub owner: u32,
    /// Indices into [`World::units`].
    pub units: Vec<usize>,
    /// Its slowest member's, which they all keep to so as to stay together.
    pub speed: f32,
    /// Where it stands, or is headed: its members take their places in the
    /// formation around it.
    pub position: [f32; 2],
    /// The way it faces, or will once there.
    pub facing: [f32; 2],
    pub formations: Vec<aaa_map::Formation>,
    /// Index into `formations` of the one it's in.
    pub formation: usize,
}

pub struct Unit {
    /// Its object's id in the map.
    pub id: u32,
    pub template: String,
    /// Index into [`World::regiments`].
    pub regiment: usize,
    pub domain: Domain,
    /// In world units.
    pub position: [f32; 2],
    /// Where it was a tick ago, to draw it in between.
    pub previous: [f32; 2],
    /// The way it faces, a unit vector.
    pub facing: [f32; 2],
    /// The way to face once it arrives.
    end_facing: [f32; 2],
    /// Waypoints still ahead, the next last.
    path: Vec<[f32; 2]>,
}

impl Unit {
    pub fn moving(&self) -> bool {
        !self.path.is_empty()
    }
}

impl World {
    pub fn new(map: &Map) -> World {
        let sides = Sides {
            players: map.players.clone(),
            teams: map.teams.clone(),
        };
        let mut world = World::from_regiments(sides, map.pathing.clone(), &map.regiments);
        world.owners = map
            .things
            .iter()
            .filter_map(|t| Some((t.id, aaa_map::owner(t)?)))
            .collect();
        world
    }

    pub fn from_regiments(
        sides: Sides,
        pathing: Pathing,
        regiments: &[aaa_map::Regiment],
    ) -> World {
        let mut world = World {
            sides,
            owners: HashMap::new(),
            pathing,
            regiments: Vec::new(),
            units: Vec::new(),
            ticks: 0,
        };
        for (r, regiment) in regiments.iter().enumerate() {
            let first = world.units.len();
            for m in &regiment.members {
                let [x, y, _] = m.thing.position;
                let facing =
                    normalised([m.thing.facing[0], m.thing.facing[1]]).unwrap_or([0.0, -1.0]);
                let (tx, ty) = tile_at([x, y]);
                // Units start on what they travel on.
                let domain = match world.pathing.passable(tx, ty, Domain::Land)
                    || !world.pathing.passable(tx, ty, Domain::Naval)
                {
                    true => Domain::Land,
                    false => Domain::Naval,
                };
                world.units.push(Unit {
                    id: m.thing.id,
                    template: m.thing.template.clone(),
                    regiment: r,
                    domain,
                    position: [x, y],
                    previous: [x, y],
                    facing,
                    end_facing: facing,
                    path: Vec::new(),
                });
            }
            let units: Vec<usize> = (first..world.units.len()).collect();
            let position = match regiment.primary {
                Some(p) => world.units[first + p].position,
                None => world.average(&units),
            };
            world.regiments.push(Regiment {
                id: regiment.id,
                template: regiment.template.clone(),
                owner: regiment.owner,
                units,
                speed: regiment
                    .members
                    .iter()
                    .map(|m| m.speed)
                    .fold(f32::INFINITY, f32::min),
                position,
                facing: normalised(regiment.facing).unwrap_or([0.0, -1.0]),
                formations: regiment.formations.clone(),
                formation: regiment.formation,
            });
        }
        world
    }

    /// Where a regiment's members stand on average.
    pub fn centre(&self, regiment: usize) -> [f32; 2] {
        self.average(&self.regiments[regiment].units)
    }

    fn average(&self, units: &[usize]) -> [f32; 2] {
        let n = units.len().max(1) as f32;
        let [x, y] = units.iter().fold([0.0, 0.0], |[x, y], &u| {
            let [ux, uy] = self.units[u].position;
            [x + ux, y + uy]
        });
        [x / n, y / n]
    }

    /// Whether `player` may give `regiment` orders.
    pub fn commands(&self, player: u32, regiment: usize) -> bool {
        self.regiments[regiment].owner == player
    }

    /// Sends a regiment to `target`, turned to face the way it's going,
    /// each member making for its place in the formation there.
    pub fn order_move(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        if !self.commands(player, regiment) {
            return Err(NotYours);
        }
        let r = &mut self.regiments[regiment];
        let from = r.position;
        if let Some(heading) = normalised([target[0] - from[0], target[1] - from[1]]) {
            r.facing = heading;
        }
        r.position = target;
        self.take_places(regiment);
        Ok(())
    }

    /// Switches a regiment to another of its formations, its members
    /// taking their new places where it stands, or is headed.
    pub fn set_formation(
        &mut self,
        player: u32,
        regiment: usize,
        formation: usize,
    ) -> Result<(), NotYours> {
        if !self.commands(player, regiment) {
            return Err(NotYours);
        }
        let r = &mut self.regiments[regiment];
        if formation < r.formations.len() && formation != r.formation {
            r.formation = formation;
            self.take_places(regiment);
        }
        Ok(())
    }

    /// Sends each of a regiment's members to its place in the formation.
    fn take_places(&mut self, regiment: usize) {
        let r = &self.regiments[regiment];
        let [px, py] = r.position;
        let [fx, fy] = r.facing;
        let offsets = &r.formations[r.formation].offsets;
        for (&u, &[right, ahead]) in r.units.iter().zip(offsets) {
            let place = [px + right * fy + ahead * fx, py - right * fx + ahead * fy];
            let unit = &mut self.units[u];
            let mut way = path::find(&self.pathing, unit.domain, unit.position, place);
            way.reverse();
            unit.path = way;
            unit.end_facing = r.facing;
            if unit.path.is_empty() {
                unit.facing = r.facing;
            }
        }
    }

    /// Plays one [`TICK`].
    pub fn tick(&mut self) {
        for unit in &mut self.units {
            unit.previous = unit.position;
            let mut left = self.regiments[unit.regiment].speed * TICK;
            while let Some(&next) = unit.path.last() {
                let (dx, dy) = (next[0] - unit.position[0], next[1] - unit.position[1]);
                let distance = dx.hypot(dy);
                if let Some(f) = normalised([dx, dy]) {
                    unit.facing = f;
                }
                if distance > left {
                    unit.position[0] += dx / distance * left;
                    unit.position[1] += dy / distance * left;
                    break;
                }
                unit.position = next;
                left -= distance;
                unit.path.pop();
                if unit.path.is_empty() {
                    unit.facing = unit.end_facing;
                }
            }
        }
        self.ticks += 1;
    }
}

/// An order to a regiment from a player that doesn't own it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotYours;

/// The players in a match and how they stand toward each other.
#[derive(Debug, Clone, Default)]
pub struct Sides {
    pub players: Vec<Player>,
    pub teams: Vec<Team>,
}

impl Sides {
    pub fn player(&self, id: u32) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }

    /// How the player `from` stands toward the player `to`, by their ids;
    /// neutral toward a player the match doesn't have.
    pub fn relation(&self, from: u32, to: u32) -> Relation {
        if from == to {
            return Relation::Itself;
        }
        let index = |id| self.players.iter().position(|p| p.id == id);
        let (Some(a), Some(b)) = (index(from), index(to)) else {
            return Relation::Neutral;
        };
        let (pa, pb) = (&self.players[a], &self.players[b]);
        // Players put in a team keep their own stored relations at war
        // with everyone, so the team's speak for them.
        let stance = match (pa.team, pb.team) {
            (Some(ta), Some(tb)) => self.teams[ta].team_relations.get(tb),
            (Some(ta), None) => self.teams[ta].relations.get(b),
            (None, Some(tb)) => pa.team_relations.get(tb),
            (None, None) => pa.relations.get(b),
        };
        stance.copied().unwrap_or(Relation::Neutral)
    }

    /// The player a person plays by default: the first a person may.
    pub fn first_playable(&self) -> Option<&Player> {
        self.players.iter().find(|p| p.playable && !p.independent)
    }
}

fn normalised([x, y]: [f32; 2]) -> Option<[f32; 2]> {
    let length = x.hypot(y);
    (length > 1e-6).then(|| [x / length, y / length])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tgck::map::{Detail, Thing};

    fn member(id: u32, position: [f32; 2], speed: f32) -> aaa_map::Member {
        aaa_map::Member {
            thing: Thing {
                template: "soldier".into(),
                class: tgck::map::ACTOR,
                id,
                name: String::new(),
                position: [position[0], position[1], 0.0],
                facing: [0.0, 1.0, 0.0],
                flags: 0,
                scale: 1.0,
                variation: -1,
                seen_by: 0,
                detail: Detail::Raw(Vec::new()),
            },
            speed,
        }
    }

    /// Two soldiers side by side facing +y, the slower one on the right,
    /// who can also line up one behind the other.
    fn world(pathing: Pathing) -> World {
        let formation = |id: &str, offsets| aaa_map::Formation {
            id: id.into(),
            offsets,
        };
        let regiment = aaa_map::Regiment {
            id: 1,
            template: "squad".into(),
            owner: 7,
            facing: [0.0, 1.0],
            members: vec![member(2, [9.0, 10.0], 3.0), member(3, [11.0, 10.0], 2.0)],
            formations: vec![
                formation("line", vec![[-1.0, 0.0], [1.0, 0.0]]),
                formation("file", vec![[0.0, 1.0], [0.0, -1.0]]),
            ],
            formation: 0,
            primary: None,
        };
        World::from_regiments(Sides::default(), pathing, &[regiment])
    }

    fn run(world: &mut World, seconds: f32) {
        for _ in 0..(seconds / TICK).round() as usize {
            world.tick();
        }
    }

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-3
    }

    #[test]
    fn regiments_move_at_their_slowest_members_speed() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.order_move(7, 0, [10.0, 20.0]).unwrap();
        run(&mut w, 1.0);
        assert!(near(w.units[0].position, [9.0, 12.0]));
        assert!(near(w.units[1].position, [11.0, 12.0]));
        assert!(w.units.iter().all(Unit::moving));
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [9.0, 20.0]));
        assert!(!w.units.iter().any(Unit::moving));
    }

    #[test]
    fn formations_turn_to_face_the_way_they_go() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        // Facing +x now, so the left-hand man ends up on the +y side.
        w.order_move(7, 0, [30.0, 10.0]).unwrap();
        run(&mut w, 20.0);
        assert!(
            near(w.units[0].position, [30.0, 11.0]),
            "{:?}",
            w.units[0].position
        );
        assert!(near(w.units[1].position, [30.0, 9.0]));
        assert!(near(w.units[0].facing, [1.0, 0.0]));
    }

    #[test]
    fn members_go_back_to_their_places_after_being_kept_from_them() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        // The right-hand man's place at 11,20 is blocked.
        pathing.set_blocked(5, 10);
        let mut w = world(pathing);
        w.order_move(7, 0, [10.0, 20.0]).unwrap();
        run(&mut w, 10.0);
        assert!(!near(w.units[1].position, [11.0, 20.0]));
        w.order_move(7, 0, [10.0, 30.0]).unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [9.0, 30.0]));
        assert!(near(w.units[1].position, [11.0, 30.0]));
    }

    #[test]
    fn changing_formation_rearranges_them_where_they_stand() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.set_formation(7, 0, 1).unwrap();
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [10.0, 11.0]));
        assert!(near(w.units[1].position, [10.0, 9.0]));
        assert!(near(w.units[1].facing, [0.0, 1.0]));
    }

    #[test]
    fn units_go_around_what_blocks_them() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        for x in 0..15 {
            pathing.set_blocked(x, 8);
        }
        let mut w = world(pathing.clone());
        w.order_move(7, 0, [10.0, 30.0]).unwrap();
        for _ in 0..400 {
            w.tick();
            for u in &w.units {
                let (x, y) = tile_at(u.position);
                assert!(pathing.passable(x, y, Domain::Land), "{:?}", u.position);
            }
        }
        assert!(near(w.units[0].position, [9.0, 30.0]));
    }

    #[test]
    fn regiments_only_take_orders_from_their_owner() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        assert_eq!(w.order_move(8, 0, [10.0, 20.0]), Err(NotYours));
        assert_eq!(w.set_formation(8, 0, 1), Err(NotYours));
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [9.0, 10.0]));
        assert_eq!(w.regiments[0].formation, 0);
    }

    fn player(id: u32, team: Option<usize>, relations: &[u32], team_relations: &[u32]) -> Player {
        let stored = |r: &[u32]| r.iter().map(|&r| Relation::from_u32(r).unwrap()).collect();
        Player {
            id,
            name: format!("kingdom_{id}"),
            color: String::new(),
            rgb: [1.0; 3],
            team,
            independent: team.is_none(),
            playable: team.is_some(),
            relations: stored(relations),
            team_relations: stored(team_relations),
        }
    }

    #[test]
    fn teams_decide_relations_for_their_players() {
        let team = |id, relations: [Relation; 4], team_relations| Team {
            id,
            name: String::new(),
            relations: relations.into(),
            team_relations,
        };
        use Relation::{Ally, Enemy, Neutral};
        // Two players a team, at odds with each other as they're stored,
        // and civilians neutral to everyone.
        let sides = Sides {
            players: vec![
                player(1, Some(0), &[0, 3, 3, 3], &[3, 3]),
                player(2, Some(0), &[3, 0, 3, 3], &[3, 3]),
                player(3, Some(1), &[3, 3, 0, 3], &[3, 3]),
                player(4, None, &[2, 2, 2, 0], &[2, 2]),
            ],
            teams: vec![
                team(10, [Enemy, Enemy, Enemy, Neutral], vec![Ally, Enemy]),
                team(11, [Enemy, Enemy, Enemy, Neutral], vec![Enemy, Ally]),
            ],
        };
        assert_eq!(sides.relation(1, 1), Relation::Itself);
        assert_eq!(sides.relation(1, 2), Ally);
        assert_eq!(sides.relation(2, 3), Enemy);
        assert_eq!(sides.relation(3, 4), Neutral);
        assert_eq!(sides.relation(4, 1), Neutral);
        assert_eq!(sides.relation(1, 99), Neutral);
        assert_eq!(sides.first_playable().map(|p| p.id), Some(1));
    }

    #[test]
    fn players_without_teams_keep_their_own_relations() {
        let sides = Sides {
            players: vec![
                player(1, None, &[0, 1, 3], &[]),
                player(2, None, &[1, 0, 3], &[]),
                player(3, None, &[2, 3, 0], &[]),
            ],
            teams: Vec::new(),
        };
        assert_eq!(sides.relation(1, 2), Relation::Ally);
        assert_eq!(sides.relation(1, 3), Relation::Enemy);
        assert_eq!(sides.relation(3, 1), Relation::Neutral);
    }
}
