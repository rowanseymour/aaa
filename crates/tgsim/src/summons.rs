//! Special operations that bring something new onto the map for their
//! user, put down where they're aimed or flown in there.

use std::f32::consts::TAU;

use serde::{Deserialize, Serialize};
use tgmap::pathing::CELL_SIZE;
use tgmap::{Domain, Id, Obstacle, Shape, Summon, Tile};

use crate::delivery::spot;
use crate::{Notice, Piece, Refused, World, bounding_radius};

/// The way what's summoned faces.
pub(crate) const FACING: [f32; 2] = [0.0, -1.0];

/// How far from where a single thing is aimed a clear spot is looked for,
/// in rings a world unit apart of [`RING`] spots each.
const RINGS: u32 = 12;
const RING: u32 = 8;

/// The rings a regiment's members look through, each spaced by its own
/// size.
const REGIMENT_RINGS: u32 = 10;

/// What a special operation put on the map, for one held to one at a
/// time to wait on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summoned {
    pub player: u32,
    pub operation: Id,
    pub piece: Piece,
}

impl World {
    /// Whether `player` may summon with `operation` now: nothing it put
    /// down is still there, for one held to one at a time, and the side
    /// owns nothing with its unique flag.
    pub(crate) fn may_summon(
        &self,
        player: u32,
        operation: &Id,
        summon: &Summon,
    ) -> Result<(), Refused> {
        let waiting = summon.one_at_a_time
            && (self.summoned.iter())
                .any(|s| s.player == player && &s.operation == operation && self.there(s.piece));
        let unique = summon
            .unique
            .as_ref()
            .is_some_and(|flag| self.owns_flagged(player, flag));
        match waiting || unique {
            true => Err(Refused::Summoned),
            false => Ok(()),
        }
    }

    /// Whether what `piece` is is still on the map, in anyone's hands.
    fn there(&self, piece: Piece) -> bool {
        match piece {
            Piece::Organization(r) => !self.destroyed(r),
            Piece::Building(b) => !self.buildings[b].gone(),
        }
    }

    /// Whether `player` owns anything on the map whose kind has `flag`.
    fn owns_flagged(&self, player: u32, flag: &Id) -> bool {
        let flagged = |template: &Id| {
            let kind = self.rules.kinds.get(template);
            kind.is_some_and(|k| k.flags.contains(flag))
        };
        let organization = |r: usize| {
            let o = &self.organizations[r];
            o.owner == player && !self.destroyed(r) && flagged(&o.template)
        };
        let building = |b: &crate::Building| b.owner == player && !b.gone() && flagged(&b.template);
        (0..self.organizations.len()).any(organization) || self.buildings.iter().any(building)
    }

    /// Whether a summon can be aimed at `at`: anywhere, or for one held
    /// to the main land, on a tile of it.
    pub(crate) fn summons_to(&self, summon: &Summon, at: [f32; 2]) -> bool {
        let side = self.pathing.side();
        let tile = Tile::at(at).index(side);
        let main_land = self.main_land.get_or_init(|| self.find_main_land());
        !summon.main_land || tile.is_some_and(|t| main_land[t])
    }

    /// Which tiles, row by row, are of the map's main land: the body of
    /// land tiles, joined along their sides, with the most tiles, the first
    /// found of the largest.
    fn find_main_land(&self) -> Vec<bool> {
        let side = self.pathing.side();
        let tile = |i: usize| Tile {
            x: (i % side) as i32,
            y: (i / side) as i32,
        };
        let land: Vec<bool> = (0..side * side)
            .map(|i| self.lie(tile(i)) == Lie::Land)
            .collect();
        let mut body = vec![usize::MAX; side * side];
        let (mut main, mut most) = (None, 0);
        for start in 0..side * side {
            if !land[start] || body[start] != usize::MAX {
                continue;
            }
            body[start] = start;
            let (mut todo, mut count) = (vec![start], 0);
            while let Some(i) = todo.pop() {
                count += 1;
                let Tile { x, y } = tile(i);
                let sides = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)];
                for (x, y) in sides {
                    let next = Tile { x, y }.index(side).filter(|&n| land[n]);
                    if let Some(n) = next.filter(|&n| body[n] == usize::MAX) {
                        body[n] = start;
                        todo.push(n);
                    }
                }
            }
            if count > most {
                (main, most) = (Some(start), count);
            }
        }
        body.into_iter().map(|b| Some(b) == main).collect()
    }

    /// What a tile is, by its terrain and the map's own flags alone.
    fn lie(&self, tile: Tile) -> Lie {
        let land = self.pathing.crossable(tile, Domain::Land);
        let water = self.pathing.crossable(tile, Domain::Naval);
        match (land, water) {
            (true, false) => Lie::Land,
            (false, true) => Lie::Water,
            (false, false) => Lie::Impassable,
            (true, true) => {
                let terrain = self.tile_terrains.get(self.tile_terrains.at(tile.centre()));
                match (terrain.mountain, terrain.water) {
                    (true, _) => Lie::Mountain,
                    (_, true) => Lie::Water,
                    _ => Lie::Land,
                }
            }
        }
    }

    /// Everything that blocks a summon: buildings, scenery and units.
    pub(crate) fn blocking(&self) -> Vec<Obstacle> {
        self.blocking_but(None)
    }

    /// As [`World::blocking`], but for the building at `except`.
    pub(crate) fn blocking_but(&self, except: Option<usize>) -> Vec<Obstacle> {
        let units = self.unit_spots(|_| true);
        let buildings = (self.buildings.iter().enumerate())
            .filter(|&(i, b)| !b.gone() && Some(i) != except)
            .filter_map(|(_, b)| b.obstacle.clone());
        buildings
            .chain(self.scenery.iter().cloned())
            .chain(units)
            .collect()
    }

    /// The first of `spots` with room to stand for something of `shape`
    /// facing `facing`: on the map, on land, and clear of `blocking`.
    pub(crate) fn first_clear(
        &self,
        mut spots: impl Iterator<Item = [f32; 2]>,
        facing: [f32; 2],
        shape: Option<Shape>,
        blocking: &[Obstacle],
    ) -> Option<[f32; 2]> {
        spots.find(|&p| (self.pathing).room_for(&spot(p, facing, shape), Domain::Land, blocking))
    }

    /// Puts down what `operation` summons for `player` at the clear spot
    /// nearest `at`, telling the side if there's none.
    pub(crate) fn summon(
        &mut self,
        player: u32,
        operation: &Id,
        summon: &Summon,
        at: [f32; 2],
    ) -> Result<(), Refused> {
        if !self.summons_to(summon, at) {
            return Err(Refused::Misplaced);
        }
        let template = &summon.template;
        let mut organization = (self.rules.raisable.get(template))
            .ok_or(Refused::Unable)?
            .clone();
        organization.owner = player;
        organization.place(at, FACING);
        let (lone, flown) = (
            organization.lone(),
            self.rules.airlifts.contains_key(template),
        );
        // A regiment looks for room member by member, as it comes down if
        // it's flown in.
        let to = match lone {
            true => {
                let shape = organization.members[0].shape;
                self.first_clear(rings(at, 1.0, RINGS), FACING, shape, &self.blocking())
            }
            false => Some(at),
        };
        let r = match (to, flown) {
            (None, _) => None,
            // What's flown in never holds the operation back.
            (Some(to), true) => match self.fly_in(player, organization, to) {
                true => return Ok(()),
                false => None,
            },
            (Some(to), false) if lone => {
                organization.place(to, FACING);
                Some(self.add_new_organization(organization))
            }
            (Some(_), false) => self.summon_regiment(organization, at),
        };
        match r {
            Some(r) => self.summoned.push(Summoned {
                player,
                operation: operation.clone(),
                piece: Piece::Organization(r),
            }),
            None => self.notices.push(Notice::SummonFailed {
                player,
                template: template.clone(),
            }),
        }
        Ok(())
    }

    /// Puts a regiment down with its leader at the clear spot nearest
    /// `at`, then each other member where it finds room, returning its
    /// index; none if its leader finds none.
    fn summon_regiment(
        &mut self,
        mut organization: tgmap::Organization,
        at: [f32; 2],
    ) -> Option<usize> {
        let leader = organization.primary.unwrap_or(0);
        let shape = organization.members[leader].shape;
        let mut blocking = self.blocking();
        let found = self.first_clear(
            rings(at, step(shape), REGIMENT_RINGS),
            FACING,
            shape,
            &blocking,
        )?;
        blocking.push(spot(found, FACING, shape));
        let [x, y] = organization.members[leader].position;
        for m in &mut organization.members {
            m.position = [m.position[0] + found[0] - x, m.position[1] + found[1] - y];
        }
        let r = self.add_new_organization(organization);
        let units = self.organizations[r].units.clone();
        let others = || (units.iter().copied().enumerate()).filter(|&(p, _)| p != leader);
        // Each finds room clear of those already there.
        for (_, u) in others() {
            self.units[u].awaited = true;
        }
        let recruited = self.recruiter(r).is_some();
        for (p, u) in others() {
            if recruited {
                self.arrive(r, p);
                continue;
            }
            let (place, shape) = (self.units[u].position, self.units[u].shape);
            let found = self.first_clear(
                rings(place, step(shape), REGIMENT_RINGS),
                FACING,
                shape,
                &blocking,
            );
            // One with nowhere to stand is refilled as if lost.
            if let Some(p) = found {
                if shape.is_some() {
                    blocking.push(spot(p, FACING, shape));
                }
                let unit = &mut self.units[u];
                unit.awaited = false;
                unit.stand_at(p, FACING);
            }
        }
        Some(r)
    }
}

/// What a tile is, for finding the map's main land.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Lie {
    Land,
    Water,
    Mountain,
    Impassable,
}

/// How far apart the rings round a member of `shape` are tried: its
/// blocking radius, and at least a quarter tile.
fn step(shape: Option<Shape>) -> f32 {
    shape.map_or(0.0, bounding_radius).max(CELL_SIZE / 4.0)
}

/// The spots tried for a summon aimed at `at`, nearest first: there,
/// then `rings` rings round it `step` apart.
fn rings(at: [f32; 2], step: f32, rings: u32) -> impl Iterator<Item = [f32; 2]> {
    std::iter::once(at).chain(rings_round(at, FACING, step, rings))
}

/// `rings` rings of [`RING`] points round `at`, nearest first and `step`
/// apart, each starting straight out the way `facing` points and turning
/// an eighth at a time clockwise seen from above.
pub(crate) fn rings_round(
    at: [f32; 2],
    facing: [f32; 2],
    step: f32,
    rings: u32,
) -> impl Iterator<Item = [f32; 2]> {
    let start = libm::atan2f(facing[1], facing[0]);
    (1..=rings).flat_map(move |ring| {
        (0..RING).map(move |i| {
            let angle = start - TAU * i as f32 / RING as f32;
            let r = ring as f32 * step;
            [at[0] + r * libm::cosf(angle), at[1] + r * libm::sinf(angle)]
        })
    })
}

crate::hash::digest!(Summoned {
    player,
    operation,
    piece
});

#[cfg(test)]
mod tests {
    use tgmap::{Domain, Operation, Pathing, Shape};

    use super::*;
    use crate::testing::{building, kind, truck, with_buildings};
    use crate::{Command, SpecialOp};

    const AT: [f32; 2] = [30.0, 30.0];

    /// Player 1, with 1000 experience, whose one operation summons a
    /// truck for 100 and is ready again at once.
    fn summoning(one_at_a_time: bool, unique: Option<&str>) -> World {
        let pathing = Pathing::uniform(32, Domain::Land);
        let mut w = World::from_organizations(crate::Sides::default(), pathing, &[]);
        w.rules.raisable.insert(Id::from("truck"), truck());
        let summon = Summon {
            template: Id::from("truck"),
            main_land: false,
            one_at_a_time,
            unique: unique.map(Id::from),
        };
        (w.rules.operations).insert(Id::from("reorganization"), Operation::Summon(summon));
        let ops = vec![SpecialOp::new(Id::from("reorganization"), 100.0, 0.0)];
        w.lead(1, 1000.0, None, ops);
        w
    }

    fn summon(at: [f32; 2]) -> Command {
        Command::Operate {
            operation: 0,
            at,
            on: None,
        }
    }

    #[test]
    fn summons_appear_for_their_user_at_the_nearest_clear_spot() {
        let mut w = with_buildings(summoning(false, None), &[building(9, 2, AT)]);
        w.apply(1, summon(AT)).unwrap();
        let r = w.organizations.len() - 1;
        assert_eq!(w.organizations[r].owner, 1);
        // The house is 6 across, and each ring starts the way summons face.
        assert_eq!(w.leader_position(r), [AT[0], AT[1] - 4.0]);
        assert_eq!(w.experience(1), Some(900.0));
        let summoned = Summoned {
            player: 1,
            operation: Id::from("reorganization"),
            piece: Piece::Organization(r),
        };
        assert_eq!(w.summoned, [summoned]);
    }

    /// `w` with its operation summoning three tanks a row, 3 apart, led by
    /// the first.
    fn summons_tanks(w: &mut World) {
        let mut tanks = truck();
        tanks.template = Id::from("tanks");
        tanks.unpacks = None;
        tanks.primary = Some(0);
        tanks.control_range = Some(30.0);
        let tank = |i: u32| tgmap::Member {
            shape: Some(Shape::Circle { radius: 1.0 }),
            ..crate::testing::member(i, [0.0, 0.0], 3.0)
        };
        tanks.members = (1..=3).map(tank).collect();
        tanks.formations[0].offsets = vec![[0.0, 0.0], [3.0, 0.0], [6.0, 0.0]];
        w.rules.raisable.insert(Id::from("tanks"), tanks);
        if let Some(Operation::Summon(s)) = w.rules.operations.get_mut(&Id::from("reorganization"))
        {
            s.template = Id::from("tanks");
        }
    }

    #[test]
    fn a_regiments_members_come_out_of_a_building_near_that_recruits_them() {
        let mut factory = building(9, 1, [AT[0], AT[1] + 20.0]);
        factory.template = Id::from("factory");
        factory.deploy = Some(tgmap::DeployPoints {
            creation: [0.0, 4.0],
            deployment: [0.0, 8.0],
        });
        let mut w = with_buildings(summoning(false, None), &[factory.clone()]);
        summons_tanks(&mut w);
        (w.rules.recruit_lists).insert(Id::from("factory"), vec![Id::from("tanks")]);
        w.apply(1, summon(AT)).unwrap();
        let r = w.organizations.len() - 1;
        let units = w.organizations[r].units.clone();
        assert_eq!(w.units[units[0]].position, AT);
        let out = crate::deploy_point(factory.position, factory.facing, [0.0, 4.0]);
        for &u in &units[1..] {
            assert!(w.units[u].alive());
            assert_eq!(w.units[u].position, out);
        }
    }

    #[test]
    fn a_regiments_leader_finds_room_then_each_other_member() {
        let mut w = with_buildings(summoning(false, None), &[building(9, 2, AT)]);
        summons_tanks(&mut w);
        w.apply(1, summon(AT)).unwrap();
        let r = w.organizations.len() - 1;
        let units = w.organizations[r].units.clone();
        assert!(units.iter().all(|&u| w.units[u].alive()));
        // The leader, round from the house in its rings 1 apart.
        let leader = w.units[units[0]].position;
        assert_eq!(leader[0], AT[0]);
        assert!(leader[1] <= AT[1] - 4.0);
        // The others clear of the house and of one another.
        let house = w.buildings[0].obstacle.as_ref().unwrap();
        for (i, &u) in units.iter().enumerate() {
            let me = spot(w.units[u].position, FACING, w.units[u].shape);
            assert!(!house.overlaps(&me));
            for &v in &units[i + 1..] {
                assert!(crate::distance(w.units[u].position, w.units[v].position) >= 2.0);
            }
        }
    }

    #[test]
    fn the_main_land_is_the_largest_body_of_land() {
        let mut w = summoning(false, None);
        // Water down x = 10 cuts off a smaller strip to the west.
        for y in 0..32 {
            w.pathing.set(Tile { x: 10, y }, Domain::Naval);
        }
        if let Some(Operation::Summon(s)) = w.rules.operations.get_mut(&Id::from("reorganization"))
        {
            s.main_land = true;
        }
        let refused = [[10.0, 30.0], [21.0, 30.0], [-5.0, 30.0]];
        for at in refused {
            assert_eq!(w.apply(1, summon(at)), Err(Refused::Misplaced), "{at:?}");
        }
        assert_eq!(w.experience(1), Some(1000.0));
        w.apply(1, summon(AT)).unwrap();
        assert_eq!(w.summoned.len(), 1);
    }

    #[test]
    fn one_at_a_time_waits_for_what_it_summoned_to_go() {
        let mut w = summoning(true, None);
        w.apply(1, summon(AT)).unwrap();
        assert_eq!(w.check(1, &summon(AT)), Err(Refused::Summoned));
        // Not even in other hands.
        let r = w.organizations.len() - 1;
        w.organizations[r].owner = 2;
        assert_eq!(w.check(1, &summon(AT)), Err(Refused::Summoned));
        let u = w.organizations[r].units[0];
        w.units[u].die(w.ticks);
        assert_eq!(w.check(1, &summon(AT)), Ok(()));
    }

    #[test]
    fn a_unique_summon_waits_while_its_side_owns_the_like() {
        let mut w = summoning(false, Some("provides_corps"));
        let corps = tgmap::Kind {
            flags: vec![Id::from("provides_corps")],
            ..kind(&[])
        };
        w.rules.kinds.insert(Id::from("truck"), corps.clone());
        w.apply(1, summon(AT)).unwrap();
        assert_eq!(w.check(1, &summon(AT)), Err(Refused::Summoned));
        // One taken from it frees it.
        let r = w.organizations.len() - 1;
        w.organizations[r].owner = 2;
        assert_eq!(w.check(1, &summon(AT)), Ok(()));
        // As does what the side owns of its own.
        let mut hq = building(9, 1, [10.0, 10.0]);
        hq.template = Id::from("corps_hq");
        w.rules.kinds.insert(Id::from("corps_hq"), corps);
        w.add_buildings(&[hq]);
        assert_eq!(w.check(1, &summon(AT)), Err(Refused::Summoned));
    }

    #[test]
    fn a_summon_with_nowhere_to_go_is_spent_and_told() {
        let mut big = building(9, 2, AT);
        let wide = Shape::Rect {
            width: 30.0,
            height: 30.0,
        };
        big.obstacle.as_mut().unwrap().shape = wide;
        let mut w = with_buildings(summoning(false, None), &[big]);
        let before = w.organizations.len();
        w.apply(1, summon(AT)).unwrap();
        assert_eq!(w.organizations.len(), before);
        assert_eq!(w.experience(1), Some(900.0));
        let failed = Notice::SummonFailed {
            player: 1,
            template: Id::from("truck"),
        };
        assert_eq!(w.take_notices(), [failed]);
    }

    /// Has what's summoned flown in by a plane.
    fn flown(w: &mut World, template: &str) {
        let airlift = tgmap::Airlift {
            transport: "plane".into(),
            speed: 7.0,
            turn_radius: 18.0,
            height: 17.0,
            taxi_height: 1.0,
            delay: 0.4,
        };
        w.rules.airlifts.insert(Id::from(template), airlift);
    }

    #[test]
    fn regiments_with_a_transport_are_flown_straight_to_where_theyre_aimed() {
        let mut w = with_buildings(summoning(true, None), &[building(9, 2, AT)]);
        let company = crate::testing::raised_company(&mut w, 0);
        w.rules.raisable.insert(Id::from("company"), company);
        if let Some(Operation::Summon(s)) = w.rules.operations.get_mut(&Id::from("reorganization"))
        {
            s.template = Id::from("company");
        }
        flown(&mut w, "company");
        // The house there is no reason to look elsewhere.
        w.apply(1, summon(AT)).unwrap();
        assert_eq!(w.experience(1), Some(900.0));
        let plane = &w.deliveries[0];
        assert_eq!((plane.owner, plane.target, plane.hq), (1, AT, None));
        assert!(w.organizations.is_empty() && w.take_notices().is_empty());
        // Nothing waits on it, though the operation is one at a time.
        assert!(w.summoned.is_empty());
        // It comes down beside the house.
        for _ in 0..400 {
            w.tick();
        }
        let r = w.organizations.len() - 1;
        assert_eq!(w.organizations[r].owner, 1);
        assert!(!w.organizations[r].arriving);
        assert!(crate::distance(w.leader_position(r), AT) < 6.0);
        // In along x from the far edge, x = 0 being nearer.
        let to = [10.0, AT[1]];
        w.apply(1, summon(to)).unwrap();
        let plane = &w.deliveries[1];
        let size = w.pathing.size();
        assert_eq!(plane.position, [(size - CELL_SIZE).next_down(), to[1]]);
        assert_eq!(plane.facing, [-1.0, 0.0]);
    }

    #[test]
    fn single_things_with_a_transport_are_flown_to_the_nearest_clear_spot() {
        let mut w = with_buildings(summoning(false, None), &[building(9, 2, AT)]);
        flown(&mut w, "truck");
        w.apply(1, summon(AT)).unwrap();
        assert_eq!(w.deliveries[0].target, [AT[0], AT[1] - 4.0]);
        assert!(w.organizations.is_empty());
        // With nowhere to go, no plane is sent.
        let mut w = summoning(false, None);
        w.pathing = Pathing::uniform(32, Domain::Naval);
        flown(&mut w, "truck");
        w.apply(1, summon(AT)).unwrap();
        assert!(w.deliveries.is_empty());
        assert_eq!(w.experience(1), Some(900.0));
        let failed = Notice::SummonFailed {
            player: 1,
            template: Id::from("truck"),
        };
        assert_eq!(w.take_notices(), [failed]);
    }
}
