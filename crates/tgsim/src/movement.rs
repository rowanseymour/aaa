//! Units moving: finding their way, driving each leg and keeping their
//! places in their organization's formation.

use std::f32::consts::FRAC_PI_4;

use tgmap::{Id, Obstacle, Tile};

use crate::value::MULTIPLY_FLOOR;

use crate::{
    ARRIVAL_REACH, EXACT_REACH, LOOK, Leg, NotYours, TICK, Trailer, Unit, World, normalised, path,
    within, within_strictly,
};

/// How near a unit's destination is for a move that has to end right there
/// to go straight at it.
const STRAIGHT_REACH: f32 = 3.0;

/// How far an organization is sent from where it was headed before the
/// move sets it off along a route.
pub(crate) const SETTING_OFF: f32 = 0.5;

/// How many times a unit finds a way to a destination before giving up on
/// stopping there.
const WAYS_TRIED: u8 = 3;

/// How near a leg's end a unit taking it up has to be to skip it.
const LEG_REACH: f32 = 0.1;

/// How far to the side of its leg a unit gets before making for it again.
const OFF_LINE: f32 = 1.0;

const LOOK_SNAPPED: u32 = 5;

/// The pace, in world units a second, at or below which a member ahead of
/// its place stands until it comes.
const WAITING_PACE: f32 = 0.2;

/// How near the corner at the end of its leg a member keeping its place
/// starts counting its place's sideways offset in.
const CORNER_REACH: f32 = 6.0;

/// How much further than its regiment a member on a detour has to go to
/// go flat out.
const DETOUR_CATCHING_UP: f32 = 4.0;

/// How far at a time a member pulls a point of its route it can't stand
/// on back towards its leader's.
const PULL_BACK: f32 = 0.5;

impl Trailer {
    pub(crate) fn new(trailer: &tgmap::Trailer, position: [f32; 2], facing: [f32; 2]) -> Trailer {
        let (position, facing) = match trailer.placed {
            Some((_, at, way)) => (at, normalised(way).unwrap_or(facing)),
            None => {
                // Straight behind it.
                let hitch = tgmap::from_local(position, facing, trailer.hitch);
                let [x, y] = trailer.coupling;
                (tgmap::from_local(hitch, facing, [-x, -y]), facing)
            }
        };
        Trailer {
            template: trailer.template.clone(),
            position,
            previous: position,
            facing,
            hitch: trailer.hitch,
            coupling: trailer.coupling,
        }
    }

    /// Stands it straight behind its unit.
    pub(crate) fn behind(&mut self, position: [f32; 2], facing: [f32; 2]) {
        let hitch = tgmap::from_local(position, facing, self.hitch);
        let [x, y] = self.coupling;
        self.position = tgmap::from_local(hitch, facing, [-x, -y]);
        self.previous = self.position;
        self.facing = facing;
    }

    /// Turns to point at its unit's hitch, and moves so that its coupling
    /// meets it.
    fn follow(&mut self, position: [f32; 2], facing: [f32; 2]) {
        let hitch = tgmap::from_local(position, facing, self.hitch);
        let to = [hitch[0] - self.position[0], hitch[1] - self.position[1]];
        self.facing = normalised(to).unwrap_or(self.facing);
        let [x, y] = self.coupling;
        self.position = tgmap::from_local(hitch, self.facing, [-x, -y]);
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

    /// How far it has left to go along its path.
    pub(crate) fn route_left(&self) -> f32 {
        let mut at = self.position;
        let mut left = 0.0;
        for &p in self.path.iter().rev() {
            left += crate::distance(at, p);
            at = p;
        }
        left
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
        self.pace = 1.0;
        self.detour = false;
    }

    /// Stops it where it is, with nowhere left to go.
    pub(crate) fn halt(&mut self) {
        self.path.clear();
        self.leg = None;
        self.goal = None;
        self.pace = 1.0;
        self.detour = false;
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
        self.closing = None;
        self.busy = 0;
        self.decided = 0;
        self.met = None;
        self.target = None;
        self.shot = None;
        self.fired_ago.fill(f32::INFINITY);
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
    /// It turns at a fixed rate whatever its speed, `own` top speed over its
    /// radius, so at that speed it keeps to its turning circle.
    fn drive(&mut self, top: f32, own: f32) {
        self.take_leg();
        let (Some(&next), Some(leg)) = (self.path.last(), self.leg) else {
            self.speed = 0.0;
            return;
        };
        if self.pace <= 0.0 {
            self.speed = 0.0;
            return;
        }
        match leg.snapped || self.turn_radius <= 0.0 {
            true => self.facing = leg.heading,
            false => {
                steer(&mut self.facing, leg.heading, own / self.turn_radius * TICK);
            }
        }
        self.speed = match self.speed < top && self.acceleration > 0.0 {
            true => (self.speed + self.acceleration * TICK).min(top),
            false => top,
        };
        let step = self.speed * self.pace * TICK;
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
            self.detour = false;
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
            self.detour = true;
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
    /// The state the organization at `r` holds for its part in a fight, as
    /// the data names it: engaged, or targeted while another engages it
    /// and it doesn't engage back.
    pub(crate) fn fight_state(&self, r: usize) -> Option<&'static str> {
        match self.organizations[r].engaged.is_empty() {
            false => Some(tgmap::ENGAGED),
            true => self.engagers(r).next().map(|_| tgmap::TARGETED),
        }
    }

    /// What the organization at `r` adds to each member's top speed past
    /// the member's own properties: its formation's, and its state's in a
    /// fight.
    fn handed_down_speed(&self, r: usize) -> f32 {
        let formation = self.formation_effect(r).map_or(0.0, |e| e.speed);
        let state = (self.fight_state(r)).and_then(|p| self.rules.combat_effects.get(&Id::from(p)));
        formation + state.map_or(0.0, |e| e.speed)
    }

    /// The unit at `u`'s top speed: its own, times what its properties and
    /// the `handed` down from its organization make of it.
    fn top_speed(&self, u: usize, handed: f32) -> f32 {
        let factor = 1.0 + self.boost(u).speed + handed;
        self.units[u].speed_max * factor.max(MULTIPLY_FLOOR)
    }

    /// How fast the organization at `r` moves: as fast as its slowest
    /// member present can.
    pub(crate) fn regiment_speed(&self, r: usize) -> f32 {
        let own = self.unhurried_speed(r);
        // Its group's, which only ever slows it.
        self.organizations[r]
            .group_speed
            .map_or(own, |s| s.min(own))
    }

    /// How fast the organization at `r` moves left to itself: as fast as
    /// its slowest member present can, whatever its group would have.
    pub(crate) fn unhurried_speed(&self, r: usize) -> f32 {
        let handed = self.handed_down_speed(r);
        let present = (self.organizations[r].units.iter()).filter(|&&u| self.units[u].alive());
        let slowest = (present.map(|&u| self.top_speed(u, handed))).fold(f32::INFINITY, f32::min);
        match slowest.is_finite() {
            true => slowest,
            false => self.organizations[r].speed,
        }
    }

    /// Sends the organization at `organization` to stand at `target`
    /// facing `facing`. Any such move unsettles it; one further than
    /// [`SETTING_OFF`] sets it off along a route, which ends its
    /// entrenchment.
    pub(crate) fn send(&mut self, organization: usize, target: [f32; 2], facing: [f32; 2]) {
        let r = &mut self.organizations[organization];
        r.settled = None;
        if !within(r.position, target, SETTING_OFF) {
            r.travelling = true;
            r.entrenched = None;
        }
        r.position = target;
        r.facing = facing;
        self.take_places(organization);
    }

    pub(crate) fn set_formation(
        &mut self,
        player: u32,
        organization: usize,
        formation: usize,
    ) -> Result<(), NotYours> {
        self.may_command(player, organization)?;
        let r = &mut self.organizations[organization];
        if formation < r.formations.len() && formation != r.formation {
            r.formation = formation;
            // An engaged organization keeps to its combat formation for now.
            if r.current_formation() == formation {
                self.take_places(organization);
            }
        }
        Ok(())
    }

    /// Sends each of an organization's members to its place in the
    /// formation. One setting off along a route hands its leader the way
    /// there, and each other member a copy of it fitted to its place.
    pub(crate) fn take_places(&mut self, organization: usize) {
        let r = &self.organizations[organization];
        let facing = r.facing;
        // A move to unpack has to end right there.
        let exact = r.unpack_at.is_some();
        let places = self.formation_places(organization);
        let leader = (self.pacer(organization))
            .map(|p| r.units[p])
            .filter(|_| r.travelling);
        let lead = leader.and_then(|l| places.iter().find(|(u, _)| *u == l).copied());
        if let Some((l, place)) = lead {
            self.take_place(l, place, facing, exact);
        }
        let route = lead
            .map(|(l, _)| self.units[l].path.clone())
            .unwrap_or_default();
        for (u, place) in places {
            if !self.units[u].alive() || lead.is_some_and(|(l, _)| l == u) {
                continue;
            }
            match (lead, route.is_empty()) {
                (Some((l, _)), false) => self.take_copy(organization, l, u, &route, place),
                _ => self.take_place(u, place, facing, exact),
            }
        }
    }

    /// Sets the member at `u` of the organization at `r` off along a copy
    /// of its leader at `l`'s `route` (next point last), fitted to its
    /// `place` at the end. In formation, each point is moved by where the
    /// member's place is from the leader's, laid along the point's own
    /// heading, and all but the last slid to meet the next leg's line, so
    /// its legs run alongside the leader's; without one, the route is
    /// moved bodily by where the member stands from the leader. A point
    /// it can't stand on is pulled back towards the leader's.
    fn take_copy(&mut self, r: usize, l: usize, u: usize, route: &[[f32; 2]], place: [f32; 2]) {
        let organization = &self.organizations[r];
        let leader = &self.units[l];
        let points: Vec<[f32; 2]> = route.iter().rev().copied().collect();
        let mut headings = Vec::with_capacity(points.len());
        let mut from = leader.position;
        for &p in &points {
            let last = headings.last().copied().unwrap_or(leader.facing);
            headings.push(normalised([p[0] - from[0], p[1] - from[1]]).unwrap_or(last));
            from = p;
        }
        let broken = organization.broken_places.len() == organization.units.len()
            && (organization.routing() || organization.chasing());
        let unit = &self.units[u];
        let mut moved: Vec<[f32; 2]> = match broken {
            true => {
                let [dx, dy] = [
                    unit.position[0] - leader.position[0],
                    unit.position[1] - leader.position[1],
                ];
                points.iter().map(|p| [p[0] + dx, p[1] + dy]).collect()
            }
            false => {
                let offsets = &organization.formations[organization.current_formation()].offsets;
                let offset = |m: usize| {
                    let p = organization.units.iter().position(|&x| x == m);
                    p.and_then(|p| offsets.get(p))
                        .copied()
                        .unwrap_or([0.0, 0.0])
                };
                let ([mx, my], [lx, ly]) = (offset(u), offset(l));
                let local = [mx - lx, my - ly];
                let mut moved: Vec<[f32; 2]> = (points.iter().zip(&headings))
                    .map(|(&p, &h)| tgmap::from_local(p, h, local))
                    .collect();
                for i in 0..moved.len().saturating_sub(1) {
                    let (a, h) = (moved[i], headings[i]);
                    let (b, k) = (moved[i + 1], headings[i + 1]);
                    let cross = h[0] * k[1] - h[1] * k[0];
                    if cross.abs() > 1e-4 {
                        let t = ((b[0] - a[0]) * k[1] - (b[1] - a[1]) * k[0]) / cross;
                        moved[i] = [a[0] + h[0] * t, a[1] + h[1] * t];
                    }
                }
                moved
            }
        };
        if let Some(end) = moved.last_mut() {
            *end = place;
        }
        // Each point it can't go straight to from the one before is
        // pulled back towards the leader's; with none to be had, it finds
        // its own way.
        let mut at = unit.position;
        for (point, &towards) in moved.iter_mut().zip(&points) {
            let Some(reached) = self.reachable(u, at, *point, towards) else {
                let facing = self.organizations[r].facing;
                self.take_place(u, place, facing, false);
                return;
            };
            *point = reached;
            at = reached;
        }
        let first = (moved.first().copied()).zip(headings.first().copied());
        moved.reverse();
        let facing = self.organizations[r].facing;
        let unit = &mut self.units[u];
        unit.take(moved, false);
        // Its first leg holds the leader's heading, not the way to it.
        if let Some(([x, y], [hx, hy])) = first {
            unit.from = [x - hx, y - hy];
        }
        unit.closing = None;
        unit.end_facing = facing;
    }

    /// `point`, or failing that the first spot back towards `towards`, a
    /// step at a time, that the unit at `u` can drive to straight from
    /// `from`; `None` if not even `towards` is.
    fn reachable(
        &self,
        u: usize,
        from: [f32; 2],
        point: [f32; 2],
        towards: [f32; 2],
    ) -> Option<[f32; 2]> {
        let domain = self.units[u].domain;
        let apart = crate::distance(point, towards);
        let steps = (apart / PULL_BACK).ceil() as u32;
        let back = |s: u32| match s < steps {
            true => {
                let t = s as f32 * PULL_BACK / apart;
                [
                    point[0] + (towards[0] - point[0]) * t,
                    point[1] + (towards[1] - point[1]) * t,
                ]
            }
            false => towards,
        };
        (0..=steps)
            .map(back)
            .find(|&p| path::clear(&self.pathing, domain, from, p))
    }

    /// Where each of an organization's members has its place, by unit.
    pub(crate) fn formation_places(&self, organization: usize) -> Vec<(usize, [f32; 2])> {
        let r = &self.organizations[organization];
        let facing = r.facing;
        let [x, y] = r.position;
        let broken = (r.routing() || r.chasing()) && r.broken_places.len() == r.units.len();
        match broken {
            // Without a formation each keeps where it stood from the
            // leader, turning with nothing.
            true => (r.units.iter())
                .zip(&r.broken_places)
                .map(|(&u, &[dx, dy])| (u, [x + dx, y + dy]))
                .collect(),
            false => (r.units.iter())
                .zip(&r.formations[r.current_formation()].offsets)
                .map(|(&u, &offset)| (u, tgmap::from_local(r.position, facing, offset)))
                .collect(),
        }
    }

    /// Sends a unit to stand at `place` facing `facing`.
    pub(crate) fn take_place(&mut self, u: usize, place: [f32; 2], facing: [f32; 2], exact: bool) {
        let way = self.plan(u, place, exact);
        let unit = &mut self.units[u];
        unit.take(way, exact);
        unit.closing = None;
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

    /// What the member at `u` of the organization at `r`, heading for its
    /// place along its copy of the route, stretches its steps by while its
    /// leader drives at the `regiment`'s speed: the pace that brings it
    /// back to its place, half the way it's out of place each second, or
    /// on a detour the faster the further behind it is; never more than
    /// [`Rules::catchup_speed`](crate::Rules) times its `own` top speed.
    /// Nothing for one that's well ahead of its place.
    fn place_pace(&self, r: usize, u: usize, regiment: f32, own: f32) -> f32 {
        let organization = &self.organizations[r];
        let Some(lp) = self.pacer(r) else {
            return 1.0;
        };
        let leader = &self.units[organization.units[lp]];
        let p = organization.units.iter().position(|&m| m == u);
        let Some(p) = p.filter(|&p| p != lp && regiment > 0.0) else {
            return 1.0;
        };
        let unit = &self.units[u];
        let most = self.rules.catchup_speed * own;
        if unit.detour {
            let behind = unit.route_left() - leader.route_left();
            let share = (behind / DETOUR_CATCHING_UP).clamp(0.0, 1.0);
            return (regiment + (most - regiment).max(0.0) * share) / regiment;
        }
        let along = leader.leg.map_or(leader.facing, |l| l.heading);
        let broken = organization.broken_places.len() == organization.units.len()
            && (organization.routing() || organization.chasing());
        let local = match broken {
            // Without a formation each keeps where it stood from the leader.
            true => {
                let ([px, py], [lx, ly]) = (
                    organization.broken_places[p],
                    organization.broken_places[lp],
                );
                tgmap::to_local([0.0, 0.0], along, [px - lx, py - ly])
            }
            false => {
                let offsets = &organization.formations[organization.current_formation()].offsets;
                let offset = |p: usize| offsets.get(p).copied().unwrap_or([0.0, 0.0]);
                let ([px, py], [lx, ly]) = (offset(p), offset(lp));
                [px - lx, py - ly]
            }
        };
        let (left, leads) = (unit.path.len(), leader.path.len());
        let ahead = match left == leads {
            true => {
                let place = tgmap::from_local(leader.position, along, local);
                let [hx, hy] = unit.leg.map_or(along, |l| l.heading);
                let mut d = (place[0] - unit.position[0]) * hx + (place[1] - unit.position[1]) * hy;
                // Round a corner, the outer file has further to go.
                if let (Some(&next), Some(&after)) =
                    (unit.path.last(), unit.path.iter().rev().nth(1))
                {
                    let near = crate::distance(unit.position, next);
                    let onward = normalised([after[0] - next[0], after[1] - next[1]]);
                    if let (true, Some([nx, ny])) = (near < CORNER_REACH, onward) {
                        let turn = (hx * ny - hy * nx).signum();
                        d += local[0] * turn * (1.0 - near / CORNER_REACH);
                    }
                }
                d
            }
            false => {
                let d = unit.route_left() - leader.route_left();
                let behind = left as f32 - leads as f32;
                match d.signum() == behind.signum() {
                    true => d,
                    false => behind / 2.0,
                }
            }
        };
        let pace = (regiment + ahead / 2.0).min(most);
        match pace <= WAITING_PACE {
            true => 0.0,
            false => pace / regiment,
        }
    }

    /// The place in the organization at `r` of the member the rest keep
    /// their places from: its leader, or while it has none present, the
    /// first of its members that is.
    pub(crate) fn pacer(&self, r: usize) -> Option<usize> {
        let organization = &self.organizations[r];
        let present = |&p: &usize| self.units[organization.units[p]].alive();
        (organization.leader.filter(present))
            .or_else(|| (0..organization.units.len()).find(present))
    }

    /// Has each regiment's member on its way to its place look, on its
    /// beat, at how far out of place it is while its leader drives, and
    /// keep the pace that brings it back. With the leader standing, or
    /// going after a target of its own, it goes at the regiment's speed:
    /// its place from the leader is then no guide to the place it's
    /// making for.
    fn keep_places(&mut self, tops: &[f32], handed: &[f32]) {
        for u in 0..self.units.len() {
            let unit = &self.units[u];
            if !unit.alive() || unit.goal.is_none() || unit.closing.is_some() {
                self.units[u].pace = 1.0;
                continue;
            }
            if !(self.ticks + u as u64).is_multiple_of(u64::from(LOOK)) {
                continue;
            }
            let r = unit.organization;
            let organization = &self.organizations[r];
            let driving = (self.pacer(r)).is_some_and(|lp| {
                let leader = &self.units[organization.units[lp]];
                leader.moving() && leader.closing.is_none()
            });
            let pace = match driving {
                true => self.place_pace(r, u, tops[r], self.top_speed(u, handed[r])),
                false => 1.0,
            };
            self.units[u].pace = pace;
        }
    }

    /// Moves every unit on along its path. One that's run out of path
    /// short of where it was to stop finds another way from where it is,
    /// until it's tried enough; once done it faces as it was told to.
    /// Regiments' members keep their places on the way.
    pub(crate) fn travel(&mut self) {
        // One waiting for the rest of its group stands.
        let tops: Vec<f32> = (0..self.organizations.len())
            .map(|r| match self.waiting_for_group(r) {
                true => 0.0,
                false => self.regiment_speed(r),
            })
            .collect();
        let handed: Vec<f32> = (0..self.organizations.len())
            .map(|r| self.handed_down_speed(r))
            .collect();
        self.keep_places(&tops, &handed);
        for u in 0..self.units.len() {
            let limit = self.speed_limit(u).unwrap_or(f32::INFINITY);
            let own = self.top_speed(u, handed[self.units[u].organization]);
            let unit = &mut self.units[u];
            unit.previous = unit.position;
            unit.drive(tops[unit.organization].min(limit), own);
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

    use tgmap::{Domain, Obstacle, Pathing, Shape, Tile};

    use crate::{ARRIVAL_REACH, Command, Sides, Unit, World, within};

    #[test]
    fn units_stood_somewhere_afresh_keep_nothing_of_what_they_were_doing() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.apply(
            7,
            Command::Move {
                organization: 0,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        let u = &mut w.units[0];
        u.target = Some(crate::Target::Unit(1));
        u.shot = Some((3, 0));
        u.closing = Some(1.0);
        u.stand_at([4.0, 6.0], [1.0, 0.0]);
        assert!(!u.moving() && u.speed == 0.0 && u.closing.is_none());
        assert_eq!((u.position, u.previous), ([4.0, 6.0], [4.0, 6.0]));
        assert_eq!((u.target, u.shot), (None, None));
        // It stays there, facing as it was stood.
        run(&mut w, 1.0);
        assert_eq!(w.units[0].position, [4.0, 6.0]);
        assert_eq!(w.units[0].facing, [1.0, 0.0]);
    }

    #[test]
    fn organizations_move_at_their_slowest_members_speed() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.apply(
            7,
            Command::Move {
                organization: 0,
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
        m.fighter = Some(tgmap::Fighter {
            health: 200.0,
            ..tgmap::Fighter::default()
        });
        m.health = 0.25;
        let organization = tgmap::Organization {
            hq: None,
            id: 2,
            template: "soldier".into(),
            owner: 7,
            facing: [0.0, 1.0],
            members: vec![m],
            formations: vec![tgmap::Formation {
                id: String::new(),
                offsets: vec![[0.0, 0.0]],
                aggressive: false,
            }],
            formation: 0,
            combat: None,
            operational_range: None,
            control_range: None,
            primary: Some(0),
            morale: 1.0,
            experience: 0.0,
            efficiency: None,
            unpacks: None,
        };
        let w = World::from_organizations(
            Sides::default(),
            Pathing::uniform(20, Domain::Land),
            &[organization],
        );
        assert_eq!(w.units[0].health, 50.0);
    }

    fn speeding(amount: f32) -> tgmap::CombatEffect {
        tgmap::CombatEffect {
            speed: amount,
            ..Default::default()
        }
    }

    #[test]
    fn a_regiment_moves_at_its_formations_and_its_fights_speed() {
        let mut w = battle(
            &[1, 2],
            vec![soldier(Vec::new()), soldier(Vec::new())],
            at_war(),
        );
        assert_eq!(w.regiment_speed(0), 2.0);
        w.rules
            .formation_effects
            .insert("line".into(), speeding(0.25));
        assert_eq!(w.regiment_speed(0), 2.5);
        // Engaged it adds its state's; targeted, without engaging back,
        // the other's.
        w.rules
            .combat_effects
            .insert(tgmap::ENGAGED.into(), speeding(0.25));
        w.rules
            .combat_effects
            .insert(tgmap::TARGETED.into(), speeding(-0.1));
        w.organizations[1]
            .engaged
            .push(crate::Piece::Organization(0));
        assert!((w.regiment_speed(0) - 2.3).abs() < 1e-5);
        w.organizations[0]
            .engaged
            .push(crate::Piece::Organization(1));
        assert_eq!(w.regiment_speed(0), 3.0);
        // The amounts add, never below a twentieth.
        w.rules
            .formation_effects
            .insert("line".into(), speeding(-5.0));
        assert!((w.regiment_speed(0) - 0.1).abs() < 1e-5);
    }

    #[test]
    fn a_regiment_keeps_to_its_slowest_member_as_its_properties_have_it() {
        let mut w = battle(&[1], vec![soldier(Vec::new())], at_war());
        let mut tank = w.units[0].clone();
        tank.template = "tank".into();
        tank.speed_max = 1.0;
        w.units.push(tank);
        w.organizations[0].units.push(1);
        assert_eq!(w.regiment_speed(0), 1.0);
        // A technology on tanks speeds up only the tank.
        let mut engines = speeding(3.0);
        engines.requires = vec!["tank".into()];
        w.rules.combat_effects.insert("engines".into(), engines);
        let kind = |p: &str| tgmap::Kind {
            properties: vec![p.into()],
            flags: Vec::new(),
            soft: tgmap::Shape::Point,
        };
        w.rules.kinds.insert("tank".into(), kind("tank"));
        w.technologies.insert(1, vec!["engines".into()]);
        assert_eq!(w.regiment_speed(0), 2.0);
        // Once the slowest is gone the rest go at their own.
        w.units[0].die(0);
        assert_eq!(w.regiment_speed(0), 4.0);
    }

    #[test]
    fn units_speed_up_to_their_top_speed_and_stop_at_once() {
        let mut w = vehicle(2.0, 0.0, None);
        w.apply(
            7,
            Command::Move {
                organization: 0,
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
                organization: 0,
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
    fn units_turn_faster_as_their_top_speed_rises() {
        let mut w = vehicle(0.0, 1.0, None);
        let faster = tgmap::CombatEffect {
            speed: 1.0,
            ..Default::default()
        };
        w.rules.formation_effects.insert("lone".into(), faster);
        w.apply(
            7,
            Command::Move {
                organization: 0,
                to: [20.0, 10.0],
            },
        )
        .unwrap();
        w.tick();
        // Twice as fast, it turns twice as far, keeping its circle.
        let (sin, cos) = libm::sincosf(0.2);
        assert!(
            near(w.units[0].facing, [sin, cos]),
            "{:?}",
            w.units[0].facing
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
            w.apply(
                7,
                Command::Move {
                    organization: 0,
                    to,
                },
            )
            .unwrap();
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
        let trailer = tgmap::Trailer {
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
                organization: 0,
                to: [20.0, 10.0],
            },
        )
        .unwrap();
        w.tick();
        // The truck turns to +x at once and goes 0.2: the hitch swings out
        // behind it to 8.2,10 and the trailer swings round after it.
        let t = w.units[0].trailer.as_ref().unwrap();
        let hitch = [8.2, 10.0];
        let coupled = tgmap::from_local(t.position, t.facing, [0.0, 1.0]);
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
                organization: 0,
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

    /// The two soldiers sent up to 10,30, the slower right-hand man,
    /// whose speed is the regiment's, standing at `at`.
    fn moved_with_the_right_hand_man_at(at: [f32; 2]) -> World {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.units[1].position = at;
        let to = [10.0, 30.0];
        w.apply(
            7,
            Command::Move {
                organization: 0,
                to,
            },
        )
        .unwrap();
        w
    }

    #[test]
    fn a_member_behind_its_place_catches_up_with_it() {
        let mut w = moved_with_the_right_hand_man_at([11.0, 6.0]);
        run(&mut w, 6.0);
        // At the regiment's speed of 2 it would still be 4 behind; it
        // closes in faster, up to 1.75 times its own top speed.
        let (leader, behind) = (w.units[0].position[1], w.units[1].position[1]);
        assert!(leader - behind < 1.5, "{leader} {behind}");
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [9.0, 30.0]));
        assert!(near(w.units[1].position, [11.0, 30.0]));
    }

    #[test]
    fn a_member_well_ahead_of_its_place_waits_for_it() {
        let mut w = moved_with_the_right_hand_man_at([11.0, 18.0]);
        // Once it's looked, its place, beside the leader, is well behind
        // it, so it stands.
        run(&mut w, 1.5);
        let stood = w.units[1].position;
        run(&mut w, 1.0);
        assert!(near(w.units[1].position, stood));
        assert!(w.units[1].moving());
        run(&mut w, 15.0);
        assert!(near(w.units[1].position, [11.0, 30.0]));
    }

    #[test]
    fn a_member_catches_up_no_faster_than_its_catchup_speed() {
        let mut w = moved_with_the_right_hand_man_at([11.0, 1.0]);
        w.rules.catchup_speed = 1.25;
        run(&mut w, 1.2);
        let from = w.units[1].position[1];
        run(&mut w, 1.0);
        // Far behind, it would ask for far more than 1.25 × its 2.
        let went = w.units[1].position[1] - from;
        assert!((went - 2.5).abs() < 0.05, "{went}");
    }

    #[test]
    fn members_follow_the_leaders_route_alongside_it_meeting_its_corners() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.organizations[0].travelling = true;
        // North to 9,30, then east to 29,30: a right turn.
        let route = vec![[29.0, 30.0], [9.0, 30.0]];
        w.take_copy(0, 0, 1, &route, [29.0, 28.0]);
        // 2 to the leader's right, it turns inside the corner, before it.
        let unit = &w.units[1];
        assert_eq!(unit.path.len(), 2);
        assert!(near(unit.path[1], [11.0, 28.0]), "{:?}", unit.path);
        assert!(near(unit.path[0], [29.0, 28.0]));
        // Its first leg holds the leader's heading, north.
        let [fx, fy] = [
            unit.path[1][0] - unit.from[0],
            unit.path[1][1] - unit.from[1],
        ];
        assert!(fx.abs() < 1e-4 && fy > 0.0);
    }

    #[test]
    fn a_member_whose_copy_is_walled_off_finds_its_own_way() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        // A wall across the member's line north, but not the leader's.
        for x in 5..8 {
            pathing.set_blocked(Tile::new(x, 10));
        }
        let mut w = world(pathing);
        w.organizations[0].travelling = true;
        let route = vec![[9.0, 30.0]];
        w.take_copy(0, 0, 1, &route, [11.0, 30.0]);
        // Pulled back to beside the leader's route, or round on its own.
        let unit = &w.units[1];
        let straight = unit.path.len() == 1 && unit.from[0] > 10.5;
        assert!(!straight, "{:?}", unit.path);
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
                organization: 0,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(!near(w.units[1].position, [11.0, 20.0]));
        w.apply(
            7,
            Command::Move {
                organization: 0,
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
                organization: 0,
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
                organization: 0,
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
                organization: 0,
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
                organization: 0,
                to: [10.0, 30.0],
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [9.0, 30.0]));
        assert!(w.units[1].position[1] < 12.0);
    }
}
