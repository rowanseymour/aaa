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
            world.regiments.push(Regiment {
                id: regiment.id,
                template: regiment.template.clone(),
                owner: regiment.owner,
                units: (first..world.units.len()).collect(),
                speed: regiment
                    .members
                    .iter()
                    .map(|m| m.speed)
                    .fold(f32::INFINITY, f32::min),
            });
        }
        world
    }

    /// Where a regiment's members stand on average.
    pub fn centre(&self, regiment: usize) -> [f32; 2] {
        let units = &self.regiments[regiment].units;
        let n = units.len().max(1) as f32;
        let [x, y] = units.iter().fold([0.0, 0.0], |[x, y], &u| {
            let [ux, uy] = self.units[u].position;
            [x + ux, y + uy]
        });
        [x / n, y / n]
    }

    /// Sends a regiment to `target`, its members keeping their places in
    /// the formation, turned to face the way it's going.
    pub fn order_move(&mut self, regiment: usize, target: [f32; 2]) {
        let centre = self.centre(regiment);
        let units = self.regiments[regiment].units.clone();
        let Some(heading) = normalised([target[0] - centre[0], target[1] - centre[1]]) else {
            return;
        };
        let was = units
            .iter()
            .map(|&u| self.units[u].facing)
            .fold([0.0, 0.0], |[x, y], [fx, fy]| [x + fx, y + fy]);
        let was = normalised(was).unwrap_or(heading);
        // The turn from the old heading to the new, as a rotation.
        let (cos, sin) = (
            was[0] * heading[0] + was[1] * heading[1],
            was[0] * heading[1] - was[1] * heading[0],
        );
        for u in units {
            let unit = &self.units[u];
            let [ox, oy] = [unit.position[0] - centre[0], unit.position[1] - centre[1]];
            let place = [
                target[0] + ox * cos - oy * sin,
                target[1] + ox * sin + oy * cos,
            ];
            let mut way = path::find(&self.pathing, unit.domain, unit.position, place);
            way.reverse();
            let unit = &mut self.units[u];
            unit.path = way;
            unit.end_facing = heading;
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

    /// Two soldiers side by side facing +y, the slower one on the right.
    fn world(pathing: Pathing) -> World {
        let regiment = aaa_map::Regiment {
            id: 1,
            template: "squad".into(),
            owner: 7,
            members: vec![member(2, [9.0, 10.0], 3.0), member(3, [11.0, 10.0], 2.0)],
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
