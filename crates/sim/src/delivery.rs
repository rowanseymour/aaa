//! Deliveries: regiments a building recruits to a place, flown in from
//! the map's edge and dropped there by parachute.

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

/// An aircraft bringing a regiment onto the map.
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
    /// Where it was sent to drop its regiment.
    pub target: [f32; 2],
    /// The HQ whose place it holds for its regiment, by its
    /// organization's object id.
    pub hq: Option<u32>,
    /// The regiment it's dropping, by index into [`World::regiments`],
    /// once its first member is out.
    pub regiment: Option<usize>,
    /// The tick it left the map on.
    pub left: Option<u64>,
    /// What it's yet to begin dropping.
    cargo: Option<Box<aaa_map::Regiment>>,
    airlift: Airlift,
    /// Where it came in, and goes out.
    entry: [f32; 2],
    /// The way its run goes, from where it came in over its target.
    line: [f32; 2],
    /// How far along its run it's flown, and how far along the next
    /// drop is.
    flown: f32,
    next_drop: f32,
    /// How many members it's let go, dropped or not.
    released: usize,
}

impl Delivery {
    /// The template of the regiment it brings.
    pub fn cargo(&self) -> Option<&Id> {
        self.cargo.as_ref().map(|c| &c.template)
    }

    /// Where it is `t` of the way from the last tick to this one.
    pub fn drawn_at(&self, t: f32) -> [f32; 2] {
        let ([px, py], [x, y]) = (self.previous, self.position);
        [px + (x - px) * t, py + (y - py) * t]
    }

    /// Whether its next drop is due.
    fn due(&self, members: usize) -> bool {
        self.flown >= self.next_drop && self.released < members
    }

    /// Whether it's begun its drop, which it flies at half speed, and
    /// has more to let go.
    fn dropping(&self, members: usize) -> bool {
        (self.released > 0 || self.flown >= self.next_drop) && self.released < members
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
    /// Whether a regiment of `template` could be delivered to `at` for
    /// `player`: every member in its place there, and the regiment's own
    /// spot, on ground the side has explored with room to stand. What
    /// stands on ground out of its sight doesn't count against it.
    pub fn room_to_land(&self, player: u32, template: &Id, at: [f32; 2]) -> bool {
        let Some(mut regiment) = self.rules.raisable.get(template).cloned() else {
            return false;
        };
        regiment.place(at, [0.0, -1.0]);
        let members = regiment.members.iter().map(|m| (m.position, m.shape));
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
    fn clear(&self, spot: &Obstacle) -> bool {
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

    /// The deliveries holding a place at the HQ `hq` for a regiment
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

    /// Sends an aircraft in to drop `regiment`, which the building at `b`
    /// recruited, at `to`; false for a regiment nothing flies.
    pub(crate) fn dispatch(&mut self, b: usize, regiment: aaa_map::Regiment, to: [f32; 2]) -> bool {
        let Some(airlift) = self.rules.airlifts.get(&regiment.template).cloned() else {
            return false;
        };
        let building = &self.buildings[b];
        let (owner, from) = (building.owner, building.position);
        let entry = self.entry(from, to);
        let line = normalised([to[0] - entry[0], to[1] - entry[1]]).unwrap_or(building.facing);
        // It slows to half speed for the drop, which is centred on the
        // target as if those dropped were half as far apart as they are.
        let pace = airlift.speed / 2.0;
        let lead = (regiment.members.iter())
            .map(|m| self.carry(&m.template, airlift.height, pace))
            .fold(0.0, f32::max);
        let before = (regiment.members.len().saturating_sub(1) / 2) as f32;
        let (d, id, hq) = (self.deliveries.len(), self.take_ids(1), self.hq_of(b));
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
            regiment: None,
            left: None,
            cargo: Some(Box::new(regiment)),
            entry,
            line,
            flown: 0.0,
            next_drop: distance(entry, to) - lead - airlift.delay * pace * before,
            released: 0,
            airlift,
        });
        self.notices.push(Notice::Recruited {
            player: owner,
            building: b,
            delivery: d,
        });
        true
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
            let Some(r) = delivery.regiment else {
                continue;
            };
            let regiment = &self.regiments[r];
            let out = delivery.released == regiment.units.len();
            let down = (regiment.units.iter()).all(|&u| self.units[u].descent.is_none());
            if regiment.arriving && out && down {
                self.regiments[r].arriving = false;
            }
        }
    }

    fn fly(&mut self, d: usize) {
        let delivery = &self.deliveries[d];
        let members = match (&delivery.cargo, delivery.regiment) {
            (Some(cargo), _) => cargo.members.len(),
            (None, Some(r)) => self.regiments[r].units.len(),
            (None, None) => 0,
        };
        let speed = delivery.airlift.speed;
        if delivery.released < members {
            let pace = match delivery.dropping(members) {
                true => speed / 2.0,
                false => speed,
            };
            let delivery = &mut self.deliveries[d];
            delivery.flown += pace * TICK;
            delivery.previous = delivery.position;
            let (flown, [lx, ly]) = (delivery.flown, delivery.line);
            delivery.position = [
                delivery.entry[0] + lx * flown,
                delivery.entry[1] + ly * flown,
            ];
            while self.deliveries[d].due(members) {
                self.release(d);
            }
            return;
        }
        // Home the way it came, turning as tightly as it can.
        let delivery = &mut self.deliveries[d];
        let entry = delivery.entry;
        let step = speed * TICK;
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

    /// Lets go the next of what the delivery at `d` brings, putting its
    /// regiment on the map with the first.
    fn release(&mut self, d: usize) {
        if let Some(mut cargo) = self.deliveries[d].cargo.take() {
            let delivery = &self.deliveries[d];
            let (target, line, hq) = (delivery.target, delivery.line, delivery.hq);
            cargo.owner = delivery.owner;
            cargo.place(target, line);
            let r = self.add_new_regiment(*cargo);
            let regiment = &mut self.regiments[r];
            (regiment.position, regiment.facing) = (target, line);
            (regiment.hq, regiment.arriving) = (hq, true);
            for u in regiment.units.clone() {
                self.units[u].awaited = true;
            }
            self.deliveries[d].regiment = Some(r);
            log::debug!(
                "{} {} dropping from {} {}",
                self.regiments[r].template,
                self.regiments[r].id,
                self.deliveries[d].template,
                self.deliveries[d].id
            );
        }
        let delivery = &mut self.deliveries[d];
        let Some(r) = delivery.regiment else {
            return;
        };
        let u = self.regiments[r].units[delivery.released];
        // Let go where the drop was due, not where the tick has it.
        let along = delivery.next_drop.clamp(0.0, delivery.flown);
        let (line, target) = (delivery.line, delivery.target);
        let at = [
            delivery.entry[0] + line[0] * along,
            delivery.entry[1] + line[1] * along,
        ];
        delivery.released += 1;
        delivery.next_drop += delivery.airlift.delay * delivery.airlift.speed;
        let (height, pace) = (delivery.airlift.height, delivery.airlift.speed / 2.0);
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
        // Its regiment is where its leader is: from when he's let go,
        // formed around where he comes down, unless it's been sent on.
        let regiment = &self.regiments[r];
        let leads = regiment.leader.map(|p| regiment.units[p]) == Some(u);
        if leads && regiment.position == target {
            let offsets = &regiment.formations[regiment.current_formation()].offsets;
            let p = regiment.leader.unwrap_or(0);
            let [right, ahead] = offsets.get(p).copied().unwrap_or([0.0, 0.0]);
            let around = aaa_map::from_local(lands, regiment.facing, [-right, -ahead]);
            self.regiments[r].position = around;
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
    /// go to where it lands, where it joins its regiment and makes for
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
            // Down, it makes for its place in its regiment's formation.
            let regiment = &self.regiments[unit.regiment];
            let p = regiment.units.iter().position(|&m| m == u).unwrap_or(0);
            let offsets = &regiment.formations[regiment.current_formation()].offsets;
            let offset = offsets.get(p).copied().unwrap_or([0.0, 0.0]);
            let facing = regiment.facing;
            let place = aaa_map::from_local(regiment.position, facing, offset);
            self.take_place(u, place, facing, false);
        }
    }
}

/// The ground something of `shape` takes up standing at `at`.
fn spot(at: [f32; 2], facing: [f32; 2], shape: Option<Shape>) -> Obstacle {
    let shape = shape.unwrap_or(Shape::Point);
    Obstacle {
        position: at,
        facing,
        shape,
        soft: shape,
    }
}

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
        assert!(w.regiments.is_empty());
        // The regiment is there from the first one out, none yet down.
        while w.deliveries[0].regiment.is_none() {
            w.tick();
        }
        let r = &w.regiments[0];
        assert!(r.arriving && !w.destroyed(0) && !w.lacks_supply(0));
        assert_eq!((r.position, r.facing, w.centre(0)), (to, line, to));
        assert!(w.survivors(0).next().is_none());
        assert_eq!(w.ledger(1).upkeep, upkeep);
        let falling = w.units[0].descent.expect("falling");
        assert_eq!(falling.previous, 15.0);
        run(&mut w, 2.0);
        let lower = w.units[0].descent.expect("falling");
        assert!(lower.height < 10.0 && w.units[0].position[0] > 30.0);
        // They come down in a line, as far apart as the plane flew
        // between them.
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
        // The first short of the target by half a gap and how much
        // further its fall is reckoned to carry it than it does.
        let ahead: Vec<f32> = landed.iter().map(|p| p[1]).collect();
        let short = 1.4 + 3.5 * ((34.0f32 / 49.05).sqrt() - (30.0f32 / 49.05).sqrt());
        assert!((ahead[0] + short).abs() < 1e-3, "{landed:?}");
        assert!((ahead[1] - ahead[0] - 2.8).abs() < 1e-3, "{landed:?}");
        assert!((ahead[2] - ahead[1] - 2.8).abs() < 1e-3, "{landed:?}");
        // Once down they form up around where their leader landed,
        // facing the way the plane flew.
        run(&mut w, 8.0);
        let r = &w.regiments[0];
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
        let house = building(30, 2, [38.0, 20.0]);
        w.pathing.block(house.obstacle.as_ref().unwrap());
        w.add_buildings(&[house]);
        run(&mut w, 12.0);
        let first = w.units[0].position;
        assert!(w.units[0].alive());
        assert!((first[0] - 38.0).abs() > 3.0 || (first[1] - 20.0).abs() > 3.0);
    }

    #[test]
    fn those_still_falling_follow_a_regiment_sent_on() {
        let mut w = airfield();
        w.apply(1, deliver([40.0, 20.0])).unwrap();
        while w.deliveries.first().is_none_or(|d| d.regiment.is_none()) {
            w.tick();
        }
        let to = [40.0, 50.0];
        w.apply(1, Command::Move { regiment: 0, to }).unwrap();
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
            regiment: 0,
            at: [30.0, 40.0],
            facing: NORTH,
        };
        w.apply(1, hq).unwrap();
        run(&mut w, 30.0);
        let b = w.regiments[0].unpacked_into.expect("unpacked");
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
        run(&mut w, 20.0);
        let r = w.deliveries[0].regiment.expect("dropped");
        assert_eq!(w.regiments[r].hq, Some(50));
        assert_eq!(w.free_places(&hqs[0]), free - 1);
    }
}
