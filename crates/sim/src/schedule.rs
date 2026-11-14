//! The orders a match is given, kept with the step each was carried out on,
//! so that another copy of the match given them on the same steps plays out
//! the same way.

use crate::{Command, Refused, SECOND, World};

/// Steps between the hashes a [`Record`] keeps of its match.
const CHECK_EVERY: u64 = SECOND;

/// An order as a match was given it.
#[derive(Debug, Clone)]
pub struct Given {
    /// The ticks the match had played when it was carried out.
    pub step: u64,
    pub player: u32,
    pub command: Command,
}

/// What a match has been given and how it stood along the way: enough,
/// with how the match began, to play it out again and to tell whether
/// that went the same way.
#[derive(Debug, Clone, Default)]
pub struct Record {
    /// The ticks played.
    pub steps: u64,
    /// The orders carried out, in the order they were.
    pub given: Vec<Given>,
    /// [`World::hash`] after some of the steps, by the ticks played then.
    pub checks: Vec<(u64, u64)>,
}

/// A match played from a [`Record`] that stopped matching it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diverged {
    /// The ticks played when its hash wasn't the record's.
    pub step: u64,
}

/// The orders waiting to be carried out in a match, and the [`Record`] of
/// those that have been. They're the only way players change a match.
#[derive(Debug, Default)]
pub struct Schedule {
    waiting: Vec<(u32, Command)>,
    record: Record,
}

impl Schedule {
    /// Queues `command` from `player`, unless the match would refuse it
    /// now.
    pub fn give(&mut self, world: &World, player: u32, command: Command) -> Result<(), Refused> {
        world.check(player, &command)?;
        self.waiting.push((player, command));
        Ok(())
    }

    /// Carries out every order waiting, returning those refused. Checked
    /// when given, they're only refused where one before them changed
    /// things, such as a building told to pack and then to commission.
    /// Carried out between two ticks, they're recorded as though just
    /// before the second, which comes to the same.
    pub fn carry_out(&mut self, world: &mut World) -> Vec<(u32, Refused)> {
        let mut refused = Vec::new();
        for (player, command) in self.waiting.drain(..) {
            self.record.given.push(Given {
                step: world.ticks,
                player,
                command: command.clone(),
            });
            if let Err(e) = world.apply(player, command) {
                refused.push((player, e));
            }
        }
        refused
    }

    /// Carries out the orders waiting and plays a tick, returning the
    /// orders refused.
    pub fn step(&mut self, world: &mut World) -> Vec<(u32, Refused)> {
        let refused = self.carry_out(world);
        world.tick();
        self.record.steps = world.ticks;
        if world.ticks.is_multiple_of(CHECK_EVERY) {
            self.record.checks.push((world.ticks, world.hash()));
        }
        refused
    }

    pub fn record(&self) -> &Record {
        &self.record
    }
}

impl Record {
    /// Plays `world`, a match as this one began, through what this one
    /// was given, stopping where it no longer stands as this one did: at
    /// once for one that has already been played.
    pub fn replay(&self, world: &mut World) -> Result<(), Diverged> {
        if world.ticks != 0 {
            return Err(Diverged { step: world.ticks });
        }
        let mut given = self.given.iter().peekable();
        let mut checks = self.checks.iter().peekable();
        let mut carry_out = |world: &mut World| {
            while let Some(g) = given.next_if(|g| g.step <= world.ticks) {
                // Refused here as it was there.
                let _ = world.apply(g.player, g.command.clone());
            }
        };
        while world.ticks < self.steps {
            carry_out(world);
            world.tick();
            if let Some(&(step, hash)) = checks.next_if(|c| c.0 <= world.ticks)
                && (step != world.ticks || hash != world.hash())
            {
                return Err(Diverged { step: world.ticks });
            }
        }
        // Those given since the last tick.
        carry_out(world);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    /// Four regiments of riflemen at war.
    fn fresh() -> World {
        let rifles = (0..4).map(|_| soldier(vec![rifle(12.0, 25.0)])).collect();
        battle(&[1, 2, 3, 2], rifles, at_war())
    }

    /// A match played for a minute, its sides given orders along the way.
    fn played() -> (World, Schedule) {
        let (mut w, mut s) = (fresh(), Schedule::default());
        let go = |regiment, to| Command::Move { regiment, to };
        s.give(&w, 1, go(0, [30.0, 30.0])).unwrap();
        for _ in 0..30 {
            s.step(&mut w);
        }
        s.give(&w, 3, go(2, [10.0, 50.0])).unwrap();
        s.give(&w, 2, go(3, [20.0, 20.0])).unwrap();
        // Carried out with the match held between two ticks.
        s.carry_out(&mut w);
        s.give(&w, 2, go(3, [25.0, 25.0])).unwrap();
        for _ in 0..570 {
            s.step(&mut w);
        }
        (w, s)
    }

    #[test]
    fn orders_are_recorded_with_the_step_they_were_carried_out_on() {
        let (w, s) = played();
        let record = s.record();
        assert_eq!(record.steps, w.ticks);
        let given: Vec<_> = record.given.iter().map(|g| (g.step, g.player)).collect();
        assert_eq!(given, [(0, 1), (30, 3), (30, 2), (30, 2)]);
        assert_eq!(record.checks.len() as u64, w.ticks / CHECK_EVERY);
    }

    #[test]
    fn a_record_plays_a_fresh_match_out_the_same_way() {
        let (w, s) = played();
        assert!(w.units.iter().any(|u| u.died.is_some()));
        let mut again = fresh();
        assert_eq!(s.record().replay(&mut again), Ok(()));
        assert_eq!((again.ticks, again.hash()), (w.ticks, w.hash()));
    }

    #[test]
    fn orders_given_since_the_last_tick_are_replayed_too() {
        let (mut w, mut s) = played();
        let to = [5.0, 5.0];
        let given = (0..w.regiments.len()).any(|regiment| {
            let owner = w.regiments[regiment].owner;
            s.give(&w, owner, Command::Move { regiment, to }).is_ok()
        });
        assert!(given);
        let before = w.hash();
        s.carry_out(&mut w);
        assert_ne!(w.hash(), before);
        let mut again = fresh();
        assert_eq!(s.record().replay(&mut again), Ok(()));
        assert_eq!(again.hash(), w.hash());
    }

    #[test]
    fn a_match_that_was_given_something_else_is_caught_at_the_next_check() {
        let (_, s) = played();
        let mut record = s.record().clone();
        record.given.remove(1);
        let diverged = record.replay(&mut fresh()).unwrap_err();
        assert!(diverged.step > 30 && diverged.step < 600);
        assert!(diverged.step.is_multiple_of(CHECK_EVERY));
    }

    #[test]
    fn orders_refused_when_given_are_never_queued() {
        let (mut w, mut s) = (fresh(), Schedule::default());
        // Not player 2's to order.
        let to = [30.0, 30.0];
        assert!(s.give(&w, 2, Command::Move { regiment: 0, to }).is_err());
        assert!(s.step(&mut w).is_empty());
        assert!(s.record().given.is_empty());
    }

    #[test]
    fn orders_refused_as_they_are_carried_out_are_handed_back_and_recorded() {
        let (mut w, mut s) = (fresh(), Schedule::default());
        // Fine when given, but by its turn the regiment has gone.
        let to = [30.0, 30.0];
        s.give(&w, 1, Command::Disband { regiment: 0 }).unwrap();
        s.give(&w, 1, Command::Move { regiment: 0, to }).unwrap();
        let refused = s.step(&mut w);
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].0, 1);
        assert_eq!(s.record().given.len(), 2);
        for _ in 0..50 {
            s.step(&mut w);
        }
        let mut again = fresh();
        assert_eq!(s.record().replay(&mut again), Ok(()));
        assert_eq!(again.hash(), w.hash());
    }

    #[test]
    fn a_match_already_played_is_not_one_to_replay_into() {
        let (_, s) = played();
        let mut late = fresh();
        late.tick();
        assert_eq!(s.record().replay(&mut late), Err(Diverged { step: 1 }));
    }
}
