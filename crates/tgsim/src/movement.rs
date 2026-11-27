//! Units moving: finding their way, driving each leg and keeping their
//! places in their organization's formation.

use std::f32::consts::FRAC_PI_4;

use tgmap::pathing::CELL_SIZE;
use tgmap::{Id, Obstacle, Shape, Tile};

use crate::value::MULTIPLY_FLOOR;

use crate::{
    ARRIVAL_REACH, Banner, EXACT_REACH, LOOK, Leg, NotYours, Order, TICK, Trailer, Unit, World,
    next_chance, normalised, path, within, within_strictly,
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

/// How near the end of the leg it's driving a unit has to come for the leg
/// to end there and then.
const AT_AIM: f32 = 0.01;

/// How near the point after a route's next one has to be for the corner
/// there to be taken as the one after it.
const SAME_POINT: f32 = 0.0002;

/// The least speed a regiment hands its members that has them move.
const REGIMENT_SPEED_MIN: f32 = 0.1;

/// How far to the side of its leg a unit gets before making for it again.
const OFF_LINE: f32 = 1.0;

/// The fewest and most ticks between a driving unit's looks at its move,
/// and on a leg it's driving straight for a spot of its own.
const LOOKS: [u32; 2] = [10, 13];
const LOOKS_STRAIGHT: [u32; 2] = [3, 6];

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
        let towed =
            (trailer.trailer.as_deref()).map(|t| Box::new(Trailer::new(t, position, facing)));
        Trailer {
            template: trailer.template.clone(),
            position,
            previous: position,
            facing,
            hitch: trailer.hitch,
            coupling: trailer.coupling,
            trailer: towed,
        }
    }

    /// Stands it straight behind its unit, and what it tows behind it.
    pub(crate) fn behind(&mut self, position: [f32; 2], facing: [f32; 2]) {
        let hitch = tgmap::from_local(position, facing, self.hitch);
        let [x, y] = self.coupling;
        self.position = tgmap::from_local(hitch, facing, [-x, -y]);
        self.previous = self.position;
        self.facing = facing;
        if let Some(t) = &mut self.trailer {
            t.behind(self.position, facing);
        }
    }

    /// Turns to point at its unit's hitch, and moves so that its coupling
    /// meets it, what it tows following it in turn.
    fn follow(&mut self, position: [f32; 2], facing: [f32; 2]) {
        self.previous = self.position;
        let hitch = tgmap::from_local(position, facing, self.hitch);
        let to = [hitch[0] - self.position[0], hitch[1] - self.position[1]];
        self.facing = normalised(to).unwrap_or(self.facing);
        let [x, y] = self.coupling;
        self.position = tgmap::from_local(hitch, self.facing, [-x, -y]);
        if let Some(t) = &mut self.trailer {
            t.follow(self.position, self.facing);
        }
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

    /// Whether it's in the air: flying, falling under its parachute or
    /// falling shot down, alive or not.
    pub fn in_air(&self) -> bool {
        self.aloft || self.descent.is_some()
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

    /// How far it has left to go along its leader's route: to the leader's
    /// point its next waypoint was made from, and on along the leader's
    /// points from there; on a way of its own, along its path.
    fn left_along_leaders(&self) -> f32 {
        if self.sources.is_empty() {
            return self.route_left();
        }
        let mut at = self.position;
        let mut left = 0.0;
        for &p in self.sources.iter().rev() {
            left += crate::distance(at, p);
            at = p;
        }
        left
    }

    /// Sets it off along `path`, next waypoint last, to stop at its end.
    pub(crate) fn take(&mut self, path: Vec<[f32; 2]>, exact: bool) {
        // Being sent somewhere is a new order, which bombarding gives way to.
        self.shelling = None;
        self.goal = path.first().copied();
        self.path = path;
        self.from = self.position;
        self.leg = None;
        self.exact = exact;
        self.ways = 1;
        self.look = 0;
        self.pace = 1.0;
        self.detour = false;
        self.straight = false;
        self.stage = false;
        self.sources.clear();
        self.returning = false;
    }

    /// Sets it off along `route` to stop at `goal`, which is where it's
    /// judged to have got to, wherever the route ends.
    fn take_route(&mut self, route: Route, goal: [f32; 2], exact: bool) {
        self.take(route.points, exact);
        self.goal = Some(goal);
        (self.stage, self.straight) = (route.stage, route.straight);
    }

    /// Passes its next waypoint.
    fn pass(&mut self) {
        if let Some(next) = self.path.pop() {
            self.from = next;
        }
        self.sources.truncate(self.path.len());
    }

    /// Stops it where it is, with nowhere left to go.
    pub(crate) fn halt(&mut self) {
        self.path.clear();
        self.leg = None;
        self.goal = None;
        self.pace = 1.0;
        self.detour = false;
        self.straight = false;
        self.stage = false;
        self.sources.clear();
    }

    /// Stands it at `at` facing `facing` as if just made: still, with
    /// nowhere to go, nothing fired on and its weapons ready.
    pub(crate) fn stand_at(&mut self, at: [f32; 2], facing: [f32; 2]) {
        self.position = at;
        self.previous = at;
        self.facing = facing;
        self.end_facing = facing;
        self.halt();
        self.with = None;
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

    /// The way on from its next waypoint, along the leg after it: past a
    /// point lying on that waypoint, along the leg after that. None with
    /// no leg after it.
    fn onward(&self) -> Option<[f32; 2]> {
        let n = self.path.len();
        let next = *self.path.last()?;
        let after = self.path[n.checked_sub(2)?];
        let (from, to) = match within(next, after, SAME_POINT) {
            true => (after, self.path[n.checked_sub(3)?]),
            false => (next, after),
        };
        normalised([to[0] - from[0], to[1] - from[1]])
    }

    /// Whether it's driving straight for a spot of its own: a destination
    /// close by, or the dash of a detour.
    fn going_straight(&self) -> bool {
        self.straight || self.detour
    }

    /// Takes up the leg to its next waypoint, passing over the waypoints
    /// it's already level with or past, or within `reach` of. The leg ends
    /// short of a corner by the room it takes to turn it.
    fn take_leg(&mut self, reach: f32) {
        while self.leg.is_none() {
            let Some(&next) = self.path.last() else {
                return;
            };
            let along = [next[0] - self.from[0], next[1] - self.from[1]];
            let heading = normalised(along).unwrap_or(self.facing);
            let lead = match self.onward() {
                Some(onward) if self.turn_radius > 0.0 && !self.going_straight() => {
                    let cos = heading[0] * onward[0] + heading[1] * onward[1];
                    let sin = (heading[0] * onward[1] - heading[1] * onward[0]).abs();
                    // The tangent of half the angle between the two legs.
                    self.turn_radius * sin / (1.0 + cos)
                }
                _ => 0.0,
            };
            let aim = [next[0] - heading[0] * lead, next[1] - heading[1] * lead];
            let left =
                (aim[0] - self.position[0]) * heading[0] + (aim[1] - self.position[1]) * heading[1];
            if left <= 0.0 || within(self.position, aim, reach) {
                self.pass();
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
    /// radius, so at that speed it keeps to its turning circle. Whether
    /// it's time it looked at how its move is going.
    fn drive(&mut self, top: f32, own: f32, reach: f32) -> bool {
        self.take_leg(reach);
        let Some(leg) = self.leg.filter(|_| !self.path.is_empty()) else {
            self.speed = 0.0;
            return false;
        };
        if self.pace <= 0.0 {
            self.speed = 0.0;
            return false;
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
        if ends || within(self.position, leg.aim, AT_AIM) {
            self.pass();
            self.leg = None;
            self.detour = false;
            if self.path.is_empty() {
                self.speed = 0.0;
            }
            return false;
        }
        self.look = self.look.saturating_sub(1);
        self.look == 0
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

/// A way planned for a unit, next waypoint last.
#[derive(Debug, Clone, Default)]
pub(crate) struct Route {
    pub(crate) points: Vec<[f32; 2]>,
    /// Whether it's only the first stage of a way too long to plan at once.
    stage: bool,
    /// Whether it's a single leg straight for a destination close by.
    straight: bool,
}

impl World {
    /// The state the organization at `r` holds for its part in a fight, as
    /// the data names it: engaged, or targeted while another engages it
    /// and it doesn't engage back.
    pub fn fight_state(&self, r: usize) -> Option<&'static str> {
        let organization = &self.organizations[r];
        match organization.engaged.is_empty() {
            false => Some(tgmap::ENGAGED),
            true => organization.targeted.then_some(tgmap::TARGETED),
        }
    }

    /// Has each organization check whether it's targeted, once a second
    /// and whenever what it's doing changes.
    pub(crate) fn keep_targeted(&mut self) {
        for r in 0..self.organizations.len() {
            let doing = self.activity(r);
            let organization = &self.organizations[r];
            let beat = (self.ticks + r as u64).is_multiple_of(crate::SECOND);
            if !beat && organization.checked_as == Some(doing) {
                continue;
            }
            let targeted = organization.engaged.is_empty() && self.engagers(r).next().is_some();
            let organization = &mut self.organizations[r];
            (organization.targeted, organization.checked_as) = (targeted, Some(doing));
        }
    }

    /// What the organization at `r` adds to the top speed of its member
    /// at `u` past the member's own properties: its formation's, and its
    /// state's in a fight, each if the member is of the kind it needs.
    fn handed_down_speed(&self, r: usize, u: usize) -> f32 {
        let formation = self.formation_effect(r);
        let state = (self.fight_state(r)).and_then(|p| self.rules.combat_effects.get(&Id::from(p)));
        (formation.into_iter().chain(state))
            .filter(|e| self.qualifies(&self.units[u].template, &e.requires))
            .map(|e| e.speed)
            .sum()
    }

    /// The unit at `u`'s top speed: its own, times what its properties and
    /// the `handed` down from its organization make of it.
    pub(crate) fn top_speed(&self, u: usize, handed: f32) -> f32 {
        let factor = 1.0 + self.boost(u).speed + handed;
        self.units[u].speed_max * factor.max(MULTIPLY_FLOOR)
    }

    /// How fast the organization at `r` moves: as fast as its slowest
    /// member present can.
    pub(crate) fn regiment_speed(&self, r: usize) -> f32 {
        let own = self.unhurried_speed(r);
        // Its group's and its order's, which only ever slow it.
        let r = &self.organizations[r];
        [r.group_speed, r.ordered_speed]
            .into_iter()
            .flatten()
            .fold(own, f32::min)
    }

    /// How fast the unit at `u` can go now: its top speed, with what its
    /// organization hands down.
    pub fn unit_speed(&self, u: usize) -> f32 {
        self.top_speed(u, self.handed_down_speed(self.units[u].organization, u))
    }

    /// How fast the organization at `r` moves left to itself: as fast as
    /// its slowest member present can, whatever its group would have.
    pub fn unhurried_speed(&self, r: usize) -> f32 {
        let present = (self.organizations[r].units.iter()).filter(|&&u| self.units[u].alive());
        let slowest = (present.map(|&u| self.unit_speed(u))).fold(f32::INFINITY, f32::min);
        match slowest.is_finite() {
            true => slowest,
            false => self.organizations[r].speed,
        }
    }

    /// Sends the organization at `organization` to stand at `target`
    /// facing `facing`, which it turns to once there.
    pub(crate) fn send(&mut self, organization: usize, target: [f32; 2], facing: [f32; 2]) {
        self.set_off(organization, target, facing, true);
    }

    /// Sends the organization at `organization` to stand at `target`,
    /// facing the way it gets there.
    pub(crate) fn send_along(&mut self, organization: usize, target: [f32; 2]) {
        let r = &self.organizations[organization];
        let here = self.centre(organization);
        let off = [target[0] - here[0], target[1] - here[1]];
        let facing = normalised(off).unwrap_or(r.facing);
        self.set_off(organization, target, facing, false);
    }

    /// Any move unsettles an organization; one further than
    /// [`SETTING_OFF`] sets it off along a route, which ends its
    /// entrenchment. It faces `facing` until it has a route to face along,
    /// and turns to it once there if `ordered`.
    fn set_off(&mut self, organization: usize, target: [f32; 2], facing: [f32; 2], ordered: bool) {
        // What flies goes where its delivery takes it.
        if self.flown(organization) {
            return;
        }
        let far = !within(
            self.organizations[organization].position,
            target,
            SETTING_OFF,
        );
        let leader = self
            .pacer(organization)
            .map(|p| self.organizations[organization].units[p]);
        if far && leader.is_some_and(|l| self.no_route(l, target)) {
            self.stay(organization, ordered.then_some(facing));
            return;
        }
        let r = &mut self.organizations[organization];
        let on_its_way = r.route_end.is_some();
        r.settled = None;
        if far {
            r.travelling = true;
            r.entrenched = None;
        }
        r.position = target;
        r.facing = facing;
        r.turn_to = ordered.then_some(facing);
        (r.tries, r.sent, r.given_speed) = (0, self.ticks, f32::INFINITY);
        // One already on its way hands out the new route at its next look.
        match on_its_way && far {
            true => r.hand_out = true,
            false => self.take_places(organization),
        }
    }

    /// Leaves the organization at `r` no longer on its way along a route.
    pub(crate) fn drop_route(&mut self, r: usize) {
        let organization = &mut self.organizations[r];
        organization.route_end = None;
        organization.route.clear();
        organization.heading = None;
        organization.hand_out = false;
    }

    /// Has the organization at `organization`, whose move found no route
    /// or was given up, drop the move and rest where it stands, turned to
    /// the `facing` the move was given, if any. Its members make for their
    /// places round there, those already in them staying put.
    fn stay(&mut self, organization: usize, facing: Option<[f32; 2]>) {
        let here = self.standing_point(organization);
        self.drop_route(organization);
        let r = &mut self.organizations[organization];
        r.position = here;
        r.travelling = false;
        r.turn_to = None;
        r.ordered_speed = None;
        // An attack on the way to its target is over.
        if r.order == Order::Attack {
            r.order = Order::Move;
        }
        if let Some(facing) = facing {
            r.facing = facing;
        }
        // Those in place stay put, turning only to a way the move was to
        // face, as they would on arriving.
        self.rest_places(organization, facing.is_some());
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
                match r.route_end.is_some() {
                    true => r.hand_out = true,
                    false => self.take_places(organization),
                }
            }
        }
        Ok(())
    }

    /// Sends each of an organization's members to its place in the
    /// formation. One setting off along a route hands its leader the way
    /// to its place, facing along the route's last leg once there, and
    /// each other member a copy of it fitted to its place; on the way it
    /// faces along the leg its leader is on.
    pub(crate) fn take_places(&mut self, organization: usize) {
        self.organizations[organization].hand_out = false;
        let r = &self.organizations[organization];
        // A move to unpack has to end right there.
        let exact = r.unpack_at.is_some();
        let leader = (self.pacer(organization))
            .map(|p| r.units[p])
            .filter(|_| r.travelling);
        let lone = self.lone(organization);
        let mut places = self.destination_places(organization);
        let lead = leader.and_then(|l| places.iter().find(|(u, _)| *u == l).copied());
        // As is one already near where it's going.
        let near =
            lead.is_some_and(|(l, place)| within(self.units[l].position, place, ARRIVAL_REACH));
        let exact = exact || near;
        let route = lead
            .and_then(|(l, place)| self.plan(l, place, exact, true))
            .unwrap_or_default();
        // Closed in where it stands, it goes nowhere until it gives up,
        // nor do the rest.
        let held = lead.is_some() && route.points.is_empty() && !near;
        // A route that turns ends facing along its last leg, its places
        // turned with it. The leader's own place is the regiment's
        // position, whichever way it faces.
        if let &[end, before, ..] = &route.points[..]
            && let Some(facing) = normalised([end[0] - before[0], end[1] - before[1]])
        {
            self.organizations[organization].facing = facing;
            places = self.destination_places(organization);
        }
        let first_leg = (lead.zip(route.points.last())).and_then(|((l, _), &next)| {
            let at = self.units[l].position;
            normalised([next[0] - at[0], next[1] - at[1]])
        });
        let r = &mut self.organizations[organization];
        let end = route.points.first().copied();
        r.route_end = end.or(lead.filter(|_| held).map(|(_, place)| place));
        r.route.clone_from(&route.points);
        r.heading = first_leg.or(end.map(|_| r.facing));
        r.stage = route.stage;
        r.resend_places = false;
        // Each plan's a try, but for one moved onto something.
        if !route.points.is_empty() && r.follow.is_none() {
            r.tries = r.tries.saturating_add(1);
        }
        let facing = r.facing;
        if let Some((l, place)) = lead {
            // A regiment's leader stops where its route ends, and the
            // regiment sends it again from there; one on its own makes for
            // the spot itself.
            match (end, lone) {
                (Some(end), _) => {
                    let goal = if lone { place } else { end };
                    let unit = &mut self.units[l];
                    unit.take_route(route.clone(), goal, exact);
                    unit.closing = None;
                    unit.end_facing = facing;
                }
                (None, _) if held => {
                    self.units[l].halt();
                    return;
                }
                (None, _) => self.take_place(l, place, facing, exact),
            }
        }
        for (u, place) in places {
            if !self.units[u].alive() || lead.is_some_and(|(l, _)| l == u) {
                continue;
            }
            match (lead, route.points.is_empty()) {
                (Some((l, _)), false) => {
                    self.take_copy(organization, l, u, &route.points, (place, exact));
                }
                _ => self.take_place(u, place, facing, exact),
            }
        }
    }

    /// Once its leader has got to the end of its route, turns the
    /// organization at `r` to the way it was sent to face, if any, and
    /// sends each member to its place from where it stands.
    fn end_route(&mut self, r: usize) {
        self.drop_route(r);
        let organization = &mut self.organizations[r];
        organization.ordered_speed = None;
        organization.tries = 0;
        if let Some(facing) = organization.turn_to.take() {
            organization.facing = facing;
        }
        self.rest_places(r, true);
    }

    /// Sends each member of the organization at `r`, come to rest, to its
    /// place, but for those already in theirs, which stay put, turning to
    /// face its way if `turn` and they're in a regiment; those that don't
    /// get there it sends once more.
    fn rest_places(&mut self, r: usize, turn: bool) {
        let organization = &mut self.organizations[r];
        organization.resend_places = true;
        let facing = organization.facing;
        let exact = organization.unpack_at.is_some();
        let turn = turn && !self.lone(r);
        for (u, place) in self.formation_places(r) {
            let unit = &self.units[u];
            if !unit.alive() || unit.closing.is_some() {
                continue;
            }
            match within(unit.position, place, ARRIVAL_REACH) {
                true => {
                    let unit = &mut self.units[u];
                    unit.halt();
                    if turn {
                        unit.facing = facing;
                    }
                    unit.end_facing = facing;
                }
                false => self.take_place(u, place, facing, exact),
            }
        }
    }

    /// Has the organization at `r`, come to rest, send the members that
    /// didn't get to their places once more, once they've all stopped, and
    /// leave them where they are after that.
    fn resend_places(&mut self, r: usize) {
        let organization = &self.organizations[r];
        if !organization.resend_places || organization.resume.is_some() {
            return;
        }
        let units = || organization.units.iter().map(|&u| &self.units[u]);
        if units().any(|u| u.alive() && (u.moving() || u.goal.is_some())) {
            return;
        }
        let facing = organization.facing;
        let exact = organization.unpack_at.is_some();
        self.organizations[r].resend_places = false;
        for (u, place) in self.formation_places(r) {
            let unit = &self.units[u];
            if unit.alive()
                && unit.closing.is_none()
                && !within(unit.position, place, ARRIVAL_REACH)
            {
                self.take_place(u, place, facing, exact);
            }
        }
    }

    /// Has each organization on its way along a route follow its leader:
    /// as the leader passes a point of the route it drops it from its own
    /// copy and turns at once to face along the next leg.
    fn follow_leaders(&mut self) {
        for r in 0..self.organizations.len() {
            if self.organizations[r].route_end.is_none() {
                continue;
            }
            let Some(l) = self.pacer(r).map(|p| self.organizations[r].units[p]) else {
                continue;
            };
            let left = self.units[l].path.len();
            let organization = &mut self.organizations[r];
            while organization.route.len() > left {
                let passed = organization.route.pop();
                if let (Some(a), Some(&b)) = (passed, organization.route.last())
                    && let Some(heading) = normalised([b[0] - a[0], b[1] - a[1]])
                {
                    organization.heading = Some(heading);
                }
            }
        }
    }

    /// Has each organization on its way along a route look, about once a
    /// second: one given an order on the way hands out its new route;
    /// once its own position is where it was sent it arrives; with its
    /// leader stopped short it sends the leader again, giving up the move
    /// once it's tried enough for long enough. Until then, a member that's
    /// joined it, or come back inside its control zone, is handed a copy
    /// of what's left of the route, and one cut off from it makes its way
    /// back. A leader stopped short to fight leaves it no longer on its
    /// way.
    pub(crate) fn keep_routes(&mut self) {
        self.follow_leaders();
        for r in 0..self.organizations.len() {
            if !(self.ticks + r as u64).is_multiple_of(u64::from(LOOK)) {
                continue;
            }
            if self.organizations[r].route_end.is_none() {
                self.resend_places(r);
                continue;
            }
            if self.organizations[r].hand_out {
                self.take_places(r);
                continue;
            }
            let Some(l) = self.pacer(r).map(|p| self.organizations[r].units[p]) else {
                continue;
            };
            if self.units[l].goal.is_none() {
                self.leader_stopped(r, l);
                continue;
            }
            let organization = &self.organizations[r];
            let reach = organization.control(&self.rules.guard_rules);
            let zone = self.pathing.zone(self.units[l].position, reach);
            let exact = organization.unpack_at.is_some();
            let route = organization.route.clone();
            for (u, place) in self.destination_places(r) {
                let unit = &self.units[u];
                if u == l || !unit.alive() || unit.closing.is_some() {
                    continue;
                }
                let inside = zone.holds(unit.position);
                let due = match unit.with {
                    None => true,
                    Some(was) => inside && !was,
                };
                if due && !route.is_empty() {
                    self.take_copy(r, l, u, &route, (place, exact));
                }
                let unit = &mut self.units[u];
                unit.with = Some(inside);
                // One with no zone cuts none off.
                if !inside && reach > 0.0 {
                    unit.returning = true;
                }
            }
        }
    }

    /// Has the organization at `r`, whose leader at `l` has stopped, arrive
    /// if it's where it was sent, or else send the leader again, until
    /// it's tried enough times for long enough.
    fn leader_stopped(&mut self, r: usize, l: usize) {
        let organization = &self.organizations[r];
        let own = self
            .destination_places(r)
            .into_iter()
            .find(|&(u, _)| u == l);
        // Its own position follows its leader's, so it's where it was sent
        // once that is where the leader has its place there.
        let there = own.map_or(organization.position, |(_, place)| place);
        if within(organization.banner.at, there, ARRIVAL_REACH) {
            self.end_route(r);
            return;
        }
        // Stopped short to fight, which takes the move up again after, or
        // gone after something of its own.
        let elsewhere = self.units[l].closing.is_some();
        if organization.resume.is_some() || !organization.engaged.is_empty() || elsewhere {
            self.organizations[r].route_end = None;
            return;
        }
        // One on its own has given up already.
        let alone = self.lone(r);
        // A long way's next stage is tried afresh.
        if organization.stage {
            self.organizations[r].tries = 0;
        }
        let organization = &self.organizations[r];
        let given = (self.ticks - organization.sent) as f32 * TICK;
        let tried = organization.tries >= WAYS_TRIED && given >= self.rules.move_fail_seconds;
        let exact = organization.unpack_at.is_some();
        let route = match tried || alone {
            true => None,
            false => own.and_then(|(_, place)| self.plan(l, place, exact, false)),
        };
        let Some(route) = route else {
            let turn_to = self.organizations[r].turn_to;
            self.stay(r, turn_to);
            return;
        };
        let organization = &mut self.organizations[r];
        organization.tries = organization.tries.saturating_add(1);
        organization.stage = route.stage;
        // Closed in, it stays where it is, trying, until it may give up.
        let Some(&end) = route.points.first() else {
            return;
        };
        let facing = organization.facing;
        organization.route_end = Some(end);
        organization.route.clone_from(&route.points);
        let unit = &mut self.units[l];
        unit.take_route(route, end, exact);
        unit.end_facing = facing;
    }

    /// Where the organization at `r` is: on its way, where its own
    /// position, trailing its leader, has its formation's middle; else
    /// where it stands or was sent.
    pub fn regiment_position(&self, r: usize) -> [f32; 2] {
        let organization = &self.organizations[r];
        if organization.route_end.is_none() {
            return organization.position;
        }
        let at = organization.banner.at;
        let lead = self.pacer(r).filter(|_| !self.unorganized(r));
        let offset = lead.and_then(|p| self.offset(r, p)).unwrap_or([0.0, 0.0]);
        let [x, y] = tgmap::from_local([0.0, 0.0], self.regiment_facing(r), offset);
        [at[0] - x, at[1] - y]
    }

    /// The longest an organization's offset from the middle of several
    /// sent to a place together is cut to.
    pub fn group_zone_range_min(&self) -> f32 {
        self.rules.group_zone_range_min
    }

    /// The way the organization at `r` faces: on its way, along the leg
    /// its leader is on.
    pub fn regiment_facing(&self, r: usize) -> [f32; 2] {
        let organization = &self.organizations[r];
        organization.heading.unwrap_or(organization.facing)
    }

    /// The place at `p` in the formation of the organization at `r`, from
    /// its middle with 0,1 straight ahead.
    fn offset(&self, r: usize, p: usize) -> Option<[f32; 2]> {
        let organization = &self.organizations[r];
        let formation = organization
            .formations
            .get(organization.current_formation())?;
        formation.offsets.get(p).copied()
    }

    /// Sets the member at `u` of the organization at `r` off along a copy
    /// of its leader at `l`'s `route` (next point last), fitted to its
    /// `place` at the end, which it has to stop exactly at if `exact`. In
    /// formation, each point is moved by where the
    /// member's place is from the leader's, laid along the point's own
    /// heading, and all but the last slid to meet the next leg's line, so
    /// its legs run alongside the leader's; without one, the route is
    /// moved bodily by where the member stands from the leader, and one
    /// away from the regiment is sent on its own to the route's first
    /// point, moved by where it last stood from the leader. A point it
    /// can't drive to straight is pulled back towards the leader's.
    fn take_copy(
        &mut self,
        r: usize,
        l: usize,
        u: usize,
        route: &[[f32; 2]],
        (place, exact): ([f32; 2], bool),
    ) {
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
        let unit = &self.units[u];
        let mut moved: Vec<[f32; 2]> = match self.unorganized(r) {
            true => {
                let stood = |m: usize| {
                    let p = organization.units.iter().position(|&x| x == m);
                    p.and_then(|p| organization.broken_places.get(p)).copied()
                };
                let reach = organization.control(&self.rules.guard_rules);
                let away = !self
                    .pathing
                    .zone(leader.position, reach)
                    .holds(unit.position);
                if let (true, Some(&first), Some(a), Some(b)) =
                    (away, points.first(), stood(u), stood(l))
                {
                    let to = [first[0] + a[0] - b[0], first[1] + a[1] - b[1]];
                    let (facing, exact) = (self.regiment_facing(r), exact && points.len() == 1);
                    self.take_place(u, to, facing, exact);
                    self.units[u].with = Some(false);
                    return;
                }
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
        let first = (moved.first().copied()).zip(headings.first().copied());
        moved.reverse();
        let facing = self.organizations[r].facing;
        let unit = &mut self.units[u];
        unit.take(moved, exact);
        // What it can't reach it finds as it drives.
        unit.sources = points.into_iter().rev().collect();
        unit.with = Some(true);
        // Its first leg holds the leader's heading, not the way to it.
        if let Some(([x, y], [hx, hy])) = first {
            unit.from = [x - hx, y - hy];
        }
        unit.closing = None;
        unit.end_facing = facing;
        // Nothing stops it driving into what it can't pass, so it looks
        // the moment it sets off rather than a second on.
        self.check_point(u);
    }

    /// Has the member at `u`, on a copy of its leader's route, look at the
    /// way to its next point: one it can't drive to straight it pulls
    /// back towards the leader's point it was made from, and goes to the
    /// first spot back that it can. With none, the point counts as passed
    /// at once, or for its last, unless it's there, its move fails.
    fn check_point(&mut self, u: usize) {
        let unit = &self.units[u];
        let (Some(&next), Some(&source)) = (unit.path.last(), unit.sources.last()) else {
            return;
        };
        let at = unit.position;
        if path::clear(&self.pathing, unit.domain, at, next) {
            // Held to a heading alongside its line, it makes for the
            // point itself where that heading runs into something.
            if let Some(leg) = unit.leg {
                let [hx, hy] = leg.heading;
                let ahead = ((leg.aim[0] - at[0]) * hx + (leg.aim[1] - at[1]) * hy).max(0.0);
                let end = [at[0] + hx * ahead, at[1] + hy * ahead];
                if !path::clear(&self.pathing, unit.domain, at, end) {
                    let unit = &mut self.units[u];
                    (unit.from, unit.leg) = (at, None);
                }
            }
            return;
        }
        let reached = self.reachable(u, at, next, source);
        let unit = &mut self.units[u];
        if let Some(p) = reached {
            let last = unit.path.len() - 1;
            unit.path[last] = p;
            (unit.from, unit.leg) = (at, None);
            return;
        }
        match unit.path.len() == 1 && !within(at, next, ARRIVAL_REACH) {
            // Failing, it keeps its facing.
            true => unit.halt(),
            false => {
                unit.pass();
                (unit.from, unit.leg) = (at, None);
                // On to the next, which it looks at as it turns to it.
                self.check_point(u);
            }
        }
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

    /// Whether the organization at `r` is without an organized formation,
    /// its members keeping where they stood from the leader as it broke.
    pub(crate) fn unorganized(&self, r: usize) -> bool {
        let r = &self.organizations[r];
        (r.routing() || r.chasing()) && r.broken_places.len() == r.units.len()
    }

    /// Where each of an organization's members has its place, by unit: set
    /// out from where it is and turned the way it faces, which on its way
    /// is where its position trailing its leader is.
    pub(crate) fn formation_places(&self, organization: usize) -> Vec<(usize, [f32; 2])> {
        let at = self.regiment_position(organization);
        self.places_at(organization, at, self.regiment_facing(organization))
    }

    /// Where each of an organization's members has its place once it's
    /// where it's sent, facing the way it will there.
    pub(crate) fn destination_places(&self, organization: usize) -> Vec<(usize, [f32; 2])> {
        let r = &self.organizations[organization];
        self.places_at(organization, r.position, r.facing)
    }

    /// Where each of an organization's members has its place with the
    /// organization at `at` facing `facing`.
    fn places_at(
        &self,
        organization: usize,
        at: [f32; 2],
        facing: [f32; 2],
    ) -> Vec<(usize, [f32; 2])> {
        let r = &self.organizations[organization];
        let [x, y] = at;
        match self.unorganized(organization) {
            // Without a formation each keeps where it stood from the
            // leader, turning with nothing.
            true => (r.units.iter())
                .zip(&r.broken_places)
                .map(|(&u, &[dx, dy])| (u, [x + dx, y + dy]))
                .collect(),
            false => (r.units.iter())
                .zip(&r.formations[r.current_formation()].offsets)
                .map(|(&u, &offset)| (u, tgmap::from_local(at, facing, offset)))
                .collect(),
        }
    }

    /// Sends a unit to stand at `place` facing `facing`, stopping right
    /// there if `exact` or it's already near.
    pub(crate) fn take_place(&mut self, u: usize, place: [f32; 2], facing: [f32; 2], exact: bool) {
        // Nothing in the air sets off on a move.
        if self.units[u].in_air() {
            return;
        }
        let exact = exact || within(self.units[u].position, place, ARRIVAL_REACH);
        let Some(route) = self.plan(u, place, exact, true) else {
            // One that finds no route stops where it is, as it faces.
            self.units[u].halt();
            return;
        };
        let unit = &mut self.units[u];
        unit.take_route(route, place, exact);
        unit.closing = None;
        unit.end_facing = facing;
    }

    /// The way a unit takes to stop at `to`, `exact` for a move that has
    /// to end right there. On a move that's planned none yet, `fresh`, it
    /// goes straight at what's near enough on such a move, and turns at
    /// once to what's ahead of it but nearer than it could turn onto;
    /// otherwise it finds a way to where it [aims](Self::aimed_at).
    /// None where it [finds no route](Self::no_route).
    pub(crate) fn plan(
        &mut self,
        u: usize,
        to: [f32; 2],
        exact: bool,
        fresh: bool,
    ) -> Option<Route> {
        let unit = &self.units[u];
        let off = [to[0] - unit.position[0], to[1] - unit.position[1]];
        let straight = Route {
            points: vec![to],
            stage: false,
            straight: true,
        };
        if fresh && exact && within_strictly(unit.position, to, STRAIGHT_REACH) {
            return Some(straight);
        }
        let [fx, fy] = unit.facing;
        let ahead = fx * off[0] + fy * off[1] >= 0.0;
        let side = fx * off[1] - fy * off[0];
        let r = unit.turn_radius;
        if fresh && r > 0.0 && ahead && within_strictly(unit.turning_circle(side), to, r) {
            if let Some(facing) = normalised(off) {
                self.units[u].facing = facing;
            }
            return Some(straight);
        }
        let to = self.routed(u, to)?;
        let way = self.way_found(u, to)?;
        Some(Route {
            points: way.points,
            stage: way.stage,
            straight: false,
        })
    }

    /// A unit's way to `to`, last waypoint first, as [`World::way_found`]
    /// has it; none where that finds none.
    pub(crate) fn way(&self, u: usize, to: [f32; 2]) -> Vec<[f32; 2]> {
        self.way_found(u, to).map(|w| w.points).unwrap_or_default()
    }

    /// A unit's way to `to`, last waypoint first. It passes through the
    /// obstacles it's caught on, as one put out through its building, or
    /// beside one unpacked next to it, is: those its shape overlaps, and
    /// those blocking its tile, which reach further than their shapes do.
    /// None where it's cut short where the unit stands.
    fn way_found(&self, u: usize, to: [f32; 2]) -> Option<path::Way> {
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
        }?;
        way.points.reverse();
        Some(way)
    }

    /// Where the unit at `u` sent to `to` aims its route: the first spot on
    /// the line back from `to` towards it, a tile at a time, that it could
    /// stand on, or, where that spot falls on a thing, the spot a tile out
    /// from the thing along the thing's facing, on the unit's side. None
    /// where it would get back to where it stands first.
    fn aimed_at(&self, u: usize, to: [f32; 2]) -> Option<[f32; 2]> {
        let unit = &self.units[u];
        let (from, domain) = (unit.position, unit.domain);
        let home = Tile::at(from);
        // Where it was sent may be where it stands, or beside it; a spot
        // pulled back to its own tile is where it stands.
        let stands = |at: [f32; 2], pulled: bool| {
            self.pathing.passable(Tile::at(at), domain) && !(pulled && Tile::at(at) == home)
        };
        let off = [to[0] - from[0], to[1] - from[1]];
        let length = libm::hypotf(off[0], off[1]);
        if stands(to, false) {
            return Some(to);
        }
        let mut back = 0.0;
        while back < length {
            let t = 1.0 - back / length;
            let spot = [from[0] + off[0] * t, from[1] + off[1] * t];
            let pulled = back > 0.0;
            if pulled && stands(spot, true) {
                return Some(spot);
            }
            // Only a thing keeps it off ground of its own kind.
            let held_off = self.pathing.crossable(Tile::at(spot), domain);
            let beside = held_off.then(|| self.beside_blocker(spot, from)).flatten();
            if let Some(beside) = beside.filter(|&b| stands(b, true)) {
                return Some(beside);
            }
            back += CELL_SIZE;
        }
        None
    }

    /// The spot a tile out along its facing from the thing blocking the
    /// tile under `spot`, on `side`'s side of it, if a thing does: the
    /// middle of the row of tiles along its edge.
    fn beside_blocker(&self, spot: [f32; 2], side: [f32; 2]) -> Option<[f32; 2]> {
        let tile = Tile::at(spot);
        let buildings = self.buildings.iter().filter(|b| !b.gone());
        let mut obstacles = buildings
            .filter_map(|b| b.obstacle.as_ref())
            .chain(&self.scenery);
        let o = obstacles.find(|o| o.covers(tile))?;
        let [across, _] = tgmap::to_local(o.position, o.facing, spot);
        let [_, ahead] = tgmap::to_local(o.position, o.facing, side);
        let half = match o.shape {
            Shape::Point => 0.0,
            Shape::Circle { radius } => radius,
            Shape::Rect { height, .. } => height / 2.0,
        };
        let along = (half + CELL_SIZE / 2.0).copysign(ahead);
        Some(tgmap::from_local(o.position, o.facing, [across, along]))
    }

    /// Whether the unit at `u` finds no route to `to`, so doesn't set off:
    /// it has nowhere to [route](Self::routed) to, or its way there is cut
    /// short where it stands.
    pub(crate) fn no_route(&self, u: usize, to: [f32; 2]) -> bool {
        (self.routed(u, to)).is_none_or(|to| self.way_found(u, to).is_none())
    }

    /// Where the unit at `u` sent to `to` routes to: where it
    /// [aims](Self::aimed_at), unless there's no spot to aim at or, where
    /// the rules divide the map into areas, none of its own kind of ground
    /// joins it to that spot and a straight line there crosses some closed
    /// to it. One standing on ground closed to it has no area to judge by,
    /// and an ethereal one none to keep to.
    fn routed(&self, u: usize, to: [f32; 2]) -> Option<[f32; 2]> {
        let aimed = self.aimed_at(u, to)?;
        let unit = &self.units[u];
        let (from, domain) = (unit.position, unit.domain);
        let cut_off = self.rules.terrain_rules.areas
            && !unit.ethereal
            && self.pathing.crossable(Tile::at(from), domain)
            && !self.pathing.joined(domain, from, aimed)
            && !path::open(&self.pathing, domain, from, aimed);
        (!cut_off).then_some(aimed)
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
        let local = match self.unorganized(r) {
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
                // Both measured along the leader's route.
                let d = unit.left_along_leaders() - leader.route_left();
                let behind = left as f32 - leads as f32;
                match d == 0.0 || d.signum() == behind.signum() {
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
    /// their places from: its leader whenever that's present, else the one
    /// that took over, else the one that takes over now.
    pub(crate) fn pacer(&self, r: usize) -> Option<usize> {
        let organization = &self.organizations[r];
        let present = |&p: &usize| self.units[organization.units[p]].alive();
        (organization.leader.filter(present))
            .or(organization.pacer.filter(present))
            .or_else(|| self.next_pacer(r))
    }

    /// Who takes over leading the organization at `r` from a leader gone:
    /// the first of its members, in its layout's order, that's with it,
    /// and failing that the first present at all.
    fn next_pacer(&self, r: usize) -> Option<usize> {
        let organization = &self.organizations[r];
        let present = |&p: &usize| self.units[organization.units[p]].alive();
        let with = |p: &usize| present(p) && self.units[organization.units[*p]].with != Some(false);
        (0..organization.units.len())
            .find(with)
            .or_else(|| (0..organization.units.len()).find(present))
    }

    /// Has each organization keep who leads it: the member in its leader's
    /// place whenever there is one, else whoever took over from it, until
    /// that one goes too.
    pub(crate) fn keep_pacers(&mut self) {
        for r in 0..self.organizations.len() {
            let pacer = self.pacer(r);
            self.organizations[r].pacer = pacer;
        }
    }

    /// Has each regiment's member on its way to its place along a copy of
    /// its leader's route set its pace as it takes up a leg and on its
    /// beat: the pace that brings it back to its place. One well ahead of
    /// its place waits for it while the leader drives, and with the leader
    /// stopped its move ends there.
    fn keep_places(&mut self, tops: &[f32]) {
        for u in 0..self.units.len() {
            let unit = &self.units[u];
            if !unit.alive() || unit.goal.is_none() || unit.closing.is_some() {
                self.units[u].pace = 1.0;
                continue;
            }
            let fresh = unit.leg.is_none();
            if fresh {
                let reach = self.rules.leg_reach;
                self.units[u].take_leg(reach);
            }
            let unit = &self.units[u];
            let beat = (self.ticks + u as u64).is_multiple_of(u64::from(LOOK));
            if !(beat || fresh && unit.leg.is_some()) {
                continue;
            }
            if unit.sources.is_empty() {
                self.units[u].pace = 1.0;
                continue;
            }
            let r = unit.organization;
            // One waiting for the rest of its group doesn't look until it
            // goes on.
            if self.waiting_for_group(r) {
                continue;
            }
            let organization = &self.organizations[r];
            let leader = (self.pacer(r)).map(|lp| &self.units[organization.units[lp]]);
            let driving = leader.is_some_and(|l| l.moving());
            // A leader going after a target of its own is no guide to the
            // place its members are making for.
            let pace = match leader.is_some_and(|l| l.closing.is_some()) {
                true => 1.0,
                false => self.place_pace(r, u, tops[r], self.unit_speed(u)),
            };
            let facing = self.regiment_facing(r);
            let unit = &mut self.units[u];
            match (pace > 0.0, driving) {
                (true, _) | (false, true) => unit.pace = pace,
                (false, false) => {
                    unit.halt();
                    unit.facing = facing;
                    continue;
                }
            }
            if beat {
                self.check_point(u);
            }
        }
    }

    /// The speed the organization at `r` hands its members: its own, none
    /// while it waits for the rest of its group or below
    /// [`REGIMENT_SPEED_MIN`], and for a unit on its own, no more than it
    /// was last given, a rise in its top speed waiting for its next move.
    fn handed_speed(&mut self, r: usize) -> f32 {
        // Standing for its group isn't a speed it's given.
        if self.waiting_for_group(r) {
            return 0.0;
        }
        let mut speed = self.regiment_speed(r);
        if self.lone(r) {
            let organization = &mut self.organizations[r];
            speed = speed.min(organization.given_speed);
            organization.given_speed = speed;
        }
        match speed < REGIMENT_SPEED_MIN {
            true => 0.0,
            false => speed,
        }
    }

    /// Moves every unit on along its path. One that's run out of path
    /// short of where it was to stop finds another way from where it is,
    /// until it's tried enough. One that's arrived faces its regiment's
    /// way, or as it came for a unit on its own; one that's given up keeps
    /// its facing. Regiments' members keep their places on the way, and
    /// one making its way back to its regiment goes at its catch-up speed.
    pub(crate) fn travel(&mut self) {
        let waiting: Vec<bool> = (0..self.organizations.len())
            .map(|r| self.waiting_for_group(r))
            .collect();
        let tops: Vec<f32> = (0..self.organizations.len())
            .map(|r| self.handed_speed(r))
            .collect();
        self.keep_places(&tops);
        for u in 0..self.units.len() {
            let limit = self.speed_limit(u).unwrap_or(f32::INFINITY);
            let r = self.units[u].organization;
            let own = self.unit_speed(u);
            let facing = self.regiment_facing(r);
            let alone = self.lone(r);
            let unit = &mut self.units[u];
            unit.previous = unit.position;
            if unit.in_air() {
                continue;
            }
            // One waiting for the rest of its group stands, not turning.
            if waiting[r] {
                unit.speed = 0.0;
                continue;
            }
            let top = match unit.returning {
                true => self.rules.catchup_speed * own,
                false => tops[r],
            };
            let reach = self.rules.leg_reach;
            if unit.look == 0 {
                unit.take_leg(reach);
                // A leg it snapped onto it looks at no sooner.
                let snapped = unit.leg.is_some_and(|l| l.snapped);
                let straight = unit.going_straight() && !snapped;
                self.units[u].look = self.look_ticks(u, straight);
            }
            let unit = &mut self.units[u];
            if unit.drive(top.min(limit), own, reach) {
                self.think(u);
            }
            let unit = &self.units[u];
            if let (true, Some(goal)) = (unit.path.is_empty(), unit.goal) {
                let reach = match unit.exact {
                    true => EXACT_REACH,
                    false => ARRIVAL_REACH,
                };
                let (exact, ways, stage) = (unit.exact, unit.ways, unit.stage);
                let there = within(unit.position, goal, reach);
                // A long way's next stage is planned afresh.
                let ways = if stage { 0 } else { ways };
                let again = (!there && ways < WAYS_TRIED)
                    .then(|| self.plan(u, goal, exact, false))
                    .flatten();
                let unit = &mut self.units[u];
                match again {
                    // Given up, it stops where it is, as it faces.
                    None => {
                        unit.halt();
                        if there && !alone {
                            unit.facing = facing;
                        }
                    }
                    Some(route) => {
                        // Trying another way is the same move.
                        let shelling = unit.shelling;
                        unit.take_route(route, goal, exact);
                        (unit.ways, unit.shelling) = (ways + 1, shelling);
                    }
                }
            }
            let unit = &mut self.units[u];
            if let Some(t) = &mut unit.trailer {
                t.follow(unit.position, unit.facing);
            }
        }
    }

    /// Ticks until the unit at `u` next looks at how its move is going,
    /// drawn afresh each time: sooner while it's driving `straight` for a
    /// spot of its own.
    fn look_ticks(&self, u: usize, straight: bool) -> u32 {
        let [fewest, most] = match straight {
            true => LOOKS_STRAIGHT,
            false => LOOKS,
        };
        // Drawn without drawing on the match's chances, each unit its own.
        let mut seed = self.rng ^ self.ticks.rotate_left(32) ^ (u as u64).wrapping_mul(0x9e37_79b9);
        let chance = next_chance(&mut seed);
        (fewest + (chance * (most - fewest + 1) as f32) as u32).min(most)
    }

    /// Has the unit at `u` look at how the leg it's driving is going:
    /// gives up turning onto a last leg that ends somewhere it can't turn
    /// onto, and, once it's done turning but is left well to the side of
    /// its leg, makes for the leg again at an angle, or straight for the
    /// leg's end where that comes first, unless something's in the way.
    fn think(&mut self, u: usize) {
        let unit = &self.units[u];
        let (Some(&next), Some(leg)) = (unit.path.last(), unit.leg) else {
            return;
        };
        if unit.path.len() == 1 && !leg.snapped && unit.too_tight(next) {
            let unit = &mut self.units[u];
            unit.facing = leg.heading;
            unit.leg = Some(Leg {
                snapped: true,
                ..leg
            });
            return;
        }
        if unit.facing != leg.heading {
            return;
        }
        let at = unit.position;
        let [hx, hy] = leg.heading;
        let back = [at[0] - next[0], at[1] - next[1]];
        // How far to the leg's left it is, and short of its end.
        let aside = hx * back[1] - hy * back[0];
        let short = -(hx * back[0] + hy * back[1]);
        if aside.abs() < OFF_LINE {
            return;
        }
        // At 45°, or as much less as turning in and back out again leaves
        // room for.
        let room = match unit.turn_radius > 0.0 {
            true => 1.0 - aside.abs() / (2.0 * unit.turn_radius),
            false => -1.0,
        };
        let angle = libm::acosf(room.max(-1.0)).min(FRAC_PI_4);
        let along = aside.abs() / libm::tanf(angle);
        let dash = along >= short;
        let to = match dash {
            true => next,
            false => [
                at[0] + hy * aside + hx * along,
                at[1] - hx * aside + hy * along,
            ],
        };
        if !path::clear(&self.pathing, unit.domain, at, to) {
            return;
        }
        let unit = &mut self.units[u];
        unit.from = at;
        unit.leg = None;
        if dash {
            let off = [next[0] - at[0], next[1] - at[1]];
            unit.facing = normalised(off).unwrap_or(unit.facing);
            unit.detour = true;
            return;
        }
        unit.path.push(to);
        // Made from the same point of the leader's as the one it leads to.
        if let Some(&source) = unit.sources.last() {
            unit.sources.push(source);
        }
    }

    /// Has each organization's own position look where its leader is once
    /// a second, each on its own second, and slide that way at its
    /// catch-up pace.
    pub(crate) fn keep_banners(&mut self) {
        for r in 0..self.organizations.len() {
            if (self.ticks + r as u64).is_multiple_of(crate::SECOND) {
                let leader = self
                    .pacer(r)
                    .map(|p| self.units[self.organizations[r].units[p]].position);
                self.organizations[r].banner.aim =
                    leader.unwrap_or_else(|| self.leader_position(r));
            }
            let catchup = self.rules.catchup_speed * self.regiment_speed(r);
            let pace = catchup.max(self.rules.banner_catchup_floor);
            let banner = &mut self.organizations[r].banner;
            let (at, aim) = (banner.at, banner.aim);
            let left = crate::distance(at, aim);
            banner.at = match left <= pace * TICK {
                true => aim,
                false => {
                    let share = pace * TICK / left;
                    [
                        at[0] + (aim[0] - at[0]) * share,
                        at[1] + (aim[1] - at[1]) * share,
                    ]
                }
            };
        }
    }
}

impl Banner {
    /// One standing at `at`.
    pub(crate) fn at(at: [f32; 2]) -> Banner {
        Banner { at, aim: at }
    }
}

#[cfg(test)]
mod tests {

    use crate::testing::*;

    use tgmap::{Domain, Obstacle, Pathing, Shape, Tile};

    use crate::{ARRIVAL_REACH, Command, Sides, Unit, World, distance, normalised, path, within};

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
        w.organizations[0].route_end = None;
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
        w.keep_targeted();
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
        w.grant(1, tgmap::Id::from("engines"));
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
        while w.units[0].moving() {
            w.tick();
        }
        let u = &w.units[0];
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
            trailer: None,
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
        // A leg ends within a hundredth of its end.
        assert!(distance(w.units[1].position, [11.0, 30.0]) <= 0.01);
    }

    #[test]
    fn a_member_well_ahead_of_its_place_waits_for_it() {
        let mut w = moved_with_the_right_hand_man_at([11.0, 22.0]);
        // From the moment it takes up its leg its place, beside the
        // leader, is well behind it, so it stands.
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
    fn a_regiments_own_position_trails_its_leader_on_the_march() {
        let mut w = moved_with_the_right_hand_man_at([11.0, 1.0]);
        w.organizations[0].leader = Some(0);
        run(&mut w, 3.0);
        let behind = distance(w.units[0].position, w.organizations[0].banner.at);
        assert!(behind > 0.5 && behind < 4.0, "{behind}");
        // Once its leader stops, it catches up with him.
        run(&mut w, 20.0);
        assert!(distance(w.units[0].position, w.organizations[0].banner.at) < 1e-3);
    }

    #[test]
    fn a_regiments_own_position_catches_up_no_slower_than_the_floor() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        // 0.1 × the regiment's 2 would be well under the floor of 0.5.
        w.rules.catchup_speed = 0.1;
        w.organizations[0].leader = Some(0);
        let [x, y] = w.organizations[0].banner.at;
        w.units[0].position = [x, y + 10.0];
        run(&mut w, 1.0);
        let was = w.organizations[0].banner.at;
        run(&mut w, 1.0);
        let went = distance(was, w.organizations[0].banner.at);
        assert!((went - 0.5).abs() < 0.01, "{went}");
    }

    /// The pace the right-hand man keeps, 2 to his leader's right, both
    /// heading north from 10, his leader with `leads` and he with `path`
    /// left (next point last), at a regiment speed of 2.
    fn pace_on(leads: Vec<[f32; 2]>, path: Vec<[f32; 2]>, detour: bool) -> f32 {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        w.units[0].path = leads;
        w.units[1].path = path;
        w.units[1].detour = detour;
        w.rules.catchup_speed = 1.75;
        w.place_pace(0, 1, 2.0, 2.0)
    }

    #[test]
    fn members_keep_place_round_corners_on_other_legs_and_on_detours() {
        let leads = vec![[9.0, 40.0], [9.0, 30.0]];
        // Level with its place, on the same leg, it keeps the regiment's speed.
        assert!(
            (pace_on(leads.clone(), vec![[11.0, 40.0], [11.0, 30.0]], false) - 1.0).abs() < 1e-4
        );
        // Near a right turn it's on the inside, so slows; a left, it's
        // outside, so speeds up.
        let inside = pace_on(leads.clone(), vec![[30.0, 12.0], [11.0, 12.0]], false);
        let outside = pace_on(leads.clone(), vec![[-5.0, 12.0], [11.0, 12.0]], false);
        assert!(inside < 1.0 && outside > 1.0, "{inside} {outside}");
        // A point behind the leader though less far to go, it goes by
        // the points: half a point's worth faster.
        let behind = pace_on(
            leads.clone(),
            vec![[11.0, 39.0], [11.0, 30.0], [11.0, 12.0]],
            false,
        );
        assert!((behind - (2.0 + 0.25) / 2.0).abs() < 1e-4, "{behind}");
        // On a detour 4 or more behind, flat out.
        let detour = pace_on(leads, vec![[11.0, 60.0]], true);
        assert!((detour - 1.75).abs() < 1e-4, "{detour}");
    }

    #[test]
    fn members_follow_the_leaders_route_alongside_it_meeting_its_corners() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.organizations[0].travelling = true;
        // North to 9,30, then east to 29,30: a right turn.
        let route = vec![[29.0, 30.0], [9.0, 30.0]];
        w.take_copy(0, 0, 1, &route, ([29.0, 28.0], false));
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
    fn a_member_pulls_back_a_point_it_cant_reach_towards_its_leaders() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        // A wall across the member's line north, but not the leader's.
        for x in 5..8 {
            pathing.set_blocked(Tile::new(x, 12));
        }
        let mut w = world(pathing);
        w.organizations[0].travelling = true;
        let route = vec![[9.0, 50.0], [9.0, 30.0]];
        w.take_copy(0, 0, 1, &route, ([11.0, 50.0], false));
        // Its first point, 2 right of the leader's at 9,30, is behind the
        // wall: it goes for the first spot back that it can reach.
        let unit = &w.units[1];
        let next = unit.path[1];
        assert!(
            next[0] < 11.0 && next[0] >= 9.0 && next[1] == 30.0,
            "{next:?}"
        );
        assert!(path::clear(&w.pathing, Domain::Land, unit.position, next));
    }

    #[test]
    fn a_member_walled_off_from_its_place_gets_there_once_its_regiment_has() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        // A wall right across the member's line north, but not the leader's.
        for x in 5..8 {
            pathing.set_blocked(Tile::new(x, 10));
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
        // It stands, trying, then gives its place up while the leader goes
        // on; once the leader's there it's sent round on its own.
        for _ in 0..(20.0 / crate::TICK) as usize {
            w.tick();
            let tile = Tile::at(w.units[1].position);
            assert!(
                pathing.passable(tile, Domain::Land),
                "{:?}",
                w.units[1].position
            );
        }
        let at = w.units[1].position;
        assert!(within(at, [11.0, 30.0], ARRIVAL_REACH), "{at:?}");
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
        // Facing along the last leg of a way from just off the line.
        assert!(distance(w.units[0].position, [9.0, 30.0]) < 0.2);
        assert!(distance(w.units[1].position, [11.0, 30.0]) < 0.2);
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
        // Facing along the way's last leg, back round the wall's end.
        let facing = w.organizations[0].facing;
        assert!(facing[0] < -0.5, "{facing:?}");
        for u in &w.units {
            assert!(!u.moving() && near(u.facing, facing));
            let off = distance(u.position, [10.0, 30.0]);
            assert!((off - 1.0).abs() <= ARRIVAL_REACH, "{off}");
        }
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

    #[test]
    fn a_regiment_sent_to_face_a_way_turns_to_it_once_there() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.send(0, [10.0, 30.0], [1.0, 0.0]);
        run(&mut w, 13.0);
        // It arrives, rather than giving the move up.
        let r = &w.organizations[0];
        assert!(r.route_end.is_none() && r.tries == 0 && r.turn_to.is_none());
        assert!(distance(r.position, [10.0, 30.0]) < 1e-3);
        run(&mut w, 2.0);
        // Its left-hand man, facing east, is to the north.
        assert!(distance(w.units[0].position, [10.0, 31.0]) < 0.5);
        assert!(distance(w.units[1].position, [10.0, 29.0]) < 0.5);
        assert!(
            w.units
                .iter()
                .all(|u| !u.moving() && near(u.facing, [1.0, 0.0]))
        );
    }

    #[test]
    fn a_member_that_joins_a_moving_regiment_is_handed_a_copy_of_its_route() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        w.apply(
            7,
            Command::Move {
                organization: 0,
                to: [10.0, 60.0],
            },
        )
        .unwrap();
        run(&mut w, 2.0);
        // Put back as a fresh member, standing well behind.
        w.units[1].stand_at([11.0, 2.0], [0.0, 1.0]);
        run(&mut w, 1.1);
        let unit = &w.units[1];
        assert!(unit.moving() && !unit.sources.is_empty());
        assert!(near(unit.path[0], [11.0, 60.0]), "{:?}", unit.path);
    }

    #[test]
    fn a_member_back_inside_its_moving_regiments_zone_is_handed_a_fresh_copy() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        w.organizations[0].control_range = 4.0;
        w.apply(
            7,
            Command::Move {
                organization: 0,
                to: [10.0, 70.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        // Cut off well behind, it's seen to be away at the next look...
        w.units[1].position = [11.0, 1.0];
        run(&mut w, 1.1);
        assert_eq!(w.units[1].with, Some(false));
        // ...and handed a copy afresh once it's back beside the leader.
        let beside = w.units[0].position;
        w.units[1].position = [beside[0] + 2.0, beside[1]];
        w.units[1].path.clear();
        run(&mut w, 1.1);
        let unit = &w.units[1];
        assert_eq!(unit.with, Some(true));
        assert!(unit.moving() && !unit.sources.is_empty(), "{:?}", unit.path);
    }

    #[test]
    fn a_regiment_whose_leader_falls_on_the_way_still_arrives() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.send(0, [10.0, 30.0], [1.0, 0.0]);
        run(&mut w, 3.0);
        w.units[0].die(w.ticks);
        run(&mut w, 15.0);
        // The one left takes over and, once at its place, turns east.
        assert!(w.organizations[0].route_end.is_none());
        assert!(
            near(w.units[1].facing, [1.0, 0.0]),
            "{:?}",
            w.units[1].facing
        );
    }
    fn move_to(w: &mut World, to: [f32; 2]) {
        w.apply(
            7,
            Command::Move {
                organization: 0,
                to,
            },
        )
        .unwrap();
    }

    #[test]
    fn a_regiment_faces_along_the_leg_its_leader_is_on() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        for x in 0..15 {
            pathing.set_blocked(Tile::new(x, 8));
        }
        let mut w = world(pathing);
        move_to(&mut w, [10.0, 30.0]);
        let r = &w.organizations[0];
        let leader = &w.units[0];
        let next = *leader.path.last().unwrap();
        let first = normalised([next[0] - leader.position[0], next[1] - leader.position[1]]);
        // It sets off facing along the first leg, its places turned with it.
        assert_eq!(r.heading, first);
        assert_ne!(Some(r.facing), first);
        let mut headings = vec![w.regiment_facing(0)];
        for _ in 0..400 {
            w.tick();
            let r = &w.organizations[0];
            if r.route_end.is_none() {
                break;
            }
            if headings.last() != Some(&w.regiment_facing(0)) {
                headings.push(w.regiment_facing(0));
            }
        }
        assert!(headings.len() >= 3, "{headings:?}");
        // There, it faces along the last leg.
        assert_eq!(w.organizations[0].heading, None);
        assert_eq!(Some(&w.organizations[0].facing), headings.last());
    }

    #[test]
    fn a_regiments_places_are_set_out_from_its_trailing_position_on_the_way() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        move_to(&mut w, [10.0, 30.0]);
        run(&mut w, 3.0);
        let r = &w.organizations[0];
        let at = w.regiment_position(0);
        // The leader's place is 1 to the left of the regiment's position,
        // which trails it rather than standing where it was sent.
        let [(_, leads), (_, right)] = w.formation_places(0)[..] else {
            panic!();
        };
        assert!(near(leads, [at[0] - 1.0, at[1]]));
        assert!(near(right, [at[0] + 1.0, at[1]]));
        assert!(distance(at, r.position) > 5.0);
        assert!(distance(leads, w.units[0].position) < 4.0);
    }

    #[test]
    fn a_regiment_arrives_once_its_own_position_is_where_it_was_sent() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        move_to(&mut w, [10.0, 20.0]);
        while w.units[0].moving() {
            w.tick();
        }
        // The leader's there, but the regiment's position, trailing it,
        // isn't yet.
        assert!(w.organizations[0].route_end.is_some());
        run(&mut w, 3.0);
        assert!(w.organizations[0].route_end.is_none());
        assert!(within(w.regiment_position(0), [10.0, 20.0], ARRIVAL_REACH));
    }

    #[test]
    fn a_regiment_whose_leader_stops_short_sends_it_again_then_gives_up() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        move_to(&mut w, [10.0, 70.0]);
        run(&mut w, 1.0);
        assert_eq!(w.organizations[0].tries, 1);
        let look = crate::LOOK as f32 * crate::TICK;
        // Its leader stops short, and is sent on again at its next look.
        for tries in [2, 3] {
            w.units[0].halt();
            run(&mut w, look);
            assert_eq!(w.organizations[0].tries, tries);
            assert!(w.units[0].moving());
        }
        // Tried three times, it gives up only once 5 seconds have gone by
        // since the order, resting where it stands.
        run(&mut w, 2.0);
        w.units[0].halt();
        run(&mut w, look);
        let r = &w.organizations[0];
        assert!(r.route_end.is_none() && !r.travelling);
        assert!(distance(r.position, [10.0, 70.0]) > 10.0);
    }

    #[test]
    fn a_regiment_speed_below_a_tenth_stops_its_members() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        move_to(&mut w, [10.0, 20.0]);
        w.organizations[0].ordered_speed = Some(0.09);
        run(&mut w, 2.0);
        assert!(near(w.units[0].position, [9.0, 10.0]));
        assert!(near(w.units[1].position, [11.0, 10.0]));
        w.organizations[0].ordered_speed = Some(0.1);
        run(&mut w, 2.0);
        assert!(w.units[0].position[1] > 10.1);
    }

    #[test]
    fn a_unit_on_its_own_speeds_up_only_at_its_next_move() {
        let mut w = vehicle(0.0, 0.0, None);
        w.units[0].template = "tank".into();
        move_to(&mut w, [10.0, 30.0]);
        run(&mut w, 1.0);
        let faster = speeding(1.0);
        w.rules.formation_effects.insert("lone".into(), faster);
        let y = w.units[0].position[1];
        run(&mut w, 1.0);
        assert!((w.units[0].position[1] - y - 2.0).abs() < 1e-3);
        // Slowed, it drops at once.
        w.rules
            .formation_effects
            .insert("lone".into(), speeding(-0.5));
        let y = w.units[0].position[1];
        run(&mut w, 1.0);
        assert!((w.units[0].position[1] - y - 1.0).abs() < 1e-3);
        w.rules
            .formation_effects
            .insert("lone".into(), speeding(1.0));
        move_to(&mut w, [10.0, 40.0]);
        run(&mut w, 0.5);
        let y = w.units[0].position[1];
        run(&mut w, 1.0);
        assert!((w.units[0].position[1] - y - 4.0).abs() < 1e-3);
    }

    #[test]
    fn a_unit_on_its_own_sets_off_once_its_group_stops_waiting() {
        let mut w = vehicle(0.0, 0.0, None);
        w.units[0].template = "tank".into();
        w.organizations[0].group_speed = Some(0.0);
        move_to(&mut w, [10.0, 30.0]);
        run(&mut w, 1.0);
        let y = w.units[0].position[1];
        w.organizations[0].group_speed = None;
        run(&mut w, 1.0);
        assert!(w.units[0].position[1] - y > 0.5);
    }

    #[test]
    fn a_regiment_doing_what_has_no_formation_holds_its_broken_formations_speed() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        (w.rules.formation_effects).insert("line".into(), speeding(-0.25));
        w.rules
            .broken_effects
            .insert("squad".into(), speeding(0.25));
        (w.rules.formationless_states).insert("mind_exhausted".into());
        assert_eq!(w.regiment_speed(0), 1.5);
        // Exhausted, even while it's engaged.
        w.organizations[0].mind = crate::Mind::Exhausted;
        (w.organizations[0].engaged).push(crate::Piece::Organization(0));
        assert_eq!(w.regiment_speed(0), 2.5);
    }

    #[test]
    fn a_regiment_finds_it_is_targeted_at_its_next_check() {
        let mut w = battle(
            &[1, 2],
            vec![soldier(Vec::new()), soldier(Vec::new())],
            at_war(),
        );
        (w.rules.combat_effects).insert(tgmap::TARGETED.into(), speeding(-0.1));
        while w.ticks % crate::SECOND != 1 {
            w.tick();
        }
        w.organizations[1]
            .engaged
            .push(crate::Piece::Organization(0));
        w.tick();
        assert_eq!(w.regiment_speed(0), 2.0);
        run(&mut w, 1.0);
        assert!((w.regiment_speed(0) - 1.8).abs() < 1e-5);
    }

    #[test]
    fn what_a_regiment_hands_down_reaches_only_members_of_the_kind_it_needs() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.units[0].template = "tank".into();
        w.rules.kinds.insert("tank".into(), kind(&["tank"]));
        let mut tanks = speeding(0.5);
        tanks.requires = vec!["tank".into()];
        w.rules
            .formation_effects
            .insert("line".into(), tanks.clone());
        assert_eq!((w.unit_speed(0), w.unit_speed(1)), (4.5, 2.0));
        // So do the properties it holds for a time, but for a special
        // operation's, which reach every member.
        w.rules.formation_effects.clear();
        w.rules.combat_effects.insert("diesel".into(), tanks);
        let held = |by| crate::Held {
            property: "diesel".into(),
            until: u64::MAX,
            by,
        };
        w.organizations[0].properties.push(held(None));
        assert_eq!((w.unit_speed(0), w.unit_speed(1)), (4.5, 2.0));
        w.organizations[0].properties = vec![held(Some(7))];
        assert_eq!((w.unit_speed(0), w.unit_speed(1)), (4.5, 3.0));
    }

    #[test]
    fn a_member_well_ahead_of_its_place_ends_its_move_once_its_leader_has_stopped() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        let r = &mut w.organizations[0];
        (r.route_end, r.facing, r.ordered_speed) = (Some([10.0, 20.0]), [1.0, 0.0], Some(0.15));
        // Its leader stands, and it's level with the leader's point.
        w.units[1].take(vec![[11.0, 20.0]], false);
        w.units[1].position = [11.0, 20.0 - 1e-3];
        w.units[1].sources = vec![[11.0, 20.0 - 1e-3]];
        w.tick();
        let unit = &w.units[1];
        assert!(unit.goal.is_none() && !unit.moving());
        assert_eq!(unit.facing, [1.0, 0.0]);
    }

    #[test]
    fn a_member_cut_off_from_its_moving_regiment_makes_its_way_back_at_its_catchup_speed() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        w.organizations[0].control_range = 4.0;
        w.units[1].position = [11.0, 2.0];
        move_to(&mut w, [10.0, 60.0]);
        run(&mut w, 2.0);
        let unit = &w.units[1];
        assert!(unit.returning);
        assert!((unit.speed - 1.75 * 2.0).abs() < 1e-4, "{}", unit.speed);
        // Back with it, it's handed the regiment's speed with a fresh copy.
        run(&mut w, 10.0);
        let unit = &w.units[1];
        assert!(!unit.returning && unit.with == Some(true));
        assert!((unit.speed - 2.0).abs() < 1e-4, "{}", unit.speed);
    }

    /// The regiment routing, each member keeping where it stood from the
    /// leader: the right-hand man 2 to his right.
    fn routing(w: &mut World) {
        let r = &mut w.organizations[0];
        r.mind = crate::Mind::Routing {
            to: None,
            until: u64::MAX,
            retry: u64::MAX,
        };
        r.broken_places = vec![[0.0, 0.0], [2.0, 0.0]];
        (r.control_range, r.travelling, r.position) = (4.0, true, [10.0, 30.0]);
    }

    #[test]
    fn a_member_away_from_a_regiment_with_no_formation_makes_for_the_routes_first_point() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        routing(&mut w);
        w.units[1].position = [30.0, 10.0];
        w.take_places(0);
        // Not a copy of the route, but a way of its own to the first point
        // moved by where it stood from the leader.
        let unit = &w.units[1];
        assert!(unit.sources.is_empty());
        assert_eq!(unit.goal, Some([12.0, 30.0]));
    }

    #[test]
    fn a_member_joining_a_regiment_with_no_formation_is_handed_a_copy() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        routing(&mut w);
        w.take_places(0);
        w.units[1].halt();
        w.units[1].with = None;
        run(&mut w, crate::LOOK as f32 * crate::TICK);
        assert!(!w.units[1].sources.is_empty());
    }

    #[test]
    fn a_member_finding_no_way_to_its_points_gives_them_up_at_once() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        // A wall right across the member's line, reaching to the leader's.
        for x in 4..6 {
            pathing.set_blocked(Tile::new(x, 10));
        }
        let mut w = world(pathing);
        w.organizations[0].travelling = true;
        w.units[0].position = [9.0, 25.0];
        let route = vec![[9.0, 30.0], [9.0, 24.0]];
        w.units[1].position = [11.0, 16.0];
        w.take_copy(0, 0, 1, &route, ([11.0, 30.0], false));
        // Nothing back towards the leader's point is clear, so it's
        // passed at once, and its last, out of reach too, ends its move
        // where it stands, as it faces.
        let unit = &w.units[1];
        assert!(unit.goal.is_none() && !unit.moving());
        assert_eq!(unit.facing, [0.0, 1.0]);
    }

    #[test]
    fn a_unit_that_turns_at_once_turns_back_in_to_its_line() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        let unit = &mut w.units[1];
        unit.take(vec![[11.0, 40.0]], false);
        // Held to the leader's heading, north, 3 off its line.
        unit.from = [11.0, 10.0];
        unit.position = [14.0, 12.0];
        unit.look = crate::LOOK;
        for _ in 0..=crate::LOOK {
            if w.units[1].drive(2.0, 2.0, 0.1) {
                w.think(1);
            }
        }
        // It turns in at 45 degrees, to a point on its line.
        let unit = &w.units[1];
        let f = std::f32::consts::FRAC_1_SQRT_2;
        assert!(near(unit.facing, [-f, f]), "{:?}", unit.facing);
        assert_eq!(unit.path.len(), 2);
        assert!((unit.path[1][0] - 11.0).abs() < 1e-3);
    }

    #[test]
    fn members_face_their_regiments_way_on_arriving_and_units_on_their_own_as_they_came() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.organizations[0].heading = Some([1.0, 0.0]);
        w.organizations[0].route_end = Some([10.0, 20.0]);
        w.units[1].take(vec![[11.0, 10.5]], false);
        run(&mut w, 1.0);
        assert_eq!(w.units[1].facing, [1.0, 0.0]);
        let mut w = vehicle(0.0, 0.0, None);
        w.units[0].template = "tank".into();
        w.apply(
            7,
            Command::Move {
                organization: 0,
                to: [20.0, 10.0],
            },
        )
        .unwrap();
        w.organizations[0].facing = [0.0, -1.0];
        run(&mut w, 10.0);
        assert!(
            near(w.units[0].facing, [1.0, 0.0]),
            "{:?}",
            w.units[0].facing
        );
    }

    #[test]
    fn a_unit_that_gives_up_its_move_keeps_its_facing() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        for x in 0..20 {
            pathing.set_blocked(Tile::new(x, 8));
        }
        let mut w = world(pathing);
        w.organizations[0].facing = [1.0, 0.0];
        w.units[1].take(vec![[11.0, 10.5]], false);
        w.units[1].goal = Some([11.0, 30.0]);
        w.units[1].ways = super::WAYS_TRIED;
        run(&mut w, 1.0);
        let unit = &w.units[1];
        assert!(unit.goal.is_none());
        assert_eq!(unit.facing, [0.0, 1.0]);
    }

    #[test]
    fn a_new_order_on_the_way_takes_effect_at_the_regiments_next_look() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        move_to(&mut w, [10.0, 60.0]);
        run(&mut w, 1.0);
        while (w.ticks + 1).is_multiple_of(u64::from(crate::LOOK)) {
            w.tick();
        }
        move_to(&mut w, [60.0, 10.0]);
        let old = w.units[1].path.clone();
        w.tick();
        assert_eq!(w.units[1].path, old);
        assert!(w.underway(0));
        run(&mut w, crate::LOOK as f32 * crate::TICK);
        assert_ne!(w.units[1].path, old);
        let place = w.destination_places(0)[0].1;
        assert_eq!(w.organizations[0].route_end, Some(place));
        assert!(distance(place, [60.0, 10.0]) < 1.1);
    }

    #[test]
    fn a_member_sets_its_pace_as_it_takes_up_a_leg() {
        let mut w = moved_with_the_right_hand_man_at([11.0, 2.0]);
        w.tick();
        // Well behind, it's going faster than the regiment from the off.
        assert!(w.units[1].pace > 1.0, "{}", w.units[1].pace);
    }

    #[test]
    fn a_member_as_far_along_its_leaders_route_as_the_leader_keeps_its_pace() {
        let leads = vec![[9.0, 40.0], [9.0, 30.0]];
        // A point behind, its own copy longer than the leader's, but as
        // far from the leader's points.
        let mut w = world(Pathing::uniform(40, Domain::Land));
        w.units[0].path = leads.clone();
        w.units[1].path = vec![[11.0, 40.0], [11.0, 30.0], [11.0, 12.0]];
        w.units[1].sources = vec![[9.0, 40.0], [9.0, 30.0], [9.0, 10.0]];
        w.units[1].position = [9.0, 10.0];
        let pace = w.place_pace(0, 1, 2.0, 2.0);
        assert!((pace - 1.0).abs() < 1e-4, "{pace}");
    }

    #[test]
    fn a_regiment_whose_leader_falls_is_led_by_the_first_member_still_with_it() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        let third = w.units[1].clone();
        w.units.push(third);
        w.organizations[0].units.push(2);
        let offsets = &mut w.organizations[0].formations[0].offsets;
        offsets.push([3.0, 0.0]);
        w.organizations[0].leader = Some(0);
        w.keep_pacers();
        assert_eq!(w.organizations[0].pacer, Some(0));
        w.units[0].die(0);
        w.units[1].with = Some(false);
        w.keep_pacers();
        assert_eq!(w.organizations[0].pacer, Some(2));
        // It keeps the lead once the straggler's back.
        w.units[1].with = Some(true);
        w.keep_pacers();
        assert_eq!(w.organizations[0].pacer, Some(2));
        // Until its leader's place is filled again.
        w.units[0].died = None;
        w.units[0].health = 1.0;
        w.keep_pacers();
        assert_eq!(w.pacer(0), Some(0));
    }

    /// A unit on its own, a vehicle turning round a radius of 2.
    fn lone_vehicle() -> World {
        let mut w = vehicle(0.0, 1.0, None);
        w.units[0].template = "tank".into();
        assert!(w.lone(0));
        w
    }

    fn moved(w: &mut World, to: [f32; 2]) {
        w.apply(
            7,
            Command::Move {
                organization: 0,
                to,
            },
        )
        .unwrap();
    }

    /// Water from y = 16 up.
    fn lake(side: usize, from: i32) -> Pathing {
        let mut pathing = Pathing::uniform(side, Domain::Land);
        for x in 0..side as i32 {
            for y in from..side as i32 {
                pathing.set(Tile::new(x, y), Domain::Naval);
            }
        }
        pathing
    }

    #[test]
    fn a_lone_unit_sent_into_a_lake_stops_at_the_shore_and_gives_up_there() {
        let mut w = lone_vehicle();
        w.pathing = lake(20, 8);
        moved(&mut w, [10.0, 19.0]);
        // Headed for the shore, it's still to get to where it was sent.
        assert_eq!(w.units[0].goal, Some([10.0, 19.0]));
        run(&mut w, 5.0);
        let u = &w.units[0];
        assert!(!u.moving() && u.goal.is_none());
        assert!(
            u.position[1] < 16.0 && u.position[1] > 13.0,
            "{:?}",
            u.position
        );
        assert!(!w.organizations[0].travelling);
        assert!(w.organizations[0].route_end.is_none());
    }

    #[test]
    fn a_regiment_sent_into_a_lake_tries_from_the_shore_until_it_may_give_up() {
        let mut w = world(lake(20, 8));
        moved(&mut w, [10.0, 20.0]);
        // At the shore by now, it tries again until 5 seconds are up.
        run(&mut w, 4.5);
        assert!(w.organizations[0].route_end.is_some());
        assert!(w.organizations[0].tries >= 3);
        run(&mut w, 2.5);
        let r = &w.organizations[0];
        assert!(r.route_end.is_none() && !r.travelling);
        // It rests where it stopped, not where it was sent.
        assert!(r.position[1] < 16.0, "{:?}", r.position);
        assert!(w.units.iter().all(|u| u.position[1] < 16.0));
    }

    #[test]
    fn a_regiment_closed_in_where_its_leader_stands_gives_up_without_moving() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        // Everything round the leader's tile, 4,5, is blocked.
        for (x, y) in [
            (3, 4),
            (4, 4),
            (5, 4),
            (3, 5),
            (5, 5),
            (3, 6),
            (4, 6),
            (5, 6),
        ] {
            pathing.set_blocked(Tile::new(x, y));
        }
        let mut w = world(pathing);
        let before: Vec<[f32; 2]> = w.units.iter().map(|u| u.position).collect();
        moved(&mut w, [10.0, 30.0]);
        // It's set off, but nobody moves.
        assert!(w.organizations[0].travelling);
        run(&mut w, 3.0);
        assert!(w.organizations[0].route_end.is_some());
        run(&mut w, 4.0);
        assert!(w.organizations[0].route_end.is_none());
        assert!(!w.organizations[0].travelling);
        let after: Vec<[f32; 2]> = w.units.iter().map(|u| u.position).collect();
        assert_eq!(before, after);
    }

    #[test]
    fn a_route_longer_than_the_tile_limit_is_driven_a_stage_at_a_time() {
        let mut w = lone_vehicle();
        w.pathing = Pathing::uniform(800, Domain::Land);
        w.units[0].speed_max = 40.0;
        w.units[0].turn_radius = 0.0;
        let to = [1550.0, 10.0];
        moved(&mut w, to);
        assert!(w.units[0].stage);
        // Three stages and more, more plans than a move may make at once.
        let mut stages = 1;
        let mut end = w.units[0].path.first().copied();
        for _ in 0..(60.0 / crate::TICK) as usize {
            w.tick();
            let now = w.units[0].path.first().copied();
            stages += usize::from(now.is_some() && now != end);
            end = now;
        }
        assert!(stages > 3, "{stages}");
        assert!(
            within(w.units[0].position, to, ARRIVAL_REACH),
            "{:?}",
            w.units[0].position
        );
    }

    #[test]
    fn a_move_whose_tile_route_is_cut_short_where_it_stands_fails_at_once() {
        let mut pathing = Pathing::uniform(300, Domain::Land);
        // A corridor east along row 0 and back along row 2: 254 steps out
        // it's no nearer row 2's start than where it began.
        for x in 0..299 {
            pathing.set_blocked(Tile::new(x, 1));
        }
        for x in 0..300 {
            pathing.set_blocked(Tile::new(x, 3));
        }
        let mut w = lone_vehicle();
        w.pathing = pathing;
        w.units[0].stand_at([1.0, 1.0], [1.0, 0.0]);
        w.organizations[0].position = [1.0, 1.0];
        assert!(w.no_route(0, [1.0, 5.0]));
        moved(&mut w, [1.0, 5.0]);
        assert!(!w.organizations[0].travelling && !w.units[0].moving());
    }

    #[test]
    fn ethereal_things_are_not_kept_to_their_area() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        // A channel all the way across.
        for y in 0..20 {
            pathing.set(Tile::new(8, y), Domain::Naval);
        }
        let mut w = lone_vehicle();
        w.pathing = pathing;
        w.rules.terrain_rules.areas = true;
        assert!(w.no_route(0, [30.0, 10.0]));
        w.units[0].ethereal = true;
        assert!(!w.no_route(0, [30.0, 10.0]));
    }

    #[test]
    fn looks_come_every_one_to_one_point_three_seconds_and_sooner_dashing() {
        let mut w = lone_vehicle();
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..40 {
            w.tick();
            let n = w.look_ticks(0, false);
            assert!((10..=13).contains(&n), "{n}");
            seen.insert(n);
            assert!((3..=6).contains(&w.look_ticks(0, true)));
        }
        assert!(seen.len() > 1, "{seen:?}");
    }

    /// The lone vehicle at 10,10 facing north on a leg north along x = 12,
    /// 2 to its right, with `blocked` tiles.
    fn off_its_line(blocked: &[(i32, i32)]) -> World {
        let mut w = lone_vehicle();
        for &(x, y) in blocked {
            w.pathing.set_blocked(Tile::new(x, y));
        }
        let u = &mut w.units[0];
        u.path = vec![[12.0, 36.0]];
        u.from = [12.0, 0.0];
        u.goal = Some([12.0, 36.0]);
        u.take_leg(0.1);
        w
    }

    #[test]
    fn units_off_their_line_make_no_detour_through_what_is_in_the_way() {
        let mut w = off_its_line(&[]);
        w.think(0);
        assert_eq!(w.units[0].path.len(), 2, "{:?}", w.units[0].path);
        // The turn-in point, 2 ahead at 45°, is on a blocked tile.
        let mut w = off_its_line(&[(6, 6)]);
        w.think(0);
        assert_eq!(w.units[0].path, [[12.0, 36.0]]);
        assert!(w.units[0].leg.is_some());
    }

    #[test]
    fn corners_are_led_into_past_a_point_lying_on_the_next() {
        let mut w = lone_vehicle();
        let u = &mut w.units[0];
        // North to 10,20, then a point all but on it, then east.
        u.path = vec![[30.0, 20.0], [10.0, 20.0001], [10.0, 20.0]];
        let [x, y] = u.onward().unwrap();
        assert!((x - 1.0).abs() < 1e-3 && y.abs() < 1e-3, "{x} {y}");
        u.take_leg(0.1);
        // A right angle round a radius of 2 is led into 2 early.
        let aim = u.leg.unwrap().aim;
        assert!(distance(aim, [10.0, 18.0]) < 1e-3, "{aim:?}");
        // With nothing after those two, there's no corner to lead into.
        u.path = vec![[10.0, 20.0001], [10.0, 20.0]];
        assert_eq!(u.onward(), None);
        // Nor on a leg straight for a spot of its own.
        u.path = vec![[30.0, 20.0], [10.0, 20.0]];
        (u.leg, u.straight) = (None, true);
        u.take_leg(0.1);
        assert_eq!(u.leg.unwrap().aim, [10.0, 20.0]);
    }

    #[test]
    fn legs_are_passed_within_the_datas_reach_of_their_end() {
        let mut w = lone_vehicle();
        let u = &mut w.units[0];
        u.path = vec![[10.0, 30.0], [10.0, 10.4]];
        u.take_leg(0.1);
        assert_eq!(u.path.len(), 2);
        u.leg = None;
        u.take_leg(0.5);
        assert_eq!(u.path, [[10.0, 30.0]]);
    }

    #[test]
    fn close_destinations_go_straight_only_before_the_move_has_planned() {
        let mut w = lone_vehicle();
        // An exact move 1.5 ahead, and one 1.5 behind.
        for to in [[10.0, 11.5], [10.0, 8.5]] {
            let first = w.plan(0, to, true, true).unwrap();
            assert!(first.straight && first.points == [to]);
            let again = w.plan(0, to, true, false).unwrap();
            assert!(!again.straight);
        }
        // Inside its turning circle, it turns to face it at once, but only
        // on a first plan.
        let beside = [11.0, 10.5];
        assert!(!w.plan(0, beside, false, false).unwrap().straight);
        assert_eq!(w.units[0].facing, [0.0, 1.0]);
        assert!(w.plan(0, beside, false, true).unwrap().straight);
        assert_ne!(w.units[0].facing, [0.0, 1.0]);
    }

    #[test]
    fn a_move_to_somewhere_already_near_has_to_end_right_there() {
        let mut w = lone_vehicle();
        w.take_place(0, [10.0, 10.4], [0.0, 1.0], false);
        assert!(w.units[0].exact);
        w.take_place(0, [10.0, 14.0], [0.0, 1.0], false);
        assert!(!w.units[0].exact);
    }

    #[test]
    fn a_regiment_whose_move_fails_keeps_where_it_stands_and_drops_an_attack() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        // An island to the north, cut off by water.
        for x in 0..20 {
            pathing.set(Tile::new(x, 10), Domain::Naval);
        }
        let mut w = world(pathing);
        w.rules.terrain_rules.areas = true;
        // The right-hand man stands a little off his place.
        w.units[1].position = [11.3, 10.0];
        let position = w.organizations[0].position;
        w.advance(7, 0, [10.0, 30.0], crate::Order::Attack, None)
            .unwrap();
        let r = &w.organizations[0];
        assert_eq!(r.order, crate::Order::Move);
        assert!(!r.travelling && r.position == position);
        // Already in his place, near enough, he stays put.
        assert!(!w.units[1].moving());
        run(&mut w, 2.0);
        assert_eq!(w.units[1].position, [11.3, 10.0]);
    }

    #[test]
    fn a_member_that_gives_up_on_its_place_is_sent_once_more_and_then_left() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        // The right-hand man's place at 11,20 is blocked.
        pathing.set_blocked(Tile::new(5, 10));
        let mut w = world(pathing);
        moved(&mut w, [10.0, 20.0]);
        while w.organizations[0].route_end.is_some() {
            w.tick();
        }
        // Once everyone's stopped, it's sent again, then left be.
        assert!(w.organizations[0].resend_places);
        run(&mut w, 5.0);
        assert!(!w.organizations[0].resend_places);
        let at = w.units[1].position;
        assert!(!within(at, [11.0, 20.0], ARRIVAL_REACH));
        for _ in 0..(10.0 / crate::TICK) as usize {
            w.tick();
            assert!(w.units[1].goal.is_none() && w.units[1].position == at);
        }
    }

    #[test]
    fn trailers_tow_trailers_of_their_own() {
        let second = tgmap::Trailer {
            template: "limber".into(),
            hitch: [0.0, -1.0],
            coupling: [0.0, 1.0],
            placed: None,
            trailer: None,
        };
        let first = tgmap::Trailer {
            template: "gun".into(),
            hitch: [0.0, -2.0],
            coupling: [0.0, 1.0],
            placed: None,
            trailer: Some(Box::new(second)),
        };
        let mut w = vehicle(0.0, 0.0, Some(first));
        // Straight behind: the first at 10,7, its own at 10,5.
        let t = w.units[0].trailer.as_ref().unwrap();
        assert!(near(t.trailer.as_ref().unwrap().position, [10.0, 5.0]));
        moved(&mut w, [30.0, 10.0]);
        run(&mut w, 15.0);
        let t = w.units[0].trailer.as_ref().unwrap();
        let towed = t.trailer.as_ref().unwrap();
        let hitch = tgmap::from_local(t.position, t.facing, [0.0, -1.0]);
        let coupled = tgmap::from_local(towed.position, towed.facing, [0.0, 1.0]);
        assert!(near(coupled, hitch), "{coupled:?} {hitch:?}");
        assert!(near(towed.facing, [1.0, 0.0]), "{:?}", towed.facing);
    }
}
