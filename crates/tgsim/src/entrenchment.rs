//! Entrenchment: an organization that rests or guards long enough digs in,
//! holding the entrenched state's properties, and handing each member those
//! it qualifies for, until it next sets off along a route.

use tgmap::Id;

use crate::morale::Mind;
use crate::supply::BEAT;
use crate::{TICK, World};

impl World {
    /// Looks at whether each organization is still resting or guarding, and
    /// once a second, on its own beat, digs in one that has been for long
    /// enough.
    pub(crate) fn keep_entrenchment(&mut self) {
        for r in 0..self.organizations.len() {
            // One still trying for where it was sent is travelling, moving
            // or not.
            let trying = self.organizations[r].route_end.is_some();
            if self.organizations[r].travelling && !trying && !self.underway(r) {
                self.organizations[r].travelling = false;
            }
            let calm = self.settled_in(r);
            let ticks = self.ticks;
            let organization = &mut self.organizations[r];
            if !calm {
                organization.settled = None;
                continue;
            }
            let since = *organization.settled.get_or_insert(ticks);
            if organization.entrenched.is_some() || !(ticks + r as u64).is_multiple_of(BEAT) {
                continue;
            }
            let Some(time) = self.entrenchment_time(r) else {
                continue;
            };
            if (ticks - since) as f32 * TICK >= time && self.leader_moves(r) {
                self.organizations[r].entrenched = Some(self.dug_in(r));
            }
        }
    }

    /// Whether the organization at `r` is entrenched.
    pub fn entrenched(&self, r: usize) -> bool {
        self.organizations[r].entrenched.is_some()
    }

    /// Whether the organization at `r` is, as a whole, resting or guarding:
    /// steady, out of any fight, on no order but to stand or guard, and
    /// not on its way anywhere. Its members walking to their places on
    /// their own doesn't count against it.
    fn settled_in(&self, r: usize) -> bool {
        let o = &self.organizations[r];
        let guarding = o.mind == Mind::Steady && o.guarding() && !self.destroyed(r);
        !o.travelling && o.resume.is_none() && (self.at_rest(r) || guarding)
    }

    /// Seconds the organization at `r` has to rest or guard to entrench:
    /// its template's time, divided by 1 plus what its properties add to
    /// the rate. `None` for one whose template sets none.
    fn entrenchment_time(&self, r: usize) -> Option<f32> {
        let organization = &self.organizations[r];
        let (owner, template) = (organization.owner, &organization.template);
        let e = &self.rules.entrenchment;
        let time = *e.times.get(template)?;
        let held = &organization.properties;
        let on = [crate::targets::Thing::Organization(r)];
        let bearing = self.bearing_with_own(&e.rates, |e| &e.requires, owner, template, held, &on);
        let rate = 1.0 + bearing.map(|e| e.rate).sum::<f32>();
        (rate > 0.0).then(|| time / rate)
    }

    /// Whether the organization at `r`'s leader can move at all: one led
    /// by a thing that can't never entrenches. Without a leader, any
    /// member that can will do.
    fn leader_moves(&self, r: usize) -> bool {
        let organization = &self.organizations[r];
        let moves = |&u: &usize| self.units[u].speed_max > 0.0;
        match organization.leader {
            Some(p) => moves(&organization.units[p]),
            None => organization.units.iter().any(moves),
        }
    }

    /// The entrenched state's properties the organization at `r` takes:
    /// each that any of its slots qualifies for, an empty slot counting as
    /// the template it waits for.
    fn dug_in(&self, r: usize) -> Vec<Id> {
        let slots = &self.organizations[r].units;
        let properties = self.rules.entrenchment.properties.iter();
        let taken = properties.filter(|(_, needs)| {
            slots
                .iter()
                .any(|&u| self.qualifies(&self.units[u].template, needs))
        });
        taken.map(|(p, _)| p.clone()).collect()
    }

    /// Whether things made from `template` have every property in `needs`.
    pub(crate) fn qualifies(&self, template: &Id, needs: &[Id]) -> bool {
        let kind = self.rules.kinds.get(template);
        needs
            .iter()
            .all(|p| kind.is_some_and(|k| k.properties.contains(p)))
    }

    /// The properties the unit at `u` takes from its organization's
    /// entrenchment: those it qualifies for.
    pub(crate) fn entrenched_on(&self, u: usize) -> impl Iterator<Item = &Id> {
        let unit = &self.units[u];
        let held = self.organizations[unit.organization]
            .entrenched
            .iter()
            .flatten();
        let needs = |p: &Id| {
            let mut listed = self.rules.entrenchment.properties.iter();
            listed
                .find(|(q, _)| q == p)
                .map_or(&[][..], |(_, n)| n.as_slice())
        };
        held.filter(move |p| self.qualifies(&unit.template, needs(p)))
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::*;
    use crate::{Command, Piece, TICK};

    use tgmap::{CombatEffect, EntrenchmentRate, Id, MoraleEffect};

    /// Player 1's soldier at 10,10, in a squad that entrenches after 30
    /// seconds: foot soldiers dig in for 4 DV, vehicles for 2, and either
    /// loses morale a third slower.
    fn trench() -> crate::World {
        let mut w = battle(&[1], vec![soldier(Vec::new())], at_war());
        let e = &mut w.rules.entrenchment;
        e.times.insert("squad".into(), 30.0);
        let needs = |p: &str| vec![Id::from(p)];
        e.properties = vec![
            ("foot_dug_in".into(), needs("infantry")),
            ("wheels_dug_in".into(), needs("vehicle")),
        ];
        for (p, defense) in [("foot_dug_in", 4.0), ("wheels_dug_in", 2.0)] {
            let effect = CombatEffect {
                defense,
                ..CombatEffect::default()
            };
            w.rules.combat_effects.insert(p.into(), effect);
            let morale = MoraleEffect {
                loss_rate: 0.5,
                ..MoraleEffect::default()
            };
            w.rules.morale_effects.insert(p.into(), morale);
        }
        w.rules.kinds.insert("soldier".into(), kind(&["infantry"]));
        w.rules.kinds.insert("jeep".into(), kind(&["vehicle"]));
        w.rules.kinds.insert("squad".into(), kind(&["org_unit"]));
        w
    }

    fn loss_rate(w: &crate::World) -> f32 {
        w.morale_effects(0).map(|e| e.loss_rate).sum()
    }

    /// Gives `command`, planned at once rather than at the regiment's
    /// next look.
    fn order(w: &mut crate::World, command: Command) {
        w.apply(1, command).unwrap();
        w.plan_pending();
    }

    #[test]
    fn a_regiment_sitting_from_the_start_entrenches_its_time_in() {
        let mut w = trench();
        run(&mut w, 29.0);
        assert!(!w.entrenched(0));
        assert_eq!(w.boost(0).defense, 0.0);
        run(&mut w, 1.0 + TICK);
        assert!(w.entrenched(0));
        // Its foot soldier takes the infantry's, and it only that.
        assert_eq!(w.boost(0).defense, 4.0);
        assert_eq!(loss_rate(&w), 0.5);
    }

    #[test]
    fn a_regiment_holds_what_any_member_qualifies_for_and_each_member_its_own() {
        let mut w = trench();
        w.units[0].template = "jeep".into();
        run(&mut w, 31.0);
        let wheels = Some(vec![Id::from("wheels_dug_in")]);
        assert_eq!(w.organizations[0].entrenched, wheels);
        // One of both kinds holds both, which both count.
        let mut both = world(tgmap::Pathing::uniform(40, tgmap::Domain::Land));
        both.rules = trench().rules;
        both.units[1].template = "jeep".into();
        run(&mut both, 31.0);
        assert_eq!(loss_rate(&both), 1.0);
        assert_eq!(both.boost(0).defense, 4.0);
        assert_eq!(both.boost(1).defense, 2.0);
    }

    #[test]
    fn what_a_regiment_holds_can_speed_entrenching() {
        let mut w = trench();
        let rate = EntrenchmentRate {
            rate: 0.25,
            requires: vec!["org_unit".into()],
        };
        w.rules.entrenchment.rates.insert("spades".into(), rate);
        w.grant(1, Id::from("spades"));
        run(&mut w, 25.0);
        assert!(w.entrenched(0));
    }

    #[test]
    fn a_move_stays_entrenched_until_it_plans_and_for_good_if_that_fails() {
        let mut w = trench();
        for y in 0..w.pathing.side() as i32 {
            w.pathing.set(tgmap::Tile::new(10, y), tgmap::Domain::Naval);
        }
        run(&mut w, 31.0);
        let across = Command::Move {
            organization: 0,
            to: [30.0, 10.0],
        };
        w.apply(1, across).unwrap();
        // Until its look it's as it was; then it finds no way and stays so.
        assert!(w.entrenched(0) && w.organizations[0].pending.is_some());
        run(&mut w, 2.0);
        assert!(w.entrenched(0) && !w.underway(0));
    }

    #[test]
    fn only_setting_off_ends_it() {
        let mut w = trench();
        run(&mut w, 31.0);
        // Not a fight, a formation change, or a move to where it stands.
        w.organizations[0].engaged.push(Piece::Organization(0));
        w.keep_entrenchment();
        w.organizations[0].engaged.clear();
        let here = w.organizations[0].position;
        let nudge = [here[0] + 0.4, here[1]];
        order(
            &mut w,
            Command::Move {
                organization: 0,
                to: nudge,
            },
        );
        run(&mut w, 2.0);
        assert!(w.entrenched(0));
        let to = [here[0], here[1] + 5.0];
        order(
            &mut w,
            Command::Move {
                organization: 0,
                to,
            },
        );
        w.tick();
        assert!(!w.entrenched(0));
        // It counts afresh once it's there.
        run(&mut w, 20.0);
        assert!(!w.underway(0) && !w.entrenched(0));
    }

    #[test]
    fn a_move_that_finds_no_route_leaves_it_be() {
        let mut w = trench();
        // Water from 20 to 22 the length of the map.
        for y in 0..w.pathing.side() as i32 {
            w.pathing.set(tgmap::Tile::new(10, y), tgmap::Domain::Naval);
        }
        run(&mut w, 31.0);
        let (here, facing) = (w.organizations[0].position, w.organizations[0].facing);
        // Across the water there's no route: it stays as it was, its
        // attack dropped.
        let across = Command::Attack {
            organization: 0,
            to: [30.0, 10.0],
        };
        order(&mut w, across);
        run(&mut w, 2.0);
        assert!(w.entrenched(0));
        assert!(!w.underway(0));
        assert_eq!(w.organizations[0].position, here);
        assert_eq!(w.organizations[0].facing, facing);
        assert_eq!(w.organizations[0].order, crate::Order::Move);
        // One sent with a facing turns to it all the same.
        w.send(0, [30.0, 10.0], [0.0, 1.0]);
        w.plan_pending();
        assert_eq!(w.organizations[0].facing, [0.0, 1.0]);
        assert!(w.entrenched(0));
        // Into the water it heads for the near shore, which ends it.
        order(
            &mut w,
            Command::Move {
                organization: 0,
                to: [21.0, 10.0],
            },
        );
        w.tick();
        assert!(!w.entrenched(0));
        run(&mut w, 10.0);
        let at = w.units[0].position;
        assert!((17.0..20.0).contains(&at[0]), "{at:?}");
        // From the shore itself the nearest spot short of the water is
        // where it stands, so there's no route.
        let mut w = trench();
        for y in 0..w.pathing.side() as i32 {
            w.pathing.set(tgmap::Tile::new(10, y), tgmap::Domain::Naval);
        }
        w.units[0].position = [19.5, 10.0];
        w.organizations[0].position = [19.5, 10.0];
        run(&mut w, 31.0);
        let wade = Command::Move {
            organization: 0,
            to: [21.0, 10.0],
        };
        order(&mut w, wade);
        w.tick();
        assert!(w.entrenched(0));
    }

    #[test]
    fn a_fight_doesnt_end_it_whatever_the_formation() {
        let mut w = skirmish();
        w.rules = trench().rules;
        // Its man stands off its anchor, so the two never coincide.
        for f in &mut w.organizations[0].formations {
            f.offsets = vec![[0.0, -3.0]];
        }
        run(&mut w, 31.0);
        assert!(w.entrenched(0));
        w.apply(
            2,
            Command::Move {
                organization: 1,
                to: [10.0, 12.0],
            },
        )
        .unwrap();
        run(&mut w, 20.0);
        assert!(w.destroyed(1), "the fight is over");
        assert!(w.entrenched(0));
        // Nor does stopping it where it stands.
        order(&mut w, Command::Stop { organization: 0 });
        run(&mut w, 2.0);
        assert!(w.entrenched(0));
    }

    #[test]
    fn any_other_activity_starts_the_count_again() {
        let mut w = trench();
        run(&mut w, 20.0);
        w.organizations[0].engaged.push(Piece::Organization(0));
        w.keep_entrenchment();
        w.organizations[0].engaged.clear();
        run(&mut w, 20.0);
        assert!(!w.entrenched(0));
        run(&mut w, 11.0);
        assert!(w.entrenched(0));
    }

    #[test]
    fn nothing_led_by_what_cant_move_entrenches() {
        let mut w = trench();
        w.units[0].speed_max = 0.0;
        run(&mut w, 40.0);
        assert!(!w.entrenched(0));
        w.rules.entrenchment.times.clear();
        w.units[0].speed_max = 2.0;
        run(&mut w, 40.0);
        assert!(!w.entrenched(0));
    }
}
