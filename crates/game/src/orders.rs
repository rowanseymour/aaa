//! What the match is given. The game plays its copy of a match as a
//! client of the match's host, which here is in the same program, with a
//! copy of its own: nothing changes the match but by asking the host.

use aaa_server::{Client, Event, Host, Local, Setup};
use aaa_sim::{Action, Command, Record, Refused, SPEEDS, World};
use bevy::prelude::*;

/// The orders every player gives, and the record of those carried out.
#[derive(Resource)]
pub(crate) struct Orders {
    host: Host<Local>,
    client: Client<Local>,
    /// What's been asked of the host since it was last heard from.
    asked: Vec<Action>,
}

#[cfg(test)]
impl Default for Orders {
    /// Those of no match at all, for tests of what only asks.
    fn default() -> Self {
        let pathing = aaa_map::Pathing::uniform(1, aaa_map::Domain::Land);
        let world = World::from_regiments(Default::default(), pathing, &[]);
        Orders::hosting(Setup::default(), world)
    }
}

impl Orders {
    /// Those of a match hosted here, which `setup` set up, `world` being
    /// the host's copy of it.
    pub(crate) fn hosting(setup: Setup, world: World) -> Orders {
        let mut host = Host::new(setup, world);
        let (ours, mut theirs) = aaa_server::pair();
        // Whoever runs the host plays any side, or none, and may cheat.
        let admitted = host.admit(ours, Vec::new());
        host.trust(admitted.expect("a host lets in the first to ask"));
        // The game set the match up itself.
        let welcome = aaa_server::welcome(&mut theirs);
        debug_assert!(matches!(welcome, Ok(Some(_))));
        Orders {
            host,
            client: Client::new(theirs),
            asked: Vec::new(),
        }
    }

    /// Asks for `command` from `player`, unless `world`, the game's copy
    /// of the match, would refuse it now.
    pub(crate) fn give(
        &mut self,
        world: &World,
        player: u32,
        command: Command,
    ) -> Result<(), Refused> {
        world.check(player, &command)?;
        self.act(Action::Order { player, command });
        Ok(())
    }

    /// Asks for `action`.
    pub(crate) fn act(&mut self, action: Action) {
        self.asked.push(action.clone());
        if self.client.ask(action).is_err() {
            warn!("the match's host has gone");
        }
    }

    /// Whether the match is held still.
    pub(crate) fn paused(&self) -> bool {
        self.client.schedule().paused()
    }

    /// As [`Orders::paused`], once what's been asked is carried out.
    pub(crate) fn pausing(&self) -> bool {
        let asked = self.asked.iter().rev().find_map(|a| match a {
            Action::Pause(pause) => Some(*pause),
            _ => None,
        });
        asked.unwrap_or(self.paused())
    }

    /// How many times as fast the match is played.
    pub(crate) fn speed(&self) -> f32 {
        self.client.schedule().speed()
    }

    /// As [`Orders::speed`], once what's been asked is carried out.
    pub(crate) fn speeding(&self) -> f32 {
        let asked = self.asked.iter().rev().find_map(|a| match a {
            Action::Speed(speed) if SPEEDS.contains(speed) => Some(*speed),
            _ => None,
        });
        asked.unwrap_or(self.speed())
    }

    /// What the match was set up from.
    pub(crate) fn setup(&self) -> &Setup {
        self.host.setup()
    }

    pub(crate) fn record(&self) -> &Record {
        self.client.schedule().record()
    }

    /// Has what's been asked carried out in `world`, the game's copy of
    /// the match, without a tick going by, returning the orders refused.
    pub(crate) fn carry_out(&mut self, world: &mut World) -> Vec<(u32, Refused)> {
        let events = self.host.relay();
        self.heard(world, events)
    }

    /// As [`Orders::carry_out`], and then plays a tick.
    pub(crate) fn step(&mut self, world: &mut World) -> Vec<(u32, Refused)> {
        let events = self.host.tick();
        self.heard(world, events)
    }

    /// Has `world` do what the host has said, after a turn of the host's
    /// that brought `events`.
    fn heard(&mut self, world: &mut World, events: Vec<Event>) -> Vec<(u32, Refused)> {
        for event in events {
            // With both copies here, only by the match not playing out
            // the same way twice.
            warn!("the match's host says: {event:?}");
        }
        if self.client.advance(world, usize::MAX).is_err() {
            warn!("the match's host has gone");
        }
        self.asked.clear();
        self.client.take_refused()
    }
}

#[cfg(test)]
mod tests {
    use aaa_map::{Domain, Pathing};

    use super::*;

    fn fresh() -> World {
        World::from_regiments(Default::default(), Pathing::uniform(4, Domain::Land), &[])
    }

    #[test]
    fn what_is_asked_is_carried_out_in_both_copies_at_once() {
        let mut world = fresh();
        let mut orders = Orders::hosting(Setup::default(), world.clone());
        orders.act(Action::Speed(2.0));
        orders.act(Action::Pause(true));
        // Known to be coming before it's come.
        assert!(orders.pausing() && !orders.paused());
        assert_eq!((orders.speeding(), orders.speed()), (2.0, 1.0));
        assert!(orders.carry_out(&mut world).is_empty());
        assert!(orders.pausing() && orders.paused());
        assert_eq!((orders.speeding(), orders.speed()), (2.0, 2.0));
        assert_eq!((orders.record().given.len(), world.ticks), (2, 0));
        // More at once than a host hears from a client it doesn't trust.
        for _ in 0..200 {
            orders.act(Action::Pause(false));
        }
        orders.carry_out(&mut world);
        assert_eq!(orders.record().given.len(), 202);
        assert!(!orders.paused());
    }

    #[test]
    fn a_step_plays_a_tick_of_both_copies() {
        let mut world = fresh();
        let mut orders = Orders::hosting(Setup::default(), world.clone());
        // Held still or not: the game's clock is what's held.
        orders.act(Action::Pause(true));
        for _ in 0..25 {
            assert!(orders.step(&mut world).is_empty());
        }
        assert_eq!(world.ticks, 25);
        assert_eq!(orders.host.world().ticks, 25);
        assert_eq!(orders.host.world().hash(), world.hash());
        assert_eq!(orders.record(), orders.host.schedule().record());
        assert_eq!(orders.record().checks.len(), 2);
        // An order the match refuses is said to be.
        let stop = Command::Stop { regiment: 0 };
        orders.act(Action::Order {
            player: 1,
            command: stop.clone(),
        });
        assert_eq!(orders.carry_out(&mut world), [(1, Refused::NotYours)]);
        // And one the game's copy would refuse isn't asked for.
        assert_eq!(orders.give(&world, 1, stop), Err(Refused::NotYours));
        assert_eq!(orders.record().given.len(), 2);
    }
}
