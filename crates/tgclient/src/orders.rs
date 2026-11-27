//! What the match is given. The game plays its copy of a match as a
//! client of the match's host: nothing changes the match but by asking
//! the host. The host is in the same program, with a copy of its own, or
//! elsewhere, when the game has joined its match.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, HashMap, VecDeque};

use bevy::prelude::*;
use tgserver::protocol::{Gone, KickVote, Said, Status, Welcome};
use tgserver::{Client, Event, Host, Link, Local, Setup};
use tgsim::{Action, Command, ComputerPlayer, Outcome, Record, Refused, SPEEDS, World};

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
    /// Whether the game has left the match, after which there's nothing
    /// more of it to play.
    quit: bool,
    /// While orders are gathered to go as one group, whose they are, the
    /// selection group they go to, and those given so far.
    gathered: Option<(u32, Option<u8>, Vec<Command>)>,
    /// Whether the orders given this frame go on the end of their
    /// regiments' queues, as when Shift or Ctrl is held for a click on the
    /// map.
    queuing: bool,
    /// Whether the game has taken that the host ended the match.
    ended_taken: bool,
    /// What the host knows the game's client as.
    me: u32,
    /// The clients found gone since the game last asked, but for the
    /// game's own, and whether that was kicked out.
    left: Vec<Gone>,
    kicked: bool,
    /// The players each client the host has said of gives the orders of,
    /// and what the person at it goes by, kept once it's gone, for what's
    /// said of it after.
    seen: HashMap<u32, (Vec<u32>, String)>,
    computers: Computers,
}

/// The computer players the game runs for a match shared between
/// machines: those its machine added to the game, whose orders it gives
/// as it does its person's.
#[derive(Default)]
struct Computers {
    /// Their sides, by player id.
    sides: Vec<u32>,
    profiles: tgserver::Profiles,
    /// Each side's, once it's first thought for.
    players: BTreeMap<u32, Box<dyn ComputerPlayer>>,
}

/// Where the match's host is.
enum Hosted {
    /// Here, doing whatever it's asked.
    Here(Box<Host<Local>>),
    /// Shared between machines, taking from here only the orders of these
    /// players, and of the rest only what it takes from anyone who plays.
    /// players: elsewhere, at `at`, or here, by `host`, which the game
    /// has hear from everyone as it plays.
    Shared {
        players: Vec<u32>,
        at: Option<String>,
        host: Option<Box<Host<Line>>>,
    },
}

/// `command` put on the end of its regiments' queues.
fn queued(command: Command) -> Command {
    Command::Queued {
        command: Box::new(command),
    }
}

#[cfg(test)]
impl Default for Orders {
    /// Those of no match at all, for tests of what only asks.
    fn default() -> Self {
        let pathing = tgmap::Pathing::uniform(1, tgmap::Domain::Land);
        let world = World::from_organizations(Default::default(), pathing, &[]);
        Orders::hosting(Setup::default(), world, None, "Player")
    }
}

impl Orders {
    /// Those of a match hosted here, which `setup` set up, `world` being
    /// the host's copy of it, for a person going by `name`.
    pub(crate) fn hosting(setup: Setup, world: World, person: Option<u32>, name: &str) -> Orders {
        // Theirs is the side the others hear has quit should they leave
        // early.
        let players = person.filter(|&p| world.sides.player(p).is_some());
        let mut host = Host::new(setup.clone(), world);
        let (ours, mut theirs) = tgserver::pair();
        // Whoever runs the host may give any side's orders, and cheat.
        let me = host.admit(ours, players.into_iter().collect(), name);
        let me = me.expect("a host lets in the first to ask");
        host.trust(me);
        // The game set the match up itself.
        let welcome = tgserver::welcome(&mut theirs);
        debug_assert!(matches!(welcome, Ok(Some(_))));
        Orders::of(
            Hosted::Here(Box::new(host)),
            Box::new(theirs),
            setup,
            me,
            Vec::new(),
        )
    }

    /// Those of a match whose host, at the other end of `link`, let the
    /// game in with `welcome`.
    pub(crate) fn joined(link: Line, welcome: Welcome, at: String) -> Orders {
        let (players, me) = (welcome.players, welcome.client);
        let host = Hosted::Shared {
            players,
            at: Some(at),
            host: None,
        };
        Orders::of(host, link, welcome.setup, me, welcome.computers)
    }

    /// Those of a match between machines that `host`, here, let the game
    /// in to at the other end of `link` with `welcome`; the host is given
    /// its own copy of the match once it's set up, with [`Orders::build`].
    pub(crate) fn sharing(host: Host<Line>, link: Line, welcome: Welcome) -> Orders {
        let (players, me) = (welcome.players, welcome.client);
        let host = Hosted::Shared {
            players,
            at: None,
            host: Some(Box::new(host)),
        };
        Orders::of(host, link, welcome.setup, me, welcome.computers)
    }

    /// Gives a host here of a match between machines its copy of the
    /// match, set up as the game's was.
    pub(crate) fn build(&mut self, world: World) {
        if let Hosted::Shared {
            host: Some(host), ..
        } = &mut self.host
        {
            host.build(world);
        }
    }

    /// Has the computer players the game runs, and those of a host here
    /// for the sides nobody else plays, play by `profiles`.
    pub(crate) fn computers_from(&mut self, profiles: tgserver::Profiles) {
        match &mut self.host {
            Hosted::Here(host) => host.computers_from(profiles.clone()),
            Hosted::Shared {
                host: Some(host), ..
            } => host.computers_from(profiles.clone()),
            Hosted::Shared { host: None, .. } => {}
        }
        self.computers.profiles = profiles;
    }

    fn of(host: Hosted, link: Line, setup: Setup, me: u32, computers: Vec<u32>) -> Orders {
        Orders {
            host,
            client: Client::new(link),
            setup,
            asked: VecDeque::new(),
            diverged: 0,
            gone: false,
            quit: false,
            gathered: None,
            queuing: false,
            ended_taken: false,
            me,
            left: Vec::new(),
            kicked: false,
            seen: HashMap::new(),
            computers: Computers {
                sides: computers,
                ..Default::default()
            },
        }
    }

    /// Whether the match is hosted here, for the game alone or for others
    /// too.
    pub(crate) fn hosts(&self) -> bool {
        match &self.host {
            Hosted::Here(_) => true,
            Hosted::Shared { host, .. } => host.is_some(),
        }
    }

    /// The players the game was let in to a multiplayer match to give the
    /// orders of; `None` for a match it plays alone, where it gives
    /// anyone's.
    pub(crate) fn players(&self) -> Option<&[u32]> {
        match &self.host {
            Hosted::Here(_) => None,
            Hosted::Shared { players, .. } => Some(players),
        }
    }

    /// Has the game's client tell the time by `clock`.
    #[cfg(test)]
    pub(crate) fn clock(&mut self, clock: tgserver::Clock) {
        self.client.clock(clock);
    }

    /// Where the match's host is, if it's elsewhere.
    pub(crate) fn host_at(&self) -> Option<&str> {
        match &self.host {
            Hosted::Here(_) => None,
            Hosted::Shared { at, .. } => at.as_deref(),
        }
    }

    /// How everyone in the match stood when the host last said.
    pub(crate) fn status(&self) -> Option<&Status> {
        self.client.status()
    }

    /// Whether the game is for dropping those holding the match, as the
    /// host last said: it clears every vote as it drops them.
    pub(crate) fn voted(&self) -> bool {
        let status = self.status().map(|s| s.seats.iter());
        let mut seats = status.into_iter().flatten();
        seats.any(|s| s.client == self.me && s.vote)
    }

    /// What the host knows the game's client as.
    pub(crate) fn me(&self) -> u32 {
        self.me
    }

    /// The players `client` gave the orders of and what the person at it
    /// went by, as the host first said of it, if it ever did.
    pub(crate) fn seen(&self, client: u32) -> Option<(&[u32], &str)> {
        let (players, name) = self.seen.get(&client)?;
        Some((players, name))
    }

    /// Whether the game is for kicking `client` out, as the host last
    /// said.
    pub(crate) fn kicking(&self, client: u32) -> bool {
        let status = self.status().map(|s| s.seats.iter());
        let mut seats = status.into_iter().flatten();
        seats.any(|s| s.client == self.me && s.kicking.contains(&client))
    }

    /// Tells the host whether the game is for kicking `client` out.
    pub(crate) fn kick(&mut self, client: u32, vote: bool) {
        // A host that's gone is found so as the match is next played.
        let _ = self.client.kick(client, vote);
    }

    /// The kick votes cast or taken back since this was last asked.
    pub(crate) fn take_kicking(&mut self) -> Vec<KickVote> {
        self.client.take_kicking()
    }

    /// Has the host pass `words` on to everyone at once, outside the
    /// steps, so that they get through while the match is held.
    pub(crate) fn say(&mut self, words: String) {
        // A host that's gone is found so as the match is next played.
        let _ = self.client.say(words);
    }

    /// What's been said for everyone since this was last asked.
    pub(crate) fn take_said(&mut self) -> Vec<Said> {
        self.client.take_said()
    }

    /// Whether the game has been kicked out of the match.
    pub(crate) fn kicked(&self) -> bool {
        self.kicked
    }

    /// Tells the host whether the game is for dropping those holding the
    /// match.
    pub(crate) fn vote(&mut self, drop: bool) {
        // A host that's gone is found so as the match is next played.
        let _ = self.client.vote(drop);
    }

    /// Whether the match is a multiplayer game, hosted here for others to
    /// join or joined elsewhere, and so isn't the game's to do as it likes
    /// with.
    pub(crate) fn multiplayer(&self) -> bool {
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

    /// Whether the host has said it's leaving the match now that it's
    /// over, the first time this is asked after it has.
    pub(crate) fn take_ended(&mut self) -> bool {
        let ended = self.client.ended() && !self.ended_taken;
        self.ended_taken |= ended;
        ended
    }

    /// The other clients of the match found gone since this was last
    /// asked.
    pub(crate) fn take_left(&mut self) -> Vec<Gone> {
        std::mem::take(&mut self.left)
    }

    /// Whether the host, being elsewhere, has yet to begin the match:
    /// it may hold it until every copy is set up, and carries out nothing
    /// asked of it before then.
    pub(crate) fn waiting(&self) -> bool {
        let record = self.record();
        self.multiplayer() && !self.gone && record.steps == 0 && record.given.is_empty()
    }

    /// Whether the host would carry `action` out if asked.
    pub(crate) fn may(&self, action: &Action) -> bool {
        !self.gone && !self.waiting() && self.allowed(action)
    }

    /// Whether the players given the orders of here may ask for `action`
    /// at all, as against just now.
    pub(crate) fn allowed(&self, action: &Action) -> bool {
        let allowed = |players| tgserver::allows(players, action);
        let computer = |player| self.computers.sides.contains(player);
        let computer = match action {
            Action::Order { player, .. } | Action::NameComputer { player, .. } => computer(player),
            _ => false,
        };
        computer || self.players().is_none_or(allowed)
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
        let command = match self.queuing && self.gathered.is_none() {
            true => queued(command),
            false => command,
        };
        world.check(player, &command)?;
        let action = Action::Order { player, command };
        if !self.may(&action) {
            return Err(Refused::NotYours);
        }
        match (&mut self.gathered, action) {
            (Some((_, _, gathered)), Action::Order { command, .. }) => gathered.push(command),
            (None, action) => self.act(action),
            (Some(_), _) => unreachable!("only orders are gathered"),
        }
        Ok(())
    }

    /// From now until [`Orders::send_gathered`], gathers the orders
    /// `player` gives rather than asking for each, to ask for them as one
    /// group, given to the `selection` group.
    pub(crate) fn gather(&mut self, player: u32, selection: Option<u8>) {
        self.gathered = Some((player, selection, Vec::new()));
    }

    /// Whether the orders given now are gathered for a group.
    pub(crate) fn gathering(&self) -> bool {
        self.gathered.is_some()
    }

    /// Asks for the orders gathered as one group, or for one alone as it
    /// is.
    pub(crate) fn send_gathered(&mut self) {
        let queuing = std::mem::take(&mut self.queuing);
        let Some((player, selection, mut orders)) = self.gathered.take() else {
            return;
        };
        let command = match orders.len() {
            0 => return,
            1 => orders.remove(0),
            _ => Command::Group { orders, selection },
        };
        let command = match queuing {
            true => queued(command),
            false => command,
        };
        self.act(Action::Order { player, command });
    }

    /// Has the orders given from now until they're sent this frame go on
    /// the end of their regiments' queues, when `queuing`.
    pub(crate) fn queue(&mut self, queuing: bool) {
        self.queuing = queuing;
    }

    /// Asks for `action`, unless the host is known not to carry it out.
    pub(crate) fn act(&mut self, action: Action) {
        if !self.may(&action) {
            debug!("not asked of the match's host: {action:?}");
            return;
        }
        let clock = matches!(
            action,
            Action::Pause(_) | Action::Speed(_) | Action::Skip(_)
        )
        .then(|| action.clone());
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

    /// Whether a scripted scene is being hurried on, once what's been
    /// asked is carried out.
    pub(crate) fn skipping(&self) -> bool {
        let asked = self.asked.iter().rev().find_map(|(_, a)| match a {
            Action::Skip(skip) => Some(*skip),
            _ => None,
        });
        asked.unwrap_or(self.client.schedule().skipping())
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
        if self.quit {
            return Vec::new();
        }
        let played = match &mut self.host {
            Hosted::Here(host) => {
                said(host.relay());
                self.client.advance(world, usize::MAX)
            }
            // The game's clock stands still with the match, so no tick
            // falls due to play the one the match goes on with.
            Hosted::Shared { host, .. } => {
                if let Some(host) = host.as_deref_mut().filter(|h| !h.waiting()) {
                    said(host.relay());
                }
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
        if self.quit {
            return Vec::new();
        }
        let played = match &mut self.host {
            Hosted::Here(host) => {
                said(host.tick());
                self.client.advance(world, usize::MAX)
            }
            Hosted::Shared { host, .. } => {
                match host.as_deref_mut() {
                    Some(host) if host.waiting() => said(host.wait()),
                    Some(host) => said(host.turn()),
                    None => {}
                }
                let ticks = self.client.behind().clamp(1, CAUGHT_UP);
                self.client.play(world, ticks)
            }
        };
        let refused = self.played(played.is_err());
        self.think(world);
        refused
    }

    /// Has the computer players the game runs think about `world`, the
    /// game's copy of the match, as a host does for its own: for sides
    /// still in it, while it goes on, asking the host for their orders.
    /// One made by a profile is first named after it, for every machine.
    fn think(&mut self, world: &World) {
        let held = self.status().is_some_and(|s| s.held);
        // Nor while a scripted scene plays.
        let film = world.triggers.cutscene;
        if self.computers.sides.is_empty() || held || film || self.paused() || self.waiting() {
            return;
        }
        let Computers {
            sides,
            profiles,
            players,
        } = &mut self.computers;
        let playing = |p: &u32| world.outcome(*p) == Outcome::Playing;
        let mut orders = Vec::new();
        for &player in sides.iter().filter(|p| playing(p)) {
            let computer = match players.entry(player) {
                Entry::Occupied(made) => made.into_mut(),
                Entry::Vacant(made) => {
                    if let Some(name) = profiles.names().get(&player).cloned() {
                        orders.push(Action::NameComputer { player, name });
                    }
                    made.insert(profiles.player(&self.setup, world, player))
                }
            };
            let order = |command| Action::Order { player, command };
            orders.extend(computer.think(world, player).into_iter().map(order));
        }
        for order in orders {
            self.act(order);
        }
    }

    /// Hears from a host elsewhere while the game's copy of the match is
    /// still being set up, so that it doesn't take the game for lost: how
    /// everyone stands, who's gone and what's said.
    pub(crate) fn wait(&mut self) {
        if self.quit || !self.multiplayer() {
            return;
        }
        if let Hosted::Shared {
            host: Some(host), ..
        } = &mut self.host
        {
            match host.waiting() {
                true => said(host.wait()),
                false => said(host.turn()),
            }
        }
        let lost = self.client.wait().is_err();
        // Nothing's carried out before then, so nothing is refused.
        self.played(lost);
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
        for seat in self.client.status().into_iter().flat_map(|s| &s.seats) {
            let seen = || (seat.players.clone(), seat.name.clone());
            self.seen.entry(seat.client).or_insert_with(seen);
            // A side resigned is taken from whoever played it.
            if let Hosted::Shared { players, .. } = &mut self.host
                && seat.client == self.me
                && *players != seat.players
            {
                players.clone_from(&seat.players);
            }
        }
        for gone in self.client.take_left() {
            match gone.client == self.me {
                true => self.kicked |= gone.kicked,
                false => self.left.push(gone),
            }
        }
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

impl Orders {
    /// Has the game leave the match, once: ending it, if it's the host.
    pub(crate) fn leave(&mut self) {
        if std::mem::replace(&mut self.quit, true) {
            return;
        }
        match &mut self.host {
            Hosted::Here(host) => host.end(),
            Hosted::Shared {
                host: Some(host), ..
            } => host.end(),
            // Failing only where the host has already gone.
            Hosted::Shared { host: None, .. } => _ = self.client.leave(),
        }
    }
}

impl Drop for Orders {
    fn drop(&mut self) {
        self.leave();
    }
}

/// Leaves the match as the game exits, rather than trusting what tears
/// the app down to drop it. The orders stay, as the app may run another
/// frame whose systems still ask for them.
pub(crate) fn leave_on_exit(mut exit: MessageReader<AppExit>, orders: Option<ResMut<Orders>>) {
    if exit.read().count() > 0
        && let Some(mut orders) = orders
    {
        orders.leave();
    }
}

/// Logs what a turn of a host here brought.
fn said(events: Vec<Event>) {
    for event in events {
        match event {
            Event::Held(true) => info!("the match is held"),
            Event::Held(false) => info!("the match goes on"),
            Event::Left(client) => info!("client {client} has left the match"),
            // With both copies here, only by the match not playing out
            // the same way twice.
            _ => warn!("the match's host says: {event:?}"),
        }
    }
}

/// Matches hosted as though elsewhere, for tests of what plays in one.
#[cfg(test)]
pub(crate) mod testing {
    use tgmap::{Domain, Formation, Member, Organization, Pathing};
    use tgsim::Sides;

    use super::*;

    /// A match of players 1 to 3, the first two each with an organization of
    /// one soldier: organization 0 the first's and organization 1 the second's.
    pub(crate) fn fresh() -> World {
        fresh_by(tgsim::Rules::default())
    }

    /// A [`fresh`] match played by `rules`.
    pub(crate) fn fresh_by(rules: tgsim::Rules) -> World {
        let organization = |i: u32| Organization {
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
                ethereal: false,
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
            morale: 1.0,
            experience: 0.0,
            efficiency: None,
            unpacks: None,
        };
        let sides = Sides {
            names: Default::default(),
            computers: Default::default(),
            players: [1, 2, 3].map(crate::tests::player).into(),
            teams: Vec::new(),
        };
        let pathing = Pathing::uniform(40, Domain::Land);
        let organizations = [organization(0), organization(1)];
        World::from_organizations_by(sides, pathing, rules, &organizations)
    }

    /// What a match was given but for the names its host gives sides as it
    /// begins.
    pub(crate) fn asked(record: &Record) -> Vec<Action> {
        let given = record.given.iter().map(|g| g.action.clone());
        given
            .filter(|a| !matches!(a, Action::Name { .. }))
            .collect()
    }

    /// Has `host` let in a client asking for `players`, going by
    /// "Player n" for the first of them or "Watcher" for none: its end of
    /// the link, and what it was let in with.
    pub(crate) fn let_in(host: &mut Host<Local>, players: &[u32]) -> (Local, Welcome) {
        let (ours, mut theirs) = tgserver::pair();
        let name = players
            .first()
            .map_or("Watcher".into(), |p| format!("Player {p}"));
        tgserver::join(&mut theirs, players.to_vec(), name).unwrap();
        host.greet(ours);
        assert!(matches!(host.seat()[..], [Ok(_)]));
        let welcome = tgserver::welcome(&mut theirs).unwrap().unwrap();
        (theirs, welcome)
    }

    /// The host of a [`fresh`] match, the orders of a game let in to it
    /// to play `players`, and the game's copy of the match.
    pub(crate) fn joined(players: &[u32]) -> (Host<Local>, Orders, World) {
        let mut host = Host::new(Setup::default(), fresh());
        let (link, welcome) = let_in(&mut host, players);
        (
            host,
            Orders::joined(Box::new(link), welcome, "host".into()),
            fresh(),
        )
    }
}

#[cfg(test)]
mod tests {
    use tgmap::{Domain, Pathing};

    use super::testing::joined;
    use super::*;

    fn fresh() -> World {
        World::from_organizations(Default::default(), Pathing::uniform(4, Domain::Land), &[])
    }

    impl Orders {
        fn host(&self) -> &Host<Local> {
            match &self.host {
                Hosted::Here(host) => host,
                Hosted::Shared { .. } => panic!("not a match hosted here alone"),
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

    fn stop(player: u32, organization: usize) -> Action {
        let command = Command::Stop { organization };
        Action::Order { player, command }
    }

    /// A computer player that sends the second side's organization along
    /// x, once.
    #[derive(Default)]
    struct Mover(bool);

    impl ComputerPlayer for Mover {
        fn think(&mut self, _: &World, _: u32) -> Vec<Command> {
            let moved = std::mem::replace(&mut self.0, true);
            let to = [30.0, 20.0];
            let order = Command::Move {
                organization: 1,
                to,
            };
            (!moved).then_some(order).into_iter().collect()
        }
    }

    #[test]
    fn a_joined_game_gives_the_orders_of_the_computer_players_it_runs() {
        let mut host = Host::new(Setup::default(), super::testing::fresh());
        let (ours, mut theirs) = tgserver::pair();
        host.admit_running(ours, vec![1], vec![2], "Bob").unwrap();
        let welcome = tgserver::welcome(&mut theirs).unwrap().unwrap();
        let mut orders = Orders::joined(Box::new(theirs), welcome, "host".into());
        orders.computers.players.insert(2, Box::<Mover>::default());
        let mut world = super::testing::fresh();
        for _ in 0..3 {
            host.turn();
            orders.step(&mut world);
        }
        let moved = Action::Order {
            player: 2,
            command: Command::Move {
                organization: 1,
                to: [30.0, 20.0],
            },
        };
        assert!(orders.may(&moved));
        assert_eq!(super::testing::asked(host.schedule().record()), [moved]);
        assert_eq!(world.hash(), host.world().hash());
    }

    #[test]
    fn a_joined_game_makes_the_computer_players_it_runs_by_their_profiles() {
        let mut host = Host::new(Setup::default(), super::testing::fresh());
        let (ours, mut theirs) = tgserver::pair();
        host.admit_running(ours, vec![1], vec![2], "Bob").unwrap();
        let welcome = tgserver::welcome(&mut theirs).unwrap().unwrap();
        let mut orders = Orders::joined(Box::new(theirs), welcome, "host".into());
        orders.computers_from(tgserver::Profiles::default());
        let mut world = super::testing::fresh();
        host.turn();
        orders.step(&mut world);
        // One for the side its machine runs, and none for its person's.
        let made: Vec<_> = orders.computers.players.keys().copied().collect();
        assert_eq!(made, [2]);
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
        let stop = Command::Stop { organization: 0 };
        assert_eq!(orders.give(&world, 1, stop), Err(Refused::NotYours));
        assert!(orders.multiplayer());
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
            orders.give(&world, 1, Command::Stop { organization: 0 }),
            Ok(())
        );
        orders.carry_out(&mut world);
        assert!(testing::asked(orders.record()).is_empty());
        host.turn();
        orders.step(&mut world);
        assert_eq!(orders.record(), host.schedule().record());
        assert_eq!(testing::asked(orders.record()).len(), 1);
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
        // Nor is what its host wouldn't carry out for it.
        orders.act(Action::Speed(2.0));
        // However long the host takes to hear of it.
        for _ in 0..3 {
            orders.carry_out(&mut world);
            assert!(orders.pausing() && !orders.paused());
            assert_eq!(orders.speeding(), 1.0);
        }
        host.turn();
        // A match held still plays what it's given without a tick due.
        orders.carry_out(&mut world);
        assert!(orders.pausing() && orders.paused());
        // Another's asking the same isn't its own coming back: the host
        // hears only so much from a client in a turn.
        for _ in 0..32 {
            orders.act(Action::Pause(false));
            orders.act(Action::Pause(true));
        }
        orders.act(Action::Pause(false));
        other.ask(Action::Pause(false)).unwrap();
        other.ask(Action::Pause(true)).unwrap();
        host.turn();
        orders.carry_out(&mut world);
        assert_eq!((orders.pausing(), orders.paused()), (false, true));
        host.turn();
        orders.carry_out(&mut world);
        assert_eq!((orders.pausing(), orders.paused()), (false, false));
        // Having come back, it's no longer what's coming.
        other.ask(Action::Pause(true)).unwrap();
        host.turn();
        orders.carry_out(&mut world);
        assert!(orders.pausing() && orders.paused());
        // The tick the match went on with came with it.
        other.advance(&mut theirs, usize::MAX).unwrap();
        assert_eq!((world.ticks, world.hash()), (2, theirs.hash()));
    }

    #[test]
    fn only_what_a_host_elsewhere_would_carry_out_is_asked_of_it() {
        let (mut host, mut orders, mut world) = begun(&[1]);
        // Another player's orders, and what's outside the match's rules.
        let theirs = Command::Stop { organization: 1 };
        assert_eq!(world.check(2, &theirs), Ok(()));
        assert_eq!(orders.give(&world, 2, theirs), Err(Refused::NotYours));
        for cheat in [Action::Win(1), Action::Lose(2), Action::Destroy(1)] {
            assert!(!orders.may(&cheat));
            orders.act(cheat);
        }
        assert!(orders.may(&stop(1, 0)) && orders.may(&Action::Pause(true)));
        host.turn();
        orders.step(&mut world);
        assert_eq!((world.ticks, testing::asked(orders.record()).len()), (2, 0));
        // One only watching asks for nothing, and expects nothing.
        let (mut host, mut watching, mut world) = begun(&[]);
        watching.act(Action::Pause(true));
        assert!(!watching.pausing());
        assert_eq!(
            watching.give(&world, 1, Command::Stop { organization: 0 }),
            Err(Refused::NotYours)
        );
        host.turn();
        watching.step(&mut world);
        assert_eq!((world.ticks, host.schedule().record().given.len()), (2, 0));
        // A host here does whatever it's asked.
        let hosting = Orders::hosting(Setup::default(), fresh(), None, "Player");
        assert!(!hosting.multiplayer() && hosting.players().is_none());
        assert!(hosting.may(&Action::Win(1)) && hosting.may(&stop(2, 1)));
    }

    #[test]
    fn a_match_hosted_here_for_others_to_join_is_multiplayer() {
        let mut host: Host<Line> = Host::new(Setup::default(), super::testing::fresh());
        let (ours, mut theirs) = tgserver::pair();
        tgserver::join(&mut theirs, vec![1], "Player 1".into()).unwrap();
        host.greet(Box::new(ours));
        assert!(matches!(host.seat()[..], [Ok(_)]));
        let welcome = tgserver::welcome(&mut theirs).unwrap().unwrap();
        let orders = Orders::sharing(host, Box::new(theirs), welcome);
        assert!(orders.multiplayer());
    }

    #[test]
    fn a_host_that_goes_is_found_gone_once_what_it_sent_is_played() {
        let (mut host, mut orders, mut world) = begun(&[1]);
        for _ in 0..3 {
            host.turn();
        }
        let hash = host.world().hash();
        orders.act(Action::Pause(true));
        assert!(orders.pausing());
        drop(host);
        orders.carry_out(&mut world);
        assert!(!orders.gone());
        orders.step(&mut world);
        assert_eq!((world.ticks, world.hash()), (4, hash));
        assert!(orders.gone());
        // Nothing asked is coming now, and nothing more is asked.
        assert!(!orders.pausing());
        orders.act(Action::Pause(true));
        assert!(!orders.pausing() && !orders.may(&Action::Pause(true)));
        assert_eq!(
            orders.give(&world, 1, Command::Stop { organization: 0 }),
            Err(Refused::NotYours)
        );
        orders.step(&mut world);
        assert_eq!(world.ticks, 4);
    }

    #[test]
    fn a_joined_game_hears_who_leaves_and_that_the_host_ended_the_match() {
        let (mut host, mut orders, mut world) = joined(&[1]);
        let (link, _) = super::testing::let_in(&mut host, &[2]);
        let mut other = tgserver::Client::new(link);
        let (link, judge) = super::testing::let_in(&mut host, &[]);
        let mut judge_client = tgserver::Client::new(link);
        host.trust(judge.client);
        host.turn();
        orders.step(&mut world);
        other.leave().unwrap();
        host.turn();
        orders.step(&mut world);
        let left: Vec<_> = orders.take_left().into_iter().map(|g| g.client).collect();
        assert_eq!(left, [2]);
        assert_eq!(orders.take_left(), []);
        assert!(!orders.gone() && !orders.take_ended());
        // The host leaves the match once it's over.
        judge_client.ask(Action::Win(1)).unwrap();
        host.turn();
        assert!(host.world().over);
        host.finish();
        for _ in 0..3 {
            orders.step(&mut world);
        }
        assert!(orders.take_ended());
        assert!(!orders.take_ended());
    }

    #[test]
    fn a_joined_game_hears_it_was_kicked_out() {
        let (mut host, mut orders, mut world) = joined(&[1]);
        let mut others = [2, 3].map(|p| {
            let (link, _) = super::testing::let_in(&mut host, &[p]);
            tgserver::Client::new(link)
        });
        host.turn();
        orders.step(&mut world);
        for other in &mut others {
            other.kick(orders.me(), true).unwrap();
        }
        host.turn();
        for _ in 0..2 {
            orders.step(&mut world);
        }
        assert!(orders.gone() && orders.kicked());
        // Its own going isn't another's.
        assert_eq!(orders.take_left(), []);
        let votes: Vec<_> = orders.take_kicking().iter().map(|v| v.votes).collect();
        assert_eq!(votes, [1, 2]);
    }

    #[test]
    fn a_game_leaving_a_match_it_hosts_ends_it() {
        let mut orders = Orders::hosting(Setup::default(), super::testing::fresh(), None, "Player");
        let Hosted::Here(host) = &mut orders.host else {
            unreachable!()
        };
        let (link, _) = super::testing::let_in(host, &[2]);
        let mut other = tgserver::Client::new(link);
        let mut ours = super::testing::fresh();
        orders.act(Action::Win(1));
        orders.carry_out(&mut ours);
        assert!(ours.over);
        drop(orders);
        let mut world = super::testing::fresh();
        assert_eq!(other.advance(&mut world, usize::MAX), Err(tgserver::Lost));
        assert!(other.ended());
    }

    #[test]
    fn a_game_exiting_leaves_the_match_but_keeps_its_orders() {
        let mut orders = Orders::hosting(Setup::default(), super::testing::fresh(), None, "Player");
        let Hosted::Here(host) = &mut orders.host else {
            unreachable!()
        };
        let (link, _) = super::testing::let_in(host, &[2]);
        let mut other = tgserver::Client::new(link);
        let mut app = App::new();
        app.add_message::<AppExit>()
            .insert_resource(orders)
            .add_systems(Last, leave_on_exit);
        app.world_mut().write_message(AppExit::Success);
        app.update();
        let mut world = super::testing::fresh();
        assert_eq!(other.advance(&mut world, usize::MAX), Err(tgserver::Lost));
        // What runs in the frames before the app stops finds the orders,
        // with nothing more to play.
        let mut orders = app.world_mut().resource_mut::<Orders>();
        let mut ours = super::testing::fresh();
        orders.act(Action::Win(1));
        orders.carry_out(&mut ours);
        orders.step(&mut ours);
        assert!(!ours.over);
    }

    #[test]
    fn a_game_leaving_a_match_it_hosts_early_is_heard_to_quit() {
        let mut orders =
            Orders::hosting(Setup::default(), super::testing::fresh(), Some(1), "Player");
        let Hosted::Here(host) = &mut orders.host else {
            unreachable!()
        };
        let (link, _) = super::testing::let_in(host, &[2]);
        let mut other = tgserver::Client::new(link);
        drop(orders);
        let mut world = super::testing::fresh();
        assert_eq!(other.advance(&mut world, usize::MAX), Err(tgserver::Lost));
        assert!(!other.ended());
        let left = other.take_left();
        assert_eq!((left[0].client, &left[0].players[..]), (1, &[1][..]));
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
        let mut orders = Orders::hosting(Setup::default(), world.clone(), None, "Player");
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
        let mut orders = Orders::hosting(Setup::default(), world.clone(), None, "Player");
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
        let stop = Command::Stop { organization: 0 };
        orders.act(Action::Order {
            player: 1,
            command: stop.clone(),
        });
        assert_eq!(orders.carry_out(&mut world), [(1, Refused::NotYours)]);
        // And one the game's copy would refuse isn't asked for.
        assert_eq!(orders.give(&world, 1, stop), Err(Refused::NotYours));
        assert_eq!(orders.record().given.len(), 2);
    }

    #[test]
    fn orders_given_while_queuing_go_queued_until_sent() {
        let mut world = super::testing::fresh();
        let mut orders = Orders::hosting(Setup::default(), world.clone(), None, "Player");
        let to = |at| Command::Move {
            organization: 0,
            to: at,
        };
        let sent = |orders: &Orders| {
            let asked = super::testing::asked(orders.record());
            match asked.last() {
                Some(Action::Order { command, .. }) => command.clone(),
                other => panic!("{other:?}"),
            }
        };
        orders.queue(true);
        orders.give(&world, 1, to([2.0, 2.0])).unwrap();
        orders.carry_out(&mut world);
        assert_eq!(sent(&orders), queued(to([2.0, 2.0])));
        // Gathered, the group goes queued as a whole.
        orders.gather(1, Some(2));
        orders.give(&world, 1, to([3.0, 3.0])).unwrap();
        orders.give(&world, 1, to([3.0, 1.0])).unwrap();
        orders.send_gathered();
        orders.carry_out(&mut world);
        let orders_given = vec![to([3.0, 3.0]), to([3.0, 1.0])];
        let group = Command::Group {
            orders: orders_given,
            selection: Some(2),
        };
        assert_eq!(sent(&orders), queued(group));
        // Once sent, what's given next isn't.
        orders.give(&world, 1, to([1.0, 1.0])).unwrap();
        orders.carry_out(&mut world);
        assert_eq!(sent(&orders), to([1.0, 1.0]));
    }
}
