//! Units moving: finding their way, driving each leg and keeping their
//! places in their regiment's formation.

use std::f32::consts::FRAC_PI_4;

use aaa_map::{Id, Obstacle, Tile};

use crate::{
    ARRIVAL_REACH, EXACT_REACH, LOOK, Leg, NotYours, TICK, Trailer, Unit, World, normalised, path,
    within, within_strictly,
};

/// How near a unit's destination is for a move that has to end right there
/// to go straight at it.
const STRAIGHT_REACH: f32 = 3.0;

/// How many times a unit finds a way to a destination before giving up on
/// stopping there.
const WAYS_TRIED: u8 = 3;

/// How near a leg's end a unit taking it up has to be to skip it.
const LEG_REACH: f32 = 0.1;

/// How far to the side of its leg a unit gets before making for it again.
const OFF_LINE: f32 = 1.0;

const LOOK_SNAPPED: u32 = 5;

impl Trailer {
    pub(crate) fn new(trailer: &aaa_map::Trailer, position: [f32; 2], facing: [f32; 2]) -> Trailer {
        let (position, facing) = match trailer.placed {
            Some((_, at, way)) => (at, normalised(way).unwrap_or(facing)),
            None => {
                // Straight behind it.
                let hitch = aaa_map::from_local(position, facing, trailer.hitch);
                let [x, y] = trailer.coupling;
                (aaa_map::from_local(hitch, facing, [-x, -y]), facing)
            }
        };
        Trailer {
            template: Id::from(trailer.template.as_str()),
            position,
            previous: position,
            facing,
            hitch: trailer.hitch,
            coupling: trailer.coupling,
        }
    }

    /// Stands it straight behind its unit.
    pub(crate) fn behind(&mut self, position: [f32; 2], facing: [f32; 2]) {
        let hitch = aaa_map::from_local(position, facing, self.hitch);
        let [x, y] = self.coupling;
        self.position = aaa_map::from_local(hitch, facing, [-x, -y]);
        self.previous = self.position;
        self.facing = facing;
    }

    /// Turns to point at its unit's hitch, and moves so that its coupling
    /// meets it.
    fn follow(&mut self, position: [f32; 2], facing: [f32; 2]) {
        let hitch = aaa_map::from_local(position, facing, self.hitch);
        let to = [hitch[0] - self.position[0], hitch[1] - self.position[1]];
        self.facing = normalised(to).unwrap_or(self.facing);
        let [x, y] = self.coupling;
        self.position = aaa_map::from_local(hitch, self.facing, [-x, -y]);
    }

    /// Where to draw it, as [`Unit::drawn_at`].
    pub fn drawn_at(&self, t: f32) -> [f32; 2] {
        let ([px, py], [x, y]) = (self.previous, self.position);
        [px + (x - px) * t, py + (y - py) * t]
    }
}

impl Unit {
    pub fn moving(&self) -> bool {
        !self.path.is_empty()
    }

    /// Whether it's still on the map and in one piece.
    pub fn alive(&self) -> bool {
        self.died.is_none() && self.left.is_none() && !self.awaited
    }

    pub(crate) fn die(&mut self, tick: u64) {
        self.health = 0.0;
        self.died = Some(tick);
        self.besieged = None;
        self.halt();
        self.speed = 0.0;
        self.target = None;
    }

    /// Sets it off along `path`, next waypoint last, to stop at its end.
    pub(crate) fn take(&mut self, path: Vec<[f32; 2]>, exact: bool) {
        self.goal = path.first().copied();
        self.path = path;
        self.from = self.position;
        self.leg = None;
        self.exact = exact;
        self.ways = 1;
        self.look = LOOK;
    }

    /// Stops it where it is, with nowhere left to go.
    pub(crate) fn halt(&mut self) {
        self.path.clear();
        self.leg = None;
        self.goal = None;
    }

    /// Stands it at `at` facing `facing` as if just made: still, with
    /// nowhere to go, nothing fired on and its weapons ready.
    pub(crate) fn stand_at(&mut self, at: [f32; 2], facing: [f32; 2]) {
        self.position = at;
        self.previous = at;
        self.facing = facing;
        self.end_facing = facing;
        self.halt();
        self.speed = 0.0;
        self.closing = false;
        self.target = None;
        self.shot = None;
        self.reloads.fill(0.0);
        if let Some(t) = &mut self.trailer {
            t.behind(at, facing);
        }
    }

    /// The middle of the tightest circle it can turn to its left, for a
    /// positive `side`, or its right.
    fn turning_circle(&self, side: f32) -> [f32; 2] {
        let [x, y] = self.position;
        let [fx, fy] = self.facing;
        let r = self.turn_radius.copysign(side);
        [x - fy * r, y + fx * r]
    }

    /// Whether `at` is somewhere it can't turn onto: inside the tightest
    /// circle it can turn either way.
    fn too_tight(&self, at: [f32; 2]) -> bool {
        let r = self.turn_radius;
        r > 0.0
            && [1.0, -1.0]
                .iter()
                .any(|&side| within_strictly(self.turning_circle(side), at, r))
    }

    /// Takes up the leg to its next waypoint, passing over the waypoints
    /// it's already level with or past. The leg ends short of a corner by
    /// the room it takes to turn it.
    fn take_leg(&mut self) {
        while self.leg.is_none() {
            let Some(&next) = self.path.last() else {
                return;
            };
            let along = [next[0] - self.from[0], next[1] - self.from[1]];
            let heading = normalised(along).unwrap_or(self.facing);
            let onward = (self.path.len().checked_sub(2))
                .and_then(|i| normalised([self.path[i][0] - next[0], self.path[i][1] - next[1]]));
            let lead = match onward {
                Some(onward) if self.turn_radius > 0.0 => {
                    let cos = heading[0] * onward[0] + heading[1] * onward[1];
                    let sin = (heading[0] * onward[1] - heading[1] * onward[0]).abs();
                    // The tangent of half the angle between the two legs.
                    let lead = self.turn_radius * sin / (1.0 + cos);
                    let length = libm::hypotf(along[0], along[1]);
                    if lead < length { lead } else { length }
                }
                _ => 0.0,
            };
            let aim = [next[0] - heading[0] * lead, next[1] - heading[1] * lead];
            let left =
                (aim[0] - self.position[0]) * heading[0] + (aim[1] - self.position[1]) * heading[1];
            if left <= 0.0 || within(self.position, aim, LEG_REACH) {
                self.path.pop();
                self.from = next;
                continue;
            }
            self.leg = Some(Leg {
                heading,
                aim,
                snapped: self.path.len() == 1 && self.too_tight(next),
            });
        }
    }

    /// Plays out a tick of heading along its path at up to `top` speed: it
    /// turns toward its leg's heading as fast as it can, speeds up toward
    /// `top` and goes the way it faces, until it's level with the leg's
    /// end, however far to the side that leaves it. It never slows to turn.
    fn drive(&mut self, top: f32) {
        self.take_leg();
        let (Some(&next), Some(leg)) = (self.path.last(), self.leg) else {
            self.speed = 0.0;
            return;
        };
        match leg.snapped || self.turn_rate <= 0.0 {
            true => self.facing = leg.heading,
            false => {
                steer(&mut self.facing, leg.heading, self.turn_rate * TICK);
            }
        }
        self.speed = match self.speed < top && self.acceleration > 0.0 {
            true => (self.speed + self.acceleration * TICK).min(top),
            false => top,
        };
        let step = self.speed * TICK;
        let left = (leg.aim[0] - self.position[0]) * leg.heading[0]
            + (leg.aim[1] - self.position[1]) * leg.heading[1];
        let ends = left <= step;
        // Only one going straight along the leg stops level with its end.
        let go = match ends && self.facing == leg.heading {
            true => left.max(0.0),
            false => step,
        };
        self.position[0] += self.facing[0] * go;
        self.position[1] += self.facing[1] * go;
        if ends {
            self.path.pop();
            self.from = next;
            self.leg = None;
            if self.path.is_empty() {
                self.speed = 0.0;
            }
            return;
        }
        self.look = self.look.saturating_sub(1);
        if self.look == 0 {
            self.think(next, leg);
        }
    }

    /// Looks at how the leg to `next` is going: gives up turning onto a
    /// last leg that ends somewhere it can't turn onto, and, once it's done
    /// turning but is left well to the side of its leg, makes for the leg
    /// again at an angle, or straight for the leg's end where that comes
    /// first.
    fn think(&mut self, next: [f32; 2], leg: Leg) {
        self.look = match leg.snapped {
            true => LOOK_SNAPPED,
            false => LOOK,
        };
        if self.turn_radius <= 0.0 {
            return;
        }
        if self.path.len() == 1 && !leg.snapped && self.too_tight(next) {
            self.facing = leg.heading;
            self.leg = Some(Leg {
                snapped: true,
                ..leg
            });
            self.look = LOOK_SNAPPED;
            return;
        }
        if self.facing != leg.heading {
            return;
        }
        let [hx, hy] = leg.heading;
        let back = [self.position[0] - next[0], self.position[1] - next[1]];
        // How far to the leg's left it is, and short of its end.
        let aside = hx * back[1] - hy * back[0];
        let short = -(hx * back[0] + hy * back[1]);
        if aside.abs() < OFF_LINE {
            return;
        }
        // At 45°, or as much less as turning in and back out again leaves
        // room for.
        let room = 1.0 - aside.abs() / (2.0 * self.turn_radius);
        let angle = libm::acosf(room.max(-1.0)).min(FRAC_PI_4);
        let along = aside.abs() / libm::tanf(angle);
        self.from = self.position;
        self.leg = None;
        self.look = LOOK;
        if along >= short {
            let to = [next[0] - self.position[0], next[1] - self.position[1]];
            self.facing = normalised(to).unwrap_or(self.facing);
            return;
        }
        self.path.push([
            self.position[0] + hy * aside + hx * along,
            self.position[1] - hx * aside + hy * along,
        ]);
    }

    /// Where to draw it a fraction `t` of the way through the tick that
    /// moved it from [`Unit::previous`] to [`Unit::position`].
    pub fn drawn_at(&self, t: f32) -> [f32; 2] {
        let ([px, py], [x, y]) = (self.previous, self.position);
        [px + (x - px) * t, py + (y - py) * t]
    }
}

/// Turns `facing` toward `to`, both unit vectors, by up to `most` radians,
/// returning how far it turned.
fn steer(facing: &mut [f32; 2], to: [f32; 2], most: f32) -> f32 {
    let [fx, fy] = *facing;
    let angle = libm::atan2f(fx * to[1] - fy * to[0], fx * to[0] + fy * to[1]);
    if angle.abs() <= most {
        *facing = to;
        return angle.abs();
    }
    let (sin, cos) = libm::sincosf(most * angle.signum());
    *facing = [fx * cos - fy * sin, fx * sin + fy * cos];
    most
}

impl World {
    pub(crate) fn send(&mut self, regiment: usize, target: [f32; 2], facing: [f32; 2]) {
        let r = &mut self.regiments[regiment];
        r.position = target;
        r.facing = facing;
        self.take_places(regiment);
    }

    pub(crate) fn set_formation(
        &mut self,
        player: u32,
        regiment: usize,
        formation: usize,
    ) -> Result<(), NotYours> {
        self.may_command(player, regiment)?;
        let r = &mut self.regiments[regiment];
        if formation < r.formations.len() && formation != r.formation {
            r.formation = formation;
            // An engaged regiment keeps to its combat formation for now.
            if r.current_formation() == formation {
                self.take_places(regiment);
            }
        }
        Ok(())
    }

    /// Sends each of a regiment's members to its place in the formation.
    pub(crate) fn take_places(&mut self, regiment: usize) {
        let r = &self.regiments[regiment];
        let facing = r.facing;
        // A move to unpack has to end right there.
        let exact = r.unpack_at.is_some();
        let offsets = &r.formations[r.current_formation()].offsets;
        let places: Vec<(usize, [f32; 2])> = r
            .units
            .iter()
            .zip(offsets)
            .map(|(&u, &offset)| (u, aaa_map::from_local(r.position, facing, offset)))
            .collect();
        for (u, place) in places {
            if !self.units[u].alive() {
                continue;
            }
            self.take_place(u, place, facing, exact);
        }
    }

    /// Sends a unit to stand at `place` facing `facing`.
    pub(crate) fn take_place(&mut self, u: usize, place: [f32; 2], facing: [f32; 2], exact: bool) {
        let way = self.plan(u, place, exact);
        let unit = &mut self.units[u];
        unit.take(way, exact);
        unit.closing = false;
        unit.end_facing = facing;
        if unit.path.is_empty() {
            unit.goal = None;
            unit.facing = facing;
        }
    }

    /// The way a unit takes to stop at `to`, last waypoint first, `exact`
    /// for a move that has to end right there. It goes straight at what's
    /// near enough on such a move, and turns at once to what's ahead of it
    /// but nearer than it could turn onto; otherwise it finds a way.
    pub(crate) fn plan(&mut self, u: usize, to: [f32; 2], exact: bool) -> Vec<[f32; 2]> {
        let unit = &self.units[u];
        let off = [to[0] - unit.position[0], to[1] - unit.position[1]];
        if exact && within_strictly(unit.position, to, STRAIGHT_REACH) {
            return vec![to];
        }
        let [fx, fy] = unit.facing;
        let ahead = fx * off[0] + fy * off[1] >= 0.0;
        let side = fx * off[1] - fy * off[0];
        let r = unit.turn_radius;
        if r > 0.0 && ahead && within_strictly(unit.turning_circle(side), to, r) {
            if let Some(facing) = normalised(off) {
                self.units[u].facing = facing;
            }
            return vec![to];
        }
        self.way(u, to)
    }

    /// A unit's way to `to`, last waypoint first. It passes through the
    /// obstacles it's caught on, as one put out through its building, or
    /// beside one unpacked next to it, is: those its shape overlaps, and
    /// those blocking its tile, which reach further than their shapes do.
    pub(crate) fn way(&self, u: usize, to: [f32; 2]) -> Vec<[f32; 2]> {
        let unit = &self.units[u];
        let tile = Tile::at(unit.position);
        let body = unit.shape.map(|shape| Obstacle {
            position: unit.position,
            facing: unit.facing,
            shape,
            soft: shape,
        });
        let caught: Vec<&Obstacle> = self
            .buildings
            .iter()
            .filter(|b| !b.gone())
            .filter_map(|b| b.obstacle.as_ref())
            .chain(&self.scenery)
            .filter(|o| o.covers(tile) || body.as_ref().is_some_and(|b| b.overlaps(o)))
            .collect();
        let mut way = if caught.is_empty() {
            path::find(&self.pathing, unit.domain, unit.position, to)
        } else {
            let pathing = self.pathing.without(caught);
            path::find(&pathing, unit.domain, unit.position, to)
        };
        way.reverse();
        way
    }

    /// Moves every unit on along its path. One that's run out of path
    /// short of where it was to stop finds another way from where it is,
    /// until it's tried enough; once done it faces as it was told to.
    pub(crate) fn travel(&mut self) {
        for u in 0..self.units.len() {
            let limit = self.speed_limit(u).unwrap_or(f32::INFINITY);
            let unit = &mut self.units[u];
            unit.previous = unit.position;
            unit.drive(self.regiments[unit.regiment].speed.min(limit));
            if let (true, Some(goal)) = (unit.path.is_empty(), unit.goal) {
                let reach = match unit.exact {
                    true => EXACT_REACH,
                    false => ARRIVAL_REACH,
                };
                let (exact, ways) = (unit.exact, unit.ways);
                let again = match within(unit.position, goal, reach) || ways >= WAYS_TRIED {
                    true => Vec::new(),
                    false => self.plan(u, goal, exact),
                };
                let unit = &mut self.units[u];
                match again.is_empty() {
                    true => {
                        unit.goal = None;
                        unit.facing = unit.end_facing;
                    }
                    false => {
                        unit.take(again, exact);
                        (unit.goal, unit.ways) = (Some(goal), ways + 1);
                    }
                }
            }
            let unit = &mut self.units[u];
            if let Some(t) = &mut unit.trailer {
                t.previous = t.position;
                t.follow(unit.position, unit.facing);
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::testing::*;

    use aaa_map::{Domain, Obstacle, Pathing, Shape, Tile};

    use crate::{ARRIVAL_REACH, Command, Sides, Unit, World, within};

    #[test]
    fn units_stood_somewhere_afresh_keep_nothing_of_what_they_were_doing() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        let u = &mut w.units[0];
        u.target = Some(crate::Target::Unit(1));
        u.shot = Some((3, 0));
        u.closing = true;
        u.stand_at([4.0, 6.0], [1.0, 0.0]);
        assert!(!u.moving() && u.speed == 0.0 && !u.closing);
        assert_eq!((u.position, u.previous), ([4.0, 6.0], [4.0, 6.0]));
        assert_eq!((u.target, u.shot), (None, None));
        // It stays there, facing as it was stood.
        run(&mut w, 1.0);
        assert_eq!(w.units[0].position, [4.0, 6.0]);
        assert_eq!(w.units[0].facing, [1.0, 0.0]);
    }

    #[test]
    fn regiments_move_at_their_slowest_members_speed() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        assert!(near(w.units[0].position, [9.0, 12.0]));
        assert!(near(w.units[1].position, [11.0, 12.0]));
        assert!(w.units.iter().all(Unit::moving));
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [9.0, 20.0]));
        assert!(!w.units.iter().any(Unit::moving));
    }

    #[test]
    fn units_start_as_hurt_as_the_map_has_them() {
        let mut m = member(2, [10.0, 10.0], 2.0);
        m.fighter = Some(aaa_map::Fighter {
            health: 200.0,
            ..aaa_map::Fighter::default()
        });
        m.health = 0.25;
        let regiment = aaa_map::Regiment {
            hq: None,
            id: 2,
            template: "soldier".into(),
            owner: 7,
            facing: [0.0, 1.0],
            members: vec![m],
            formations: vec![aaa_map::Formation {
                id: String::new(),
                offsets: vec![[0.0, 0.0]],
                aggressive: false,
            }],
            formation: 0,
            combat: None,
            operational_range: None,
            control_range: None,
            primary: Some(0),
            unpacks: None,
        };
        let w = World::from_regiments(
            Sides::default(),
            Pathing::uniform(20, Domain::Land),
            &[regiment],
        );
        assert_eq!(w.units[0].health, 50.0);
    }

    #[test]
    fn units_speed_up_to_their_top_speed_and_stop_at_once() {
        let mut w = vehicle(2.0, 0.0, None);
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 30.0],
            },
        )
        .unwrap();
        // 0.2 a tick until it's at 2 a second after a second.
        run(&mut w, 1.0);
        assert!((w.units[0].speed - 2.0).abs() < 1e-4);
        assert!(
            near(w.units[0].position, [10.0, 11.1]),
            "{:?}",
            w.units[0].position
        );
        run(&mut w, 15.0);
        assert!(near(w.units[0].position, [10.0, 30.0]));
        assert_eq!(w.units[0].speed, 0.0);
    }

    #[test]
    fn units_that_turn_over_time_drive_round_corners() {
        let mut w = vehicle(0.0, 1.0, None);
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [20.0, 10.0],
            },
        )
        .unwrap();
        w.tick();
        // A tenth of a radian toward +x, having gone 0.2 that way.
        let u = &w.units[0];
        let (sin, cos) = libm::sincosf(0.1);
        assert!(near(u.facing, [sin, cos]), "{:?}", u.facing);
        assert!(near(u.position, [10.0 + 0.2 * sin, 10.0 + 0.2 * cos]));
        run(&mut w, 2.0);
        assert!(w.units[0].position[1] > 10.5, "it swings wide");
        run(&mut w, 20.0);
        let u = &w.units[0];
        assert!(!u.moving());
        // Left to the side of where it was going, but near enough.
        assert!(!near(u.position, [20.0, 10.0]));
        assert!(
            within(u.position, [20.0, 10.0], ARRIVAL_REACH),
            "{:?}",
            u.position
        );
    }

    #[test]
    fn units_that_turn_over_time_stop_near_what_is_inside_their_turning_circle() {
        // It turns round a radius of 2 at its top speed.
        for to in [
            [10.0, 4.0],
            [11.0, 10.0],
            [8.0, 10.5],
            [10.0, 9.0],
            [4.0, 8.0],
        ] {
            let mut w = vehicle(0.0, 1.0, None);
            w.apply(7, Command::Move { regiment: 0, to }).unwrap();
            let mut ticks = 0;
            while w.units[0].moving() {
                w.tick();
                ticks += 1;
                assert!(ticks < 300, "{to:?}: {:?}", w.units[0].position);
            }
            let at = w.units[0].position;
            assert!(within(at, to, ARRIVAL_REACH), "{to:?}: {at:?}");
        }
    }

    #[test]
    fn trailers_follow_their_hitch_round_corners() {
        let trailer = aaa_map::Trailer {
            template: "gun".into(),
            hitch: [0.0, -2.0],
            coupling: [0.0, 1.0],
            placed: None,
        };
        let mut w = vehicle(0.0, 0.0, Some(trailer));
        let t = w.units[0].trailer.as_ref().unwrap();
        assert!(near(t.position, [10.0, 7.0]), "{:?}", t.position);
        assert!(near(t.facing, [0.0, 1.0]));
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [20.0, 10.0],
            },
        )
        .unwrap();
        w.tick();
        // The truck turns to +x at once and goes 0.2: the hitch swings out
        // behind it to 8.2,10 and the trailer swings round after it.
        let t = w.units[0].trailer.as_ref().unwrap();
        let hitch = [8.2, 10.0];
        let coupled = aaa_map::from_local(t.position, t.facing, [0.0, 1.0]);
        assert!(near(coupled, hitch), "{coupled:?}");
        assert!(t.facing[0] < 0.0 && t.facing[1] > 0.0, "{:?}", t.facing);
        run(&mut w, 10.0);
        let t = w.units[0].trailer.as_ref().unwrap();
        assert!(near(t.facing, [1.0, 0.0]), "{:?}", t.facing);
        assert!(near(t.position, [17.0, 10.0]), "{:?}", t.position);
    }

    #[test]
    fn formations_turn_to_face_the_way_they_go() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        // Facing +x now, so the left-hand man ends up on the +y side.
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [30.0, 10.0],
            },
        )
        .unwrap();
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
        pathing.set_blocked(Tile::new(5, 10));
        let mut w = world(pathing);
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(!near(w.units[1].position, [11.0, 20.0]));
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 30.0],
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [9.0, 30.0]));
        assert!(near(w.units[1].position, [11.0, 30.0]));
    }

    #[test]
    fn changing_formation_rearranges_them_where_they_stand() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.apply(
            7,
            Command::Formation {
                regiment: 0,
                formation: 1,
            },
        )
        .unwrap();
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [10.0, 11.0]));
        assert!(near(w.units[1].position, [10.0, 9.0]));
        assert!(near(w.units[1].facing, [0.0, 1.0]));
    }

    #[test]
    fn units_go_around_what_blocks_them() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        for x in 0..15 {
            pathing.set_blocked(Tile::new(x, 8));
        }
        let mut w = world(pathing.clone());
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 30.0],
            },
        )
        .unwrap();
        for _ in 0..400 {
            w.tick();
            for u in &w.units {
                let tile = Tile::at(u.position);
                assert!(pathing.passable(tile, Domain::Land), "{:?}", u.position);
            }
        }
        assert!(near(w.units[0].position, [9.0, 30.0]));
    }

    #[test]
    fn units_pass_through_scenery_they_stand_in() {
        // A rock around the left-hand man, blocking every tile near him.
        let rock = Obstacle {
            position: [9.0, 10.0],
            facing: [0.0, 1.0],
            shape: Shape::Circle { radius: 3.0 },
            soft: Shape::Circle { radius: 4.0 },
        };
        let mut pathing = Pathing::uniform(20, Domain::Land);
        pathing.block(&rock);
        let mut w = world(pathing);
        w.scenery.push(rock);
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 30.0],
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [9.0, 30.0]));
    }

    #[test]
    fn units_pass_through_what_their_shape_overlaps() {
        // A wall right across the map, 2 ahead of them: it doesn't block
        // their tiles, but the left-hand man is big enough to overlap it.
        let wall = Obstacle {
            position: [20.0, 12.5],
            facing: [0.0, 1.0],
            shape: Shape::Rect {
                width: 40.0,
                height: 1.0,
            },
            soft: Shape::Rect {
                width: 41.0,
                height: 2.0,
            },
        };
        let mut pathing = Pathing::uniform(20, Domain::Land);
        pathing.block(&wall);
        let mut w = world(pathing);
        w.scenery.push(wall);
        w.units[0].shape = Some(Shape::Circle { radius: 2.5 });
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 30.0],
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [9.0, 30.0]));
        assert!(w.units[1].position[1] < 12.0);
    }
}
