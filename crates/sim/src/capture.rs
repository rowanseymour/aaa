//! Buildings falling and changing hands, and cities going to whoever
//! holds their zone.

use crate::{CAPTURE_HEALTH, Fallen, Notice, Relation, State, Target, World};

impl World {
    /// Deals with a building out of health: `by` captures it if it can be
    /// captured, else it's destroyed if it can be, else disabled.
    pub(crate) fn fall(&mut self, b: usize, by: Option<u32>) {
        let ticks = self.ticks;
        let building = &mut self.buildings[b];
        building.health = 0.0;
        building.target = None;
        if building.fighter.destructible {
            building.besieged = None;
        }
        // Cities change hands by who holds them instead.
        let taker = by.filter(|_| building.fighter.capturable && building.zone.is_none());
        if let Some(by) = taker {
            building.health = CAPTURE_HEALTH * building.fighter.health;
            self.change_hands(b, by);
        } else if building.fighter.destructible {
            building.state = State::Destroyed(ticks);
            if let Some(o) = building.obstacle.take() {
                self.pathing.unblock(&o);
            }
            self.notices.push(Notice::Destroyed {
                owner: building.owner,
                what: Fallen::Building(b),
            });
            self.drop_queue(b);
            // What could have been sold pays its owner as if it had been.
            if self.sells(b) {
                self.pay_sale(b);
            }
        } else {
            building.state = State::Disabled;
        }
    }

    /// Gives a building, and the rest of its city if it's one's centre, to
    /// `owner`.
    pub(crate) fn change_hands(&mut self, b: usize, owner: u32) {
        let taken = &self.buildings[b];
        log::debug!("{} {} taken by player {owner}", taken.template, taken.id);
        // Each side's told only of what it sees, but a side always sees its
        // own: the loser's told while it's still theirs, the taker once it
        // is.
        self.notices.push(Notice::Lost {
            player: taken.owner,
            building: b,
            by: owner,
        });
        let ids = &self.buildings[b].members;
        let members: Vec<usize> = (0..self.buildings.len())
            .filter(|&i| i != b && ids.contains(&self.buildings[i].id))
            .collect();
        for i in std::iter::once(b).chain(members) {
            self.drop_queue(i);
            // An HQ that changes side loses its regiments.
            if let Some(hq) = self.hq_of(i) {
                self.detach_all(hq);
            }
            let building = &mut self.buildings[i];
            building.owner = owner;
            building.captured = Some(self.ticks);
            building.target = None;
            building.rally = None;
            // A relay has to connect on its new side.
            building.supply.judged = false;
        }
        self.notices.push(Notice::Captured {
            player: owner,
            building: b,
        });
    }

    /// Hands each city to whoever holds its zone, as long as the side
    /// holding it has none of its own there to keep it.
    pub(crate) fn occupy(&mut self) {
        for c in 0..self.buildings.len() {
            let destroyed = self.buildings[c].gone();
            let Some(zone) = self.buildings[c].zone.filter(|_| !destroyed) else {
                continue;
            };
            if let Some(winner) = self.holder(c, zone)
                && winner != self.buildings[c].owner
            {
                self.change_hands(c, winner);
            }
        }
    }

    /// The armed things in the zone of the city centred on `c`, each with
    /// its owner and its health's share. Independent sides' and nobody's
    /// don't count, nor does the centre.
    pub(crate) fn occupants(&self, c: usize, zone: f32) -> Vec<(u32, f32)> {
        let zone = self.pathing.zone(self.buildings[c].position, zone);
        let counts = |o: u32| self.sides.player(o).is_some_and(|p| !p.independent);
        self.targets()
            .filter(|&t| t != Target::Building(c) && self.in_zone(&zone, t))
            .filter(|&t| counts(self.owner(t)))
            .filter_map(|t| {
                let f = self.fighter(t).filter(|f| !f.weapons.is_empty())?;
                let health = match t {
                    Target::Unit(u) => self.units[u].health,
                    Target::Building(b) => self.buildings[b].health,
                };
                Some((self.owner(t), health / f.health))
            })
            .collect()
    }

    /// Who should hold the city centred on `c`: its owner while any of its
    /// own are in the zone or no enemy of its is, else the side there with
    /// none of its enemies there and no ally there stronger or in danger.
    fn holder(&self, c: usize, zone: f32) -> Option<u32> {
        let owner = self.buildings[c].owner;
        let occupants = self.occupants(c, zone);
        let weight = |of: &dyn Fn(u32) -> bool| -> f32 {
            occupants
                .iter()
                .filter(|(o, _)| of(*o))
                .map(|(_, w)| w)
                .sum()
        };
        let enemy = |of: u32| move |o: u32| self.sides.relation(o, of) == Relation::Enemy;
        let own = weight(&|o| o == owner);
        if owner != 0 && (own > 0.0 || weight(&enemy(owner)) <= 0.0) {
            return Some(owner);
        }
        let players: Vec<u32> = self.sides.players.iter().map(|p| p.id).collect();
        let present = |p: u32| weight(&|o| o == p);
        let threat = |p: u32| weight(&|o| self.sides.relation(p, o) == Relation::Enemy);
        let winner = players.iter().copied().find(|&p| {
            threat(p) <= 0.0
                && present(p) > 0.0
                && !players.iter().any(|&q| {
                    q != p
                        && self.sides.relation(p, q) == Relation::Ally
                        && (present(p) < present(q) || threat(q) > 0.0)
                })
        });
        winner.or((owner != 0).then_some(owner))
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::*;

    use aaa_map::{Domain, Tile};

    use crate::{Command, Fallen, Notice, State, World};

    /// What `w`'s sides have been told of buildings changing hands.
    fn captures(w: &World) -> Vec<Notice> {
        let notices = w.notices.iter().cloned();
        notices
            .filter(|n| matches!(n, Notice::Lost { .. } | Notice::Captured { .. }))
            .collect()
    }

    fn captured(building: usize, player: u32) -> Notice {
        Notice::Captured { player, building }
    }

    #[test]
    fn buildings_out_of_health_are_captured_destroyed_or_disabled() {
        let mut bunker = building(20, 2, [10.0, 20.0]);
        bunker.fighter.capturable = true;
        let mut pillbox = building(21, 2, [20.0, 10.0]);
        pillbox.fighter.destructible = true;
        let house = building(22, 2, [0.0, 10.0]);
        let mut w = siege(&[bunker, pillbox, house]);
        assert!(!w.pathing.passable(Tile::new(10, 5), Domain::Land));
        run(&mut w, 30.0);
        // Its new owner's rifle leaves it be at the share it's left with.
        let b = &w.buildings[0];
        assert_eq!((b.owner, b.state), (1, State::Standing));
        assert!((b.health - 30.0).abs() < 1e-3);
        assert!(b.captured.is_some());
        assert!(matches!(w.buildings[1].state, State::Destroyed(_)));
        assert!(w.pathing.passable(Tile::new(10, 5), Domain::Land));
        assert_eq!(w.buildings[2].state, State::Disabled);
        assert_eq!(w.buildings[2].owner, 2);
        assert_eq!(w.units[0].target, None);
        assert_eq!(captures(&w), [lost(0, 2, 1), captured(0, 1)]);
        let destroyed = Notice::Destroyed {
            owner: 2,
            what: Fallen::Building(1),
        };
        assert_eq!(w.notices.iter().filter(|&n| *n == destroyed).count(), 1);
    }

    #[test]
    fn cities_go_to_whoever_holds_their_zone() {
        let mut w = siege(&city());
        run(&mut w, 1.0);
        assert_eq!(w.buildings[0].owner, 2);
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [30.0, 10.0],
            },
        )
        .unwrap();
        run(&mut w, 15.0);
        assert!(w.buildings.iter().all(|b| b.owner == 1));
        assert!(w.buildings[0].captured.is_some());
        // Told once, of the centre, not of the house that went with it.
        assert_eq!(captures(&w), [lost(0, 2, 1), captured(0, 1)]);
        // Leaving doesn't give it back.
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 10.0],
            },
        )
        .unwrap();
        run(&mut w, 15.0);
        assert!(w.buildings.iter().all(|b| b.owner == 1));
    }

    #[test]
    fn cities_take_members_by_id_when_they_change_hands() {
        let mut w = siege(&city()[..1]);
        // Its member 31 comes after it, and nothing has the id 99.
        w.buildings[0].members.push(99);
        w.add_buildings(&city()[1..]);
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [30.0, 10.0],
            },
        )
        .unwrap();
        run(&mut w, 15.0);
        assert!(w.buildings.iter().all(|b| b.owner == 1));
    }

    #[test]
    fn cities_hold_out_while_their_own_are_there() {
        let mut w = battle(
            &[1, 2],
            vec![
                soldier(vec![rifle(1.0, 30.0)]),
                soldier(vec![rifle(1.0, 30.0)]),
            ],
            at_war(),
        );
        w.add_buildings(&city());
        // Player 2's soldier inside the zone, out of the other's reach.
        w.units[1].position = [40.0, 15.0];
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [30.0, 10.0],
            },
        )
        .unwrap();
        run(&mut w, 15.0);
        assert_eq!(w.buildings[0].owner, 2);
        w.units[1].died = Some(w.ticks);
        w.tick();
        assert_eq!(w.buildings[0].owner, 1);
    }

    #[test]
    fn independent_sides_dont_take_cities() {
        let mut sides = at_war();
        sides.players[0].independent = true;
        let mut w = battle(&[1], vec![soldier(vec![rifle(1.0, 30.0)])], sides);
        w.add_buildings(&city());
        w.units[0].position = [40.0, 15.0];
        run(&mut w, 1.0);
        assert_eq!(w.buildings[0].owner, 2);
        // The same soldier takes it for a side that isn't.
        w.sides.players[0].independent = false;
        w.tick();
        assert_eq!(w.buildings[0].owner, 1);
    }
}
