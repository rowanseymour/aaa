//! Packed buildings unpacking where they're sent, and buildings packing
//! back up.

use aaa_map::{Domain, Obstacle, Unpacks};

use crate::{ARRIVAL_REACH, LOOK, Left, Notice, Order, State, TICK, World, normalised, within};

/// Where a packed building sent to unpack at `at` stands: the nearest
/// whole units to it.
pub fn unpack_site(at: [f32; 2]) -> [f32; 2] {
    at.map(f32::round)
}

/// The way a building ordered to unpack facing `facing` faces, as a unit
/// vector.
pub(crate) fn unpack_facing(facing: [f32; 2]) -> [f32; 2] {
    normalised(facing).unwrap_or(aaa_map::UNPACKED_FACING)
}

impl World {
    /// Whether what `unpacks` makes would fit standing at `at` facing
    /// `facing`, with the room around it buildings need: among the
    /// buildings and scenery where it blocks, and anywhere on the map
    /// where it doesn't.
    pub fn room_to_unpack(&self, unpacks: &Unpacks, at: [f32; 2], facing: [f32; 2]) -> bool {
        let obstacle = unpacks.building.obstacle.as_ref();
        let placed = Obstacle {
            position: at,
            facing: unpack_facing(facing),
            shape: unpacks.room,
            soft: unpacks.room,
        };
        if obstacle.is_none() {
            return self.pathing.on_map(&placed);
        }
        let standing = self.buildings.iter().filter(|b| !b.gone());
        let others = standing.filter_map(|b| b.obstacle.as_ref());
        self.pathing
            .room_for(&placed, Domain::Land, others.chain(&self.scenery))
    }

    /// Unpacks the packed buildings that have reached where they were sent
    /// to unpack, each leaving the map for the building it makes, and
    /// calls it off for those that can't get there or no longer have room.
    pub(crate) fn unpack(&mut self) {
        for r in 0..self.regiments.len() {
            let Some((at, facing)) = self.regiments[r].unpack_at else {
                continue;
            };
            if self.destroyed(r) {
                self.regiments[r].unpack_at = None;
                continue;
            }
            let again = self.regiments[r].unpack_again;
            if self.underway(r) || again.is_some_and(|tick| self.ticks < tick) {
                continue;
            }
            // The truck is the regiment's one survivor: the regiment
            // isn't destroyed and holds a single packed building.
            let (Some(unpacks), Some(truck)) =
                (self.regiments[r].unpacks.clone(), self.survivors(r).next())
            else {
                self.regiments[r].unpack_at = None;
                continue;
            };
            // It unpacks where it's stopped, if that's where it was sent.
            let stop = [at[0] + unpacks.offset[0], at[1] + unpacks.offset[1]];
            let here = self.units[truck].position;
            let at = unpack_site([here[0] - unpacks.offset[0], here[1] - unpacks.offset[1]]);
            let there = within(here, stop, ARRIVAL_REACH);
            if there && !self.room_to_unpack(&unpacks, at, facing) && again.is_none() {
                self.regiments[r].unpack_again = Some(self.ticks + u64::from(LOOK));
                continue;
            }
            let regiment = &mut self.regiments[r];
            regiment.unpack_at = None;
            regiment.unpack_again = None;
            if !there || !self.room_to_unpack(&unpacks, at, facing) {
                let player = self.regiments[r].owner;
                let told = Notice::CannotUnpack {
                    player,
                    regiment: r,
                };
                self.notices.push(told);
                continue;
            }
            let truck_id = self.units[truck].id;
            // It keeps the share of its health it had as a truck.
            let health = self
                .survivors(r)
                .filter_map(|u| Some(self.units[u].health / self.units[u].fighter.as_ref()?.health))
                .fold(1.0_f32, f32::min);
            for u in self.regiments[r].units.clone() {
                let unit = &mut self.units[u];
                if unit.alive() {
                    unit.left = Some(Left::Unpacked(self.ticks));
                    unit.halt();
                    unit.target = None;
                }
            }
            let mut building = unpacks.building;
            // The truck and its building are one thing to the player, so
            // it keeps its id and its place in the truck's organization.
            building.id = truck_id;
            building.owner = self.regiments[r].owner;
            building.position = at;
            building.facing = facing;
            building.health = health;
            if let Some(o) = &mut building.obstacle {
                o.position = at;
                o.facing = facing;
                self.pathing.block(o);
                self.clear_under(o);
            }
            let b = self.buildings.len();
            self.regiments[r].unpacked_into = Some(b);
            // It keeps the truck's number rather than taking one.
            self.push_buildings(std::slice::from_ref(&building), false);
            self.buildings[b].number = self.regiments[r].number;
            log::debug!(
                "{} {} unpacking into {}",
                self.regiments[r].template,
                truck_id,
                building.template
            );
            self.buildings[b].unpacked_from = Some(r);
            let ticks = (unpacks.seconds / TICK).round() as u64;
            self.buildings[b].unpacking = Some((self.ticks, self.ticks + ticks));
        }
        for b in &mut self.buildings {
            if let Some((_, end)) = b.unpacking
                && self.ticks >= end
            {
                b.unpacking = None;
            }
        }
        for b in 0..self.buildings.len() {
            if let Some((_, end)) = self.buildings[b].packing
                && self.ticks >= end
            {
                self.pack_up(b);
            }
        }
    }

    /// Takes away for good the scenery whose shape overlaps `footprint`, a
    /// building's as it goes up.
    fn clear_under(&mut self, footprint: &Obstacle) {
        let cleared = &mut self.cleared;
        self.clearable.retain(|(id, shape)| {
            let under = shape.overlaps(footprint);
            if under {
                cleared.push(*id);
            }
            !under
        });
    }

    /// Takes a building that's done packing off the map, putting out what
    /// it packs into where it stood, with the share of health it had.
    fn pack_up(&mut self, b: usize) {
        let building = &mut self.buildings[b];
        building.packing = None;
        if !building.standing() {
            return;
        }
        let Some(packs) = self.packing.get(&building.template).cloned() else {
            return;
        };
        log::debug!("{} {} packed up", building.template, building.id);
        let share = building.health / building.fighter.health;
        let at = building.position;
        let owner = building.owner;
        if let Some(o) = building.obstacle.clone() {
            self.pathing.unblock(&o);
        }
        let (id, organization) = (building.id, building.organization.clone());
        let from = building.unpacked_from;
        let stand = [at[0] + packs.offset[0], at[1] + packs.offset[1]];
        // A truck that unpacked here comes back, in its organization, to
        // whoever holds the building now.
        let r = match from {
            Some(r) => {
                self.return_truck(r, stand, packs.facing);
                self.regiments[r].owner = owner;
                r
            }
            None => {
                // It hands the truck its id.
                let mut truck = packs.truck;
                match truck.lone() {
                    true => truck.renumber(id),
                    // A truck that's an organization takes the id itself,
                    // its members new ones.
                    false => {
                        let first = self.take_ids(truck.members.len() as u32);
                        for (m, id) in truck.members.iter_mut().zip(first..) {
                            m.id = id;
                        }
                        truck.id = id;
                    }
                }
                if let Some(mut within) = organization.map(|o| *o) {
                    within.members = std::mem::take(&mut truck.members);
                    within.unpacks = truck.unpacks.take();
                    truck = within;
                }
                truck.owner = owner;
                truck.place(stand, packs.facing);
                // It keeps the building's number rather than taking one.
                let r = self.push_regiment(&truck, false);
                self.regiments[r].number = self.buildings[b].number;
                r
            }
        };
        for u in self.regiments[r].units.clone() {
            let unit = &mut self.units[u];
            if unit.alive()
                && let Some(f) = &unit.fighter
            {
                unit.health = f.health * share;
            }
        }
        self.buildings[b].state = State::Packed(self.ticks, r);
    }

    /// Puts the members of a regiment that unpacked back on the map, in
    /// its formation around `position` facing `facing`, as if just made:
    /// it keeps only its ids and place.
    fn return_truck(&mut self, regiment: usize, position: [f32; 2], facing: [f32; 2]) {
        let r = &mut self.regiments[regiment];
        r.unpacked_into = None;
        r.unpack_at = None;
        r.rally = None;
        r.order = Order::Move;
        r.engaged.clear();
        r.resume = None;
        r.position = position;
        r.facing = facing;
        let offsets = &r.formations[r.current_formation()].offsets;
        for (&u, &offset) in r.units.iter().zip(offsets) {
            let unit = &mut self.units[u];
            if !matches!(unit.left, Some(Left::Unpacked(_))) {
                continue;
            }
            unit.left = None;
            let at = aaa_map::from_local(position, facing, offset);
            unit.position = at;
            unit.previous = at;
            unit.facing = facing;
            unit.end_facing = facing;
            unit.halt();
            unit.speed = 0.0;
            unit.closing = false;
            unit.target = None;
            unit.shot = None;
            unit.reloads.fill(0.0);
            if let Some(t) = &mut unit.trailer {
                t.behind(at, facing);
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::testing::*;
    use std::collections::HashSet;

    use aaa_map::{Domain, Id, Obstacle, Pathing, Shape, Tile};

    use crate::{Command, Notice, Refused, State, World};

    /// A regiment of one truck.
    fn convoy() -> aaa_map::Regiment {
        aaa_map::Regiment {
            hq: None,
            template: "convoy".into(),
            ..truck()
        }
    }

    #[test]
    fn trucks_unpack_where_theres_room_once_there() {
        let mut w = headquarters();
        let mut t = truck();
        t.owner = 1;
        t.renumber(9);
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        // Over the HQ, too close beside it, and over the map's edge.
        assert_eq!(
            w.order_unpack(1, r, [22.0, 22.0], NORTH),
            Err(Refused::NoRoom)
        );
        assert_eq!(
            w.order_unpack(1, r, [28.0, 20.0], NORTH),
            Err(Refused::NoRoom)
        );
        assert_eq!(
            w.order_unpack(1, r, [78.0, 30.0], NORTH),
            Err(Refused::NoRoom)
        );
        assert_eq!(
            w.order_unpack(2, r, [30.0, 40.0], NORTH),
            Err(Refused::NotYours)
        );
        assert_eq!(w.order_unpack(1, r, [30.2, 39.6], NORTH), Ok([30.0, 40.0]));
        run(&mut w, 3.0);
        assert!(w.units[0].alive());
        assert_eq!(w.buildings.len(), 1);
        run(&mut w, 1.0);
        assert!(!w.units[0].alive());
        assert!(w.units[0].died.is_none());
        assert!(w.destroyed(r));
        let depot = &w.buildings[1];
        assert_eq!((depot.template.as_str(), depot.owner), ("depot", 1));
        // It takes over the truck's id.
        assert_eq!(depot.id, 9);
        assert_eq!(depot.position, [30.0, 40.0]);
        let (from, to) = depot.unpacking.unwrap();
        assert_eq!(to - from, 70);
        assert!(!w.pathing.passable(Tile::new(15, 20), Domain::Land));
        run(&mut w, 7.0);
        assert!(w.buildings[1].unpacking.is_none());
    }

    #[test]
    fn unpacking_clears_the_scenery_under_the_building() {
        let mut w = headquarters();
        let tree = |at: [f32; 2]| Obstacle {
            position: at,
            facing: NORTH,
            shape: Shape::Circle { radius: 0.2 },
            soft: Shape::Circle { radius: 0.5 },
        };
        // The depot will cover 26 to 34 across: one tree stands within it,
        // one reaches into its edge, one is just clear of it though within
        // the room it needs, and one is further off.
        w.clearable = vec![
            (50, tree([31.0, 41.0])),
            (51, tree([34.1, 40.0])),
            (52, tree([34.3, 40.0])),
            (53, tree([40.0, 40.0])),
        ];
        let mut t = truck();
        t.owner = 1;
        t.renumber(9);
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        // The trees don't stand in its way.
        w.order_unpack(1, r, [30.0, 40.0], NORTH).unwrap();
        run(&mut w, 2.0);
        assert!(w.cleared.is_empty());
        run(&mut w, 2.0);
        assert_eq!(w.buildings.len(), 2);
        assert_eq!(w.cleared, [50, 51]);
        let ids: Vec<u32> = w.clearable.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [52, 53]);
    }

    #[test]
    fn trucks_that_turn_over_time_unpack_behind_and_beside_themselves() {
        for at in [[30.0, 24.0], [34.0, 30.0], [27.0, 31.0]] {
            let mut w = World::from_regiments(at_war(), Pathing::uniform(40, Domain::Land), &[]);
            let mut t = truck();
            t.owner = 1;
            // Round a radius of 3, as the game's trucks turn.
            t.members[0].turn_rate = 1.0;
            t.renumber(9);
            t.place([30.0, 30.0], [0.0, 1.0]);
            let r = w.add_regiment(&t);
            w.order_unpack(1, r, at, NORTH).unwrap();
            run(&mut w, 20.0);
            assert_eq!(w.buildings.len(), 1, "{at:?}: {:?}", w.units[0].position);
            assert_eq!(w.buildings[0].position, at);
        }
    }

    #[test]
    fn trucks_left_no_room_ask_again_then_say_they_cant_unpack() {
        let mut w = headquarters();
        let mut t = truck();
        t.owner = 1;
        t.renumber(9);
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        w.order_unpack(1, r, [30.0, 40.0], NORTH).unwrap();
        // Something goes up there before it arrives.
        w.add_buildings(&[building(40, 1, [30.0, 40.0])]);
        run(&mut w, 4.0);
        assert!(w.regiments[r].unpack_at.is_some() && w.notices.is_empty());
        run(&mut w, 1.5);
        assert!(w.regiments[r].unpack_at.is_none() && w.units[0].alive());
        let told = Notice::CannotUnpack {
            player: 1,
            regiment: r,
        };
        assert_eq!(w.notices, [told]);
    }

    #[test]
    fn other_orders_call_off_unpacking() {
        let mut w = headquarters();
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        w.apply(
            1,
            Command::Unpack {
                regiment: r,
                at: [30.0, 40.0],
                facing: NORTH,
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        w.apply(1, Command::Stop { regiment: r }).unwrap();
        run(&mut w, 5.0);
        assert!(w.units[0].alive());
        assert_eq!(w.buildings.len(), 1);
    }

    #[test]
    fn trucks_unpack_facing_the_way_theyre_told() {
        let mut w = headquarters();
        let mut t = truck();
        t.owner = 1;
        let unpacks = t.unpacks.as_mut().unwrap();
        let long = Shape::Rect {
            width: 4.0,
            height: 16.0,
        };
        let obstacle = unpacks.building.obstacle.as_mut().unwrap();
        (obstacle.shape, obstacle.soft) = (long, long);
        unpacks.room = Shape::Rect {
            width: 4.0,
            height: 20.0,
        };
        t.place([30.0, 60.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        // Long side north to south it would reach the map's edge; east to
        // west it fits.
        assert_eq!(
            w.order_unpack(1, r, [30.0, 70.0], NORTH),
            Err(Refused::NoRoom)
        );
        assert_eq!(
            w.order_unpack(1, r, [30.0, 70.0], [2.0, 0.0]),
            Ok([30.0, 70.0])
        );
        run(&mut w, 5.0);
        let depot = &w.buildings[1];
        assert_eq!(depot.facing, [1.0, 0.0]);
        assert_eq!(depot.obstacle.as_ref().unwrap().facing, [1.0, 0.0]);
        assert!(!w.pathing.passable(Tile::new(12, 35), Domain::Land));
        assert!(w.pathing.passable(Tile::new(15, 32), Domain::Land));
    }

    #[test]
    fn buildings_pack_back_into_their_place_in_an_organization() {
        let mut w = headquarters();
        w.packing.insert("depot".into(), packs());
        let mut c = convoy();
        c.owner = 1;
        c.renumber(30);
        c.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&c);
        let u = w.regiments[r].units[0];
        w.apply(
            1,
            Command::Unpack {
                regiment: r,
                at: [30.0, 40.0],
                facing: NORTH,
            },
        )
        .unwrap();
        run(&mut w, 11.0);
        let depot = w.regiments[r].unpacked_into.unwrap();
        assert_eq!(w.buildings[depot].id, 31);
        w.buildings[depot].health *= 0.8;
        w.apply(1, Command::Pack { building: depot }).unwrap();
        run(&mut w, 7.1);
        let State::Packed(_, packed) = w.buildings[depot].state else {
            panic!("not packed: {:?}", w.buildings[depot].state);
        };
        // The same regiment, with the same truck, back where the depot stood.
        assert_eq!(packed, r);
        assert_eq!((w.regiments.len(), w.units.len()), (1, 1));
        let regiment = &w.regiments[r];
        assert_eq!((regiment.id, regiment.template.as_str()), (30, "convoy"));
        assert_eq!(regiment.unpacked_into, None);
        assert!(!w.destroyed(r));
        assert_eq!(w.units[u].id, 31);
        assert!(near(w.units[u].position, [30.0, 40.0]));
        assert!(near(w.units[u].facing, [1.0, 0.0]));
        assert!((w.units[u].health - 80.0).abs() < 1e-3);
        // And it can go and unpack again.
        w.apply(
            1,
            Command::Unpack {
                regiment: r,
                at: [30.0, 60.0],
                facing: NORTH,
            },
        )
        .unwrap();
        run(&mut w, 15.0);
        assert_eq!(w.regiments[r].unpacked_into, Some(2));
    }

    #[test]
    fn captured_buildings_pack_into_trucks_for_their_new_owner() {
        let mut w = headquarters();
        w.packing.insert("depot".into(), packs());
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        w.apply(
            1,
            Command::Unpack {
                regiment: r,
                at: [30.0, 40.0],
                facing: NORTH,
            },
        )
        .unwrap();
        run(&mut w, 11.0);
        let depot = w.regiments[r].unpacked_into.unwrap();
        w.change_hands(depot, 2);
        w.apply(2, Command::Pack { building: depot }).unwrap();
        run(&mut w, 7.1);
        assert!(matches!(w.buildings[depot].state, State::Packed(_, p) if p == r));
        assert_eq!(w.regiments[r].owner, 2);
    }

    #[test]
    fn buildings_a_map_has_pack_into_their_place_there() {
        let mut w = headquarters();
        w.packing.insert("depot".into(), packs());
        let depot = |id, x| {
            let mut depot = truck().unpacks.unwrap().building;
            (depot.id, depot.owner, depot.position) = (id, 1, [x, 40.0]);
            depot.obstacle.as_mut().unwrap().position = depot.position;
            depot
        };
        // One standing in an organization, with another member beside it,
        // and one on its own.
        let mut within = depot(41, 30.0);
        within.organization = Some(Box::new(aaa_map::Regiment {
            hq: None,
            id: 40,
            template: "hq_unpacked".into(),
            members: Vec::new(),
            ..convoy()
        }));
        w.set_numbered(HashSet::from([Id::from("hq_unpacked")]));
        w.add_buildings(&[within, depot(50, 60.0)]);
        assert_eq!(
            (w.buildings[1].number, w.buildings[2].number),
            (Some(1), None)
        );
        for b in [1, 2] {
            w.apply(1, Command::Pack { building: b }).unwrap();
        }
        run(&mut w, 7.1);
        let packed = |b: usize| match w.buildings[b].state {
            State::Packed(_, r) => &w.regiments[r],
            ref state => panic!("not packed: {state:?}"),
        };
        // The truck takes the building's id and place in the organization,
        // which keeps its own.
        let r = packed(1);
        assert_eq!((r.id, r.template.as_str(), r.owner), (40, "hq_unpacked", 1));
        // It keeps the building's number rather than taking the next.
        assert_eq!(r.number, Some(1));
        assert_eq!(w.units[r.units[0]].id, 41);
        assert!(near(w.units[r.units[0]].position, [30.0, 40.0]));
        assert!(r.unpacks.is_some());
        // Alone it packs into a lone truck under its id.
        let r = packed(2);
        assert_eq!((r.id, r.template.as_str()), (50, "truck"));
        assert_eq!(w.units[r.units[0]].id, 50);
        assert_eq!(w.regiments.len(), 2);
    }

    #[test]
    fn buildings_pack_back_into_their_trucks() {
        let mut w = headquarters();
        w.packing.insert("depot".into(), packs());
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        w.set_numbered(HashSet::from([Id::from("truck")]));
        w.add_regiment(&t);
        let r = w.add_regiment(&t);
        assert_eq!(w.regiments[r].number, Some(2));
        w.apply(
            1,
            Command::Unpack {
                regiment: r,
                at: [30.0, 40.0],
                facing: NORTH,
            },
        )
        .unwrap();
        run(&mut w, 4.0);
        let depot = w.regiments[r].unpacked_into.unwrap();
        // The building is the truck to the player, number and all.
        assert_eq!(w.buildings[depot].number, Some(2));
        // Not while it's unpacking, nor someone else's, nor what packs to
        // nothing.
        assert_eq!(w.order_pack(1, depot), Err(Refused::Unable));
        run(&mut w, 7.0);
        assert_eq!(w.order_pack(2, depot), Err(Refused::NotYours));
        assert_eq!(w.order_pack(1, 0), Err(Refused::Unable));
        let full = w.buildings[depot].health;
        w.buildings[depot].health = full * 0.5;
        assert_eq!(w.order_pack(1, depot), Err(Refused::Unable));
        w.buildings[depot].health = full * 0.8;
        w.apply(1, Command::Pack { building: depot }).unwrap();
        run(&mut w, 6.9);
        assert!(w.buildings[depot].standing());
        assert!(!w.pathing.passable(Tile::new(15, 20), Domain::Land));
        run(&mut w, 0.2);
        let State::Packed(_, packed) = w.buildings[depot].state else {
            panic!("not packed: {:?}", w.buildings[depot].state);
        };
        assert!(w.buildings[depot].gone());
        assert!(w.pathing.passable(Tile::new(15, 20), Domain::Land));
        // A lone truck's building stands in no organization, so packs into
        // a lone truck: the one it unpacked from, under its old id.
        assert_eq!(packed, r);
        assert_eq!(w.regiments.len(), 2);
        assert_eq!(w.regiments[packed].template, "truck");
        assert_eq!(w.regiments[packed].number, Some(2));
        let u = w.regiments[packed].units[0];
        assert_eq!(w.regiments[packed].owner, 1);
        assert!(near(w.units[u].position, [30.0, 40.0]));
        assert!(near(w.units[u].facing, [1.0, 0.0]));
        assert!((w.units[u].health - 80.0).abs() < 1e-3);
        // And the truck can go and unpack again.
        w.apply(
            1,
            Command::Unpack {
                regiment: packed,
                at: [30.0, 60.0],
                facing: NORTH,
            },
        )
        .unwrap();
    }
}
