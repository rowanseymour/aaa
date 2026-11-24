//! Units choosing what to fight and which of their abilities to use on it.

use tgmap::{Id, Ratings, Weapon};

use crate::combat::resistance;
use crate::{CONTACT, Order, TICK, Target, World, distance, next_chance};

/// How one of a unit's abilities rates against a target it can be used
/// on, worst first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Rating {
    /// Out of range, for a unit that can go after it.
    MoveIntoRange,
    /// In reach but not ready.
    Wait,
    UseNow,
}

impl Rating {
    fn of(self, amounts: &Ratings) -> f32 {
        match self {
            Rating::MoveIntoRange => amounts.move_into_range,
            Rating::Wait => amounts.wait,
            Rating::UseNow => amounts.use_now,
        }
    }
}

/// The ability a unit would use on a target, and how it scores there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Pick {
    /// By index into its fighter's weapons.
    pub weapon: usize,
    /// How far it reaches.
    pub range: f32,
    pub rating: Rating,
    pub score: f32,
}

impl World {
    /// Whether the unit at `u` has `weapon` to use: it holds all the
    /// properties it needs, its organization's level is within its limits,
    /// and it's a bombarding one while its organization bombards.
    pub(crate) fn has(&self, u: usize, weapon: &Weapon) -> bool {
        let organization = &self.organizations[self.units[u].organization];
        let [lowest, highest] = weapon.choice.levels;
        let level = organization.level;
        (weapon.bombard || !organization.bombarding)
            && lowest.is_none_or(|l| level >= l)
            && highest.is_none_or(|h| level <= h)
            && weapon.choice.requires.iter().all(|p| self.unit_holds(u, p))
    }

    /// Whether the unit at `u` holds property `p`: from its template, its
    /// side's technologies, its organization's level or what its
    /// organization holds for a time.
    fn unit_holds(&self, u: usize, p: &Id) -> bool {
        let unit = &self.units[u];
        let r = unit.organization;
        let organization = &self.organizations[r];
        let kind = self.rules.kinds.get(&unit.template);
        let on = self.weighed(crate::Target::Unit(u));
        (kind.is_some_and(|k| k.properties.contains(p)) && self.keeps_own(&on, p))
            || (self.holds(organization.owner, p)
                && self.keeps_side_copy(organization.owner, &on, p))
            || self.level_property(r) == Some(p)
            || self.handed_down(r).any(|h| &h.property == p)
            || unit.properties.iter().any(|h| &h.property == p)
    }

    /// How far each of the unit at `u`'s weapons reaches: its own range,
    /// and what its properties add to those that fire projectiles or
    /// bombard.
    pub(crate) fn ranges(&self, u: usize) -> Vec<f32> {
        let Some(fighter) = &self.units[u].fighter else {
            return Vec::new();
        };
        let boost = self.boost(u);
        (fighter.weapons.iter())
            .map(|w| {
                let projectile = w.projectile.is_some() || w.flight.is_some();
                w.range
                    + if projectile {
                        boost.projectile_range
                    } else {
                        0.0
                    }
                    + if w.bombard { boost.bombard_range } else { 0.0 }
            })
            .collect()
    }

    /// Whether the unit at `u`'s weapon `w` has recharged.
    pub(crate) fn ready(&self, u: usize, w: usize) -> bool {
        let unit = &self.units[u];
        let (Some(fighter), Some(&ago)) = (&unit.fighter, unit.fired_ago.get(w)) else {
            return false;
        };
        ago.is_infinite()
            || ago * self.fire_rate(Target::Unit(u)) >= fighter.weapons[w].recharge - 1e-4
    }

    /// How many ticks from now the unit at `u`'s weapon `w` will have
    /// recharged by, from one to `most`, as [`World::ready`] will find it.
    pub(crate) fn recharged_in(&self, u: usize, w: usize, most: u64) -> u64 {
        let unit = &self.units[u];
        let (Some(fighter), Some(&ago)) = (&unit.fighter, unit.fired_ago.get(w)) else {
            return most;
        };
        let (rate, recharge) = (self.fire_rate(Target::Unit(u)), fighter.weapons[w].recharge);
        let mut later = ago;
        for n in 1..most {
            later += TICK;
            if later * rate >= recharge - 1e-4 {
                return n;
            }
        }
        most
    }

    /// Whether the unit at `u` can go after what's out of its range: it
    /// moves, and its organization's engaged.
    fn can_close(&self, u: usize) -> bool {
        let unit = &self.units[u];
        unit.speed_max > 0.0 && !self.organizations[unit.organization].engaged.is_empty()
    }

    /// Whether the unit at `u` could use `weapon` on `t`, however far it
    /// is: `t` matches its targets, is beyond its minimum range and is
    /// within its turret's arc.
    pub(crate) fn bears_on(&self, u: usize, weapon: &Weapon, t: Target) -> bool {
        let s = Target::Unit(u);
        let inside = || self.gap(s, t).is_some_and(|g| g <= weapon.min_range);
        self.can_target(s, weapon, t)
            && !(weapon.min_range > 0.0 && inside())
            && self.in_arc(s, weapon, t)
    }

    /// How the unit at `u`'s weapon `w`, reaching `range`, rates against
    /// `t`, if it can be used on it at all.
    pub(crate) fn rate(&self, u: usize, w: usize, range: f32, t: Target) -> Option<Rating> {
        let weapon = self.units[u].fighter.as_ref()?.weapons.get(w)?;
        if self.fighter(t).is_none() || !self.bears_on(u, weapon, t) {
            return None;
        }
        if self.gap(Target::Unit(u), t).is_some_and(|g| g > range) {
            return self.can_close(u).then_some(Rating::MoveIntoRange);
        }
        Some(match self.ready(u, w) {
            true => Rating::UseNow,
            false => Rating::Wait,
        })
    }

    /// What the unit at `u`'s weapon `w`, rating `rating` against `t`,
    /// scores. Going after a target with a weapon still recharging scores
    /// as waiting would, but with its fighter's bonus for moving.
    pub(crate) fn score(&self, u: usize, w: usize, rating: Rating, t: Target) -> f32 {
        let Some(fighter) = &self.units[u].fighter else {
            return 0.0;
        };
        let weapon = &fighter.weapons[w];
        let choice = &weapon.choice;
        let as_if = match rating == Rating::MoveIntoRange && !self.ready(u, w) {
            true => Rating::Wait,
            false => rating,
        };
        let own = choice.priority * as_if.of(&choice.weight) + as_if.of(&choice.bonus);
        let score = own + rating.of(&fighter.bonus);
        let main = weapon.main_damage().next();
        match (fighter.check_resistance, main, self.fighter(t)) {
            (true, Some(d), Some(target)) => {
                let shelter = self.shelter(t);
                let ground = self.protection(t, &shelter);
                score * (1.0 - resistance(target, &ground, &d.kind))
            }
            _ => score,
        }
    }

    /// The best of the unit at `u`'s weapons, reaching `ranges`, to use on
    /// `t`, if any can be. Ties go, in the weapons' order, to the longer
    /// range of those to go after it with, else to the higher priority,
    /// the shorter range, then a coin from `rng`.
    pub(crate) fn choose(
        &self,
        u: usize,
        ranges: &[f32],
        t: Target,
        rng: &mut u64,
    ) -> Option<Pick> {
        use std::cmp::Ordering::{Equal, Greater, Less};
        let fighter = self.units[u].fighter.as_ref()?;
        let mut best: Option<Pick> = None;
        for (w, weapon) in fighter.weapons.iter().enumerate() {
            if !self.has(u, weapon) {
                continue;
            }
            let range = ranges[w];
            let Some(rating) = self.rate(u, w, range, t) else {
                continue;
            };
            let pick = Pick {
                weapon: w,
                range,
                rating,
                score: self.score(u, w, rating, t),
            };
            let Some(was) = best else {
                best = Some(pick);
                continue;
            };
            let (old, old_range) = (&fighter.weapons[was.weapon].choice, ranges[was.weapon]);
            let better = match pick.score.total_cmp(&was.score) {
                Greater => true,
                Less => false,
                Equal if rating == Rating::MoveIntoRange => range > old_range,
                Equal => match weapon.choice.priority.total_cmp(&old.priority) {
                    Equal => match range.total_cmp(&old_range) {
                        Equal => next_chance(rng) < 0.5,
                        shorter => shorter.is_lt(),
                    },
                    higher => higher.is_gt(),
                },
            };
            if better {
                best = Some(pick);
            }
        }
        best
    }

    /// What `t` adds to its best weapon's score for the unit at `u`: more
    /// while it's attacking that unit.
    fn attacker_bonus(&self, u: usize, t: Target) -> f32 {
        let target = match t {
            Target::Unit(v) => self.units[v].target,
            Target::Building(b) => self.buildings[b].target,
        };
        match target == Some(Target::Unit(u)) {
            true => self.rules.attacker_bonus,
            false => 0.0,
        }
    }

    /// How far before others the unit at `u` takes on `t`: its
    /// organization's attack target, then armed enemies that are
    /// aggressive, then other armed ones, then what can be captured or
    /// destroyed, then the rest.
    fn tier(&self, u: usize, t: Target) -> u8 {
        let Some(f) = self.fighter(t) else {
            return 0;
        };
        let r = self.units[u].organization;
        let attacked = match (self.organizations[r].order, t) {
            (Order::Pursue(e), Target::Unit(v)) => self.units[v].organization == e,
            _ => false,
        };
        let armed = !f.weapons.is_empty();
        let aggressive = match t {
            Target::Unit(v) => self.organizations[self.units[v].organization].aggressive(),
            Target::Building(_) => true,
        };
        match (attacked, armed) {
            (true, _) if self.rules.attack_target_first => 6,
            (true, true) => 5,
            (false, true) if aggressive => 5,
            (true, false) => 4,
            (false, true) => 3,
            (false, false) if f.capturable || f.destructible => 2,
            (false, false) => 1,
        }
    }

    /// Whether `t` is among what the organization at `r` is engaged with,
    /// itself or by its organization.
    fn engaged_with(&self, r: usize, t: Target) -> bool {
        self.organizations[r].engaged.contains(&self.piece(t))
    }

    /// What the unit at `u` takes on, and with which weapon. It keeps its
    /// target while its organization's still engaged with it and it has a
    /// weapon for it, unless it's going after one below the aggressive, or
    /// an enemy that's hit it or that it's run into since it last decided
    /// rates better; otherwise it picks afresh from the enemies within its
    /// organization's operational range of its centre, and what its
    /// organization is engaged with. A unit only picks while its
    /// organization is engaged.
    pub(crate) fn decide_attack(&self, u: usize, rng: &mut u64) -> Option<(Target, Pick)> {
        let unit = &self.units[u];
        let r = unit.organization;
        if self.organizations[r].engaged.is_empty() {
            return None;
        }
        let ranges = self.ranges(u);
        let (centre, reach) = (
            self.centre(r),
            self.organizations[r].operational_range.max(CONTACT),
        );
        let near = |t: Target| self.reach(centre, t) <= reach;
        let kept = unit
            .target
            .filter(|&t| self.engaged_with(r, t))
            .and_then(|t| Some((t, self.choose(u, &ranges, t, rng)?)));
        let keeps = kept.filter(|&(t, _)| unit.closing.is_none() || self.tier(u, t) >= 5);
        if let Some((t, pick)) = keeps {
            let by = unit
                .struck
                .filter(|s| s.at >= unit.decided)
                .and_then(|s| s.from);
            let worth = |(a, p): (Target, Pick)| (p.rating, p.score + self.attacker_bonus(u, a));
            let mut best = (t, pick);
            for a in [by, unit.met].into_iter().flatten() {
                if a == t || !near(a) {
                    continue;
                }
                if let Some(p) = self.choose(u, &ranges, a, rng)
                    && worth((a, p)) > worth(best)
                {
                    best = (a, p);
                }
            }
            return Some(best);
        }
        let from = unit.position;
        let candidates = self
            .targets()
            .filter(|&t| self.hostile(r, self.piece(t)) && near(t));
        let mut best: Option<(Target, Pick, (u8, f32, f32))> = None;
        for t in candidates.collect::<Vec<_>>() {
            let tier = self.tier(u, t);
            // What its organization engages it's handed, whatever its tier.
            if tier < 2 && !self.engaged_with(r, t) {
                continue;
            }
            let pick = match kept {
                Some((k, pick)) if k == t => Some(pick),
                _ => self.choose(u, &ranges, t, rng),
            };
            let Some(pick) = pick else {
                continue;
            };
            let rank = (
                tier,
                pick.score + self.attacker_bonus(u, t),
                -distance(from, self.position(t)),
            );
            if best.as_ref().is_none_or(|(_, _, was)| rank > *was) {
                best = Some((t, pick, rank));
            }
        }
        best.map(|(t, pick, _)| (t, pick))
    }
}

#[cfg(test)]
mod tests {
    use tgmap::{Choice, Id, Ratings, Weapon};

    use crate::testing::*;
    use crate::{Piece, Target};

    use super::Rating;

    fn choice(priority: f32, weight: [f32; 3], bonus: [f32; 3]) -> Choice {
        let ratings = |[move_into_range, use_now, wait]: [f32; 3]| Ratings {
            move_into_range,
            use_now,
            wait,
        };
        Choice {
            priority,
            weight: ratings(weight),
            bonus: ratings(bonus),
            ..Choice::default()
        }
    }

    /// The banzai infantryman of the combat spec's worked example: a rifle,
    /// and a charge only at vehicles that does far more.
    fn banzai() -> tgmap::Fighter {
        let gun = Weapon {
            recharge: 3.0,
            choice: choice(1.0, [1.0, 1.0, 0.0], [3.0, 50.0, 50.0]),
            ..rifle(12.0, 10.0)
        };
        let mut charge = Weapon {
            recharge: 50.0,
            targets: tgmap::Targets {
                required: vec![Id::from("enemy"), Id::from("vehicle")],
                excluded: Vec::new(),
            },
            hits: vec![tgmap::WeaponHit::main(tgmap::Damage {
                kind: Id::from("armor_piercing"),
                amount: 200.0,
            })],
            choice: choice(1.0, [1.0, 1.0, 0.0], [110.0, 110.0, 100.0]),
            ..rifle(0.5, 0.0)
        };
        charge.choice.lasts = 1.0;
        let mut man = soldier(vec![gun, charge]);
        man.check_resistance = true;
        man
    }

    fn tank() -> tgmap::Fighter {
        let mut tank = soldier(Vec::new());
        tank.health = 1000.0;
        tank.flags = vec![Id::from("vehicle")];
        tank.resists = [(Id::from("gunfire"), 0.6)].into();
        tank
    }

    #[test]
    fn units_run_at_what_their_best_ability_is_for_and_use_it() {
        let mut w = battle(&[1, 2], vec![banzai(), tank()], at_war());
        w.organizations[1].formations[0].aggressive = false;
        w.organizations[0].engaged = vec![Piece::Organization(1)];
        // The rifle could fire now, 1 × 1 + 50 at 0.4 for the tank's
        // resistance, but the charge scores 1 × 1 + 110 to go after it.
        let tank = Target::Unit(1);
        let rifle = w.rate(0, 0, 12.0, tank).unwrap();
        assert_eq!(rifle, Rating::UseNow);
        assert!((w.score(0, 0, rifle, tank) - 20.4).abs() < 1e-4);
        let best = w.choose(0, &w.ranges(0), tank, &mut 0).unwrap();
        assert_eq!(
            (best.weapon, best.rating, best.score),
            (1, Rating::MoveIntoRange, 111.0)
        );
        run(&mut w, 6.0);
        // It ran at the tank and charged it, never firing the rifle.
        assert_eq!(w.units[1].health, 800.0);
        assert!(w.gap(Target::Unit(0), tank).unwrap() <= 0.5);
        // Recharging, the charge now scores only as waiting would, 1 × 0 +
        // 100, but still beats the rifle's 20.4.
        let best = w.choose(0, &w.ranges(0), tank, &mut 0).unwrap();
        assert_eq!(
            (best.weapon, best.rating, best.score),
            (1, Rating::Wait, 100.0)
        );
    }

    #[test]
    fn units_use_their_rifle_on_what_their_charge_cant_reach() {
        let mut w = battle(&[1, 2], vec![banzai(), soldier(Vec::new())], at_war());
        w.organizations[0].engaged = vec![Piece::Organization(1)];
        let man = Target::Unit(1);
        let best = w.choose(0, &w.ranges(0), man, &mut 0).unwrap();
        assert_eq!(
            (best.weapon, best.rating, best.score),
            (0, Rating::UseNow, 51.0)
        );
        w.tick();
        assert_eq!(w.units[0].shot.map(|(_, w)| w), Some(0));
        // Recharging it waits, 1 × 0 + 50.
        let best = w.choose(0, &w.ranges(0), man, &mut 0).unwrap();
        assert_eq!((best.rating, best.score), (Rating::Wait, 50.0));
    }

    #[test]
    fn abilities_need_their_properties_levels_and_bombarding() {
        let mut w = battle(
            &[1, 2],
            vec![soldier(vec![rifle(12.0, 10.0)]), soldier(Vec::new())],
            at_war(),
        );
        let man = Target::Unit(1);
        fn gun(w: &mut crate::World) -> &mut Weapon {
            &mut w.units[0].fighter.as_mut().unwrap().weapons[0]
        }
        assert!(w.choose(0, &w.ranges(0), man, &mut 0).is_some());
        gun(&mut w).choice.requires = vec![Id::from("tech_napalm")];
        assert!(w.choose(0, &w.ranges(0), man, &mut 0).is_none());
        w.grant(1, Id::from("tech_napalm"));
        assert!(w.choose(0, &w.ranges(0), man, &mut 0).is_some());
        gun(&mut w).choice.levels = [Some(1), None];
        assert!(w.choose(0, &w.ranges(0), man, &mut 0).is_none());
        w.organizations[0].level = 1;
        assert!(w.choose(0, &w.ranges(0), man, &mut 0).is_some());
        gun(&mut w).choice.levels = [None, Some(0)];
        assert!(w.choose(0, &w.ranges(0), man, &mut 0).is_none());
        gun(&mut w).choice.levels = [None; 2];
        w.organizations[0].bombarding = true;
        assert!(w.choose(0, &w.ranges(0), man, &mut 0).is_none());
        gun(&mut w).bombard = true;
        assert!(w.choose(0, &w.ranges(0), man, &mut 0).is_some());
    }

    #[test]
    fn units_use_one_ability_at_a_time() {
        let lasting = |amount| {
            let mut gun = rifle(12.0, amount);
            gun.choice.lasts = 1.0;
            gun.choice.bonus.use_now = 1.0;
            gun
        };
        let mut w = battle(
            &[1, 2],
            vec![
                soldier(vec![lasting(10.0), lasting(5.0)]),
                soldier(Vec::new()),
            ],
            at_war(),
        );
        w.organizations[1].formations[0].aggressive = false;
        w.tick();
        w.tick();
        // Both are ready: on a tie the shorter range wins, and with them
        // the same, a coin.
        let fired = w.units[0].shot.unwrap().1;
        assert_eq!(w.units[1].health, 100.0 - [10.0, 5.0][fired]);
        run(&mut w, 0.9);
        assert_eq!(w.units[1].health, 100.0 - [10.0, 5.0][fired]);
        run(&mut w, 0.2);
        assert_eq!(w.units[1].health, 85.0);
    }

    #[test]
    fn ties_go_to_the_higher_priority_then_the_shorter_range() {
        let mut w = battle(
            &[1, 2],
            vec![
                soldier(vec![rifle(12.0, 10.0), rifle(11.0, 10.0)]),
                soldier(Vec::new()),
            ],
            at_war(),
        );
        w.organizations[0].engaged = vec![Piece::Organization(1)];
        let man = Target::Unit(1);
        assert_eq!(w.choose(0, &w.ranges(0), man, &mut 0).unwrap().weapon, 1);
        // A higher priority wins even where it scores the same.
        let gun = &mut w.units[0].fighter.as_mut().unwrap().weapons[0];
        gun.choice = choice(1.0, [1.0, 0.0, 1.0], [0.0; 3]);
        assert_eq!(w.choose(0, &w.ranges(0), man, &mut 0).unwrap().weapon, 0);
        // Of those to go after it with, the longer range.
        w.units[1].position = [10.0, 26.0];
        let gun = &mut w.units[0].fighter.as_mut().unwrap().weapons[0];
        gun.choice = choice(0.0, [1.0; 3], [0.0; 3]);
        let best = w.choose(0, &w.ranges(0), man, &mut 0).unwrap();
        assert_eq!((best.weapon, best.rating), (0, Rating::MoveIntoRange));
    }

    #[test]
    fn units_take_on_aggressive_enemies_first_and_what_cant_be_taken_last() {
        // A passive armed enemy nearer, an aggressive one further, and a
        // house that can be neither captured nor destroyed nearest.
        let rifles = || soldier(vec![rifle(4.0, 1.0)]);
        let mut w = battle(
            &[1, 2, 2],
            vec![soldier(vec![rifle(30.0, 10.0)]), rifles(), rifles()],
            at_war(),
        );
        w.organizations[1].formations[0].aggressive = false;
        let mut house = building(20, 2, [16.0, 10.0]);
        house.fighter.destructible = false;
        w.add_buildings(&[house]);
        w.organizations[0].engaged = vec![Piece::Organization(1)];
        w.tick();
        assert_eq!(w.units[0].target, Some(Target::Unit(2)));
        // And sticks with it while it's engaged with it, even once it's
        // no longer aggressive.
        w.organizations[0].engaged = vec![Piece::Organization(1), Piece::Organization(2)];
        w.organizations[2].formations[0].aggressive = false;
        run(&mut w, 1.0);
        assert_eq!(w.units[0].target, Some(Target::Unit(2)));
        // Once it's gone it picks afresh when it next decides, the house
        // last...
        w.units[2].die(w.ticks);
        run(&mut w, 0.6);
        assert_eq!(w.units[0].target, Some(Target::Unit(1)));
        // ...and only as what its regiment's engaged with.
        w.units[1].die(w.ticks);
        w.organizations[0].engaged.clear();
        w.organizations[0].formations[0].aggressive = false;
        w.organizations[0].engaged = vec![Piece::Organization(1)];
        run(&mut w, 0.6);
        assert_eq!(w.units[0].target, None);
        w.organizations[0].engaged = vec![Piece::Building(0)];
        run(&mut w, 0.6);
        assert_eq!(w.units[0].target, Some(Target::Building(0)));
    }

    #[test]
    fn units_turn_on_an_enemy_that_hits_them_if_it_rates_better() {
        let mut w = battle(
            &[1, 2, 2],
            vec![
                soldier(vec![rifle(30.0, 1.0)]),
                soldier(Vec::new()),
                soldier(vec![rifle(30.0, 10.0)]),
            ],
            at_war(),
        );
        for r in &mut w.organizations[1..] {
            r.formations[0].aggressive = false;
        }
        w.organizations[0].engaged = vec![Piece::Organization(1), Piece::Organization(2)];
        w.organizations[2].engaged = vec![Piece::Organization(0)];
        w.units[0].target = Some(Target::Unit(1));
        w.tick();
        // It fires on the one it had, and is hit by the other.
        assert_eq!(w.units[1].health, 99.0);
        assert_eq!(w.units[0].health, 90.0);
        assert_eq!(w.units[0].target, Some(Target::Unit(1)));
        // Both now rate waiting, but what fires on it gets its 0.5 more.
        w.tick();
        assert_eq!(w.units[0].target, Some(Target::Unit(2)));
    }

    #[test]
    fn units_turn_on_an_enemy_they_run_into_if_it_rates_better() {
        let rifles = || soldier(vec![rifle(4.0, 1.0)]);
        let mut w = battle(
            &[1, 2, 2],
            vec![soldier(vec![rifle(12.0, 10.0)]), rifles(), rifles()],
            at_war(),
        );
        w.units[1].position = [10.0, 35.0];
        w.units[2].position = [10.0, 18.0];
        w.organizations[0].engaged = vec![Piece::Organization(1), Piece::Organization(2)];
        // Going after an aggressive enemy, it keeps after it...
        w.units[0].target = Some(Target::Unit(1));
        w.units[0].closing = Some(12.0);
        let decided = w.decide_attack(0, &mut 0).unwrap();
        assert_eq!(
            (decided.0, decided.1.rating),
            (Target::Unit(1), Rating::MoveIntoRange)
        );
        // ...until it runs into one it can fire on where it is.
        w.units[0].met = Some(Target::Unit(2));
        let decided = w.decide_attack(0, &mut 0).unwrap();
        assert_eq!(
            (decided.0, decided.1.rating),
            (Target::Unit(2), Rating::UseNow)
        );
    }

    #[test]
    fn units_wait_a_moment_once_engaged_before_deciding() {
        let mut w = battle(
            &[1, 2],
            vec![soldier(vec![rifle(12.0, 10.0)]), soldier(Vec::new())],
            at_war(),
        );
        w.organizations[1].formations[0].aggressive = false;
        let first_shot = |mut w: crate::World, reaction: f32| {
            w.rules.engage_reaction = reaction;
            let mut ticks = 0;
            while w.units[1].health == 100.0 && ticks < 200 {
                w.tick();
                ticks += 1;
            }
            ticks
        };
        // Engaged on the first tick, it fires on the next with no pause,
        // and some time in the next 10 s with one.
        assert_eq!(first_shot(w.clone(), 0.0), 2);
        let ticks = first_shot(w, 10.0);
        assert!((3..=102).contains(&ticks), "{ticks}");
    }

    #[test]
    fn waiting_units_decide_again_once_recharged_or_after_a_while() {
        let mut w = battle(
            &[1, 2],
            vec![soldier(vec![rifle(12.0, 10.0)]), soldier(Vec::new())],
            at_war(),
        );
        w.organizations[1].formations[0].aggressive = false;
        // Engaged on the first tick, it fires on the next.
        w.tick();
        w.tick();
        assert_eq!(w.units[1].health, 90.0);
        // With 2 s to recharge it holds for 0.6 s...
        w.tick();
        let held = w.units[0].busy - w.units[0].decided;
        w.units[0].busy = 0;
        // ...and for less when it's sooner ready.
        w.units[0].fired_ago[0] = 1.75;
        w.tick();
        assert_eq!([held, w.units[0].busy - w.units[0].decided], [6, 2]);
    }

    #[test]
    fn units_go_from_one_target_to_the_next_then_back_to_their_place() {
        let mut w = battle(
            &[1, 2, 2],
            vec![
                soldier(vec![rifle(2.0, 100.0)]),
                soldier(Vec::new()),
                soldier(Vec::new()),
            ],
            at_war(),
        );
        for r in &mut w.organizations[1..] {
            r.formations[0].aggressive = false;
        }
        w.units[2].position = [16.0, 22.0];
        w.organizations[0].engaged = vec![Piece::Organization(1), Piece::Organization(2)];
        let mut ticks = 0;
        while w.units[1].alive() {
            w.tick();
            ticks += 1;
            assert!(ticks < 100);
        }
        let place = [10.0, 10.0];
        let there = crate::distance(w.units[0].position, place);
        run(&mut w, 0.3);
        // It took on the next from where it was, not heading back first.
        assert_eq!(w.units[0].target, Some(Target::Unit(2)));
        assert!(crate::distance(w.units[0].position, place) >= there);
        run(&mut w, 10.0);
        assert!(!w.units[2].alive());
        assert!(near(w.units[0].position, place) && w.units[0].closing.is_none());
    }
}
