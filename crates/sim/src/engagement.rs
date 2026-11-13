//! Regiments taking on enemies: engaging, closing in, pursuing and
//! breaking off.

use aaa_map::Zone;

use crate::{
    CONTACT, Enemy, Order, PURSUIT_DRIFT, PURSUIT_REACH, Relation, Step, Target, World, normalised,
    squared_distance, within, within_strictly,
};

impl World {
    /// Ends the engagements whose enemies are gone, then engages regiments
    /// with what `fired_on` says fired on them, by regiment, where it's in
    /// their operational range, [`Regiment::aggressive`](crate::Regiment::aggressive) ones with enemies
    /// that come within their control range and enemy units their moving
    /// units run into, and those on [`Order::Pursue`] with their enemy
    /// once near enough. Regiments on [`Order::Run`] engage nothing.
    pub(crate) fn engage(&mut self, fired_on: &[(usize, Target)]) {
        for r in 0..self.regiments.len() {
            if let Order::Pursue(e) = self.regiments[r].order
                && self.lost(r, e)
            {
                self.give_up(r);
            }
            let regiment = &self.regiments[r];
            if !matches!(regiment.order, Order::Move | Order::Pursue(_))
                && regiment.engaged.is_empty()
                && !self.survivors(r).any(|u| self.units[u].moving())
            {
                let before = regiment.current_formation();
                self.regiments[r].order = Order::Move;
                if self.regiments[r].current_formation() != before {
                    self.take_places(r);
                }
            }
            let engaged = std::mem::take(&mut self.regiments[r].engaged);
            if engaged.is_empty() {
                continue;
            }
            let left: Vec<Enemy> = engaged
                .into_iter()
                .filter(|&e| self.within_range(r, e))
                .collect();
            let done = left.is_empty();
            self.regiments[r].engaged = left;
            if done {
                self.disengage(r);
            }
        }
        let mut meetings: Vec<(usize, Enemy)> = Vec::new();
        for &(r, from) in fired_on {
            let reach = self.regiments[r].operational_range;
            if within_strictly(self.centre(r), self.position(from), reach) {
                meetings.push((r, self.enemy(from)));
            }
        }
        for (u, unit) in self.units.iter().enumerate() {
            if !unit.alive() || !unit.moving() || !self.regiments[unit.regiment].aggressive() {
                continue;
            }
            let met = self.units.iter().enumerate().find(|&(v, other)| {
                v != u
                    && other.alive()
                    && within(unit.position, other.position, CONTACT)
                    && self.hostile(unit.regiment, Enemy::Regiment(other.regiment))
            });
            if let Some((_, other)) = met {
                meetings.push((unit.regiment, Enemy::Regiment(other.regiment)));
            }
        }
        for r in 0..self.regiments.len() {
            if self.regiments[r].aggressive() && !self.destroyed(r) {
                meetings.extend(self.in_control(r).into_iter().map(|e| (r, e)));
            }
        }
        for (r, enemy) in meetings {
            let regiment = &self.regiments[r];
            // What's already gone or out of reach would only be dropped
            // again, such as what fired the tick it died.
            if regiment.order == Order::Run
                || regiment.engaged.contains(&enemy)
                || !self.within_range(r, enemy)
                || self.destroyed(r)
            {
                continue;
            }
            let first = regiment.engaged.is_empty();
            self.regiments[r].engaged.push(enemy);
            if first {
                self.hold(r, enemy);
            }
        }
        for r in 0..self.regiments.len() {
            if let Order::Pursue(e) = self.regiments[r].order
                && self.regiments[r].engaged.is_empty()
            {
                self.pursue(r, e);
            }
        }
    }

    /// Sends each armed member of an engaged regiment that can fire on
    /// nothing after the enemy it would fire on first, and stops those
    /// that have come within reach to fire.
    pub(crate) fn close_in(&mut self) {
        for u in 0..self.units.len() {
            let unit = &self.units[u];
            let r = unit.regiment;
            if !unit.alive() || self.regiments[r].engaged.is_empty() {
                continue;
            }
            let Some(fighter) = unit.fighter.as_ref() else {
                continue;
            };
            let s = Target::Unit(u);
            let weapons = &fighter.weapons;
            if weapons
                .iter()
                .any(|w| self.targets().any(|t| self.can_hit(s, w, t)))
            {
                if unit.closing {
                    let unit = &mut self.units[u];
                    unit.halt();
                    unit.closing = false;
                }
                continue;
            }
            // Those still taking their places carry on.
            if unit.moving() && !unit.closing {
                continue;
            }
            let Some((goal, range)) = self.quarry(u) else {
                continue;
            };
            if unit.closing
                && unit
                    .path
                    .first()
                    .is_some_and(|&end| self.reach(end, goal) <= range)
            {
                continue;
            }
            let way = self.way(u, self.position(goal));
            let unit = &mut self.units[u];
            unit.take(way, false);
            unit.closing = true;
        }
    }

    /// What a member of an engaged regiment goes after when it can fire on
    /// nothing, with the reach of its longest weapon that fires on it: the
    /// nearest of the enemy its regiment pursues, where that's within its
    /// operational range, else the nearest of what it's engaged with.
    fn quarry(&self, u: usize) -> Option<(Target, f32)> {
        let r = self.units[u].regiment;
        let regiment = &self.regiments[r];
        let s = Target::Unit(u);
        let from = self.units[u].position;
        let weapons = &self.units[u].fighter.as_ref()?.weapons;
        let range = |t: Target| {
            let ranges = weapons.iter().filter(|w| self.can_target(s, w, t));
            ranges.map(|w| w.range).max_by(f32::total_cmp)
        };
        let nearest = |ts: Vec<Target>| {
            ts.into_iter()
                .filter_map(|t| Some((t, range(t)?)))
                .min_by(|a, b| self.reach(from, a.0).total_cmp(&self.reach(from, b.0)))
        };
        let units = |e: usize| self.survivors(e).map(Target::Unit).collect::<Vec<_>>();
        if let Order::Pursue(e) = regiment.order {
            let here = self.centre(r);
            let pursued = units(e);
            let near = pursued
                .iter()
                .any(|&t| within_strictly(here, self.position(t), regiment.operational_range));
            if let Some(found) = nearest(pursued).filter(|_| near) {
                return Some(found);
            }
        }
        let engaged = regiment.engaged.iter().flat_map(|&enemy| match enemy {
            Enemy::Regiment(e) => units(e),
            Enemy::Building(b) => vec![Target::Building(b)],
        });
        nearest(engaged.collect())
    }

    /// Whether the enemy regiment `e` a regiment is pursuing is no longer
    /// one to go after.
    fn lost(&self, r: usize, e: usize) -> bool {
        self.destroyed(e) || !self.hostile(r, Enemy::Regiment(e))
    }

    /// Calls off a regiment's pursuit, leaving it to stand where it is
    /// once done with any engagement.
    pub(crate) fn give_up(&mut self, r: usize) {
        let here = self.centre(r);
        let regiment = &mut self.regiments[r];
        regiment.order = Order::Move;
        regiment.resume = None;
        if let Some(guard) = &mut regiment.guard {
            guard.step = Step::Unsent;
        }
        if regiment.engaged.is_empty() {
            let facing = regiment.facing;
            self.send(r, here, facing);
        }
    }

    /// Moves a regiment that isn't engaged on after the enemy regiment `e`
    /// it's pursuing: engaging it once it's in [`PURSUIT_REACH`], else
    /// turning after it as it gets away, and giving up where it stops
    /// short of it.
    fn pursue(&mut self, r: usize, e: usize) {
        if self.destroyed(r) {
            return;
        }
        let reach = PURSUIT_REACH * self.regiments[r].control(&self.rules.guard_rules);
        let zone = self.control_zone(r);
        let in_control = (self.survivors(e)).any(|u| self.in_zone(&zone, Target::Unit(u)));
        if in_control && within(self.centre(r), self.centre(e), reach) {
            let enemy = Enemy::Regiment(e);
            self.regiments[r].engaged.push(enemy);
            self.hold(r, enemy);
            return;
        }
        if !self.chase(r, e, false) && !self.underway(r) {
            if let Some(guard) = &mut self.regiments[r].guard {
                guard.unreachable = Some(e);
            }
            self.give_up(r);
        }
    }

    /// Sends a regiment toward where the enemy regiment `e` stands on
    /// average, `afresh` or once it's drifted [`PURSUIT_DRIFT`] off where
    /// the regiment's headed. Returns whether it was sent.
    pub(crate) fn chase(&mut self, r: usize, e: usize, afresh: bool) -> bool {
        let (here, there) = (self.centre(r), self.centre(e));
        let off = squared_distance(self.regiments[r].position, there);
        if !afresh && off <= PURSUIT_DRIFT * squared_distance(here, there) {
            return false;
        }
        let facing = normalised([there[0] - here[0], there[1] - here[1]]);
        let facing = facing.unwrap_or(self.regiments[r].facing);
        self.send(r, there, facing);
        true
    }

    /// The enemies with something within a regiment's control range that
    /// its members can fire on.
    fn in_control(&self, r: usize) -> Vec<Enemy> {
        let zone = self.control_zone(r);
        let fires_on = |t: Target| {
            self.survivors(r).any(|u| {
                let f = self.units[u].fighter.iter().flat_map(|f| &f.weapons);
                f.into_iter()
                    .any(|w| self.can_target(Target::Unit(u), w, t))
            })
        };
        let mut found = Vec::new();
        for t in self.targets() {
            let enemy = self.enemy(t);
            if !found.contains(&enemy)
                && self.in_zone(&zone, t)
                && self.hostile(r, enemy)
                && fires_on(t)
            {
                found.push(enemy);
            }
        }
        found
    }

    /// The tiles within a regiment's control range of where it stands.
    fn control_zone(&self, r: usize) -> Zone {
        let reach = self.regiments[r].control(&self.rules.guard_rules);
        self.pathing.zone(self.centre(r), reach)
    }

    /// Whether `t` is in `zone`: by the tile under it, or for what blocks
    /// tiles any of those under it.
    pub(crate) fn in_zone(&self, zone: &Zone, t: Target) -> bool {
        let blocking = match t {
            Target::Building(b) => self.buildings[b].obstacle.as_ref(),
            Target::Unit(_) => None,
        };
        match blocking {
            Some(o) => zone.meets(o),
            None => zone.holds(self.position(t)),
        }
    }

    /// The owner of what a regiment or building engages.
    pub(crate) fn enemy_owner(&self, enemy: Enemy) -> u32 {
        match enemy {
            Enemy::Regiment(e) => self.regiments[e].owner,
            Enemy::Building(b) => self.buildings[b].owner,
        }
    }

    pub(crate) fn hostile(&self, r: usize, enemy: Enemy) -> bool {
        let owner = self.enemy_owner(enemy);
        self.sides.relation(self.regiments[r].owner, owner) == Relation::Enemy
    }

    /// Whether `enemy` is still one to `r` and in its operational range, or
    /// close enough to be run into where it has none.
    fn within_range(&self, r: usize, enemy: Enemy) -> bool {
        let reach = self.regiments[r].operational_range.max(CONTACT);
        let here = self.centre(r);
        let near = match enemy {
            Enemy::Regiment(e) => self
                .survivors(e)
                .any(|u| within_strictly(here, self.units[u].position, reach)),
            Enemy::Building(b) => {
                let building = &self.buildings[b];
                building.standing() && within_strictly(here, building.position, reach)
            }
        };
        near && self.hostile(r, enemy)
    }

    /// Stops a regiment that's just engaged where it stands, in its combat
    /// formation facing `enemy`, to carry on where it was going afterwards.
    fn hold(&mut self, r: usize, enemy: Enemy) {
        let here = self.centre(r);
        let there = self.middle(enemy);
        let regiment = &mut self.regiments[r];
        // A pursuit takes up its enemy afresh instead.
        if !matches!(regiment.order, Order::Pursue(_)) {
            regiment.resume = Some((regiment.position, regiment.facing));
        }
        let facing = normalised([there[0] - here[0], there[1] - here[1]]);
        let facing = facing.unwrap_or(regiment.facing);
        self.send(r, here, facing);
    }

    /// Sends a regiment that's no longer engaged on its way again, in the
    /// formation it was put in, or after the enemy it's pursuing.
    fn disengage(&mut self, r: usize) {
        if let Order::Pursue(e) = self.regiments[r].order {
            self.chase(r, e, true);
            return;
        }
        // A guard picks an intruder or its post afresh.
        if let Some(guard) = &mut self.regiments[r].guard {
            (guard.step, guard.unreachable) = (Step::Unsent, None);
            self.regiments[r].resume = None;
            return;
        }
        match self.regiments[r].resume.take() {
            Some((target, facing)) => self.send(r, target, facing),
            None => self.take_places(r),
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::testing::*;

    use aaa_map::{Domain, Fighter, Id, Pathing, Tile};

    use crate::{CONTACT, Command, Enemy, Order, Refused, State, Target, Unit, distance};

    #[test]
    fn moving_regiments_stop_to_fight_what_fires_on_them_then_carry_on() {
        let mut w = skirmish();
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        // In the enemy's range after 4s, at 18.
        run(&mut w, 4.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert_eq!(w.regiments[0].current_formation(), 1);
        assert!(w.units[0].health < 100.0);
        let stopped = w.units[0].position;
        assert!(stopped[1] < 20.0, "{stopped:?}");
        // Orders to move only take effect once it's done.
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [30.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        assert!(near(w.units[0].position, stopped));
        assert!(near(w.units[0].facing, [0.0, 1.0]));
        run(&mut w, 2.0);
        assert!(!w.units[1].alive());
        assert!(w.regiments[0].engaged.is_empty());
        assert_eq!(w.regiments[0].current_formation(), 0);
        run(&mut w, 15.0);
        assert!(near(w.units[0].position, [30.0, 20.0]));
    }

    #[test]
    fn moving_regiments_carry_on_past_what_dies_as_it_fires_on_them() {
        // A gun that fires on the move and kills outright.
        let mut gun = rifle(12.0, 500.0);
        gun.turret = true;
        let mut w = battle(
            &[1, 2],
            vec![soldier(vec![gun]), soldier(vec![rifle(12.0, 10.0)])],
            at_war(),
        );
        w.units[1].position = [10.0, 30.0];
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 60.0],
            },
        )
        .unwrap();
        // Each fires as the first comes into range, 4s on.
        let mut before = w.units[0].position;
        for _ in 0..60 {
            w.tick();
            let u = &w.units[0];
            assert!(w.regiments[0].engaged.is_empty());
            assert!(near(u.facing, [0.0, 1.0]) && u.position[1] > before[1]);
            before = u.position;
        }
        assert!(!w.units[1].alive() && w.units[0].health < 100.0);
    }

    #[test]
    fn aggressive_regiments_engage_enemies_they_run_into() {
        let mut w = skirmish();
        w.regiments[0].formations[0].aggressive = true;
        // Too short a control range to see it coming.
        w.regiments[0].control_range = 1.0;
        // Unarmed, so it doesn't fire on them first.
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.units[0].fighter = Some(soldier(vec![rifle(2.0, 50.0)]));
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 9.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(!w.units[0].moving());
        assert!(distance(w.units[0].position, w.units[1].position) <= CONTACT);
        run(&mut w, 15.0);
        assert!(!w.units[1].alive());
        assert!(near(w.units[0].position, [10.0, 50.0]));
    }

    #[test]
    fn moving_regiments_engage_buildings_that_fire_on_them() {
        let mut pillbox = building(20, 2, [21.0, 24.0]);
        pillbox.fighter.destructible = true;
        pillbox.fighter.weapons = vec![rifle(12.0, 10.0)];
        let mut w = siege(&[pillbox]);
        w.units[0].fighter = Some(soldier(vec![rifle(12.0, 50.0)]));
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        // Each in the other's range, edge to edge, from y 12.1, after 1s.
        run(&mut w, 2.0);
        assert_eq!(w.regiments[0].engaged, [Enemy::Building(0)]);
        assert!(!w.units[0].moving());
        run(&mut w, 2.0);
        assert!(matches!(w.buildings[0].state, State::Destroyed(_)));
        assert!(w.regiments[0].engaged.is_empty());
        run(&mut w, 20.0);
        assert!(near(w.units[0].position, [10.0, 50.0]));
    }

    #[test]
    fn attacking_regiments_engage_what_comes_within_control_range_then_carry_on() {
        let mut w = skirmish();
        // Unarmed, so it never fires on them to draw them in.
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.apply(
            1,
            Command::Attack {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        assert_eq!(w.regiments[0].current_formation(), 1);
        // In the tiles its control range of 18 takes in from 14, after 2s;
        // its rifle reaches from 18.
        run(&mut w, 1.5);
        assert!(w.regiments[0].engaged.is_empty());
        run(&mut w, 1.0);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(w.regiments[0].position[1] < 16.0);
        assert!(w.underway(0));
        run(&mut w, 6.0);
        assert!(!w.units[1].alive());
        run(&mut w, 25.0);
        assert!(near(w.regiments[0].position, [10.0, 50.0]));
        assert!(!w.underway(0));
        assert_eq!(w.regiments[0].order, Order::Move);
        assert_eq!(w.regiments[0].current_formation(), 0);
    }

    #[test]
    fn passive_regiments_only_answer_fire() {
        let mut w = skirmish();
        w.units[1].position = [10.0, 26.0];
        // The enemy's within its control range but out of its own reach.
        w.units[1].fighter = Some(soldier(vec![rifle(12.0, 10.0)]));
        w.units[0].fighter = Some(soldier(vec![rifle(12.0, 50.0)]));
        run(&mut w, 3.0);
        assert!(w.regiments[0].engaged.is_empty());
        w.regiments[0].formations[0].aggressive = true;
        w.tick();
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
    }

    #[test]
    fn attacking_regiments_leave_friends_alone() {
        let mut w = skirmish();
        w.regiments[1].owner = 1;
        w.apply(
            1,
            Command::Attack {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 6.0);
        assert!(w.regiments[0].engaged.is_empty());
        assert!(w.units[1].alive());
    }

    #[test]
    fn pursuing_regiments_engage_their_enemy_once_within_control_range() {
        let mut w = skirmish();
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.apply(
            1,
            Command::Pursue {
                regiment: 0,
                enemy: 1,
            },
        )
        .unwrap();
        assert_eq!(w.regiments[0].order, Order::Pursue(1));
        assert_eq!(w.regiments[0].current_formation(), 1);
        // Away to 38 by 4s; in the tiles its control range of 18 takes in
        // after 6s.
        w.apply(
            2,
            Command::Move {
                regiment: 1,
                to: [10.0, 38.0],
            },
        )
        .unwrap();
        run(&mut w, 5.5);
        assert!(w.regiments[0].engaged.is_empty());
        assert!(w.units[0].moving());
        run(&mut w, 1.0);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(w.regiments[0].position[1] < 24.0);
        run(&mut w, 6.0);
        assert!(!w.units[1].alive());
        assert_eq!(w.regiments[0].order, Order::Move);
        // It forms up again where it engaged, and goes no further.
        let stopped = w.regiments[0].position;
        run(&mut w, 5.0);
        assert!(near(w.regiments[0].position, stopped));
        assert!(!w.underway(0));
    }

    #[test]
    fn pursuing_regiments_engage_what_they_cant_fire_on_once_near_enough() {
        let mut w = skirmish();
        // Nothing of the pursuer's fires on it, so it never draws it in.
        w.units[1].fighter = Some(Fighter {
            flags: vec![Id::from("tank")],
            ..soldier(Vec::new())
        });
        w.apply(
            1,
            Command::Pursue {
                regiment: 0,
                enemy: 1,
            },
        )
        .unwrap();
        // Within 0.6 of its control range of 18, from 19.2, after 4.6s.
        run(&mut w, 4.0);
        assert!(w.regiments[0].engaged.is_empty());
        run(&mut w, 1.0);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!((18.5..20.5).contains(&w.units[0].position[1]));
    }

    #[test]
    fn engaged_members_close_in_on_what_they_cant_reach() {
        let mut w = skirmish();
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.units[0].fighter = Some(soldier(vec![rifle(4.0, 50.0)]));
        w.apply(
            1,
            Command::Pursue {
                regiment: 0,
                enemy: 1,
            },
        )
        .unwrap();
        // Engaged from 19.2, after 4.6s, then closing in alone to 26.
        run(&mut w, 5.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(w.units[0].moving());
        assert!(w.regiments[0].position[1] < 21.0);
        run(&mut w, 4.0);
        assert!(distance(w.units[0].position, w.units[1].position) <= 4.0);
        assert!(w.units[1].health < 100.0);
        assert!(w.regiments[0].position[1] < 21.0);
    }

    #[test]
    fn pursuing_regiments_fight_what_fires_on_them_then_carry_on() {
        let mut w = battle(
            &[1, 2, 2],
            vec![
                soldier(vec![rifle(12.0, 50.0)]),
                soldier(vec![rifle(12.0, 10.0)]),
                soldier(Vec::new()),
            ],
            at_war(),
        );
        w.units[1].position = [20.0, 10.0];
        w.units[2].position = [10.0, 38.0];
        w.apply(
            1,
            Command::Pursue {
                regiment: 0,
                enemy: 2,
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(!w.units[0].moving());
        run(&mut w, 3.0);
        assert!(!w.units[1].alive());
        assert!(w.regiments[0].engaged.is_empty());
        assert!(w.units[0].moving());
        assert_eq!(w.regiments[0].order, Order::Pursue(2));
        run(&mut w, 15.0);
        assert!(!w.units[2].alive());
    }

    #[test]
    fn pursuing_regiments_fire_on_their_enemy_first() {
        let mut w = battle(
            &[1, 2, 2],
            vec![
                soldier(vec![rifle(12.0, 50.0)]),
                soldier(vec![rifle(12.0, 10.0)]),
                soldier(Vec::new()),
            ],
            at_war(),
        );
        // Armed and nearer, but not the one pursued.
        w.units[1].position = [16.0, 10.0];
        w.units[2].position = [10.0, 20.0];
        w.apply(
            1,
            Command::Pursue {
                regiment: 0,
                enemy: 2,
            },
        )
        .unwrap();
        run(&mut w, 0.5);
        assert_eq!(w.units[0].target, Some(Target::Unit(2)));
        run(&mut w, 2.0);
        assert!(!w.units[2].alive());
        assert_eq!(w.units[1].health, 100.0);
        run(&mut w, 4.0);
        assert!(!w.units[1].alive());
    }

    #[test]
    fn pursuing_regiments_give_up_on_what_they_cant_reach() {
        let mut w = skirmish();
        w.units[1].fighter = Some(soldier(Vec::new()));
        for x in 0..40 {
            w.pathing.set_blocked(Tile::new(x, 10));
        }
        w.apply(
            1,
            Command::Pursue {
                regiment: 0,
                enemy: 1,
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(w.regiments[0].engaged.is_empty());
        assert_eq!(w.regiments[0].order, Order::Move);
        assert!(!w.underway(0));
        assert!(w.units[0].position[1] < 20.0);
    }

    #[test]
    fn regiments_only_pursue_enemies_and_only_when_armed() {
        let mut w = skirmish();
        w.regiments[1].owner = 1;
        assert_eq!(w.order_pursue(1, 0, 1), Err(Refused::Unable));
        let mut w = skirmish();
        w.units[0].fighter = Some(soldier(Vec::new()));
        assert_eq!(w.order_pursue(1, 0, 1), Err(Refused::Unable));
        assert_eq!(w.order_pursue(2, 0, 1), Err(Refused::NotYours));
    }

    #[test]
    fn running_regiments_break_off_and_ignore_enemies() {
        let mut w = skirmish();
        // Too slow to keep after it.
        w.regiments[1].speed = 0.5;
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 4.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        w.apply(
            1,
            Command::Run {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        assert_eq!(w.regiments[0].order, Order::Run);
        for _ in 0..200 {
            w.tick();
            assert!(w.regiments[0].engaged.is_empty());
        }
        assert!(w.units[0].alive() && w.units[1].alive());
        assert!(near(w.units[0].position, [10.0, 50.0]));
        assert_eq!(w.regiments[0].order, Order::Move);
    }

    #[test]
    fn stopped_regiments_form_up_where_they_are() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        w.apply(7, Command::Stop { regiment: 0 }).unwrap();
        run(&mut w, 5.0);
        assert!(near(w.regiments[0].position, [10.0, 12.0]));
        assert!(near(w.units[0].position, [9.0, 12.0]));
        assert!(near(w.units[1].position, [11.0, 12.0]));
        assert!(!w.units.iter().any(Unit::moving));
        assert!(w.order_stop(1, 0).is_err());
    }

    #[test]
    fn regiments_are_underway_until_they_arrive() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        assert!(!w.underway(0));
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        assert!(w.underway(0));
        assert!(near(w.destination(0), [10.0, 20.0]));
        run(&mut w, 30.0);
        assert!(!w.underway(0));
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 5.0],
            },
        )
        .unwrap();
        run(&mut w, 0.5);
        w.apply(7, Command::Stop { regiment: 0 }).unwrap();
        assert!(!near(w.destination(0), [10.0, 5.0]));
    }

    #[test]
    fn fighting_on_the_way_keeps_the_destination() {
        let mut w = skirmish();
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 4.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(w.underway(0));
        assert!(near(w.destination(0), [10.0, 50.0]));
    }

    #[test]
    fn stopped_regiments_stay_put_after_fighting() {
        let mut w = skirmish();
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 4.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        let held = w.regiments[0].position;
        w.apply(1, Command::Stop { regiment: 0 }).unwrap();
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        w.destroy(1);
        run(&mut w, 10.0);
        assert!(w.regiments[0].engaged.is_empty());
        assert!(near(w.regiments[0].position, held));
    }
}
