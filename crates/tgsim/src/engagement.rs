//! Organizations taking on enemies: engaging, closing in, pursuing and
//! breaking off.

use tgmap::Zone;

use crate::supply::BEAT;
use crate::{
    CONTACT, Order, PURSUIT_DRIFT, PURSUIT_REACH, Piece, Relation, SECOND, Step, TICK, Target,
    Unreached, World, next_chance, normalised, squared_distance, within, within_strictly,
};

/// How far a unit that found no way to its target has to move for it to
/// look again.
const UNMOVED: f32 = 0.5;

/// Ticks after which a unit that found no way to its target looks again,
/// as the way may have opened.
const UNREACHED_RETRY: u64 = 5 * SECOND;

impl World {
    /// Ends the engagements whose enemies are gone, then engages organizations
    /// with what `fired_on` says fired on them, by organization, where it's in
    /// their operational range,
    /// [`Organization::aggressive`](crate::Organization::aggressive) ones with
    /// enemies that come within their control range and enemy units their
    /// moving units run into, and those on [`Order::Pursue`] with their enemy
    /// once near enough. Organizations on [`Order::Run`], or routing, engage
    /// nothing. On its beat an organization breaks off to chase the enemy
    /// it's pursuing as that slips out of its control zone, and stops once
    /// it's back in it or out of reach.
    pub(crate) fn engage(&mut self, fired_on: &[(usize, Target)]) {
        for r in 0..self.organizations.len() {
            if let Order::Pursue(e) = self.organizations[r].order {
                if self.lost(r, e) {
                    self.give_up(r);
                } else if (self.ticks + r as u64).is_multiple_of(BEAT) {
                    self.keep_chasing(r, e);
                }
            }
            let organization = &self.organizations[r];
            if !matches!(organization.order, Order::Move | Order::Pursue(_))
                && organization.engaged.is_empty()
                && !self.survivors(r).any(|u| self.units[u].moving())
            {
                let before = organization.current_formation();
                self.organizations[r].order = Order::Move;
                if self.organizations[r].current_formation() != before {
                    self.take_places(r);
                }
            }
            let engaged = std::mem::take(&mut self.organizations[r].engaged);
            if engaged.is_empty() {
                continue;
            }
            let left: Vec<Piece> = engaged
                .into_iter()
                .filter(|&e| self.within_range(r, e))
                .collect();
            let done = left.is_empty();
            self.organizations[r].engaged = left;
            if done {
                self.disengage(r);
            }
        }
        let mut meetings: Vec<(usize, Piece)> = Vec::new();
        for &(r, from) in fired_on {
            let reach = self.organizations[r].operational_range;
            if within_strictly(self.centre(r), self.position(from), reach) {
                meetings.push((r, self.piece(from)));
            }
        }
        let mut run_into = Vec::new();
        for (u, unit) in self.units.iter().enumerate() {
            if !unit.alive()
                || !unit.moving()
                || !self.organizations[unit.organization].aggressive()
            {
                continue;
            }
            let met = self.units.iter().enumerate().find(|&(v, other)| {
                v != u
                    && other.alive()
                    && within(unit.position, other.position, CONTACT)
                    && self.hostile(unit.organization, Piece::Organization(other.organization))
            });
            if let Some((v, other)) = met {
                meetings.push((unit.organization, Piece::Organization(other.organization)));
                run_into.push((u, v));
            }
        }
        for (u, v) in run_into {
            self.units[u].met = Some(Target::Unit(v));
        }
        for r in 0..self.organizations.len() {
            if self.organizations[r].aggressive() && !self.destroyed(r) {
                meetings.extend(self.in_control(r).into_iter().map(|e| (r, e)));
            }
        }
        for (r, enemy) in meetings {
            let organization = &self.organizations[r];
            // What's already gone or out of reach would only be dropped
            // again, such as what fired the tick it died.
            if organization.order == Order::Run
                || organization.routing()
                || organization.engaged.contains(&enemy)
                || !self.within_range(r, enemy)
                || self.destroyed(r)
                || !self.can_engage(r, enemy)
            {
                continue;
            }
            let first = organization.engaged.is_empty();
            self.organizations[r].engaged.push(enemy);
            // A fight ends a chase, the attack order going on beneath it.
            self.organizations[r].chased = None;
            if first {
                self.hold(r, enemy);
                self.react(r);
            }
        }
        for r in 0..self.organizations.len() {
            let organization = &self.organizations[r];
            if let Order::Pursue(e) = organization.order
                && organization.engaged.is_empty()
                && !organization.chasing()
            {
                self.pursue(r, e);
            }
        }
    }

    /// Whether the enemy organization `e` has pulled out of the control
    /// zone of the organization at `r` while staying within its
    /// operational range: one to chase.
    fn slipped(&self, r: usize, e: usize) -> bool {
        let zone = self.control_zone(r);
        let in_control = (self.survivors(e)).any(|u| self.in_zone(&zone, Target::Unit(u)));
        !in_control && self.within_range(r, Piece::Organization(e))
    }

    /// Looks at how the organization at `r` stands to the enemy `e` it's
    /// pursuing: breaking off a fight to chase it once it's slipped away,
    /// or ending a chase once it's back in reach or gone too far, else
    /// turning after it.
    fn keep_chasing(&mut self, r: usize, e: usize) {
        let organization = &self.organizations[r];
        let (chasing, fighting) = (organization.chasing(), !organization.engaged.is_empty());
        if !chasing && !fighting {
            return;
        }
        match (chasing, self.slipped(r, e)) {
            (false, true) => self.break_off(r, e),
            (true, true) => _ = self.chase(r, e, false),
            (true, false) => {
                // Back to the attack order's approach, in formation.
                self.organizations[r].chased = None;
                self.chase(r, e, true);
            }
            (false, false) => {}
        }
    }

    /// Has the organization at `r` drop every fight and run after the
    /// enemy `e`, out of formation, each member where it stood from its
    /// leader and none firing.
    fn break_off(&mut self, r: usize, e: usize) {
        self.organizations[r].engaged.clear();
        self.organizations[r].chased = Some(e);
        for u in self.survivors(r).collect::<Vec<_>>() {
            let unit = &mut self.units[u];
            unit.target = None;
            unit.closing = None;
        }
        self.break_formation(r);
        self.chase(r, e, true);
    }

    /// Moves each unit that's chosen to go after its target until it's
    /// within the range it's going for. One whose target is gone stops
    /// where it is to take on the next, and goes back to its place only
    /// once there's nothing left to pick. Its way is planned afresh only
    /// once the target has drifted [`PURSUIT_DRIFT`] off where it was
    /// headed, and one that found no way there tries again only then,
    /// once it's moved or after [`UNREACHED_RETRY`].
    pub(crate) fn close_in(&mut self) {
        for u in 0..self.units.len() {
            let unit = &self.units[u];
            let Some(range) = unit.closing.filter(|_| unit.alive()) else {
                continue;
            };
            let r = unit.organization;
            let Some(t) = unit.target.filter(|&t| self.fighter(t).is_some()) else {
                // Only looked at, so the match's chances are left as they are.
                if self.decide_attack(u, &mut self.rng.clone()).is_some() {
                    self.units[u].halt();
                    continue;
                }
                let place = self.formation_places(r).into_iter().find(|&(v, _)| v == u);
                let facing = self.organizations[r].facing;
                match place {
                    Some((_, place)) => self.take_place(u, place, facing, false),
                    None => self.units[u].closing = None,
                }
                continue;
            };
            let s = Target::Unit(u);
            if self.gap(s, t).is_none_or(|gap| gap <= range) {
                self.units[u].halt();
                continue;
            }
            let goal = self.position(t);
            let near = |at: [f32; 2]| {
                squared_distance(at, goal) <= PURSUIT_DRIFT * squared_distance(unit.position, goal)
            };
            let on_course = unit.path.first().is_some_and(|&end| near(end));
            // One that found no way waits a while for it or its target to
            // move.
            let ticks = self.ticks;
            let stuck = unit.path.is_empty()
                && (unit.unreached).is_some_and(|n| {
                    n.target == t
                        && near(n.at)
                        && within(n.from, unit.position, UNMOVED)
                        && ticks < n.tick + UNREACHED_RETRY
                });
            if on_course || stuck {
                continue;
            }
            let way = self.way(u, goal);
            let unit = &mut self.units[u];
            unit.unreached = way.is_empty().then_some(Unreached {
                target: t,
                at: goal,
                from: unit.position,
                tick: ticks,
            });
            unit.take(way, false);
        }
    }

    /// Whether the enemy organization `e` an organization is pursuing is no
    /// longer one to go after.
    fn lost(&self, r: usize, e: usize) -> bool {
        self.destroyed(e) || !self.hostile(r, Piece::Organization(e))
    }

    /// Calls off an organization's pursuit, leaving it to stand where it is
    /// once done with any engagement.
    pub(crate) fn give_up(&mut self, r: usize) {
        let here = self.standing_point(r);
        let organization = &mut self.organizations[r];
        organization.order = Order::Move;
        organization.resume = None;
        if let Some(guard) = &mut organization.guard {
            guard.step = Step::Unsent;
        }
        if organization.engaged.is_empty() {
            let facing = organization.facing;
            self.send(r, here, facing);
        }
    }

    /// Moves an organization that isn't engaged on after the enemy organization
    /// `e` it's pursuing: engaging it once it's in [`PURSUIT_REACH`], else
    /// turning after it as it gets away, and giving up where it stops short of
    /// it.
    fn pursue(&mut self, r: usize, e: usize) {
        if self.destroyed(r) {
            return;
        }
        let reach = PURSUIT_REACH * self.organizations[r].control(&self.rules.guard_rules);
        let zone = self.control_zone(r);
        let in_control = (self.survivors(e)).any(|u| self.in_zone(&zone, Target::Unit(u)));
        if in_control && within(self.centre(r), self.centre(e), reach) {
            let enemy = Piece::Organization(e);
            self.organizations[r].engaged.push(enemy);
            self.hold(r, enemy);
            self.react(r);
            return;
        }
        if !self.chase(r, e, false) && !self.underway(r) {
            if let Some(guard) = &mut self.organizations[r].guard {
                guard.unreachable = Some(e);
            }
            self.give_up(r);
        }
    }

    /// Sends an organization toward where the enemy organization `e` stands on
    /// average, `afresh` or once it's drifted [`PURSUIT_DRIFT`] off where
    /// the organization's headed. Returns whether it was sent.
    pub(crate) fn chase(&mut self, r: usize, e: usize, afresh: bool) -> bool {
        let (here, there) = (self.centre(r), self.centre(e));
        let off = squared_distance(self.organizations[r].position, there);
        if !afresh && off <= PURSUIT_DRIFT * squared_distance(here, there) {
            return false;
        }
        self.send_along(r, there);
        true
    }

    /// Whether any of the organization at `r`'s members has a weapon for `t`.
    fn fires_on(&self, r: usize, t: Target) -> bool {
        self.survivors(r).any(|u| {
            let f = self.units[u].fighter.iter().flat_map(|f| &f.weapons);
            f.into_iter().any(|w| self.bears_on(u, w, t))
        })
    }

    /// Whether the organization at `r` can engage `enemy`: something of it
    /// is still there that one of its members has a weapon for.
    fn can_engage(&self, r: usize, enemy: Piece) -> bool {
        match enemy {
            Piece::Organization(e) => self.survivors(e).any(|u| self.fires_on(r, Target::Unit(u))),
            Piece::Building(b) => self.fires_on(r, Target::Building(b)),
        }
    }

    /// The enemies with something within an organization's control range that
    /// its members can fire on.
    fn in_control(&self, r: usize) -> Vec<Piece> {
        let zone = self.control_zone(r);
        let fires_on = |t: Target| self.fires_on(r, t);
        let mut found = Vec::new();
        for t in self.targets() {
            let enemy = self.piece(t);
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

    /// The tiles within an organization's control range of where it stands.
    fn control_zone(&self, r: usize) -> Zone {
        let reach = self.organizations[r].control(&self.rules.guard_rules);
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

    /// The player a piece belongs to.
    pub fn piece_owner(&self, enemy: Piece) -> u32 {
        match enemy {
            Piece::Organization(e) => self.organizations[e].owner,
            Piece::Building(b) => self.buildings[b].owner,
        }
    }

    pub(crate) fn hostile(&self, r: usize, enemy: Piece) -> bool {
        let owner = self.piece_owner(enemy);
        self.sides.relation(self.organizations[r].owner, owner) == Relation::Enemy
    }

    /// Whether `enemy` is still one to `r` and in its operational range, or
    /// close enough to be run into where it has none.
    fn within_range(&self, r: usize, enemy: Piece) -> bool {
        let reach = self.organizations[r].operational_range.max(CONTACT);
        let here = self.centre(r);
        let near = match enemy {
            Piece::Organization(e) => self
                .survivors(e)
                .any(|u| within_strictly(here, self.units[u].position, reach)),
            Piece::Building(b) => {
                let building = &self.buildings[b];
                building.standing() && within_strictly(here, building.position, reach)
            }
        };
        near && self.hostile(r, enemy)
    }

    /// Stops an organization that's just engaged where it stands, in its combat
    /// formation facing `enemy`, to carry on where it was going afterwards.
    fn hold(&mut self, r: usize, enemy: Piece) {
        let here = self.standing_point(r);
        let there = self.middle(enemy);
        let organization = &mut self.organizations[r];
        // A pursuit takes up its enemy afresh instead.
        if !matches!(organization.order, Order::Pursue(_)) {
            organization.resume = Some((organization.position, organization.facing));
        }
        let facing = normalised([there[0] - here[0], there[1] - here[1]]);
        let facing = facing.unwrap_or(organization.facing);
        self.send(r, here, facing);
    }

    /// Gives each member of an organization that's just engaged a moment of
    /// its own, up to its
    /// [`engage_reaction`](crate::Rules::engage_reaction), before it
    /// first decides what to do.
    fn react(&mut self, r: usize) {
        let longest = self.rules.engage_reaction / TICK;
        if longest <= 0.0 {
            return;
        }
        for u in self.survivors(r).collect::<Vec<_>>() {
            let pause = (next_chance(&mut self.rng) * longest).round() as u64;
            let unit = &mut self.units[u];
            unit.busy = unit.busy.max(self.ticks + pause);
        }
    }

    /// Sends an organization that's no longer engaged on its way again, in the
    /// formation it was put in, or after the enemy it's pursuing.
    fn disengage(&mut self, r: usize) {
        if let Order::Pursue(e) = self.organizations[r].order {
            match self.slipped(r, e) {
                true => self.break_off(r, e),
                false => _ = self.chase(r, e, true),
            }
            return;
        }
        // A guard picks an intruder or its post afresh.
        if let Some(guard) = &mut self.organizations[r].guard {
            (guard.step, guard.unreachable) = (Step::Unsent, None);
            self.organizations[r].resume = None;
            return;
        }
        match self.organizations[r].resume.take() {
            Some((target, facing)) => self.send(r, target, facing),
            None => self.take_places(r),
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::testing::*;

    use tgmap::{Domain, Fighter, Id, Pathing, Tile};

    use crate::{CONTACT, Command, Order, Piece, Refused, State, Target, Unit, distance};

    #[test]
    fn moving_organizations_stop_to_fight_what_fires_on_them_then_carry_on() {
        let mut w = skirmish();
        w.apply(
            1,
            Command::Move {
                organization: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        // Engaged by the enemy within its control range, which comes on to
        // fire.
        run(&mut w, 4.5);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
        assert_eq!(w.organizations[0].current_formation(), 1);
        assert!(w.units[0].health < 100.0);
        let stopped = w.units[0].position;
        assert!(stopped[1] < 20.0, "{stopped:?}");
        // Orders to move only take effect once it's done.
        w.apply(
            1,
            Command::Move {
                organization: 0,
                to: [30.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 0.5);
        assert!(near(w.units[0].position, stopped));
        assert!(near(w.units[0].facing, [0.0, 1.0]));
        run(&mut w, 2.0);
        assert!(!w.units[1].alive());
        assert!(w.organizations[0].engaged.is_empty());
        assert_eq!(w.organizations[0].current_formation(), 0);
        run(&mut w, 15.0);
        assert!(near(w.units[0].position, [30.0, 20.0]));
    }

    #[test]
    fn aggressive_organizations_engage_enemies_they_run_into() {
        let mut w = skirmish();
        w.organizations[0].formations[0].aggressive = true;
        // Too short a control range to see it coming.
        w.organizations[0].control_range = 1.0;
        // Unarmed, so it doesn't fire on them first.
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.units[0].fighter = Some(soldier(vec![rifle(2.0, 50.0)]));
        w.apply(
            1,
            Command::Move {
                organization: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 9.5);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
        assert!(!w.units[0].moving());
        assert!(distance(w.units[0].position, w.units[1].position) <= CONTACT);
        run(&mut w, 15.0);
        assert!(!w.units[1].alive());
        assert!(near(w.units[0].position, [10.0, 50.0]));
    }

    #[test]
    fn moving_organizations_engage_buildings_that_fire_on_them() {
        let mut pillbox = building(20, 2, [21.0, 24.0]);
        pillbox.fighter.destructible = true;
        pillbox.fighter.weapons = vec![rifle(12.0, 10.0)];
        let mut w = siege(&[pillbox]);
        w.units[0].fighter = Some(soldier(vec![rifle(12.0, 50.0)]));
        w.apply(
            1,
            Command::Move {
                organization: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        // Each in the other's range, edge to edge, from y 12.1, after 1s.
        run(&mut w, 2.0);
        assert_eq!(w.organizations[0].engaged, [Piece::Building(0)]);
        assert!(!w.units[0].moving());
        run(&mut w, 2.0);
        assert!(matches!(w.buildings[0].state, State::Destroyed(_)));
        assert!(w.organizations[0].engaged.is_empty());
        run(&mut w, 20.0);
        assert!(near(w.units[0].position, [10.0, 50.0]));
    }

    #[test]
    fn attacking_organizations_engage_what_comes_within_control_range_then_carry_on() {
        let mut w = skirmish();
        // Unarmed, so it never fires on them to draw them in.
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.apply(
            1,
            Command::Attack {
                organization: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        assert_eq!(w.organizations[0].current_formation(), 1);
        // In the tiles its control range of 18 takes in from 14, after 2s;
        // its rifle reaches from 18.
        run(&mut w, 1.5);
        assert!(w.organizations[0].engaged.is_empty());
        run(&mut w, 1.0);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
        assert!(w.organizations[0].position[1] < 16.0);
        assert!(w.underway(0));
        run(&mut w, 6.0);
        assert!(!w.units[1].alive());
        run(&mut w, 25.0);
        assert!(near(w.organizations[0].position, [10.0, 50.0]));
        assert!(!w.underway(0));
        assert_eq!(w.organizations[0].order, Order::Move);
        assert_eq!(w.organizations[0].current_formation(), 0);
    }

    #[test]
    fn what_dies_as_it_fires_is_not_engaged() {
        let mut w = battle(
            &[1, 2],
            vec![
                soldier(vec![rifle(12.0, 500.0)]),
                soldier(vec![rifle(12.0, 10.0)]),
            ],
            at_war(),
        );
        w.organizations[1].formations[0].aggressive = false;
        w.organizations[0].engaged = vec![Piece::Organization(1)];
        w.organizations[1].engaged = vec![Piece::Organization(0)];
        // Each fires on the other the same tick; one shot kills.
        w.tick();
        assert!(!w.units[1].alive() && w.units[0].health == 90.0);
        assert!(w.organizations[0].engaged.is_empty());
    }

    #[test]
    fn organizations_engage_only_what_they_can_fire_on() {
        let mut w = skirmish();
        w.units[1].position = [10.0, 16.0];
        // Its rifle can't hurt tanks, so neither being fired on by one nor
        // its coming near starts anything.
        w.units[1].fighter.as_mut().unwrap().flags = vec![Id::from("tank")];
        w.organizations[0].formations[0].aggressive = true;
        run(&mut w, 3.0);
        assert!(w.units[0].health < 100.0);
        assert!(w.organizations[0].engaged.is_empty());
    }

    #[test]
    fn organizations_engage_only_what_their_units_can_bear_on() {
        let mut w = battle(
            &[1, 2],
            vec![soldier(vec![rifle(12.0, 10.0)]), soldier(Vec::new())],
            at_war(),
        );
        // Too near for its rifle...
        fn gun(w: &mut crate::World) -> &mut tgmap::Weapon {
            &mut w.units[0].fighter.as_mut().unwrap().weapons[0]
        }
        gun(&mut w).min_range = 12.0;
        run(&mut w, 1.0);
        assert!(w.organizations[0].engaged.is_empty());
        // ...or behind its turret's arc, facing back.
        gun(&mut w).min_range = 0.0;
        gun(&mut w).aimed_by = Some(0);
        w.units[0].fighter.as_mut().unwrap().turrets = vec![tgmap::Turret {
            turn_rate: 1.0,
            sweep: 90f32.to_radians(),
            start: std::f32::consts::PI,
            reset: None,
            offset: [0.0; 2],
        }];
        run(&mut w, 1.0);
        assert!(w.organizations[0].engaged.is_empty());
        w.units[0].fighter.as_mut().unwrap().turrets[0].start = 0.0;
        run(&mut w, 1.0);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
    }

    #[test]
    fn passive_organizations_only_answer_fire() {
        let mut w = skirmish();
        w.units[1].position = [10.0, 26.0];
        // The enemy's within its control range but out of its own reach,
        // and passive too.
        w.units[1].fighter = Some(soldier(vec![rifle(12.0, 10.0)]));
        w.organizations[1].formations[0].aggressive = false;
        w.units[0].fighter = Some(soldier(vec![rifle(12.0, 50.0)]));
        run(&mut w, 3.0);
        assert!(w.organizations[0].engaged.is_empty());
        w.organizations[0].formations[0].aggressive = true;
        w.tick();
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
    }

    #[test]
    fn attacking_organizations_leave_friends_alone() {
        let mut w = skirmish();
        w.organizations[1].owner = 1;
        w.apply(
            1,
            Command::Attack {
                organization: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 6.0);
        assert!(w.organizations[0].engaged.is_empty());
        assert!(w.units[1].alive());
    }

    #[test]
    fn pursuing_organizations_engage_their_enemy_once_within_control_range() {
        let mut w = skirmish();
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.apply(
            1,
            Command::Pursue {
                organization: 0,
                enemy: 1,
            },
        )
        .unwrap();
        assert_eq!(w.organizations[0].order, Order::Pursue(1));
        assert_eq!(w.organizations[0].current_formation(), 1);
        // Away to 38 by 4s; in the tiles its control range of 18 takes in
        // after 6s.
        w.apply(
            2,
            Command::Move {
                organization: 1,
                to: [10.0, 38.0],
            },
        )
        .unwrap();
        run(&mut w, 5.5);
        assert!(w.organizations[0].engaged.is_empty());
        assert!(w.units[0].moving());
        run(&mut w, 1.0);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
        assert!(w.organizations[0].position[1] < 24.0);
        run(&mut w, 6.0);
        assert!(!w.units[1].alive());
        assert_eq!(w.organizations[0].order, Order::Move);
        // It forms up again where it engaged, and goes no further.
        let stopped = w.organizations[0].position;
        run(&mut w, 5.0);
        assert!(near(w.organizations[0].position, stopped));
        assert!(!w.underway(0));
    }

    #[test]
    fn pursuing_organizations_chase_their_enemy_out_of_their_control_zone() {
        let mut w = skirmish();
        let mut tough = soldier(Vec::new());
        tough.health = 1e6;
        w.units[1].fighter = Some(tough);
        w.units[1].health = 1e6;
        let pursue = Command::Pursue {
            organization: 0,
            enemy: 1,
        };
        w.apply(1, pursue).unwrap();
        run(&mut w, 8.0);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
        assert!(!w.organizations[0].chasing());
        // Faster, it pulls back past the control range of 18 but stays
        // within the operational range of 34...
        w.units[1].speed_max *= 3.0;
        let away = Command::Move {
            organization: 1,
            to: [10.0, 80.0],
        };
        w.apply(2, away).unwrap();
        let mut chased = false;
        for _ in 0..(6.0 / crate::TICK) as usize {
            w.tick();
            if w.organizations[0].chasing() {
                chased = true;
                break;
            }
        }
        // ...so the regiment breaks off and runs after it, out of formation
        // and holding fire.
        assert!(chased);
        let gap = distance(w.centre(0), w.centre(1));
        assert!(gap > 18.0 * 0.6 && gap < 34.0, "{gap}");
        assert!(w.organizations[0].engaged.is_empty());
        assert!(w.organizations[0].broken());
        assert_eq!(w.state(0), Some(tgmap::CHASING));
        assert_eq!(w.units[0].target, None);
        assert_eq!(w.organizations[0].order, Order::Pursue(1));
        assert!(w.units[0].moving());
        // Once it's caught up the chase is over and the fight back on.
        run(&mut w, 40.0);
        assert!(!w.organizations[0].chasing());
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
    }

    #[test]
    fn a_chase_ends_where_the_enemy_gets_out_of_reach_and_the_attack_goes_on() {
        let mut w = skirmish();
        let mut tough = soldier(Vec::new());
        tough.health = 1e6;
        w.units[1].fighter = Some(tough);
        w.units[1].health = 1e6;
        w.organizations[0].chased = Some(1);
        // Only a pursuit of the enemy it broke off for is a chase.
        assert!(!w.organizations[0].chasing());
        w.organizations[0].order = Order::Pursue(1);
        assert!(w.organizations[0].chasing());
        w.units[1].position = [10.0, 60.0];
        // On its beat, with the enemy beyond its operational range.
        run(&mut w, 1.0);
        assert!(!w.organizations[0].chasing());
        assert!(!w.organizations[0].broken());
        assert_eq!(w.organizations[0].order, Order::Pursue(1));
        assert!(w.underway(0));
    }

    #[test]
    fn a_chase_ends_where_the_enemy_comes_back_into_the_control_zone() {
        let mut w = skirmish();
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.organizations[0].chased = Some(1);
        w.organizations[0].order = Order::Pursue(1);
        w.units[1].position = [10.0, 20.0];
        run(&mut w, 1.0);
        assert!(!w.organizations[0].chasing());
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
    }

    #[test]
    fn pursuing_organizations_engage_what_they_cant_fire_on_once_near_enough() {
        let mut w = skirmish();
        // Nothing of the pursuer's fires on it, so it never draws it in.
        w.units[1].fighter = Some(Fighter {
            flags: vec![Id::from("tank")],
            ..soldier(Vec::new())
        });
        w.apply(
            1,
            Command::Pursue {
                organization: 0,
                enemy: 1,
            },
        )
        .unwrap();
        // Within 0.6 of its control range of 18, from 19.2, after 4.6s.
        run(&mut w, 4.0);
        assert!(w.organizations[0].engaged.is_empty());
        run(&mut w, 1.0);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
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
                organization: 0,
                enemy: 1,
            },
        )
        .unwrap();
        // Engaged from 19.2, after 4.6s, then closing in alone to 26.
        run(&mut w, 5.5);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
        assert!(w.units[0].moving());
        assert!(w.organizations[0].position[1] < 21.0);
        run(&mut w, 4.0);
        assert!(distance(w.units[0].position, w.units[1].position) <= 4.0);
        assert!(w.units[1].health < 100.0);
        assert!(w.organizations[0].position[1] < 21.0);
    }

    #[test]
    fn a_unit_with_no_way_to_its_target_tries_again_once_either_moves() {
        let mut w = skirmish();
        let open = w.pathing.clone();
        // A wall right across the map between them.
        for x in 0..w.pathing.side as i32 {
            w.pathing.set_blocked(tgmap::Tile::new(x, 10));
        }
        // It's already as near as it can get.
        w.units[0].position = [11.0, 19.0];
        w.units[0].target = Some(Target::Unit(1));
        w.units[0].closing = Some(4.0);
        w.close_in();
        assert!(w.units[0].path.is_empty() && w.units[0].unreached.is_some());
        // With the wall gone it doesn't look again while neither moves, nor
        // for being nudged...
        w.pathing = open.clone();
        w.units[0].position[0] += 0.1;
        w.close_in();
        assert!(w.units[0].path.is_empty());
        // ...but does once it has moved.
        w.units[0].position[0] += 1.0;
        w.close_in();
        assert!(!w.units[0].path.is_empty() && w.units[0].unreached.is_none());
    }

    #[test]
    fn a_unit_with_no_way_to_its_target_tries_again_after_a_while() {
        let mut w = skirmish();
        let open = w.pathing.clone();
        for x in 0..w.pathing.side as i32 {
            w.pathing.set_blocked(tgmap::Tile::new(x, 10));
        }
        w.units[0].position = [11.0, 19.0];
        w.units[0].target = Some(Target::Unit(1));
        w.units[0].closing = Some(4.0);
        w.close_in();
        w.pathing = open;
        w.ticks += super::UNREACHED_RETRY - 1;
        w.close_in();
        assert!(w.units[0].path.is_empty());
        w.ticks += 1;
        w.close_in();
        assert!(!w.units[0].path.is_empty());
    }

    #[test]
    fn pursuing_organizations_fight_what_fires_on_them_then_carry_on() {
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
                organization: 0,
                enemy: 2,
            },
        )
        .unwrap();
        // Engaged by what fired on it, it goes after its own enemy first,
        // not yet among what it's engaged with, so picked afresh...
        run(&mut w, 1.0);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
        assert_eq!(w.units[0].target, Some(Target::Unit(2)));
        assert!(w.units[0].moving());
        // ...until near enough to engage it too, when the next hit draws it
        // onto what fires on it...
        run(&mut w, 12.0);
        assert!(!w.units[1].alive() && w.units[2].alive());
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(2)]);
        assert_eq!(w.organizations[0].order, Order::Pursue(2));
        // ...and then back to its own.
        run(&mut w, 5.0);
        assert!(!w.units[2].alive());
    }

    #[test]
    fn pursuing_organizations_fire_on_their_enemy_first() {
        let mut w = battle(
            &[1, 2, 2],
            vec![
                soldier(vec![rifle(12.0, 50.0)]),
                soldier(vec![rifle(4.0, 10.0)]),
                soldier(Vec::new()),
            ],
            at_war(),
        );
        // Armed and nearer, but not the one pursued, and short of reaching
        // it, and passive, so no hit of its own draws it away.
        w.units[1].position = [16.0, 10.0];
        w.organizations[1].formations[0].aggressive = false;
        w.units[2].position = [10.0, 20.0];
        w.apply(
            1,
            Command::Pursue {
                organization: 0,
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
    fn pursuing_organizations_give_up_on_what_they_cant_reach() {
        let mut w = skirmish();
        w.units[1].fighter = Some(soldier(Vec::new()));
        for x in 0..40 {
            w.pathing.set_blocked(Tile::new(x, 10));
        }
        w.apply(
            1,
            Command::Pursue {
                organization: 0,
                enemy: 1,
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(w.organizations[0].engaged.is_empty());
        assert_eq!(w.organizations[0].order, Order::Move);
        assert!(!w.underway(0));
        assert!(w.units[0].position[1] < 20.0);
    }

    #[test]
    fn organizations_only_pursue_enemies_and_only_when_armed() {
        let mut w = skirmish();
        w.organizations[1].owner = 1;
        assert_eq!(w.order_pursue(1, 0, 1), Err(Refused::Unable));
        let mut w = skirmish();
        w.units[0].fighter = Some(soldier(Vec::new()));
        assert_eq!(w.order_pursue(1, 0, 1), Err(Refused::Unable));
        assert_eq!(w.order_pursue(2, 0, 1), Err(Refused::NotYours));
    }

    #[test]
    fn running_organizations_break_off_and_ignore_enemies() {
        let mut w = skirmish();
        // Too slow to keep after it.
        for u in w.organizations[1].units.clone() {
            w.units[u].speed_max = 0.5;
        }
        w.apply(
            1,
            Command::Move {
                organization: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 4.5);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
        w.apply(
            1,
            Command::Run {
                organization: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        assert_eq!(w.organizations[0].order, Order::Run);
        for _ in 0..200 {
            w.tick();
            assert!(w.organizations[0].engaged.is_empty());
        }
        assert!(w.units[0].alive() && w.units[1].alive());
        assert!(near(w.units[0].position, [10.0, 50.0]));
        assert_eq!(w.organizations[0].order, Order::Move);
    }

    #[test]
    fn stopped_organizations_form_up_where_they_are() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.apply(
            7,
            Command::Move {
                organization: 0,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        w.apply(7, Command::Stop { organization: 0 }).unwrap();
        run(&mut w, 5.0);
        assert!(near(w.organizations[0].position, [10.0, 12.0]));
        assert!(near(w.units[0].position, [9.0, 12.0]));
        assert!(near(w.units[1].position, [11.0, 12.0]));
        assert!(!w.units.iter().any(Unit::moving));
        assert!(w.order_stop(1, 0).is_err());
    }

    #[test]
    fn organizations_are_underway_until_they_arrive() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        assert!(!w.underway(0));
        w.apply(
            7,
            Command::Move {
                organization: 0,
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
                organization: 0,
                to: [10.0, 5.0],
            },
        )
        .unwrap();
        run(&mut w, 0.5);
        w.apply(7, Command::Stop { organization: 0 }).unwrap();
        assert!(!near(w.destination(0), [10.0, 5.0]));
    }

    #[test]
    fn fighting_on_the_way_keeps_the_destination() {
        let mut w = skirmish();
        w.apply(
            1,
            Command::Move {
                organization: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 4.5);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
        assert!(w.underway(0));
        assert!(near(w.destination(0), [10.0, 50.0]));
    }

    #[test]
    fn stopped_organizations_stay_put_after_fighting() {
        let mut w = skirmish();
        w.apply(
            1,
            Command::Move {
                organization: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 4.5);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
        let held = w.organizations[0].position;
        w.apply(1, Command::Stop { organization: 0 }).unwrap();
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
        w.destroy(1);
        run(&mut w, 10.0);
        assert!(w.organizations[0].engaged.is_empty());
        assert!(near(w.organizations[0].position, held));
    }
}
