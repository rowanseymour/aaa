//! What a match is given, orders above all, kept with the step each was
//! carried out on, so that another copy of the match given them on the same
//! steps plays out the same way.

use serde::{Deserialize, Serialize};
use tgmap::Id;

use crate::{Command, Refused, SECOND, World};

/// How many times as fast a match can be played.
pub const SPEEDS: std::ops::RangeInclusive<f32> = 0.05..=16.0;

/// Steps between the hashes a [`Record`] keeps of its match.
const CHECK_EVERY: u64 = SECOND;

/// Something a match is given: the only ways it's changed besides playing
/// on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// A player's order.
    Order { player: u32, command: Command },
    /// Holds the match still, or lets it go on.
    Pause(bool),
    /// Has the match play this many times as fast; nothing unless it's
    /// one of [`SPEEDS`].
    Speed(f32),
    /// Has a side win, as [`World::win`].
    Win(u32),
    /// Has a side lose, as [`World::lose`].
    Lose(u32),
    /// Puts an organization on the map, as [`World::raise`].
    Raise {
        template: Id,
        owner: u32,
        at: [f32; 2],
        facing: [f32; 2],
    },
    /// Kills the organization at this index into [`World::organizations`]
    /// outright.
    Destroy(usize),
    /// Words from the person playing side `from`, or from one watching
    /// for none, to those `to` reaches.
    Chat {
        from: Option<u32>,
        to: Channel,
        words: String,
    },
    /// Has side `player` go by the name of the person playing it.
    Name { player: u32, name: String },
    /// Tells everyone what came of someone asking to pause or unpause.
    Pausing(Pausing),
    /// Has the person playing a side resign it, as [`World::resign`].
    Resign(u32),
}

/// What came of a person asking to pause or unpause a match played
/// between machines, each naming a person by the first side they play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pausing {
    /// `by` paused the match.
    Paused { by: u32 },
    /// The votes towards unpausing it, where that takes a vote, as it's
    /// paused.
    Needed(Votes),
    /// `by`, who paused it, unpaused it.
    Unpaused { by: u32 },
    /// `by` voted to unpause it, or took their vote back.
    Voted { by: u32, cast: bool, votes: Votes },
    /// Enough voted, and it's unpaused.
    VotedOff,
    /// `by` asked to pause too soon after a pause of theirs was voted off.
    Refused { by: u32 },
}

/// How many are for unpausing a match, and how many it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Votes {
    pub votes: u32,
    pub needed: u32,
}

/// The most characters a line of chat holds.
pub const CHAT_LONGEST: usize = 128;

/// Who chat is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Channel {
    /// Everyone, those watching too.
    All,
    /// The sender's side and its allies, or, from one watching, those
    /// watching.
    Team,
    /// The person playing this side alone.
    To(u32),
}

impl Action {
    /// Leaves `clock`, a match's pause and speed, as it has them.
    fn set_clock(&self, clock: &mut (bool, f32)) {
        match *self {
            Action::Pause(pause) => clock.0 = pause,
            Action::Speed(speed) if SPEEDS.contains(&speed) => clock.1 = speed,
            _ => {}
        }
    }

    /// Carries it out in `world`, which is all of it but for the pause
    /// and speed a match is played at.
    fn apply(self, world: &mut World) -> Result<(), (u32, Refused)> {
        match self {
            Action::Order { player, command } => {
                return world.apply(player, command).map_err(|e| (player, e));
            }
            Action::Pause(_) | Action::Speed(_) => {}
            Action::Win(player) => world.win(player),
            Action::Lose(player) => world.lose(player),
            Action::Raise {
                template,
                owner,
                at,
                facing,
            } => _ = world.raise(&template, owner, at, facing),
            Action::Destroy(organization) if organization < world.organizations.len() => {
                world.destroy(organization)
            }
            Action::Destroy(_) => {}
            Action::Chat { from, to, words } => {
                world.notices.push(crate::Notice::Chat { from, to, words })
            }
            Action::Name { player, name } if world.sides.player(player).is_some() => {
                world.sides.names.insert(player, name);
            }
            Action::Name { .. } => {}
            Action::Pausing(pausing) => world.notices.push(crate::Notice::Pausing(pausing)),
            Action::Resign(player) => world.resign(player),
        }
        Ok(())
    }
}

/// An action as a match was given it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Given {
    /// The ticks the match had played when it was carried out.
    pub step: u64,
    pub action: Action,
}

/// What a match has been given and how it stood along the way: enough,
/// with how the match began, to play it out again and to tell whether
/// that went the same way.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// The ticks played.
    pub steps: u64,
    /// The actions carried out, in the order they were.
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

/// The actions waiting to be carried out in a match, the [`Record`] of
/// those that have been, and the pause and speed they've left it at.
#[derive(Debug)]
pub struct Schedule {
    waiting: Vec<Action>,
    record: Record,
    paused: bool,
    speed: f32,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            waiting: Vec::new(),
            record: Record::default(),
            paused: false,
            speed: 1.0,
        }
    }
}

impl Schedule {
    /// Queues `command` from `player`, unless the match would refuse it
    /// now.
    pub fn give(&mut self, world: &World, player: u32, command: Command) -> Result<(), Refused> {
        world.check(player, &command)?;
        self.act(Action::Order { player, command });
        Ok(())
    }

    /// Queues `action`.
    pub fn act(&mut self, action: Action) {
        self.waiting.push(action);
    }

    /// Whether the match is held still.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// As [`Schedule::paused`], once what's waiting is carried out.
    pub fn pausing(&self) -> bool {
        self.clock_once_carried_out().0
    }

    /// How many times as fast the match is played.
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// As [`Schedule::speed`], once what's waiting is carried out.
    pub fn speeding(&self) -> f32 {
        self.clock_once_carried_out().1
    }

    fn clock_once_carried_out(&self) -> (bool, f32) {
        let mut clock = (self.paused, self.speed);
        self.waiting.iter().for_each(|a| a.set_clock(&mut clock));
        clock
    }

    /// Carries out every action waiting, returning the orders refused.
    /// Checked when given, they're only refused where one before them
    /// changed things, such as a building told to pack and then to
    /// commission. Carried out between two ticks, actions are recorded as
    /// though just before the second, which comes to the same.
    pub fn carry_out(&mut self, world: &mut World) -> Vec<(u32, Refused)> {
        let mut refused = Vec::new();
        for action in self.waiting.drain(..) {
            let mut clock = (self.paused, self.speed);
            action.set_clock(&mut clock);
            if clock.1 != self.speed {
                world.notices.push(crate::Notice::Speed(clock.1));
            }
            (self.paused, self.speed) = clock;
            let step = world.ticks;
            self.record.given.push(Given {
                step,
                action: action.clone(),
            });
            refused.extend(action.apply(world).err());
        }
        refused
    }

    /// Carries out the actions waiting and plays a tick, returning the
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
                let _ = g.action.clone().apply(world);
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

    /// Four organizations of riflemen at war.
    fn fresh() -> World {
        let rifles = (0..4).map(|_| soldier(vec![rifle(12.0, 25.0)])).collect();
        battle(&[1, 2, 3, 2], rifles, at_war())
    }

    /// A match played for a minute, its sides given orders along the way.
    fn played() -> (World, Schedule) {
        let (mut w, mut s) = (fresh(), Schedule::default());
        let go = |organization, to| Command::Move { organization, to };
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
        let by = |g: &Given| match g.action {
            Action::Order { player, .. } => player,
            _ => 0,
        };
        let given: Vec<_> = record.given.iter().map(|g| (g.step, by(g))).collect();
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
        let given = (0..w.organizations.len()).any(|organization| {
            let owner = w.organizations[organization].owner;
            s.give(&w, owner, Command::Move { organization, to })
                .is_ok()
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
        assert!(
            s.give(
                &w,
                2,
                Command::Move {
                    organization: 0,
                    to
                }
            )
            .is_err()
        );
        assert!(s.step(&mut w).is_empty());
        assert!(s.record().given.is_empty());
    }

    #[test]
    fn orders_refused_as_they_are_carried_out_are_handed_back_and_recorded() {
        let (mut w, mut s) = (fresh(), Schedule::default());
        // Fine when given, but by its turn the organization has gone.
        let to = [30.0, 30.0];
        s.give(&w, 1, Command::Disband { organization: 0 }).unwrap();
        s.give(
            &w,
            1,
            Command::Move {
                organization: 0,
                to,
            },
        )
        .unwrap();
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
    fn what_changes_a_match_besides_orders_is_recorded_and_replayed() {
        let fresh = || {
            let mut w = fresh();
            w.rules.raisable.insert("truck".into(), truck());
            w
        };
        let (mut w, mut s) = (fresh(), Schedule::default());
        s.act(Action::Win(3));
        for _ in 0..20 {
            s.step(&mut w);
        }
        assert_eq!(w.outcome(3), crate::Outcome::Won);
        let organizations = w.organizations.len();
        let raise = |template: &str| Action::Raise {
            template: template.into(),
            owner: 1,
            at: [40.0, 40.0],
            facing: [0.0, 1.0],
        };
        s.act(raise("truck"));
        // As are a template the match can't raise, an owner it hasn't
        // got and a place that isn't on the map.
        s.act(raise("tank"));
        for (owner, at, facing) in [
            (9, [40.0, 40.0], [0.0, 1.0]),
            (1, [f32::NAN, 40.0], [0.0, 1.0]),
            (1, [-1.0, 40.0], [0.0, 1.0]),
            (1, [40.0, 1e9], [0.0, 1.0]),
            (1, [40.0, 40.0], [0.0, 0.0]),
            (1, [40.0, 40.0], [f32::NAN, 1.0]),
        ] {
            s.act(Action::Raise {
                template: "truck".into(),
                owner,
                at,
                facing,
            });
        }
        s.act(Action::Destroy(0));
        // One the match hasn't got is passed over.
        s.act(Action::Destroy(99));
        assert!(s.carry_out(&mut w).is_empty());
        assert_eq!(w.organizations.len(), organizations + 1);
        assert_eq!(w.organizations[organizations].owner, 1);
        assert!(w.destroyed(0));
        for _ in 0..20 {
            s.step(&mut w);
        }
        s.act(Action::Lose(2));
        for _ in 0..20 {
            s.step(&mut w);
        }
        assert_eq!(s.record().given.len(), 12);
        let mut again = fresh();
        assert_eq!(s.record().replay(&mut again), Ok(()));
        assert_eq!(again.hash(), w.hash());
    }

    #[test]
    fn a_side_goes_by_the_name_it_is_given() {
        let (mut w, mut s) = (fresh(), Schedule::default());
        let player = w.sides.players[0].id;
        s.act(Action::Name {
            player,
            name: "Bob".into(),
        });
        // One the match hasn't got is passed over.
        s.act(Action::Name {
            player: 99,
            name: "Nobody".into(),
        });
        s.carry_out(&mut w);
        assert_eq!(w.sides.names, [(player, "Bob".to_string())].into());
    }

    #[test]
    fn pause_and_speed_take_hold_as_they_are_carried_out() {
        let (mut w, mut s) = (fresh(), Schedule::default());
        assert!(!s.paused() && s.speed() == 1.0);
        s.act(Action::Pause(true));
        s.act(Action::Speed(4.0));
        // Asked for, and so what the next to ask finds, but not yet so.
        assert!(s.pausing() && !s.paused());
        assert!(s.speeding() == 4.0 && s.speed() == 1.0);
        let before = w.hash();
        s.carry_out(&mut w);
        assert!(s.paused() && s.pausing() && s.speed() == 4.0);
        // Neither is anything to the match itself.
        assert_eq!(w.hash(), before);
        s.act(Action::Pause(false));
        s.act(Action::Pause(true));
        s.act(Action::Pause(false));
        assert!(!s.pausing() && s.paused());
        s.step(&mut w);
        assert!(!s.paused());
        let given: Vec<u64> = s.record().given.iter().map(|g| g.step).collect();
        assert_eq!(given, [0, 0, 0, 0, 0]);
        // A speed a match isn't played at is passed over.
        for speed in [0.0, -1.0, 0.01, 17.0, f32::NAN, f32::INFINITY] {
            s.act(Action::Speed(speed));
            assert_eq!(s.speeding(), 4.0);
        }
        s.carry_out(&mut w);
        assert_eq!(s.speed(), 4.0);
    }

    #[test]
    fn a_match_already_played_is_not_one_to_replay_into() {
        let (_, s) = played();
        let mut late = fresh();
        late.tick();
        assert_eq!(s.record().replay(&mut late), Err(Diverged { step: 1 }));
    }
}
