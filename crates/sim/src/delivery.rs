//! Deliveries: organizations a building recruits or an operation summons to a
//! place, flown in from the map's edge and dropped there by parachute.

use std::f32::consts::FRAC_PI_4;

use aaa_map::{Airlift, Domain, Id, Obstacle, Shape};

use crate::{Notice, Refused, Seen, TICK, World, distance, normalised, within};

/// The game's own ticks a second, which what falls comes down by: each
/// one it drops by gravity times the tick's length squared.
const GAME_TICKS: f32 = 16.0;

/// How far around a blocked landing spot a clear one is looked for, in
/// rings a world unit apart of [`RING`] points each.
const RINGS: u32 = 16;
const RING: u32 = 8;

/// The slowest an aircraft flies, as it does coming in from rest.
const CREEP: f32 = 0.1;

/// How long a stretch an aircraft changes speed over: a longer one is
/// cut into equal pieces about this long, and it's the first of them.
const PIECE: f32 = 20.0;

/// The longest an aircraft waits to let the next go, in seconds.
const HOLD: f32 = 0.6;

/// An aircraft's change of speed, which goes evenly with how far it's
/// flown of a stretch rather than with time.
#[derive(Debug, Clone, Copy)]
struct Ramp {
    from: f32,
    to: f32,
    /// How far it changes over, and how far of that it's flown.
    over: f32,
    gone: f32,
}

impl Ramp {
    /// From `from` to `to` over a stretch `over` long, `gone` of it
    /// flown; a stretch of no length is over once any of it's flown.
    fn new(from: f32, to: f32, over: f32, gone: f32) -> Ramp {
        Ramp {
            from,
            to,
            over: over.max(f32::MIN_POSITIVE),
            gone,
        }
    }

    /// From `from` to `to` over the first piece of a stretch `length`
    /// long.
    fn over_stretch(from: f32, to: f32, length: f32) -> Ramp {
        let pieces = match length > PIECE + CREEP {
            true => libm::floorf(length / PIECE),
            false => 1.0,
        };
        Ramp::new(from, to, length / pieces, 0.0)
    }

    fn speed(&self) -> f32 {
        let done = (self.gone / self.over).min(1.0);
        (self.from + (self.to - self.from) * done).max(CREEP)
    }

    /// How far it flies in a tick, which is flown.
    fn fly(&mut self) -> f32 {
        let step = self.speed() * TICK;
        self.gone += step;
        step
    }
}

/// An aircraft bringing an organization onto the map.
#[derive(Clone)]
pub struct Delivery {
    /// Its object's id.
    pub id: u32,
    /// The aircraft's template.
    pub template: Id,
    pub owner: u32,
    pub position: [f32; 2],
    /// Where it was a tick ago, as [`Unit::previous`](crate::Unit::previous).
    pub previous: [f32; 2],
    /// The way it's flying, a unit vector.
    pub facing: [f32; 2],
    /// How far above the ground it flies.
    pub height: f32,
    /// Where it was sent to drop its organization.
    pub target: [f32; 2],
    /// The HQ whose place it holds for its organization, by its
    /// organization's object id.
    pub hq: Option<u32>,
    /// The organization it's dropping, by index into [`World::organizations`],
    /// once its first member is out.
    pub organization: Option<usize>,
    /// The tick it left the map on.
    pub left: Option<u64>,
    /// What it's yet to begin dropping.
    cargo: Option<Box<aaa_map::Organization>>,
    airlift: Airlift,
    /// Where it came in, and goes out.
    entry: [f32; 2],
    /// The way its run goes, from where it came in over its target.
    line: [f32; 2],
    /// How far along its run it's flown, and how far along the mark
    /// it lets the next go at is, if it hasn't waited too long by then.
    flown: f32,
    mark: f32,
    ramp: Ramp,
    /// How many members it's let go, dropped or not, and how many
    /// seconds ago the last, once one's gone.
    released: usize,
    held: Option<f32>,
}

impl Delivery {
    /// The template of the organization it brings.
    pub fn cargo(&self) -> Option<&Id> {
        self.cargo.as_ref().map(|c| &c.template)
    }

    /// Where it is `t` of the way from the last tick to this one.
    pub fn drawn_at(&self, t: f32) -> [f32; 2] {
        let ([px, py], [x, y]) = (self.previous, self.position);
        [px + (x - px) * t, py + (y - py) * t]
    }

    /// How far short of where the tick has it its next drop was due, at
    /// its mark or once it had held the next as long as it will; `None`
    /// for one not due.
    fn due(&self, members: usize) -> Option<f32> {
        let past_mark = Some(self.flown - self.mark).filter(|p| *p >= 0.0);
        // Ticks don't add up to the hold exactly.
        let over = self.held.map(|h| h - HOLD).filter(|o| *o > -TICK / 100.0);
        let past_hold = over.map(|o| o.max(0.0) * self.ramp.speed());
        let past = match (past_mark, past_hold) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        past.filter(|_| self.released < members)
    }
}

/// A unit coming down by parachute.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Descent {
    /// How far above the ground it is, and was a tick ago.
    pub height: f32,
    pub previous: f32,
    /// How far up it was let go, where, and where it lands.
    from: f32,
    start: [f32; 2],
    lands: [f32; 2],
}

impl Descent {
    /// Where it lands.
    pub fn lands(&self) -> [f32; 2] {
        self.lands
    }

    /// How far above the ground it was let go.
    pub fn dropped_from(&self) -> f32 {
        self.from
    }

    /// How much of its fall is still ahead of it, `t` of the way from
    /// the last tick to this one: 1 as it's let go, 0 on the ground.
    pub fn left(&self, t: f32) -> f32 {
        match self.from > 0.0 {
            true => (self.previous + (self.height - self.previous) * t) / self.from,
            false => 0.0,
        }
    }
}

impl World {
    /// Whether an organization of `template` could be delivered to `at` for
    /// `player`: every member in its place there, and the organization's own
    /// spot, on ground the side has explored with room to stand. What
    /// stands on ground out of its sight doesn't count against it.
    pub fn room_to_land(&self, player: u32, template: &Id, at: [f32; 2]) -> bool {
        let Some(mut organization) = self.rules.raisable.get(template).cloned() else {
            return false;
        };
        organization.place(at, [0.0, -1.0]);
        let members = organization.members.iter().map(|m| (m.position, m.shape));
        members.chain([(at, None)]).all(|(position, shape)| {
            let spot = spot(position, [0.0, -1.0], shape);
            // As the least explored of the ground under it is.
            let (lo, hi) = spot.bounds(spot.shape);
            let corners = [lo, hi, [lo[0], hi[1]], [hi[0], lo[1]]];
            let under = corners.into_iter().chain([position]);
            let seen = under.map(|c| self.seen(player, c)).min_by_key(|s| match s {
                Seen::Unexplored => 0,
                Seen::Explored => 1,
                Seen::Visible => 2,
            });
            match seen.unwrap_or(Seen::Unexplored) {
                Seen::Unexplored => false,
                Seen::Explored => self.pathing.room_for(&spot, Domain::Land, []),
                Seen::Visible => self.clear(&spot),
            }
        })
    }

    /// Whether `spot` is on the map, on land, and clear of everything
    /// that blocks.
    pub(crate) fn clear(&self, spot: &Obstacle) -> bool {
        let standing = self.buildings.iter().filter(|b| !b.gone());
        let others = standing.filter_map(|b| b.obstacle.as_ref());
        self.pathing
            .room_for(spot, Domain::Land, others.chain(&self.scenery))
    }

    pub(crate) fn may_deliver(
        &self,
        player: u32,
        building: usize,
        template: &Id,
        to: [f32; 2],
    ) -> Result<(), Refused> {
        self.may_commission(player, building, template)?;
        if !self.rules.airlifts.contains_key(template) {
            return Err(Refused::Unable);
        }
        match self.room_to_land(player, template, to) {
            true => Ok(()),
            false => Err(Refused::NoRoom),
        }
    }

    /// The deliveries holding a place at the HQ `hq` for an organization
    /// they've yet to begin dropping.
    pub fn bound_for(&self, hq: u32) -> impl Iterator<Item = usize> + '_ {
        (0..self.deliveries.len()).filter(move |&d| {
            let delivery = &self.deliveries[d];
            delivery.hq == Some(hq) && delivery.cargo.is_some() && delivery.left.is_none()
        })
    }

    /// The upkeep of what a delivery has yet to begin dropping, which it
    /// costs its owner in its place until it leaves.
    pub(crate) fn delivery_ledger(&self, d: usize) -> crate::Ledger {
        let delivery = &self.deliveries[d];
        let cargo = delivery.cargo.as_ref().filter(|_| delivery.left.is_none());
        let upkeep = cargo.map(|c| self.commission_ledger(c).upkeep);
        crate::Ledger {
            upkeep: upkeep.unwrap_or_default(),
            ..Default::default()
        }
    }

    /// Sends an aircraft in to drop `organization`, which the building at `b`
    /// recruited, at `to`; false for an organization nothing flies.
    pub(crate) fn dispatch(
        &mut self,
        b: usize,
        organization: aaa_map::Organization,
        to: [f32; 2],
    ) -> bool {
        let building = &self.buildings[b];
        let (player, from, facing) = (building.owner, building.position, building.facing);
        let (entry, hq) = (self.entry(from, to), self.hq_of(b));
        let Some(d) = self.airlift(player, organization, entry, to, facing, hq) else {
            return false;
        };
        self.notices.push(Notice::Recruited {
            player,
            building: b,
            delivery: d,
        });
        true
    }

    /// Sends an aircraft in to drop `organization` at `to` for `player`
    /// with no building behind it, coming in from the edge farthest from
    /// there; false for an organization nothing flies.
    pub(crate) fn fly_in(
        &mut self,
        player: u32,
        organization: aaa_map::Organization,
        to: [f32; 2],
    ) -> bool {
        let entry = self.farthest_edge(to);
        let facing = crate::summons::FACING;
        (self.airlift(player, organization, entry, to, facing, None)).is_some()
    }

    /// Sends an aircraft in from `entry` to drop `organization` at `to`,
    /// holding a place for it at `hq`, returning its index.
    fn airlift(
        &mut self,
        owner: u32,
        organization: aaa_map::Organization,
        entry: [f32; 2],
        to: [f32; 2],
        facing: [f32; 2],
        hq: Option<u32>,
    ) -> Option<usize> {
        let airlift = self.rules.airlifts.get(&organization.template).cloned()?;
        let line = normalised([to[0] - entry[0], to[1] - entry[1]]).unwrap_or(facing);
        // The drop is centred on the target as if it were flown at half
        // speed throughout, one let go every `delay`.
        let pace = airlift.speed / 2.0;
        let lead = (organization.members.iter())
            .map(|m| self.carry(&m.template, airlift.height, pace))
            .fold(0.0, f32::max);
        let before = (organization.members.len().saturating_sub(1) / 2) as f32;
        let (d, id) = (self.deliveries.len(), self.take_ids(1));
        let mark = distance(entry, to) - lead - airlift.delay * pace * before;
        self.deliveries.push(Delivery {
            id,
            template: airlift.transport.clone(),
            owner,
            position: entry,
            previous: entry,
            facing: line,
            height: airlift.height,
            target: to,
            hq,
            organization: None,
            left: None,
            cargo: Some(Box::new(organization)),
            entry,
            line,
            flown: 0.0,
            mark,
            // In from rest.
            ramp: Ramp::over_stretch(0.0, airlift.speed, mark),
            released: 0,
            held: None,
            airlift,
        });
        Some(d)
    }

    /// Where on the map's edge an aircraft with nothing to fly in over
    /// comes in to `to`: straight along whichever axis has the farther
    /// edge from it, from that edge.
    fn farthest_edge(&self, to: [f32; 2]) -> [f32; 2] {
        let size = self.pathing.size();
        let [x, y] = to;
        let (dx, dy) = (x.max(size - x), y.max(size - y));
        let edge = |c: f32| if c > size - c { 0.0 } else { size };
        let entry = match dx > dy {
            true => [edge(x), y],
            false => [x, edge(y)],
        };
        self.pathing.inside(entry)
    }

    /// Where on the map's edge an aircraft sent from `from` to `to` comes
    /// in: where the line from `to` through `from`, carried on far past
    /// the map, crosses it, so that it flies in over where it was sent
    /// from.
    fn entry(&self, from: [f32; 2], to: [f32; 2]) -> [f32; 2] {
        let size = self.pathing.size();
        let far = [size * (from[0] - to[0]), size * (from[1] - to[1])];
        // Where the way from `to` to `far` crosses the line `axis` = `at`,
        // if it does within the map's side there.
        let crossing = |axis: usize, at: f32| {
            let (a, b) = (to[axis] - at, far[axis] - at);
            let other = 1 - axis;
            let along = to[other] + (far[other] - to[other]) * a / (a - b);
            let mut point = [at; 2];
            point[other] = along;
            (a * b < 0.0 && (0.0..=size).contains(&along)).then_some(point)
        };
        let sides = [(1, 0.0), (0, size), (1, size), (0, 0.0)];
        let crossed = sides.into_iter().find_map(|(axis, at)| crossing(axis, at));
        self.pathing.inside(crossed.unwrap_or([0.0, 0.0]))
    }

    /// How far on from where it's let go something of `template` lands,
    /// dropped from `height` by what's flying at `speed`.
    fn carry(&self, template: &Id, height: f32, speed: f32) -> f32 {
        let drag = self.rules.falls.get(template).map_or(0.0, |f| f.drag);
        let seconds = libm::sqrtf(2.0 * height.max(0.0) / self.rules.gravity);
        (seconds * speed - drag).max(0.0)
    }

    /// Flies deliveries on, dropping what they bring as they pass over
    /// where it goes, and brings down what's been dropped.
    pub(crate) fn deliver(&mut self) {
        for d in 0..self.deliveries.len() {
            if self.deliveries[d].left.is_none() {
                self.fly(d);
            }
        }
        self.descend();
        for d in 0..self.deliveries.len() {
            let delivery = &self.deliveries[d];
            let Some(r) = delivery.organization else {
                continue;
            };
            let organization = &self.organizations[r];
            let out = delivery.released == organization.units.len();
            let down = (organization.units.iter()).all(|&u| self.units[u].descent.is_none());
            if organization.arriving && out && down {
                self.organizations[r].arriving = false;
            }
        }
    }

    fn fly(&mut self, d: usize) {
        let delivery = &self.deliveries[d];
        let members = match (&delivery.cargo, delivery.organization) {
            (Some(cargo), _) => cargo.members.len(),
            (None, Some(r)) => self.organizations[r].units.len(),
            (None, None) => 0,
        };
        let speed = delivery.airlift.speed;
        if delivery.released < members {
            let delivery = &mut self.deliveries[d];
            delivery.flown += delivery.ramp.fly();
            delivery.held = delivery.held.map(|h| h + TICK);
            delivery.previous = delivery.position;
            let (flown, [lx, ly]) = (delivery.flown, delivery.line);
            delivery.position = [
                delivery.entry[0] + lx * flown,
                delivery.entry[1] + ly * flown,
            ];
            if let Some(past) = delivery.due(members) {
                let pace = delivery.ramp.speed();
                self.release(d, past);
                let delivery = &mut self.deliveries[d];
                delivery.held = Some(past / pace);
                delivery.ramp = match delivery.released < members {
                    // Slowing to half speed by its next mark.
                    true => Ramp::new(pace, speed / 2.0, delivery.airlift.delay * speed, past),
                    false => {
                        let home = distance(delivery.position, delivery.entry);
                        Ramp::over_stretch(pace, speed, home)
                    }
                };
            }
            return;
        }
        // Home the way it came, turning as tightly as it can.
        let delivery = &mut self.deliveries[d];
        let entry = delivery.entry;
        let step = delivery.ramp.fly();
        delivery.previous = delivery.position;
        if within(delivery.position, entry, step) {
            delivery.position = entry;
            delivery.left = Some(self.ticks);
            return;
        }
        let [fx, fy] = delivery.facing;
        let [right, ahead] = aaa_map::to_local(delivery.position, delivery.facing, entry);
        let off = libm::atan2f(right, ahead);
        let most = match delivery.airlift.turn_radius > 0.0 {
            true => step / delivery.airlift.turn_radius,
            false => f32::INFINITY,
        };
        // Clockwise seen from above for what's to its right.
        let turn = off.clamp(-most, most);
        let (sin, cos) = (libm::sinf(turn), libm::cosf(turn));
        let facing = [fx * cos + fy * sin, fy * cos - fx * sin];
        delivery.facing = normalised(facing).unwrap_or(delivery.facing);
        delivery.position[0] += delivery.facing[0] * step;
        delivery.position[1] += delivery.facing[1] * step;
    }

    /// Lets go the next of what the delivery at `d` brings, `past` short
    /// of where the tick has it, putting its organization on the map with the
    /// first.
    fn release(&mut self, d: usize, past: f32) {
        if let Some(mut cargo) = self.deliveries[d].cargo.take() {
            let delivery = &self.deliveries[d];
            let (target, line, hq) = (delivery.target, delivery.line, delivery.hq);
            cargo.owner = delivery.owner;
            cargo.place(target, line);
            let r = self.add_new_organization(*cargo);
            let organization = &mut self.organizations[r];
            (organization.position, organization.facing) = (target, line);
            (organization.hq, organization.arriving) = (hq, true);
            for u in organization.units.clone() {
                self.units[u].awaited = true;
            }
            self.deliveries[d].organization = Some(r);
            log::debug!(
                "{} {} dropping from {} {}",
                self.organizations[r].template,
                self.organizations[r].id,
                self.deliveries[d].template,
                self.deliveries[d].id
            );
        }
        let delivery = &mut self.deliveries[d];
        let Some(r) = delivery.organization else {
            return;
        };
        let u = self.organizations[r].units[delivery.released];
        let along = (delivery.flown - past).max(0.0);
        let (line, target) = (delivery.line, delivery.target);
        let at = [
            delivery.entry[0] + line[0] * along,
            delivery.entry[1] + line[1] * along,
        ];
        delivery.released += 1;
        delivery.mark += delivery.airlift.delay * delivery.airlift.speed;
        // Thrown on at the speed it's flying as it lets go.
        let (height, pace) = (delivery.airlift.height, delivery.ramp.speed());
        let template = &self.units[u].template;
        let own = self.rules.falls.get(template).map_or(0.0, |f| f.height);
        let from = (height - own).max(0.0);
        let on = self.carry(template, from, pace);
        let aimed = (self.pathing).inside([at[0] + line[0] * on, at[1] + line[1] * on]);
        let shape = self.units[u].shape;
        // One with nowhere to land within reach never arrives.
        let Some(lands) = self.landing(aimed, line, shape) else {
            return;
        };
        let unit = &mut self.units[u];
        unit.stand_at(at, line);
        unit.descent = Some(Descent {
            height: from,
            previous: from,
            from,
            start: at,
            lands,
        });
        // Its organization is where its leader is: from when he's let go,
        // formed around where he comes down, unless it's been sent on.
        let organization = &self.organizations[r];
        let leads = organization.leader.map(|p| organization.units[p]) == Some(u);
        if leads && organization.position == target {
            let offsets = &organization.formations[organization.current_formation()].offsets;
            let p = organization.leader.unwrap_or(0);
            let [right, ahead] = offsets.get(p).copied().unwrap_or([0.0, 0.0]);
            let around = aaa_map::from_local(lands, organization.facing, [-right, -ahead]);
            self.organizations[r].position = around;
            self.take_places(r);
        }
    }

    /// The clear spot nearest `aimed` for something of `shape` to land
    /// on: there, else the first clear of the points on rings around it,
    /// nearest ring first, each ring's from the one along `line` round
    /// clockwise seen from above.
    fn landing(&self, aimed: [f32; 2], line: [f32; 2], shape: Option<Shape>) -> Option<[f32; 2]> {
        let clear = |at: [f32; 2]| self.clear(&spot(at, line, shape)).then_some(at);
        let rings = (1..=RINGS).flat_map(|ring| (0..RING).map(move |i| (ring as f32, i as f32)));
        let around = rings.map(|(radius, i)| {
            let angle = i * FRAC_PI_4;
            let offset = [radius * libm::sinf(angle), radius * libm::cosf(angle)];
            aaa_map::from_local(aimed, line, offset)
        });
        clear(aimed).or_else(|| around.into_iter().find_map(clear))
    }

    /// Brings down what's been dropped, straight from where it was let
    /// go to where it lands, where it joins its organization and makes for
    /// its place in it.
    fn descend(&mut self) {
        let fallen = self.rules.gravity / GAME_TICKS * TICK;
        for u in 0..self.units.len() {
            let unit = &mut self.units[u];
            let Some(mut descent) = unit.descent else {
                continue;
            };
            descent.previous = descent.height;
            descent.height = (descent.height - fallen).max(0.0);
            if descent.height > 0.0 {
                let done = 1.0 - descent.height / descent.from;
                let ([sx, sy], [lx, ly]) = (descent.start, descent.lands);
                unit.position = [sx + (lx - sx) * done, sy + (ly - sy) * done];
                unit.descent = Some(descent);
                continue;
            }
            let facing = unit.facing;
            unit.stand_at(descent.lands, facing);
            unit.previous = unit.position;
            unit.descent = None;
            unit.awaited = false;
            unit.health = unit.fighter.as_ref().map_or(0.0, |f| f.health);
            // Down, it makes for its place in its organization's formation.
            let organization = &self.organizations[unit.organization];
            let p = organization.units.iter().position(|&m| m == u).unwrap_or(0);
            let offsets = &organization.formations[organization.current_formation()].offsets;
            let offset = offsets.get(p).copied().unwrap_or([0.0, 0.0]);
            let facing = organization.facing;
            let place = aaa_map::from_local(organization.position, facing, offset);
            self.take_place(u, place, facing, false);
        }
    }
}

/// The ground something of `shape` takes up standing at `at`.
pub(crate) fn spot(at: [f32; 2], facing: [f32; 2], shape: Option<Shape>) -> Obstacle {
    let shape = shape.unwrap_or(Shape::Point);
    Obstacle {
        position: at,
        facing,
        shape,
        soft: shape,
    }
}

crate::hash::digest!(Ramp {
    from,
    to,
    over,
    gone
});

crate::hash::digest!(Delivery {
    id, template, owner, position, previous, facing, height, target, hq, organization, left,
    entry, line, flown, mark, ramp, released, held,
} skip {
    // Its template's.
    cargo, airlift,
});

crate::hash::digest!(Descent {
    height,
    previous,
    from,
    start,
    lands
});

#[cfg(test)]
mod tests {
    use aaa_map::{Airlift, Fall, Id};

    use crate::testing::*;
    use crate::{Command, Notice, Refused, World, distance};

    /// [`headquarters`], which can also have a [`raised_company`] flown
    /// in by a plane like the game's, taking a second to send for it.
    fn airfield() -> World {
        let mut w = headquarters();
        let mut company = raised_company(&mut w, 1);
        // Three abreast, led by the last one out.
        company.formations[0].offsets = vec![[-1.0, 0.0], [0.0, 0.0], [1.0, 0.0]];
        company.primary = Some(2);
        recruits(&mut w, "house", company, 1.0);
        w.rules.airlifts.insert(
            Id::from("company"),
            Airlift {
                transport: "plane".into(),
                speed: 7.0,
                turn_radius: 18.0,
                height: 17.0,
                delay: 0.4,
            },
        );
        let fall = Fall {
            height: 2.0,
            drag: 0.0,
        };
        w.rules.falls.insert(Id::from("soldier"), fall);
        w
    }

    fn deliver(to: [f32; 2]) -> Command {
        Command::Deliver {
            building: 0,
            template: Id::from("company"),
            to,
        }
    }

    #[test]
    fn deliveries_fly_in_over_their_building_and_drop_a_line_along_their_way() {
        let mut w = airfield();
        let to = [40.0, 20.0];
        w.apply(1, deliver(to)).unwrap();
        assert_eq!(w.buildings[0].queue.len(), 1);
        run(&mut w, 1.0);
        // From the edge behind the building, a tile in: on the way from
        // the target to a point as far off the map as the building is
        // from the target, times the map's size.
        let plane = &w.deliveries[0];
        let entry = [2.0, 20.0 - 20.0 * 40.0 / 1640.0];
        let line = plane.facing;
        assert!(near(plane.previous, entry) && line[0] > 0.99 && line[1] > 0.0);
        assert_eq!(plane.cargo(), Some(&Id::from("company")));
        let sent = Notice::Recruited {
            player: 1,
            building: 0,
            delivery: 0,
        };
        assert!(w.notices.contains(&sent));
        // It costs what it carries would, and nothing's out yet.
        let upkeep = w.ledger(1).upkeep;
        assert_eq!(upkeep.get("ammo"), 16.0);
        assert!(w.organizations.is_empty());
        // The organization is there from the first one out, none yet down.
        while w.deliveries[0].organization.is_none() {
            w.tick();
        }
        let r = &w.organizations[0];
        assert!(r.arriving && !w.destroyed(0) && !w.lacks_supply(0));
        assert_eq!((r.position, r.facing, w.centre(0)), (to, line, to));
        assert!(w.survivors(0).next().is_none());
        assert_eq!(w.ledger(1).upkeep, upkeep);
        let falling = w.units[0].descent.expect("falling");
        assert_eq!(falling.previous, 15.0);
        run(&mut w, 2.0);
        let lower = w.units[0].descent.expect("falling");
        assert!(lower.height < 10.0 && w.units[0].position[0] > 30.0);
        // They come down in a line along its way.
        let mut landed = [[0.0; 2]; 3];
        for _ in 0..80 {
            w.tick();
            for (u, unit) in w.units.iter().enumerate() {
                if let Some(d) = unit.descent {
                    landed[u] = aaa_map::to_local(to, line, d.lands());
                }
            }
        }
        assert!(landed.iter().all(|p| p[0].abs() < 1e-3), "{landed:?}");
        // The first is let go half a gap and a fall's carry at half
        // speed short of the target, and thrown on at full speed; the
        // second at the mark the plane slows to half speed by; the third
        // as far on as it flies before it's held one as long as it will.
        let ahead: Vec<f32> = landed.iter().map(|p| p[1]).collect();
        let fall = (30.0f32 / 49.05).sqrt();
        let from = -1.4 - 3.5 * (34.0f32 / 49.05).sqrt();
        assert!((ahead[0] - (from + 7.0 * fall)).abs() < 1e-3, "{landed:?}");
        assert!(
            (ahead[1] - (from + 2.8 + 3.5 * fall)).abs() < 0.05,
            "{landed:?}"
        );
        assert!((ahead[2] - ahead[1] - 2.1).abs() < 0.05, "{landed:?}");
        // Once down they form up around where their leader landed,
        // facing the way the plane flew.
        run(&mut w, 8.0);
        let r = &w.organizations[0];
        assert!(!r.arriving && r.facing == line);
        assert!(w.units.iter().all(|u| u.alive()));
        let led = aaa_map::from_local(to, line, landed[2]);
        assert!(near(
            r.position,
            aaa_map::from_local(led, line, [-1.0, 0.0])
        ));
        for (u, right) in [(0, -1.0), (1, 0.0), (2, 1.0)] {
            let place = aaa_map::from_local(r.position, line, [right, 0.0]);
            let unit = &w.units[u];
            assert!(distance(unit.position, place) < 0.6 && unit.facing == line);
        }
        // Then home the way it came.
        run(&mut w, 30.0);
        assert!(w.deliveries[0].left.is_some());
        assert!(near(w.deliveries[0].position, entry));
    }

    #[test]
    fn aircraft_change_speed_over_the_first_piece_of_a_stretch() {
        // A stretch of 65 is three pieces.
        let mut ramp = super::Ramp::over_stretch(0.0, 7.0, 65.0);
        assert_eq!(ramp.speed(), super::CREEP);
        ramp.gone = 65.0 / 6.0;
        assert!((ramp.speed() - 3.5).abs() < 1e-4);
        ramp.gone = 30.0;
        assert_eq!(ramp.speed(), 7.0);
        // One with nowhere to go starts as it is.
        let mut ramp = super::Ramp::over_stretch(0.0, 7.0, -3.0);
        assert_eq!(ramp.speed(), super::CREEP);
        ramp.fly();
        assert_eq!(ramp.speed(), 7.0);
    }

    #[test]
    fn deliveries_need_explored_ground_with_room_for_every_member() {
        let mut w = airfield();
        let truck = Command::Deliver {
            building: 0,
            template: Id::from("truck"),
            to: [40.0, 20.0],
        };
        assert_eq!(w.check(1, &truck), Err(Refused::Unable));
        // On the building, and off the map.
        assert_eq!(w.check(1, &deliver([20.0, 20.0])), Err(Refused::NoRoom));
        assert_eq!(w.check(1, &deliver([90.0, 20.0])), Err(Refused::NoRoom));
        assert_eq!(w.check(1, &deliver([40.0, 20.0])), Ok(()));
        w.set_fog(aaa_map::FogSetting::Full);
        assert_eq!(w.check(1, &deliver([70.0, 70.0])), Err(Refused::NoRoom));
        assert!(w.buildings[0].queue.is_empty());
    }

    #[test]
    fn those_dropped_onto_what_blocks_land_beside_it() {
        let mut w = airfield();
        w.apply(1, deliver([40.0, 20.0])).unwrap();
        run(&mut w, 2.0);
        // A house goes up where the first would land.
        let house = building(30, 2, [41.0, 20.0]);
        w.pathing.block(house.obstacle.as_ref().unwrap());
        w.add_buildings(&[house]);
        while w.units.first().is_none_or(|u| u.descent.is_none()) {
            w.tick();
        }
        // On a ring out from where he'd have come down.
        let lands = w.units[0].descent.expect("falling").lands();
        let aimed = [
            40.0 - 1.4 - 3.5 * (34.0f32 / 49.05).sqrt() + 7.0 * (30.0f32 / 49.05).sqrt(),
            20.0,
        ];
        assert!(distance(lands, aimed) > 0.9, "{lands:?}");
        run(&mut w, 15.0);
        assert!(w.units[0].alive());
    }

    #[test]
    fn those_still_falling_follow_an_organization_sent_on() {
        let mut w = airfield();
        w.apply(1, deliver([40.0, 20.0])).unwrap();
        while w
            .deliveries
            .first()
            .is_none_or(|d| d.organization.is_none())
        {
            w.tick();
        }
        let to = [40.0, 50.0];
        w.apply(
            1,
            Command::Move {
                organization: 0,
                to,
            },
        )
        .unwrap();
        run(&mut w, 45.0);
        let at: Vec<_> = w.units.iter().map(|u| (u.alive(), u.position)).collect();
        assert!(
            at.iter().all(|&(alive, p)| alive && distance(p, to) < 1.5),
            "{at:?}"
        );
    }

    #[test]
    fn deliveries_on_their_way_hold_their_place_at_their_hq() {
        let mut w = division();
        let hq = Command::Unpack {
            organization: 0,
            at: [30.0, 40.0],
            facing: NORTH,
        };
        w.apply(1, hq).unwrap();
        run(&mut w, 30.0);
        let b = w.organizations[0].unpacked_into.expect("unpacked");
        w.buildings[b].deploy = w.buildings[0].deploy;
        let company = raised_company(&mut w, 1);
        recruits(&mut w, "depot", company, 1.0);
        let airlift = airfield().rules.airlifts.remove(&Id::from("company"));
        w.rules
            .airlifts
            .extend(airlift.map(|a| (Id::from("company"), a)));
        let order = Command::Deliver {
            building: b,
            template: Id::from("company"),
            to: [50.0, 40.0],
        };
        let hqs = w.hqs();
        let free = w.free_places(&hqs[0]);
        w.apply(1, order).unwrap();
        run(&mut w, 1.5);
        assert_eq!(w.deliveries.len(), 1);
        assert_eq!(w.bound_for(50).count(), 1);
        assert_eq!(w.free_places(&hqs[0]), free - 1);
        run(&mut w, 30.0);
        let r = w.deliveries[0].organization.expect("dropped");
        assert_eq!(w.organizations[r].hq, Some(50));
        assert_eq!(w.free_places(&hqs[0]), free - 1);
    }
}
