//! Buildings falling and changing hands, and cities going to whoever
//! holds their zone.

use crate::triggers::Happening;
use crate::{CAPTURE_HEALTH, Notice, Piece, Relation, State, Target, World};

impl World {
    /// Deals with a building out of health: `by` captures it if it can be
    /// captured, else it's destroyed if it can be, else disabled.
    pub(crate) fn fall(&mut self, b: usize, by: Option<u32>) {
        let ticks = self.ticks;
        let building = &mut self.buildings[b];
        building.health = 0.0;
        building.target = None;
        building.aimed.clear();
        if building.fighter.destructible {
            building.besieged = None;
        }
        // Cities change hands by who holds them instead.
        let taker = by.filter(|_| building.fighter.capturable && building.zone.is_none());
        if let Some(by) = taker {
            building.health = CAPTURE_HEALTH * building.fighter.health * building.health_factor;
            self.change_hands(b, by);
            self.happen(Happening::Captured {
                building: b,
                player: by,
            });
        } else if building.fighter.destructible {
            let (owner, struck) = (building.owner, building.struck);
            if let Some(by) = self.killer(struck, owner) {
                self.credit_fall(b, by, false);
            }
            self.remove(b, State::Destroyed(ticks));
            let what = Piece::Building(b);
            self.notices.push(Notice::Destroyed { owner, what });
            // What could have been sold pays its owner as if it had been.
            if self.sells(b) {
                self.pay_sale(b);
            }
        } else {
            building.state = State::Disabled;
        }
    }

    /// Brings down a building a special operation took to no health:
    /// nothing captures it, so one that could be captured is left disabled
    /// with 1 health instead.
    pub(crate) fn fall_unclaimed(&mut self, b: usize) {
        let building = &mut self.buildings[b];
        if building.fighter.capturable && building.zone.is_none() {
            building.health = 1.0;
            building.target = None;
            building.aimed.clear();
            building.state = State::Disabled;
        } else {
            self.fall(b, None);
        }
    }

    /// The centre of the city the building at `b` belongs to, if it's one
    /// of a city's buildings but not its centre.
    pub fn city_of(&self, b: usize) -> Option<usize> {
        let id = self.buildings[b].id;
        (0..self.buildings.len()).find(|&c| {
            let centre = &self.buildings[c];
            c != b && centre.zone.is_some() && !centre.gone() && centre.members.contains(&id)
        })
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
            // An HQ that changes side loses its organizations.
            if let Some(hq) = self.hq_of(i) {
                self.detach_all(hq);
            }
            let building = &mut self.buildings[i];
            building.owner = owner;
            building.captured = Some(self.ticks);
            building.target = None;
            building.aimed.clear();
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
                self.happen(Happening::Captured {
                    building: c,
                    player: winner,
                });
            }
        }
    }

    /// The armed things in the zone of the city centred on `c`, each with
    /// its owner and its health's share. Independent sides' and nobody's
    /// don't count, nor does the centre.
    pub(crate) fn occupants(&self, c: usize, zone: f32) -> Vec<(u32, f32)> {
        let zone = self.pathing.zone(self.buildings[c].position, zone);
        self.targets()
            .filter(|&t| t != Target::Building(c) && self.in_zone(&zone, t))
            .filter(|&t| self.contends(self.owner(t)))
            .filter_map(|t| {
                let f = self.fighter(t).filter(|f| !f.weapons.is_empty())?;
                let (health, full) = match t {
                    Target::Unit(u) => (self.units[u].health, self.max_health(u)),
                    Target::Building(b) => (self.buildings[b].health, f.health),
                };
                Some((self.owner(t), health / full))
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

    use tgmap::{Domain, Tile};

    use crate::{Command, Notice, Piece, State, World};

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
    fn taking_a_building_earns_the_regiment_of_the_unit_that_took_it() {
        let mut bunker = building(20, 2, [10.0, 20.0]);
        bunker.fighter.capturable = true;
        let mut w = siege(&[bunker]);
        let levels = tgmap::Levels {
            levels: vec![tgmap::Level {
                required: 0.0,
                property: "squad_level_0".into(),
            }],
            fake_value: None,
        };
        w.rules.experience_levels.insert("squad".into(), levels);
        // Only the capture counts here, not the engagement before it.
        w.rules.experience_gains.engage_multiplier = 0.0;
        rate(&mut w, "soldier", 10.0);
        rate(&mut w, "house", 10.0);
        let mut fixed = w.rules.combat_profiles[&tgmap::Id::from("house")].clone();
        fixed.speed = None;
        w.rules.combat_profiles.insert("house".into(), fixed);
        run(&mut w, 30.0);
        assert_eq!(w.buildings[0].owner, 1);
        // Worth 0.8 of the rifleman that took it.
        let gain = 0.5 + 0.5 * 0.8;
        assert!((w.organizations[0].experience - gain).abs() < 1e-3);
    }

    #[test]
    fn buildings_out_of_health_are_captured_destroyed_or_disabled() {
        let mut bunker = building(20, 2, [10.0, 20.0]);
        bunker.fighter.capturable = true;
        let mut pillbox = building(21, 2, [20.0, 10.0]);
        pillbox.fighter.destructible = true;
        let mut house = building(22, 2, [0.0, 10.0]);
        house.fighter.destructible = false;
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
            what: Piece::Building(1),
        };
        assert_eq!(w.notices.iter().filter(|&n| *n == destroyed).count(), 1);
    }

    #[test]
    fn cities_go_to_whoever_holds_their_zone() {
        let mut w = siege(&city());
        // Walking into its zone, not stopping to fight it.
        w.organizations[0].formations[0].aggressive = false;
        run(&mut w, 1.0);
        assert_eq!(w.buildings[0].owner, 2);
        w.apply(
            1,
            Command::Move {
                organization: 0,
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
                organization: 0,
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
        w.organizations[0].formations[0].aggressive = false;
        w.apply(
            1,
            Command::Move {
                organization: 0,
                to: [30.0, 10.0],
            },
        )
        .unwrap();
        run(&mut w, 15.0);
        assert!(w.buildings.iter().all(|b| b.owner == 1));
    }

    #[test]
    fn a_citys_buildings_know_its_centre() {
        let w = siege(&city());
        assert_eq!(w.city_of(1), Some(0));
        assert_eq!(w.city_of(0), None);
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
                organization: 0,
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
