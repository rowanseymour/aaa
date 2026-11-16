//! Organizations keeping a post, at a place or beside what they guard.

use tgmap::pathing::CELL_SIZE;

use crate::{
    Guard, Order, Organization, Piece, Step, World, bounding_radius, distance, normalised, within,
};

/// How far what an organization guards moves its post before the organization
/// follows.
const POST_DRIFT: f32 = 1.0;

impl World {
    /// Has an organization guard `of` from `post`, setting off there unless
    /// it's engaged.
    pub(crate) fn post(
        &mut self,
        organization: usize,
        of: Option<Piece>,
        post: [f32; 2],
        facing: Option<[f32; 2]>,
    ) {
        let r = &mut self.organizations[organization];
        let facing = facing.unwrap_or(r.facing);
        r.take_order(Order::Move);
        r.guard = Some(Guard {
            of,
            post,
            facing,
            step: Step::Unsent,
            unreachable: None,
        });
        match r.engaged.is_empty() {
            true => self.go_to_post(organization),
            // Where it's headed once done, as for a move.
            false => r.resume = Some((post, facing)),
        }
    }

    /// The post of an organization guarding `piece`, and the way it faces
    /// there: off the piece's edge by half the organization's control range, on
    /// the side the organization is, facing away.
    pub(crate) fn beside(&self, organization: usize, piece: Piece) -> ([f32; 2], [f32; 2]) {
        let r = &self.organizations[organization];
        let here = self.centre(organization);
        let there = self.middle(piece);
        let away = normalised([here[0] - there[0], here[1] - there[1]]).unwrap_or(r.facing);
        (self.post_beside(organization, piece, away), away)
    }

    /// The point `away` from `piece` that an organization guarding it rests at,
    /// kept on the map.
    fn post_beside(&self, organization: usize, piece: Piece, away: [f32; 2]) -> [f32; 2] {
        let radius = match piece {
            Piece::Organization(_) => 0.0,
            Piece::Building(b) => {
                let shape = self.buildings[b].obstacle.as_ref().map(|o| o.shape);
                shape.map_or(0.0, bounding_radius)
            }
        };
        let off = radius + self.organizations[organization].control_range / 2.0;
        let [x, y] = self.middle(piece);
        let size = self.pathing.side as f32 * CELL_SIZE;
        [
            (x + away[0] * off).clamp(0.0, size),
            (y + away[1] * off).clamp(0.0, size),
        ]
    }

    /// Where something an organization can engage or guard is.
    pub(crate) fn middle(&self, piece: Piece) -> [f32; 2] {
        match piece {
            Piece::Organization(e) => self.centre(e),
            Piece::Building(b) => self.buildings[b].position,
        }
    }

    /// Has each organization keeping a guard that isn't engaged go after the
    /// nearest intruder, or else to its post, which follows what it
    /// guards; the guard ends once that's gone.
    pub(crate) fn keep_guard(&mut self) {
        // Who's engaged with what, looked up once for all that guard things.
        let escorts = |r: &Organization| r.guard.is_some_and(|g| g.of.is_some());
        let engaging: Vec<(usize, Piece)> = match self.organizations.iter().any(escorts) {
            true => (self.organizations.iter().enumerate())
                .flat_map(|(e, r)| r.engaged.iter().map(move |&with| (e, with)))
                .collect(),
            false => Vec::new(),
        };
        for r in 0..self.organizations.len() {
            let Some(guard) = self.organizations[r].guard else {
                continue;
            };
            if self.destroyed(r) {
                continue;
            }
            if guard.of.is_some_and(|of| self.left(of)) {
                self.organizations[r].guard = None;
                self.give_up(r);
                continue;
            }
            if !self.organizations[r].engaged.is_empty() {
                continue;
            }
            let after = match self.organizations[r].order {
                Order::Pursue(e) => Some(e),
                _ => None,
            };
            let intruder = self.intruder(r, guard, after, &engaging);
            if let Some(e) = intruder {
                if after != Some(e) {
                    let organization = &mut self.organizations[r];
                    organization.order = Order::Pursue(e);
                    organization.resume = None;
                    organization.guard = Some(Guard {
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
        let Some(mut guard) = self.organizations[r].guard else {
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
        let before = self.organizations[r].current_formation();
        if guard.step == Step::Unsent {
            guard.step = Step::Sent;
            self.organizations[r].guard = Some(guard);
            self.send(r, guard.post, guard.facing);
        } else if guard.step == Step::Sent && !underway {
            guard.step = Step::Arrived;
            self.organizations[r].guard = Some(guard);
            if self.organizations[r].current_formation() != before {
                self.take_places(r);
            }
        }
    }

    /// The enemy organization a guard goes after: the nearest of those
    /// `engaging` what it guards or, with none, the one that's engaged with
    /// first or fires on. The one it's `after` already counts as nearer, so
    /// the guard doesn't flit between two much the same way off.
    fn intruder(
        &self,
        r: usize,
        guard: Guard,
        after: Option<usize>,
        engaging: &[(usize, Piece)],
    ) -> Option<usize> {
        let of = guard.of?;
        if !self.armed(r) {
            return None;
        }
        let fair = |e: usize| {
            guard.unreachable != Some(e)
                && !self.destroyed(e)
                && self.hostile(r, Piece::Organization(e))
        };
        let mut found: Vec<usize> = (engaging.iter())
            .filter(|&&(e, with)| with == of && fair(e))
            .map(|&(e, _)| e)
            .collect();
        if found.is_empty() {
            let fought = match of {
                Piece::Organization(g) => self.organizations[g].engaged.first().copied(),
                Piece::Building(b) => self.buildings[b].target.map(|t| self.piece(t)),
            };
            if let Some(Piece::Organization(e)) = fought {
                found.extend(fair(e).then_some(e));
            }
        }
        let here = self.centre(r);
        let far = |e: usize| {
            let bias = if after == Some(e) {
                self.rules.guard_rules.bias
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

    use tgmap::Tile;

    use crate::{ARRIVAL_REACH, Command, Order, Piece, Refused, State, World, distance, within};

    /// A guard of player 1's, an ally's organization for it to guard and an
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
        w.organizations[0].control_range = 4.0;
        let guard = Command::GuardPiece {
            organization: 0,
            piece: Piece::Organization(1),
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
            organization: 0,
            to: [10.0, 12.0],
        };
        w.apply(1, guard.clone()).unwrap();
        // On the way it's as on a move.
        assert_eq!(w.organizations[0].current_formation(), 0);
        assert!(!w.organizations[0].guarding());
        run(&mut w, 0.5);
        assert!(w.organizations[0].engaged.is_empty());
        // Once there it's in its combat formation, and takes on the enemy
        // 18 off, further than its control range of 18 takes in.
        run(&mut w, 1.0);
        assert_eq!(w.organizations[0].current_formation(), 1);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
        run(&mut w, 10.0);
        assert!(!w.units[1].alive());
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [10.0, 12.0]));
        assert!(w.organizations[0].guarding());
        assert_eq!(
            w.organizations[0].control(&w.rules.guard_rules),
            18.0 * 1.25
        );
        // The same order again changes nothing; another ends it.
        w.apply(1, guard).unwrap();
        assert!(w.organizations[0].guarding());
        w.apply(1, Command::Stop { organization: 0 }).unwrap();
        assert_eq!(w.organizations[0].guard, None);
        assert_eq!(w.organizations[0].current_formation(), 0);
    }

    #[test]
    fn guards_stay_beside_what_they_guard_on_the_side_they_came_from() {
        let mut w = escort();
        run(&mut w, 5.0);
        // Half its control range off, facing away.
        assert!(within(w.units[0].position, [10.0, 18.0], ARRIVAL_REACH));
        assert_eq!(w.units[0].facing, [0.0, -1.0]);
        assert!(w.organizations[0].guarding());
        // An enemy that's fighting nobody is left alone.
        assert_eq!(w.organizations[0].order, Order::Move);
        w.apply(
            3,
            Command::Move {
                organization: 1,
                to: [30.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        assert!(!w.organizations[0].guarding());
        run(&mut w, 14.0);
        assert!(within(w.units[0].position, [30.0, 18.0], ARRIVAL_REACH));
        assert!(w.organizations[0].guarding());
    }

    #[test]
    fn guards_go_after_whoever_fights_what_they_guard_until_its_gone() {
        let mut w = escort();
        run(&mut w, 5.0);
        w.apply(
            2,
            Command::Attack {
                organization: 2,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        w.tick();
        assert!(w.organizations[2].engaged.contains(&Piece::Organization(1)));
        assert_eq!(w.organizations[0].order, Order::Pursue(2));
        assert_eq!(
            w.organizations[0].guard.unwrap().of,
            Some(Piece::Organization(1))
        );
        run(&mut w, 10.0);
        assert!(w.destroyed(2));
        // Back beside it, on the side the fight left it.
        run(&mut w, 10.0);
        assert!(within(w.units[0].position, [10.0, 22.0], ARRIVAL_REACH));
        assert!(w.organizations[0].guarding());
        w.destroy(1);
        w.tick();
        assert_eq!(w.organizations[0].guard, None);
    }

    #[test]
    fn guards_leave_intruders_they_cant_reach_and_keep_their_post() {
        let mut w = escort();
        run(&mut w, 5.0);
        for x in 0..40 {
            w.pathing.set_blocked(Tile::new(x, 12));
        }
        w.organizations[2].engaged.push(Piece::Organization(1));
        w.tick();
        assert_eq!(w.organizations[0].order, Order::Pursue(2));
        // As far as it can get by 3s, then back.
        run(&mut w, 5.0);
        assert_eq!(w.organizations[0].order, Order::Move);
        assert!(w.organizations[2].engaged.contains(&Piece::Organization(1)));
        // Back beside it, on the side that left it.
        run(&mut w, 5.0);
        assert!(w.organizations[2].engaged.contains(&Piece::Organization(1)));
        assert_eq!(w.organizations[0].order, Order::Move);
        assert!(distance(w.units[0].position, w.centre(1)) < 2.0 + ARRIVAL_REACH);
        assert!(w.organizations[0].guarding());
    }

    #[test]
    fn guards_keep_to_disabled_buildings_and_posts_on_the_map() {
        // 3 from the map's edge, the guard beyond it.
        let mut w = siege(&[building(20, 1, [10.0, 3.0])]);
        w.units[0].position = [10.0, 0.5];
        let guard = Command::GuardPiece {
            organization: 0,
            piece: Piece::Building(0),
        };
        w.apply(1, guard.clone()).unwrap();
        assert_eq!(w.organizations[0].guard.unwrap().post[1], 0.0);
        w.buildings[0].state = State::Disabled;
        w.tick();
        assert!(w.organizations[0].guard.is_some());
        assert!(w.check(1, &guard).is_ok());
    }

    #[test]
    fn organizations_guard_anything_still_there_but_themselves() {
        let mut w = escort();
        let guard = |organization, piece| Command::GuardPiece {
            organization,
            piece,
        };
        assert!(w.check(1, &guard(0, Piece::Organization(2))).is_ok());
        let unable = Err(Refused::Unable);
        assert_eq!(w.check(1, &guard(0, Piece::Organization(0))), unable);
        assert_eq!(w.check(1, &guard(0, Piece::Organization(9))), unable);
        assert_eq!(w.check(1, &guard(0, Piece::Building(0))), unable);
        w.destroy(2);
        assert_eq!(w.check(1, &guard(0, Piece::Organization(2))), unable);
    }
}
