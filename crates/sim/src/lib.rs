//! The rules of the game, played out with no rendering so that it can run
//! headless.
//!
//! A *match* is one play of a map: a *match setup* (the map, its players and
//! their nations, teams and rules) once it's running. Each *player* is a side
//! in it, controlled by a person or an AI.

pub mod path;

use aaa_map::pathing::tile_at;
use aaa_map::{Domain, Map, Pathing};

/// Seconds of play one [`World::tick`] advances.
pub const TICK: f32 = 0.1;

/// A match in progress.
pub struct World {
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
        World::from_regiments(map.pathing.clone(), &map.regiments)
    }

    pub fn from_regiments(pathing: Pathing, regiments: &[aaa_map::Regiment]) -> World {
        let mut world = World {
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

    /// Sends a regiment to `target`, turned to face the way it's going,
    /// each member making for its place in the formation there.
    pub fn order_move(&mut self, regiment: usize, target: [f32; 2]) {
        let r = &mut self.regiments[regiment];
        let from = r.position;
        if let Some(heading) = normalised([target[0] - from[0], target[1] - from[1]]) {
            r.facing = heading;
        }
        r.position = target;
        self.take_places(regiment);
    }

    /// Switches a regiment to another of its formations, its members
    /// taking their new places where it stands, or is headed.
    pub fn set_formation(&mut self, regiment: usize, formation: usize) {
        let r = &mut self.regiments[regiment];
        if formation < r.formations.len() && formation != r.formation {
            r.formation = formation;
            self.take_places(regiment);
        }
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
        World::from_regiments(pathing, &[regiment])
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
        w.order_move(0, [10.0, 20.0]);
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
        w.order_move(0, [30.0, 10.0]);
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
        w.order_move(0, [10.0, 20.0]);
        run(&mut w, 10.0);
        assert!(!near(w.units[1].position, [11.0, 20.0]));
        w.order_move(0, [10.0, 30.0]);
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [9.0, 30.0]));
        assert!(near(w.units[1].position, [11.0, 30.0]));
    }

    #[test]
    fn changing_formation_rearranges_them_where_they_stand() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.set_formation(0, 1);
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
        w.order_move(0, [10.0, 30.0]);
        for _ in 0..400 {
            w.tick();
            for u in &w.units {
                let (x, y) = tile_at(u.position);
                assert!(pathing.passable(x, y, Domain::Land), "{:?}", u.position);
            }
        }
        assert!(near(w.units[0].position, [9.0, 30.0]));
    }
}
