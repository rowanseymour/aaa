//! Regiments keeping a post, at a place or beside what they guard.

use aaa_map::pathing::CELL_SIZE;

use crate::{
    Enemy, Guard, Order, Regiment, Step, World, bounding_radius, distance, normalised, within,
};

/// How far what a regiment guards moves its post before the regiment
/// follows.
const POST_DRIFT: f32 = 1.0;

impl World {
    /// Has a regiment guard `of` from `post`, setting off there unless
    /// it's engaged.
    pub(crate) fn post(
        &mut self,
        regiment: usize,
        of: Option<Enemy>,
        post: [f32; 2],
        facing: Option<[f32; 2]>,
    ) {
        let r = &mut self.regiments[regiment];
        let facing = facing.unwrap_or(r.facing);
        r.order = Order::Move;
        r.unpack_at = None;
        r.unpack_again = None;
        r.rally = None;
        r.guard = Some(Guard {
            of,
            post,
            facing,
            step: Step::Unsent,
            unreachable: None,
        });
        match r.engaged.is_empty() {
            true => {
                r.resume = None;
                self.go_to_post(regiment);
            }
            // Where it's headed once done, as for a move.
            false => r.resume = Some((post, facing)),
        }
    }

    /// The post of a regiment guarding `thing`, and the way it faces there:
    /// off the thing's edge by half the regiment's control range, on the
    /// side the regiment is, facing away.
    pub(crate) fn beside(&self, regiment: usize, thing: Enemy) -> ([f32; 2], [f32; 2]) {
        let r = &self.regiments[regiment];
        let here = self.centre(regiment);
        let there = self.middle(thing);
        let away = normalised([here[0] - there[0], here[1] - there[1]]).unwrap_or(r.facing);
        (self.post_beside(regiment, thing, away), away)
    }

    /// The point `away` from `thing` that a regiment guarding it rests at,
    /// kept on the map.
    fn post_beside(&self, regiment: usize, thing: Enemy, away: [f32; 2]) -> [f32; 2] {
        let radius = match thing {
            Enemy::Regiment(_) => 0.0,
            Enemy::Building(b) => {
                let shape = self.buildings[b].obstacle.as_ref().map(|o| o.shape);
                shape.map_or(0.0, bounding_radius)
            }
        };
        let off = radius + self.regiments[regiment].control_range / 2.0;
        let [x, y] = self.middle(thing);
        let size = self.pathing.side as f32 * CELL_SIZE;
        [
            (x + away[0] * off).clamp(0.0, size),
            (y + away[1] * off).clamp(0.0, size),
        ]
    }

    /// Where something a regiment can engage or guard is.
    pub(crate) fn middle(&self, thing: Enemy) -> [f32; 2] {
        match thing {
            Enemy::Regiment(e) => self.centre(e),
            Enemy::Building(b) => self.buildings[b].position,
        }
    }

    /// Has each regiment keeping a guard that isn't engaged go after the
    /// nearest intruder, or else to its post, which follows what it
    /// guards; the guard ends once that's gone.
    pub(crate) fn keep_guard(&mut self) {
        // Who's engaged with what, looked up once for all that guard things.
        let escorts = |r: &Regiment| r.guard.is_some_and(|g| g.of.is_some());
        let engaging: Vec<(usize, Enemy)> = match self.regiments.iter().any(escorts) {
            true => (self.regiments.iter().enumerate())
                .flat_map(|(e, r)| r.engaged.iter().map(move |&with| (e, with)))
                .collect(),
            false => Vec::new(),
        };
        for r in 0..self.regiments.len() {
            let Some(guard) = self.regiments[r].guard else {
                continue;
            };
            if self.destroyed(r) {
                continue;
            }
            if guard.of.is_some_and(|of| self.left(of)) {
                self.regiments[r].guard = None;
                self.give_up(r);
                continue;
            }
            if !self.regiments[r].engaged.is_empty() {
                continue;
            }
            let after = match self.regiments[r].order {
                Order::Pursue(e) => Some(e),
                _ => None,
            };
            let intruder = self.intruder(r, guard, after, &engaging);
            if let Some(e) = intruder {
                if after != Some(e) {
                    let regiment = &mut self.regiments[r];
                    regiment.order = Order::Pursue(e);
                    regiment.resume = None;
                    regiment.guard = Some(Guard {
                        step: Step::Unsent,
                        ..guard
                    });
                    self.chase(r, e, true);
                }
                continue;
            }
            if after.is_some() {
                self.give_up(r);
            }
            self.go_to_post(r);
        }
    }

    /// Sends a guard with nobody to fight to its post, if it's not been
    /// since it last fought or the post has moved, and has it rest once
    /// it's got as near as it can.
    fn go_to_post(&mut self, r: usize) {
        let Some(mut guard) = self.regiments[r].guard else {
            return;
        };
        let underway = self.underway(r);
        if let Some(of) = guard.of {
            // It keeps to the side it was on when it last set off.
            let (post, facing) = match guard.step {
                Step::Unsent => self.beside(r, of),
                _ => (self.post_beside(r, of, guard.facing), guard.facing),
            };
            // A post that's moved a little waits until the guard is there.
            let moved = match underway {
                true => !within(post, guard.post, POST_DRIFT),
                false => post != guard.post,
            };
            if guard.step == Step::Unsent {
                (guard.post, guard.facing) = (post, facing);
            } else if moved {
                guard = Guard {
                    post,
                    step: Step::Unsent,
                    unreachable: None,
                    ..guard
                };
            }
        }
        let before = self.regiments[r].current_formation();
        if guard.step == Step::Unsent {
            guard.step = Step::Sent;
            self.regiments[r].guard = Some(guard);
            self.send(r, guard.post, guard.facing);
        } else if guard.step == Step::Sent && !underway {
            guard.step = Step::Arrived;
            self.regiments[r].guard = Some(guard);
            if self.regiments[r].current_formation() != before {
                self.take_places(r);
            }
        }
    }

    /// The enemy regiment a guard goes after: the nearest of those
    /// `engaging` what it guards or, with none, the one that's engaged with
    /// first or fires on. The one it's `after` already counts as nearer, so
    /// the guard doesn't flit between two much the same way off.
    fn intruder(
        &self,
        r: usize,
        guard: Guard,
        after: Option<usize>,
        engaging: &[(usize, Enemy)],
    ) -> Option<usize> {
        let of = guard.of?;
        if !self.armed(r) {
            return None;
        }
        let fair = |e: usize| {
            guard.unreachable != Some(e)
                && !self.destroyed(e)
                && self.hostile(r, Enemy::Regiment(e))
        };
        let mut found: Vec<usize> = (engaging.iter())
            .filter(|&&(e, with)| with == of && fair(e))
            .map(|&(e, _)| e)
            .collect();
        if found.is_empty() {
            let fought = match of {
                Enemy::Regiment(g) => self.regiments[g].engaged.first().copied(),
                Enemy::Building(b) => self.buildings[b].target.map(|t| self.enemy(t)),
            };
            if let Some(Enemy::Regiment(e)) = fought {
                found.extend(fair(e).then_some(e));
            }
        }
        let here = self.centre(r);
        let far = |e: usize| {
            let bias = if after == Some(e) {
                self.guard_rules.bias
            } else {
                0.0
            };
            distance(here, self.centre(e)) - bias
        };
        found.into_iter().min_by(|&a, &b| far(a).total_cmp(&far(b)))
    }
}

#[cfg(test)]
mod tests {

    use crate::testing::*;

    use aaa_map::Tile;

    use crate::{ARRIVAL_REACH, Command, Enemy, Order, Refused, State, World, distance, within};

    /// A guard of player 1's, an ally's regiment for it to guard and an
    /// enemy out of range, 10 apart in a row, the guard noticing nothing
    /// for itself.
    fn escort() -> World {
        let mut w = battle(
            &[1, 3, 2],
            vec![
                soldier(vec![rifle(6.0, 50.0)]),
                soldier(Vec::new()),
                soldier(vec![rifle(6.0, 1.0)]),
            ],
            at_war(),
        );
        w.regiments[0].control_range = 4.0;
        let guard = Command::GuardThing {
            regiment: 0,
            thing: Enemy::Regiment(1),
        };
        w.apply(1, guard).unwrap();
        w
    }

    #[test]
    fn guards_rest_at_their_post_ready_to_fight_and_come_back_after() {
        let mut w = skirmish();
        // Unarmed, so it never fires on them to draw them in.
        w.units[1].fighter = Some(soldier(Vec::new()));
        let guard = Command::Guard {
            regiment: 0,
            to: [10.0, 12.0],
        };
        w.apply(1, guard.clone()).unwrap();
        // On the way it's as on a move.
        assert_eq!(w.regiments[0].current_formation(), 0);
        assert!(!w.regiments[0].guarding());
        run(&mut w, 0.5);
        assert!(w.regiments[0].engaged.is_empty());
        // Once there it's in its combat formation, and takes on the enemy
        // 18 off, further than its control range of 18 takes in.
        run(&mut w, 1.0);
        assert_eq!(w.regiments[0].current_formation(), 1);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        run(&mut w, 10.0);
        assert!(!w.units[1].alive());
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [10.0, 12.0]));
        assert!(w.regiments[0].guarding());
        assert_eq!(w.regiments[0].control(&w.guard_rules), 18.0 * 1.25);
        // The same order again changes nothing; another ends it.
        w.apply(1, guard).unwrap();
        assert!(w.regiments[0].guarding());
        w.apply(1, Command::Stop { regiment: 0 }).unwrap();
        assert_eq!(w.regiments[0].guard, None);
        assert_eq!(w.regiments[0].current_formation(), 0);
    }

    #[test]
    fn guards_stay_beside_what_they_guard_on_the_side_they_came_from() {
        let mut w = escort();
        run(&mut w, 5.0);
        // Half its control range off, facing away.
        assert!(within(w.units[0].position, [10.0, 18.0], ARRIVAL_REACH));
        assert_eq!(w.units[0].facing, [0.0, -1.0]);
        assert!(w.regiments[0].guarding());
        // An enemy that's fighting nobody is left alone.
        assert_eq!(w.regiments[0].order, Order::Move);
        w.apply(
            3,
            Command::Move {
                regiment: 1,
                to: [30.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        assert!(!w.regiments[0].guarding());
        run(&mut w, 14.0);
        assert!(within(w.units[0].position, [30.0, 18.0], ARRIVAL_REACH));
        assert!(w.regiments[0].guarding());
    }

    #[test]
    fn guards_go_after_whoever_fights_what_they_guard_until_its_gone() {
        let mut w = escort();
        run(&mut w, 5.0);
        w.apply(
            2,
            Command::Attack {
                regiment: 2,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        w.tick();
        assert!(w.regiments[2].engaged.contains(&Enemy::Regiment(1)));
        assert_eq!(w.regiments[0].order, Order::Pursue(2));
        assert_eq!(w.regiments[0].guard.unwrap().of, Some(Enemy::Regiment(1)));
        run(&mut w, 10.0);
        assert!(w.destroyed(2));
        // Back beside it, on the side the fight left it.
        run(&mut w, 10.0);
        assert!(within(w.units[0].position, [10.0, 22.0], ARRIVAL_REACH));
        assert!(w.regiments[0].guarding());
        w.destroy(1);
        w.tick();
        assert_eq!(w.regiments[0].guard, None);
    }

    #[test]
    fn guards_leave_intruders_they_cant_reach_and_keep_their_post() {
        let mut w = escort();
        run(&mut w, 5.0);
        for x in 0..40 {
            w.pathing.set_blocked(Tile::new(x, 12));
        }
        w.regiments[2].engaged.push(Enemy::Regiment(1));
        w.tick();
        assert_eq!(w.regiments[0].order, Order::Pursue(2));
        // As far as it can get by 3s, then back.
        run(&mut w, 5.0);
        assert_eq!(w.regiments[0].order, Order::Move);
        assert!(w.regiments[2].engaged.contains(&Enemy::Regiment(1)));
        // Back beside it, on the side that left it.
        run(&mut w, 5.0);
        assert!(w.regiments[2].engaged.contains(&Enemy::Regiment(1)));
        assert_eq!(w.regiments[0].order, Order::Move);
        assert!(distance(w.units[0].position, w.centre(1)) < 2.0 + ARRIVAL_REACH);
        assert!(w.regiments[0].guarding());
    }

    #[test]
    fn guards_keep_to_disabled_buildings_and_posts_on_the_map() {
        // 3 from the map's edge, the guard beyond it.
        let mut w = siege(&[building(20, 1, [10.0, 3.0])]);
        w.units[0].position = [10.0, 0.5];
        let guard = Command::GuardThing {
            regiment: 0,
            thing: Enemy::Building(0),
        };
        w.apply(1, guard.clone()).unwrap();
        assert_eq!(w.regiments[0].guard.unwrap().post[1], 0.0);
        w.buildings[0].state = State::Disabled;
        w.tick();
        assert!(w.regiments[0].guard.is_some());
        assert!(w.check(1, &guard).is_ok());
    }

    #[test]
    fn regiments_guard_anything_still_there_but_themselves() {
        let mut w = escort();
        let guard = |regiment, thing| Command::GuardThing { regiment, thing };
        assert!(w.check(1, &guard(0, Enemy::Regiment(2))).is_ok());
        let unable = Err(Refused::Unable);
        assert_eq!(w.check(1, &guard(0, Enemy::Regiment(0))), unable);
        assert_eq!(w.check(1, &guard(0, Enemy::Regiment(9))), unable);
        assert_eq!(w.check(1, &guard(0, Enemy::Building(0))), unable);
        w.destroy(2);
        assert_eq!(w.check(1, &guard(0, Enemy::Regiment(2))), unable);
    }
}
