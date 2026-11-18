//! Special operations that do damage round where they land, what they
//! leave on what they hit, and the ground they show their user meanwhile.

use tgmap::{Id, Operation, Strike};

use crate::combat::{Hit, attenuated, damage, resistance};
use crate::{Piece, SECOND, TICK, Target, World, bounding_radius, distance};

/// A strike on its way.
#[derive(Debug, Clone, PartialEq)]
pub struct Incoming {
    pub by: u32,
    pub operation: Id,
    /// The tick it lands on.
    pub lands: u64,
    /// Where it lands: where what it follows was last.
    pub at: [f32; 2],
    /// What it was given on, which it follows.
    pub on: Option<Piece>,
}

/// A unit a strike left a property on that hurts it each second, as the
/// Atomic Bomb's radiation does.
#[derive(Debug, Clone, PartialEq)]
pub struct Afflicted {
    pub unit: usize,
    pub property: Id,
    /// The damage it takes each second, times resistance, past defence.
    pub kind: Id,
    pub amount: f32,
    /// The tick it's next hurt on.
    pub next: u64,
    /// How many more times it is.
    pub left: u32,
}

/// Ground a side is shown for a while, as round where a strike is aimed.
#[derive(Debug, Clone, PartialEq)]
pub struct Watch {
    pub player: u32,
    pub at: [f32; 2],
    pub range: f32,
    /// The tick it's gone on.
    pub until: u64,
}

impl World {
    /// Sends `player`'s strike on its way to `at`, or to `on` wherever it
    /// goes, and shows them the ground there meanwhile.
    pub(crate) fn launch_strike(
        &mut self,
        player: u32,
        operation: &Id,
        strike: &Strike,
        at: [f32; 2],
        on: Option<Piece>,
    ) {
        let ticks = self.ticks;
        let after = |seconds: f32| ticks + (seconds / TICK).round() as u64;
        self.incoming.push(Incoming {
            by: player,
            operation: operation.clone(),
            lands: after(strike.delay),
            at,
            on,
        });
        if let Some(sight) = strike.sight {
            // Its first look comes within a second, and it goes at its last.
            let looks = sight.looks.saturating_sub(1) as f32 + self.chance();
            self.watches.push(Watch {
                player,
                at,
                range: sight.range,
                until: after(looks),
            });
        }
    }

    /// Whether `player`'s strike on `targets` can be given on, and hits,
    /// what `t` is.
    pub(crate) fn strikes(&self, player: u32, targets: &tgmap::Targets, t: Target) -> bool {
        let Some(fighter) = self.fighter(t) else {
            return false;
        };
        let side = self.standing_flag(player, self.owner(t));
        let actor = Id::from("actor");
        targets.allow(fighter.flags.iter().chain([&side, &actor]))
    }

    /// Whether `player`'s strike on `targets` can be given on `on`: on any
    /// member of an organization it hits.
    pub(crate) fn strikes_piece(&self, player: u32, targets: &tgmap::Targets, on: Piece) -> bool {
        match on {
            Piece::Organization(r) => (self.organizations[r].units.iter())
                .any(|&u| self.strikes(player, targets, Target::Unit(u))),
            Piece::Building(b) => self.strikes(player, targets, Target::Building(b)),
        }
    }

    /// Where `on` is now, while it's still there.
    fn whereabouts(&self, on: Piece) -> Option<[f32; 2]> {
        match on {
            Piece::Organization(r) => (!self.destroyed(r)).then(|| self.leader_position(r)),
            Piece::Building(b) => {
                let b = &self.buildings[b];
                (!b.gone()).then_some(b.position)
            }
        }
    }

    /// Lands the strikes due, adding their hits to `hits`, and follows
    /// those still on their way. One given on something that's still
    /// there but no longer fits it does nothing at all.
    pub(crate) fn land_strikes(&mut self, hits: &mut Vec<Hit>) {
        let ticks = self.ticks;
        for i in 0..self.incoming.len() {
            if let Some(at) = self.incoming[i].on.and_then(|on| self.whereabouts(on)) {
                self.incoming[i].at = at;
            }
        }
        let (due, waiting) = std::mem::take(&mut self.incoming)
            .into_iter()
            .partition(|s| s.lands <= ticks);
        self.incoming = waiting;
        for s in due {
            let Some(Operation::Strike(strike)) = self.rules.operations.get(&s.operation) else {
                continue;
            };
            let on = s.on.filter(|&on| self.whereabouts(on).is_some());
            if on.is_some_and(|on| !self.strikes_piece(s.by, &strike.targets, on)) {
                continue;
            }
            let hit = self.blast(s.by, strike, s.at, hits);
            if let Some(a) = &strike.affliction {
                for u in hit {
                    // It isn't renewed on one that has it already.
                    let has = |x: &Afflicted| x.unit == u && x.property == a.property;
                    if !self.afflicted.iter().any(has) {
                        self.afflicted.push(Afflicted {
                            unit: u,
                            property: a.property.clone(),
                            kind: a.damage.kind.clone(),
                            amount: a.damage.amount,
                            next: ticks + SECOND,
                            left: a.seconds.round() as u32,
                        });
                    }
                }
            }
        }
        self.watches.retain(|w| w.until > ticks);
        self.afflict();
    }

    /// The properties strikes have left on a unit that it still holds.
    pub fn lasting(&self, unit: usize) -> impl Iterator<Item = &Id> {
        (self.afflicted.iter())
            .filter(move |a| a.unit == unit)
            .map(|a| &a.property)
    }

    /// Hurts each afflicted unit whose second has come, until it's taken
    /// all it has to.
    fn afflict(&mut self) {
        let ticks = self.ticks;
        let mut afflicted = std::mem::take(&mut self.afflicted);
        for a in afflicted.iter_mut().filter(|a| a.next <= ticks) {
            let u = a.unit;
            let Some(f) = self.fighter(Target::Unit(u)) else {
                a.left = 0;
                continue;
            };
            let ground = self.terrain_effects(Target::Unit(u));
            let amount = a.amount * (1.0 - resistance(f, &ground, &a.kind));
            let unit = &mut self.units[u];
            unit.health -= amount;
            if unit.health <= 0.0 {
                unit.die(ticks);
                self.lost_member(u);
            }
            a.next += SECOND;
            a.left -= 1;
        }
        afflicted.retain(|a| a.left > 0);
        self.afflicted = afflicted;
    }

    /// The hits `player`'s strike does landing at `at`: to everything
    /// within its area that it hits, whoever's it is. Returns the units
    /// hit.
    fn blast(&self, player: u32, strike: &Strike, at: [f32; 2], hits: &mut Vec<Hit>) -> Vec<usize> {
        let mut units = Vec::new();
        for target in self.targets() {
            let f = self.fighter(target).expect("targets can fight");
            let d = (distance(at, self.position(target)) - bounding_radius(f.soft)).max(0.0);
            if d > strike.area || !self.strikes(player, &strike.targets, target) {
                continue;
            }
            let ground = self.terrain_effects(target);
            let falloff = attenuated([strike.full_within, strike.area], strike.edge_damage, d);
            let main = damage(
                &strike.damage,
                f,
                &ground,
                falloff,
                self.boosts(None, target),
            );
            let side = self.standing_flag(player, self.owner(target));
            let flags = || f.flags.iter().chain([&side]);
            let extra: f32 = (strike.extra.iter())
                .filter(|(targets, _)| targets.allow(flags()))
                .map(|(_, d)| d.amount * (1.0 - resistance(f, &ground, &d.kind)))
                .sum();
            hits.push(Hit::strike(target, main + extra, player));
            if let Target::Unit(u) = target {
                units.push(u);
            }
        }
        units
    }
}

crate::hash::digest!(Incoming {
    by,
    operation,
    lands,
    at,
    on
});
crate::hash::digest!(Afflicted {
    unit,
    property,
    kind,
    amount,
    next,
    left
});
crate::hash::digest!(Watch {
    player,
    at,
    range,
    until
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    use tgmap::{Damage, Fighter, Lookout, Targets};

    use crate::{Command, SpecialOp};

    /// Soldiers of 1000 health of players 1, 2 and 1 at 10,10, 10,20 and
    /// 10,30, and player 1 with a strike like Carpet Bomb's: 400 explosive
    /// within 16, full within 4 and a quarter at the edge, 1200 more on
    /// buildings, landing 3 seconds after use and showing 30 round its
    /// centre for 5.
    fn bombing() -> World {
        let tough = || Fighter {
            health: 1000.0,
            ..soldier(Vec::new())
        };
        let mut w = battle(&[1, 2, 1], vec![tough(), tough(), tough()], at_war());
        for u in &mut w.units {
            u.health = 1000.0;
        }
        let explosive = |amount| Damage {
            kind: Id::from("explosive"),
            amount,
        };
        let strike = Strike {
            targets: Targets {
                required: vec![Id::from("actor")],
                excluded: vec![Id::from("air")],
            },
            area: 16.0,
            full_within: 4.0,
            edge_damage: 0.25,
            damage: vec![explosive(400.0)],
            extra: vec![(
                Targets {
                    required: vec![Id::from("building")],
                    excluded: Vec::new(),
                },
                explosive(1200.0),
            )],
            delay: 3.0,
            sight: Some(Lookout {
                range: 30.0,
                looks: 5,
            }),
            affliction: None,
        };
        let bomb = Id::from("bomb");
        (w.rules.operations).insert(bomb.clone(), Operation::Strike(Box::new(strike)));
        w.lead(1, 2000.0, None, vec![SpecialOp::new(bomb, 1800.0, 300.0)]);
        w
    }

    fn bomb(at: [f32; 2], on: Option<Piece>) -> Command {
        Command::Operate {
            operation: 0,
            at,
            on,
        }
    }

    #[test]
    fn a_strike_lands_after_its_delay_on_everything_in_its_area_whoevers() {
        let mut w = bombing();
        w.apply(1, bomb([10.0, 20.0], None)).unwrap();
        assert_eq!(w.watches.len(), 1);
        run(&mut w, 2.9);
        assert!(w.units.iter().all(|u| u.health == 1000.0));
        run(&mut w, 0.2);
        // Full at the centre; at 10 off, 1 − 0.75 × 6 ÷ 12 of it, the
        // user's own as much as the enemy's.
        let health: Vec<f32> = w.units.iter().map(|u| u.health).collect();
        assert_eq!(health, [750.0, 600.0, 750.0]);
        assert!(w.incoming.is_empty());
        // The ground round it shows for its 5 seconds from use.
        assert!(w.watches.len() == 1);
        run(&mut w, 2.0);
        assert!(w.watches.is_empty());
    }

    #[test]
    fn buildings_take_the_extra_damage_whole_and_their_own_sides_too() {
        let mut house = building(40, 1, [30.0, 20.0]);
        house.fighter.health = 5000.0;
        let mut w = with_buildings(bombing(), &[house]);
        // 3.5 from the house's middle is within its soft shape's circle.
        w.apply(1, bomb([26.5, 20.0], None)).unwrap();
        run(&mut w, 3.1);
        assert_eq!(w.buildings[0].health, 5000.0 - 400.0 - 1200.0);
    }

    #[test]
    fn a_strike_given_on_an_organization_follows_it_to_where_it_is_last() {
        let mut w = bombing();
        let enemy = Some(Piece::Organization(1));
        w.apply(1, bomb([10.0, 20.0], enemy)).unwrap();
        w.units[1].position = [30.0, 20.0];
        run(&mut w, 3.1);
        // Landing at 30,20, it reaches neither of player 1's.
        assert_eq!(w.units[1].health, 600.0);
        assert_eq!((w.units[0].health, w.units[2].health), (1000.0, 1000.0));
    }

    #[test]
    fn a_strike_with_no_flight_lands_as_its_used_and_its_sight_shows_meanwhile() {
        let mut w = bombing();
        w.set_fog(tgmap::FogSetting::Full);
        let far = [30.0, 60.0];
        assert_ne!(w.seen(1, far), crate::Seen::Visible);
        if let Some(Operation::Strike(s)) = w.rules.operations.get_mut(&Id::from("bomb")) {
            s.delay = 0.0;
        }
        w.apply(1, bomb([10.0, 20.0], None)).unwrap();
        w.tick();
        assert_eq!(w.units[1].health, 600.0);
        // Its 5 looks show it for 4 to 5 seconds from use.
        assert_eq!(w.seen(1, [10.0, 45.0]), crate::Seen::Visible);
        run(&mut w, 3.9);
        assert_eq!(w.seen(1, [10.0, 45.0]), crate::Seen::Visible);
        run(&mut w, 1.1);
        assert_ne!(w.seen(1, [10.0, 45.0]), crate::Seen::Visible);
    }

    #[test]
    fn a_strike_neither_bombards_nor_draws_fire_nor_captures() {
        let mut fort = building(40, 2, [30.0, 20.0]);
        fort.fighter.capturable = true;
        let mut w = with_buildings(bombing(), &[fort]);
        w.apply(1, bomb([26.5, 20.0], None)).unwrap();
        run(&mut w, 3.1);
        // Brought to no health, it's left disabled with 1, still theirs.
        let fort = &w.buildings[0];
        assert_eq!(
            (fort.state, fort.health, fort.owner),
            (crate::State::Disabled, 1.0, 2)
        );
        let w = {
            let mut w = bombing();
            w.apply(1, bomb([10.0, 20.0], None)).unwrap();
            run(&mut w, 3.1);
            w
        };
        assert!(
            w.organizations
                .iter()
                .all(|r| r.bombarded.is_empty() && r.engaged.is_empty())
        );
    }

    #[test]
    fn radiation_hurts_each_unit_hit_each_second_for_its_time_and_isnt_renewed() {
        let mut w = bombing();
        if let Some(Operation::Strike(s)) = w.rules.operations.get_mut(&Id::from("bomb")) {
            s.affliction = Some(tgmap::Affliction {
                property: Id::from("radiation"),
                damage: Damage {
                    kind: Id::from("flame"),
                    amount: 15.0,
                },
                seconds: 10.0,
            });
        }
        (w.units[1].fighter.as_mut().unwrap().resists).insert(Id::from("flame"), 0.2);
        w.commanders.get_mut(&1).unwrap().experience = 5000.0;
        w.apply(1, bomb([10.0, 20.0], None)).unwrap();
        run(&mut w, 3.1);
        assert_eq!(w.afflicted.len(), 3);
        run(&mut w, 10.0);
        // 10 seconds of 15, less its fifth.
        assert_eq!(w.units[1].health, 600.0 - 120.0);
        assert!(w.afflicted.is_empty());
    }

    #[test]
    fn a_strike_given_on_what_it_no_longer_fits_does_nothing() {
        let mut w = bombing();
        w.apply(1, bomb([10.0, 20.0], Some(Piece::Organization(1))))
            .unwrap();
        (w.units[1].fighter.as_mut().unwrap().flags).push(Id::from("air"));
        run(&mut w, 3.1);
        assert!(w.units.iter().all(|u| u.health == 1000.0));
    }
}
