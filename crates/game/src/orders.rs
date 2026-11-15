//! What the match is given. The game plays its copy of a match as a
//! client of the match's host: nothing changes the match but by asking
//! the host. The host is in the same program, with a copy of its own, or
//! elsewhere, when the game has joined its match.

use std::collections::VecDeque;

use aaa_server::protocol::Welcome;
use aaa_server::{Client, Event, Host, Link, Local, Setup};
use aaa_sim::{Action, Command, Record, Refused, SPEEDS, World};
use bevy::prelude::*;

/// The most ticks a joined match plays as one falls due: more than one
/// only to catch its host up, a part at a time so the window keeps
/// drawing.
const CAUGHT_UP: usize = 5;

/// The game's end of the link to the match's host, wherever that is.
pub(crate) type Line = Box<dyn Link + Send + Sync>;

/// The orders every player gives, and the record of those carried out.
#[derive(Resource)]
pub(crate) struct Orders {
    host: Hosted,
    client: Client<Line>,
    setup: Setup,
    /// The pauses and speeds asked of the host that it has yet to answer,
    /// each with how many actions had been asked of it by then.
    asked: VecDeque<(u64, Action)>,
    /// How many copies of the match are known to have gone their own way.
    diverged: usize,
    gone: bool,
}

/// Where the match's host is.
enum Hosted {
    /// Here, doing whatever it's asked.
    Here(Box<Host<Local>>),
    /// Elsewhere, taking from here only the orders of these players, and
    /// of the rest only what it takes from anyone who plays.
    Elsewhere { players: Vec<u32> },
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
        let mut host = Host::new(setup.clone(), world);
        let (ours, mut theirs) = aaa_server::pair();
        // Whoever runs the host plays any side, or none, and may cheat.
        let admitted = host.admit(ours, Vec::new());
        host.trust(admitted.expect("a host lets in the first to ask"));
        // The game set the match up itself.
        let welcome = aaa_server::welcome(&mut theirs);
        debug_assert!(matches!(welcome, Ok(Some(_))));
        Orders::of(Hosted::Here(Box::new(host)), Box::new(theirs), setup)
    }

    /// Those of a match whose host, at the other end of `link`, let the
    /// game in with `welcome`.
    pub(crate) fn joined(link: Line, welcome: Welcome) -> Orders {
        let players = welcome.players;
        Orders::of(Hosted::Elsewhere { players }, link, welcome.setup)
    }

    fn of(host: Hosted, link: Line, setup: Setup) -> Orders {
        Orders {
            host,
            client: Client::new(link),
            setup,
            asked: VecDeque::new(),
            diverged: 0,
            gone: false,
        }
    }

    /// The players the game was let in to a match hosted elsewhere to
    /// give the orders of; `None` for a match hosted here, where it gives
    /// anyone's.
    pub(crate) fn players(&self) -> Option<&[u32]> {
        match &self.host {
            Hosted::Here(_) => None,
            Hosted::Elsewhere { players } => Some(players),
        }
    }

    /// Whether the match's host is elsewhere, and so the match isn't the
    /// game's to do as it likes with.
    pub(crate) fn joined_to(&self) -> bool {
        self.players().is_some()
    }

    /// Tells the host that the game's copy of the match is set up: a
    /// host elsewhere may hold the match for every copy until then.
    pub(crate) fn ready(&mut self) {
        // A host that's gone is found so as the match is next played.
        let _ = self.client.ready();
    }

    /// Whether the host has gone, and with it the match.
    pub(crate) fn gone(&self) -> bool {
        self.gone
    }

    /// Whether the host went by ending the match, rather than being lost.
    pub(crate) fn ended(&self) -> bool {
        self.gone && self.client.ended()
    }

    /// The other clients of the match found gone since this was last
    /// asked, each with the players it gave the orders of.
    pub(crate) fn take_left(&mut self) -> Vec<(u32, Vec<u32>)> {
        self.client.take_left()
    }

    /// Whether the host, being elsewhere, has yet to begin the match:
    /// it may hold it until every copy is set up, and carries out nothing
    /// asked of it before then.
    pub(crate) fn waiting(&self) -> bool {
        let record = self.record();
        self.joined_to() && !self.gone && record.steps == 0 && record.given.is_empty()
    }

    /// Whether the host would carry `action` out if asked.
    pub(crate) fn may(&self, action: &Action) -> bool {
        let allowed = |players| aaa_server::allows(players, action);
        !self.gone && !self.waiting() && self.players().is_none_or(allowed)
    }

    /// Asks for `command` from `player`, unless `world`, the game's copy
    /// of the match, would refuse it now, or the host wouldn't take it
    /// from here.
    pub(crate) fn give(
        &mut self,
        world: &World,
        player: u32,
        command: Command,
    ) -> Result<(), Refused> {
        world.check(player, &command)?;
        let action = Action::Order { player, command };
        if !self.may(&action) {
            return Err(Refused::NotYours);
        }
        self.act(action);
        Ok(())
    }

    /// Asks for `action`, unless the host is known not to carry it out.
    pub(crate) fn act(&mut self, action: Action) {
        if !self.may(&action) {
            debug!("not asked of the match's host: {action:?}");
            return;
        }
        let clock = matches!(action, Action::Pause(_) | Action::Speed(_)).then(|| action.clone());
        // A host that's gone is found so as the match is next played.
        if self.client.ask(action).is_ok() {
            self.asked.extend(clock.map(|c| (self.client.asked(), c)));
        }
    }

    /// Whether the match is held still.
    pub(crate) fn paused(&self) -> bool {
        self.client.schedule().paused()
    }

    /// As [`Orders::paused`], once what's been asked is carried out.
    pub(crate) fn pausing(&self) -> bool {
        let asked = self.asked.iter().rev().find_map(|(_, a)| match a {
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
        let asked = self.asked.iter().rev().find_map(|(_, a)| match a {
            Action::Speed(speed) if SPEEDS.contains(speed) => Some(*speed),
            _ => None,
        });
        asked.unwrap_or(self.speed())
    }

    /// What the match was set up from.
    pub(crate) fn setup(&self) -> &Setup {
        &self.setup
    }

    pub(crate) fn record(&self) -> &Record {
        self.client.schedule().record()
    }

    /// Has what's been asked carried out in `world`, the game's copy of
    /// the match, without a tick going by, returning the orders refused.
    /// A host elsewhere carries it out when it hears of it, and so in
    /// `world` some time later.
    pub(crate) fn carry_out(&mut self, world: &mut World) -> Vec<(u32, Refused)> {
        let played = match &mut self.host {
            Hosted::Here(host) => {
                said(host.relay());
                self.client.advance(world, usize::MAX)
            }
            // The game's clock stands still with the match, so no tick
            // falls due to play the one the match goes on with.
            Hosted::Elsewhere { .. } => {
                let ticks = usize::from(self.client.schedule().paused());
                self.client.play(world, ticks)
            }
        };
        self.played(played.is_err())
    }

    /// As [`Orders::carry_out`], and then plays a tick. A host elsewhere
    /// says when the match plays one: `world` plays the next it's said
    /// to, if it's heard of any, and more where it has fallen behind.
    pub(crate) fn step(&mut self, world: &mut World) -> Vec<(u32, Refused)> {
        let played = match &mut self.host {
            Hosted::Here(host) => {
                said(host.tick());
                self.client.advance(world, usize::MAX)
            }
            Hosted::Elsewhere { .. } => {
                let ticks = self.client.behind().clamp(1, CAUGHT_UP);
                self.client.play(world, ticks)
            }
        };
        self.played(played.is_err())
    }

    /// Takes stock after the game's copy of the match has done what the
    /// host said, `lost` being whether the host is found gone.
    fn played(&mut self, lost: bool) -> Vec<(u32, Refused)> {
        let answered = self.client.answered();
        self.asked.retain(|&(number, _)| number > answered);
        let diverged = self.client.diverged();
        for (client, step) in &diverged[self.diverged..] {
            warn!("client {client}'s copy of the match stopped matching its host's by tick {step}");
        }
        self.diverged = diverged.len();
        if lost && !self.gone {
            match self.client.ended() {
                true => info!("the match's host has ended it"),
                false => warn!("the match's host has gone"),
            }
            self.gone = true;
            // Nothing asked comes back now.
            self.asked.clear();
        }
        self.client.take_refused()
    }
}

impl Drop for Orders {
    /// Has the game leave the match: ending it, if it's the host.
    fn drop(&mut self) {
        match &mut self.host {
            Hosted::Here(host) => host.end(),
            // Failing only where the host has already gone.
            Hosted::Elsewhere { .. } => _ = self.client.leave(),
        }
    }
}

/// Leaves the match as the game exits, rather than trusting what tears
/// the app down to drop it.
pub(crate) fn leave_on_exit(mut commands: Commands, mut exit: MessageReader<AppExit>) {
    if exit.read().count() > 0 {
        commands.remove_resource::<Orders>();
    }
}

/// Logs what a turn of a host here brought.
fn said(events: Vec<Event>) {
    for event in events {
        // With both copies here, only by the match not playing out the
        // same way twice.
        warn!("the match's host says: {event:?}");
    }
}

/// Matches hosted as though elsewhere, for tests of what plays in one.
#[cfg(test)]
pub(crate) mod testing {
    use aaa_map::{Domain, Formation, Member, Pathing, Regiment};
    use aaa_sim::Sides;

    use super::*;

    /// A match of players 1 and 2, each with a regiment of one soldier:
    /// regiment 0 the first's and regiment 1 the second's.
    pub(crate) fn fresh() -> World {
        let regiment = |i: u32| Regiment {
            hq: None,
            id: i * 2 + 1,
            template: "squad".into(),
            owner: i + 1,
            facing: [0.0, 1.0],
            members: vec![Member {
                id: i * 2 + 2,
                template: "soldier".into(),
                position: [10.0, 10.0 + 10.0 * i as f32],
                facing: [0.0, 1.0],
                speed: 2.0,
                shape: None,
                acceleration: 0.0,
                turn_rate: 0.0,
                trailer: None,
                fighter: None,
                health: 1.0,
                lost: None,
            }],
            formations: vec![Formation {
                id: "line".into(),
                offsets: vec![[0.0, 0.0]],
                aggressive: false,
            }],
            formation: 0,
            combat: None,
            operational_range: None,
            control_range: None,
            primary: None,
            unpacks: None,
        };
        let sides = Sides {
            players: [1, 2].map(crate::tests::player).into(),
            teams: Vec::new(),
        };
        let pathing = Pathing::uniform(40, Domain::Land);
        World::from_regiments(sides, pathing, &[regiment(0), regiment(1)])
    }

    /// Has `host` let in a client asking for `players`: its end of the
    /// link, and what it was let in with.
    pub(crate) fn let_in(host: &mut Host<Local>, players: &[u32]) -> (Local, Welcome) {
        let (ours, mut theirs) = aaa_server::pair();
        aaa_server::join(&mut theirs, players.to_vec()).unwrap();
        host.greet(ours);
        assert!(matches!(host.seat()[..], [Ok(_)]));
        let welcome = aaa_server::welcome(&mut theirs).unwrap().unwrap();
        (theirs, welcome)
    }

    /// The host of a [`fresh`] match, the orders of a game let in to it
    /// to play `players`, and the game's copy of the match.
    pub(crate) fn joined(players: &[u32]) -> (Host<Local>, Orders, World) {
        let mut host = Host::new(Setup::default(), fresh());
        let (link, welcome) = let_in(&mut host, players);
        (host, Orders::joined(Box::new(link), welcome), fresh())
    }
}

#[cfg(test)]
mod tests {
    use aaa_map::{Domain, Pathing};

    use super::testing::joined;
    use super::*;

    fn fresh() -> World {
        World::from_regiments(Default::default(), Pathing::uniform(4, Domain::Land), &[])
    }

    impl Orders {
        fn host(&self) -> &Host<Local> {
            match &self.host {
                Hosted::Here(host) => host,
                Hosted::Elsewhere { .. } => panic!("not a match hosted here"),
            }
        }
    }

    /// As [`joined`], the match begun and a tick of it played.
    fn begun(players: &[u32]) -> (Host<Local>, Orders, World) {
        let (mut host, mut orders, mut world) = joined(players);
        host.turn();
        orders.step(&mut world);
        assert!(!orders.waiting());
        (host, orders, world)
    }

    fn stop(player: u32, regiment: usize) -> Action {
        let command = Command::Stop { regiment };
        Action::Order { player, command }
    }

    #[test]
    fn a_joined_match_plays_the_ticks_its_host_has_sent_as_they_fall_due() {
        let (mut host, mut orders, mut world) = joined(&[1]);
        // The host may wait to hear the game has the match set up.
        host.wait();
        assert!(host.waiting() && orders.waiting());
        orders.ready();
        host.wait();
        assert!(!host.waiting());
        // Until the match begins nothing is asked, as nothing would come
        // of it.
        assert!(orders.waiting() && !orders.may(&Action::Pause(true)));
        let stop = Command::Stop { regiment: 0 };
        assert_eq!(orders.give(&world, 1, stop), Err(Refused::NotYours));
        assert!(orders.joined_to());
        assert_eq!(orders.players(), Some(&[1][..]));
        // A tick the host hasn't sent isn't played.
        assert!(orders.step(&mut world).is_empty());
        assert_eq!(world.ticks, 0);
        host.turn();
        // Nor one that isn't due.
        orders.carry_out(&mut world);
        assert_eq!(world.ticks, 0);
        orders.step(&mut world);
        assert_eq!(world.ticks, 1);
        // Behind, it catches up a part at a time.
        for _ in 0..12 {
            host.turn();
        }
        let played = [(); 3].map(|_| {
            orders.step(&mut world);
            world.ticks
        });
        assert_eq!(played, [1 + CAUGHT_UP as u64, 1 + 2 * CAUGHT_UP as u64, 13]);
        assert_eq!(world.hash(), host.world().hash());
        // What it asks for is carried out when the host says, in both.
        assert_eq!(
            orders.give(&world, 1, Command::Stop { regiment: 0 }),
            Ok(())
        );
        orders.carry_out(&mut world);
        assert!(orders.record().given.is_empty());
        host.turn();
        orders.step(&mut world);
        assert_eq!(orders.record(), host.schedule().record());
        assert_eq!(orders.record().given.len(), 1);
        assert!(!orders.gone());
    }

    #[test]
    fn what_is_asked_of_a_host_elsewhere_is_coming_until_it_comes_back() {
        let (mut host, mut orders, mut world) = joined(&[1]);
        let (link, _) = testing::let_in(&mut host, &[2]);
        let (mut other, mut theirs) = (Client::new(link), testing::fresh());
        // What would come to nothing before the match begins isn't asked.
        orders.act(Action::Pause(true));
        assert!(!orders.pausing());
        host.turn();
        orders.step(&mut world);
        orders.act(Action::Pause(true));
        orders.act(Action::Speed(2.0));
        // However long the host takes to hear of it.
        for _ in 0..3 {
            orders.carry_out(&mut world);
            assert!(orders.pausing() && !orders.paused());
            assert_eq!((orders.speeding(), orders.speed()), (2.0, 1.0));
        }
        host.turn();
        // A match held still plays what it's given without a tick due.
        orders.carry_out(&mut world);
        assert!(orders.pausing() && orders.paused());
        assert_eq!((orders.speeding(), orders.speed()), (2.0, 2.0));
        // Another's asking the same isn't its own coming back: the host
        // hears only so much from a client in a turn.
        for _ in 0..64 {
            orders.act(Action::Speed(2.0));
        }
        orders.act(Action::Speed(0.5));
        other.ask(Action::Speed(0.5)).unwrap();
        other.ask(Action::Speed(8.0)).unwrap();
        host.turn();
        orders.carry_out(&mut world);
        assert_eq!((orders.speeding(), orders.speed()), (0.5, 8.0));
        host.turn();
        orders.carry_out(&mut world);
        assert_eq!((orders.speeding(), orders.speed()), (0.5, 0.5));
        orders.act(Action::Speed(2.0));
        host.turn();
        orders.carry_out(&mut world);
        // Having come back, it's no longer what's coming.
        other.ask(Action::Pause(false)).unwrap();
        other.ask(Action::Speed(4.0)).unwrap();
        host.turn();
        orders.carry_out(&mut world);
        assert!(!orders.pausing() && !orders.paused());
        assert_eq!((orders.speeding(), orders.speed()), (4.0, 4.0));
        // The tick the match went on with came with it.
        other.advance(&mut theirs, usize::MAX).unwrap();
        assert_eq!((world.ticks, world.hash()), (2, theirs.hash()));
    }

    #[test]
    fn only_what_a_host_elsewhere_would_carry_out_is_asked_of_it() {
        let (mut host, mut orders, mut world) = begun(&[1]);
        // Another player's orders, and what's outside the match's rules.
        let theirs = Command::Stop { regiment: 1 };
        assert_eq!(world.check(2, &theirs), Ok(()));
        assert_eq!(orders.give(&world, 2, theirs), Err(Refused::NotYours));
        for cheat in [Action::Win(1), Action::Lose(2), Action::Destroy(1)] {
            assert!(!orders.may(&cheat));
            orders.act(cheat);
        }
        assert!(orders.may(&stop(1, 0)) && orders.may(&Action::Pause(true)));
        host.turn();
        orders.step(&mut world);
        assert_eq!((world.ticks, orders.record().given.len()), (2, 0));
        // One only watching asks for nothing, and expects nothing.
        let (mut host, mut watching, mut world) = begun(&[]);
        watching.act(Action::Pause(true));
        assert!(!watching.pausing());
        assert_eq!(
            watching.give(&world, 1, Command::Stop { regiment: 0 }),
            Err(Refused::NotYours)
        );
        host.turn();
        watching.step(&mut world);
        assert_eq!((world.ticks, host.schedule().record().given.len()), (2, 0));
        // A host here does whatever it's asked.
        let hosting = Orders::hosting(Setup::default(), fresh());
        assert!(!hosting.joined_to() && hosting.players().is_none());
        assert!(hosting.may(&Action::Win(1)) && hosting.may(&stop(2, 1)));
    }

    #[test]
    fn a_host_that_goes_is_found_gone_once_what_it_sent_is_played() {
        let (mut host, mut orders, mut world) = begun(&[1]);
        for _ in 0..3 {
            host.turn();
        }
        let hash = host.world().hash();
        orders.act(Action::Speed(2.0));
        assert_eq!(orders.speeding(), 2.0);
        drop(host);
        orders.carry_out(&mut world);
        assert!(!orders.gone());
        orders.step(&mut world);
        assert_eq!((world.ticks, world.hash()), (4, hash));
        assert!(orders.gone());
        // Nothing asked is coming now, and nothing more is asked.
        assert_eq!(orders.speeding(), 1.0);
        orders.act(Action::Pause(true));
        assert!(!orders.pausing() && !orders.may(&Action::Pause(true)));
        assert_eq!(
            orders.give(&world, 1, Command::Stop { regiment: 0 }),
            Err(Refused::NotYours)
        );
        orders.step(&mut world);
        assert_eq!(world.ticks, 4);
    }

    #[test]
    fn a_joined_game_hears_who_leaves_and_that_the_host_ended_the_match() {
        let (mut host, mut orders, mut world) = joined(&[1]);
        let (link, _) = super::testing::let_in(&mut host, &[2]);
        let mut other = aaa_server::Client::new(link);
        host.turn();
        orders.step(&mut world);
        other.leave().unwrap();
        host.turn();
        orders.step(&mut world);
        assert_eq!(orders.take_left(), [(2, vec![2])]);
        assert_eq!(orders.take_left(), []);
        assert!(!orders.gone() && !orders.ended());
        host.finish();
        orders.step(&mut world);
        assert!(orders.gone() && orders.ended());
    }

    #[test]
    fn a_game_leaving_a_match_it_hosts_ends_it() {
        let mut orders = Orders::hosting(Setup::default(), super::testing::fresh());
        let Hosted::Here(host) = &mut orders.host else {
            unreachable!()
        };
        let (link, _) = super::testing::let_in(host, &[2]);
        let mut other = aaa_server::Client::new(link);
        drop(orders);
        let mut world = super::testing::fresh();
        assert_eq!(other.advance(&mut world, usize::MAX), Err(aaa_server::Lost));
        assert!(other.ended());
    }

    #[test]
    fn a_game_leaving_a_match_tells_its_host() {
        let (mut host, orders, _) = begun(&[1]);
        drop(orders);
        assert_eq!(host.turn(), [Event::Left(1)]);
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
        assert_eq!(orders.host().world().ticks, 25);
        assert_eq!(orders.host().world().hash(), world.hash());
        assert_eq!(orders.record(), orders.host().schedule().record());
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
