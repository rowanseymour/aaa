//! The host of a match: the one copy that decides what the match is
//! given and when, and tells every client.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tgsim::{Action, ComputerPlayer, Outcome, Record, Schedule, World};

use crate::pausing::{Pauses, Person};
use crate::protocol::{
    Bundle, Gone, Join, KickVote, PROTOCOL, Said, Seated, Standing, Status, ToClient, ToHost,
    Welcome, decode, encode,
};
use crate::{Clock, Link, Naming, Setup, majority, ping};

/// Makes the computer player for a side of the match set up as given, by
/// the side's player id.
type MakeComputer = dyn Fn(&Setup, &World, u32) -> Box<dyn ComputerPlayer> + Send + Sync;

/// What makes the computer player taking a side over mid-match, with the
/// name of the profile it plays by, where it has one.
type MakeTaker = dyn Fn(&Setup, u32) -> (Box<dyn ComputerPlayer>, Option<String>) + Send + Sync;

/// The most messages taken from a client in one turn, so that one sending
/// without end holds nobody else up and puts only so much in a turn; the
/// rest wait for the next.
const HEARD_A_TURN: usize = 64;

/// The most clients waiting to say what they want that it keeps links
/// to.
pub(crate) const GREETED: usize = 32;

/// The longest message taken from a client, in bytes, which is well over
/// what anything it has to say comes to. A longer one is taken as
/// nonsense, so that what every copy of the match keeps of it is small.
pub(crate) const LONGEST_HEARD: usize = 1024;

/// How long a client playing the match goes unheard from before it holds
/// the match.
const UNHEARD: Duration = Duration::from_secs(2);

/// The longest a client goes without hearing anything, so that it knows
/// the host is there.
const QUIET: Duration = Duration::from_millis(100);

/// How often every client is told how everyone stands.
const TOLD: Duration = Duration::from_secs(1);

/// How long each client may hold the match before it can be dropped. It
/// drains while the client holds it and refills by a tenth as fast while
/// it doesn't, so one that keeps holding it gets less grace each time.
const ALLOWANCE: Duration = Duration::from_secs(15);
const REFILLED: u32 = 10;

/// What's left of an allowance when it counts as run out.
const RUN_OUT: Duration = Duration::from_millis(100);

/// A match and the clients playing and watching it.
pub struct Host<L> {
    setup: Setup,
    world: World,
    schedule: Schedule,
    clients: Vec<Seat<L>>,
    /// Links to those that have yet to ask to be let in.
    greeted: Vec<L>,
    /// What the next client let in is known as.
    next: u32,
    /// How many clients it's to have before the match begins, and the
    /// most it lets in, if that's been said.
    wanted: Option<usize>,
    /// Whether it has yet to have had them all at once. One that leaves
    /// before then is waited on to be replaced; one that leaves after
    /// isn't.
    gathering: bool,
    /// Whether its copy of the match is set up, which it has to be before
    /// the match begins.
    built: bool,
    begun: bool,
    clock: Clock,
    /// The time at this turn, and at the last that drained or refilled
    /// the allowances.
    now: Instant,
    counted: Option<Instant>,
    /// When the clients were last told how everyone stands.
    told: Option<Instant>,
    /// Whether the match is held for everyone, and the actions asked while
    /// it is, which wait for it to go on, each with the client that asked,
    /// if one did.
    held: bool,
    put_off: Vec<(Option<u32>, Action)>,
    /// Whether any client's standing holds the match, rather than it
    /// waiting only on the rest catching up.
    holding: bool,
    /// How many ticks it's had every copy of the match play.
    issued: u64,
    /// Whether anyone has left since the kick votes were last counted,
    /// which can bring the count needed down to the votes already cast.
    parted: bool,
    naming: Naming,
    /// The sides of those who left once all had been let in but before the
    /// match began, by whom: they're taken over as it begins.
    departed: Vec<(u32, String)>,
    /// What it has every copy of the match carry out next, whether or not
    /// the match is held: computer players taking sides over, which
    /// shouldn't wait on a hold that may be long to end.
    taking_over: Vec<Action>,
    /// Who has paused the match and who's for unpausing it, where the match
    /// is played between machines and so pausing has rules.
    pauses: Option<Pauses>,
    computer: Box<MakeComputer>,
    taker: Box<MakeTaker>,
    /// What the profile each computer player at launch plays by goes by,
    /// by player id, where it's known.
    profile_names: BTreeMap<u32, String>,
    /// The computer player of each side still in the match that no client
    /// plays, by player id, once the match has begun.
    computers: BTreeMap<u32, Box<dyn ComputerPlayer>>,
}

/// A client as the host has it.
struct Seat<L> {
    id: u32,
    link: L,
    players: Vec<u32>,
    /// The sides whose computer players its machine runs, giving their
    /// orders as it does its person's.
    computers: Vec<u32>,
    /// What the person at it goes by, unlike anyone else in the match.
    name: String,
    /// Whether it's let do anything.
    trusted: bool,
    diverged: bool,
    lost: bool,
    /// Whether it's said its copy of the match is set up.
    ready: bool,
    /// How many actions it's asked for, and how many of those it's been
    /// told are dealt with.
    asked: u64,
    answered: u64,
    /// When it was last heard from, if it has been since it was let in,
    /// and when it was last sent anything.
    heard: Option<Instant>,
    sent: Instant,
    /// How many ticks it's said its copy has played.
    played: u64,
    /// How much longer it may hold the match before it can be dropped.
    allowance: Duration,
    /// Whether it's for dropping those holding the match.
    vote: bool,
    /// How many it had asked for before the first of its actions put off
    /// while the match is held.
    put_off_from: Option<u64>,
    /// The clients it's for kicking out.
    kicking: Vec<u32>,
    /// Whether it's been kicked out, and is to be told so as it goes.
    kicked: bool,
}

/// What a turn hears from the clients.
#[derive(Default)]
struct Heard {
    /// The actions asked that the askers may have carried out, each with
    /// who asked.
    actions: Vec<(Option<u32>, Action)>,
    /// Each client's first action taken, by how many it had asked for
    /// before it.
    first_taken: Vec<(u32, u64)>,
    /// Whether any vote for dropping those holding the match changed.
    votes_changed: bool,
    /// Each kick vote cast or taken back: by whom, against whom, and which.
    kicks: Vec<(u32, u32, bool)>,
    /// What's said for everyone, to pass on at once.
    said: Vec<Said>,
    /// The clients whose copies stopped standing as the host's does, with
    /// the step each was found at.
    diverged: Vec<(u32, u64)>,
}

/// Why a client wasn't let in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Refusal {
    /// The match has begun, and what it's been given so far isn't passed
    /// on.
    Begun,
    /// The match hasn't this player.
    NoPlayer(u32),
    /// Another client gives this player's orders.
    Taken(u32),
    /// Its link was lost, or it asked with what isn't a message.
    Lost,
    /// It speaks another version of the protocol than the host's, which
    /// is this one.
    Protocol(u32),
    /// The host has as many clients, or as many waiting to ask, as it
    /// will have.
    Full,
    /// It asked without the game's password.
    Password,
    /// It was put out of the game being set up.
    Kicked,
}

/// Tells whoever is at the other end of `link` they're not let in, and
/// why.
pub fn refuse(link: &mut impl Link, refusal: Refusal) {
    let _ = link.send(encode(&ToClient::Refused(refusal)));
}

/// Keeps `link` among those `greeted` that have yet to ask to come in,
/// unless there are as many as are kept, when it's refused.
pub(crate) fn greet<L: Link>(greeted: &mut Vec<L>, mut link: L) {
    match greeted.len() < GREETED {
        true => greeted.push(link),
        false => refuse(&mut link, Refusal::Full),
    }
}

/// What `message`, the first from someone greeted, asks to come in as, if
/// it's a join this host can take.
pub(crate) fn joining(message: &[u8]) -> Result<Join, Refusal> {
    if message.len() > LONGEST_HEARD {
        return Err(Refusal::Lost);
    }
    match decode::<Join>(message) {
        Some(join) if join.protocol == PROTOCOL => Ok(join),
        Some(_) => Err(Refusal::Protocol(PROTOCOL)),
        None => Err(Refusal::Lost),
    }
}

/// Something a turn brought that those running the host may want to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A client left, its link was lost, or it sent what isn't a
    /// message.
    Left(u32),
    /// A client's copy of the match stopped standing as the host's does.
    Diverged { client: u32, step: u64 },
    /// The match is held for everyone, or let go.
    Held(bool),
}

/// Whether `words` are fit to pass on as chat: no longer than a chat
/// line holds, and nothing but what can be typed.
pub fn sayable(words: &str) -> bool {
    words.chars().count() <= tgsim::CHAT_LONGEST && !words.chars().any(char::is_control)
}

/// Whether `words` are worth passing on as chat: [sayable](sayable), and
/// not blank.
pub(crate) fn worth_saying(words: &str) -> bool {
    sayable(words) && !words.trim().is_empty()
}

/// Whether a host carries out `action` for a client it doesn't trust
/// that gives the orders of `players`.
pub fn allows(players: &[u32], action: &Action) -> bool {
    match action {
        Action::Order { player, .. } => players.contains(player),
        Action::Pause(_) => !players.is_empty(),
        Action::Resign(player) => players.contains(player),
        // Chat is from the client's own side, or from none if it only
        // watches.
        Action::Chat { from, words, .. } => {
            let speaker = match from {
                Some(player) => players.contains(player),
                None => players.is_empty(),
            };
            speaker && sayable(words)
        }
        // The speed is the host's to set, and names and what pausing came
        // to its to give.
        Action::Speed(_)
        | Action::Win(_)
        | Action::Lose(_)
        | Action::Raise { .. }
        | Action::Destroy(_)
        | Action::Name { .. }
        | Action::TakeOver { .. }
        | Action::Pausing(_) => false,
    }
}

impl<L> Seat<L> {
    /// Whether the host carries out `action` for it.
    fn may(&self, action: &Action) -> bool {
        let computer =
            matches!(action, Action::Order { player, .. } if self.computers.contains(player));
        self.trusted || computer || allows(&self.players, action)
    }

    /// Whether it gives side `player`'s orders, for its person or a
    /// computer player.
    fn plays(&self, player: u32) -> bool {
        self.players.contains(&player) || self.computers.contains(&player)
    }

    /// Whether it's staying with a side, and so has a say in kicking out.
    fn sided(&self) -> bool {
        !self.lost && !self.players.is_empty()
    }
}

impl<L: Link> Host<L> {
    /// A host of the match `world`, which `setup` set up and which has
    /// yet to be played.
    pub fn new(setup: Setup, world: World) -> Host<L> {
        let clock = Clock::default();
        Host {
            now: clock.now(),
            clock,
            counted: None,
            told: None,
            held: false,
            put_off: Vec::new(),
            holding: false,
            issued: 0,
            parted: false,
            setup,
            world,
            schedule: Schedule::default(),
            clients: Vec::new(),
            greeted: Vec::new(),
            next: 1,
            wanted: None,
            gathering: false,
            built: true,
            begun: false,
            naming: Naming::default(),
            departed: Vec::new(),
            taking_over: Vec::new(),
            pauses: None,
            computer: Box::new(|_, _, _| Box::new(tgsim::Idle)),
            taker: Box::new(|_, _| (Box::new(tgsim::Idle), None)),
            profile_names: BTreeMap::new(),
            computers: BTreeMap::new(),
        }
    }

    /// A host of the match `setup` sets up, between the sides `sides`,
    /// before its own copy is: it lets clients in and hears from them, but
    /// waits for [`Host::build`] before the match begins.
    pub fn awaiting(setup: Setup, sides: tgsim::Sides) -> Host<L> {
        let pathing = tgmap::Pathing::uniform(1, tgmap::Domain::Land);
        let mut host = Host::new(setup, World::from_organizations(sides, pathing, &[]));
        host.built = false;
        host
    }

    /// Gives it its copy of the match, which `setup` set up and which has
    /// yet to be played, for one made [awaiting](Host::awaiting) it.
    pub fn build(&mut self, world: World) {
        self.world = world;
        self.built = true;
    }

    /// Has it play the match as one between machines, where only a person
    /// with a side pauses it, and others unpause it only by vote.
    pub fn multiplayer(&mut self) {
        self.pauses.get_or_insert_default();
    }

    /// Has it play the match `speed` times as fast from its next turn on,
    /// as only whoever hosts may have it.
    pub fn speed(&mut self, speed: f32) {
        self.put_off.push((None, Action::Speed(speed)));
    }

    /// Has it name those in the match by `naming`, rather than by
    /// [`Naming::default`].
    pub fn name_by(&mut self, naming: Naming) {
        self.naming = naming;
    }

    /// Has computer players that `computer` makes play the sides nobody
    /// else does, rather than ones that give no orders. It's given how the
    /// match was set up, the match as the side first needs one, and the
    /// side's player id.
    pub fn computers_by(
        &mut self,
        computer: impl Fn(&Setup, &World, u32) -> Box<dyn ComputerPlayer> + Send + Sync + 'static,
    ) {
        self.computer = Box::new(computer);
    }

    /// Has computer players playing by `profiles` play the sides nobody
    /// else does.
    pub fn computers_from(&mut self, profiles: crate::Profiles) {
        self.profile_names.clone_from(profiles.names());
        let takers = profiles.clone();
        self.takers_by(move |setup, player| takers.taker(setup, player));
        self.computers_by(move |setup, world, player| profiles.player(setup, world, player));
    }

    /// Has computer players that `taker` makes take sides over mid-match,
    /// rather than ones that give no orders. It's given how the match was
    /// set up and the side's player id, and gives the name of the profile
    /// the one it makes plays by, if there is one.
    pub fn takers_by(
        &mut self,
        taker: impl Fn(&Setup, u32) -> (Box<dyn ComputerPlayer>, Option<String>) + Send + Sync + 'static,
    ) {
        self.taker = Box::new(taker);
    }

    /// Has it tell the time by `clock`.
    pub fn clock(&mut self, clock: Clock) {
        self.now = clock.now();
        self.clock = clock;
    }

    /// Whether the match is held for everyone, waiting on those whose
    /// standing holds it or then on the rest catching up.
    pub fn held(&self) -> bool {
        self.held
    }

    /// Has it let in `clients` clients and no more, and be
    /// [`Host::waiting`] until it has them.
    pub fn want(&mut self, clients: usize) {
        self.wanted = Some(clients);
        self.gathering = self.clients.len() < clients;
    }

    /// Whether those running it should hold off beginning the match:
    /// until it has had as many clients as it [wants](Host::want), and
    /// while any of those left has yet to say, as far as it's heard, that
    /// its copy of the match is set up. A match begun before then leaves
    /// those clients behind from the start.
    pub fn waiting(&self) -> bool {
        !self.built || self.gathering || self.clients.iter().any(|c| !c.ready)
    }

    /// Has it carry out whatever `client` asks from here on: any player's
    /// orders, and what changes a match outside its rules, such as
    /// [`Action::Win`]. For the client of whoever runs the host.
    pub fn trust(&mut self, client: u32) {
        for seat in &mut self.clients {
            seat.trusted |= seat.id == client;
        }
    }

    /// What the match was set up from.
    pub fn setup(&self) -> &Setup {
        &self.setup
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    /// How many clients it has.
    pub fn clients(&self) -> usize {
        self.clients.len()
    }

    /// Leaves the match, letting its clients go once they've had what it
    /// sent: that the clients it [trusts](Host::trust), those of whoever
    /// runs it, have left, as any other that quits, after first, of a
    /// match that's [over](World::over), that it's ended.
    pub fn end(&mut self) {
        if self.world.over {
            self.tell(&ToClient::Ended);
        }
        for seat in &mut self.clients {
            seat.lost |= seat.trusted;
        }
        self.part();
        self.clients.clear();
    }

    /// [Ends](Host::end) its part in the match, leaving its copy as it stands and the
    /// record of what it was given.
    pub fn finish(mut self) -> (World, Record) {
        self.end();
        (self.world, self.schedule.record().clone())
    }

    /// Lets in a client at the other end of `link` to give the orders of
    /// `players`, or only to watch if there are none, for a person going
    /// by `name`, numbered if anyone in the match does already. Returns
    /// what it's known as; one that isn't let in is told why.
    pub fn admit(&mut self, link: L, players: Vec<u32>, name: &str) -> Result<u32, Refusal> {
        self.admit_running(link, players, Vec::new(), name)
    }

    /// As [`Host::admit`], the client's machine also running the computer
    /// players of `computers`, which the host then leaves to it until it
    /// goes.
    pub fn admit_running(
        &mut self,
        mut link: L,
        mut players: Vec<u32>,
        mut computers: Vec<u32>,
        name: &str,
    ) -> Result<u32, Refusal> {
        players.sort_unstable();
        players.dedup();
        computers.sort_unstable();
        computers.dedup();
        computers.retain(|p| !players.contains(p));
        let taken = |name: &str| self.client_named(name);
        let name = crate::name(name).map(|n| crate::unique(&n, &self.naming.numbered, taken));
        // Its name is what it asked with, so it didn't ask with a message.
        let refused = (self.refuses(&players, &computers))
            .or_else(|| name.is_none().then_some(Refusal::Lost))
            .or_else(|| {
                let welcome = Welcome {
                    client: self.next,
                    setup: self.setup.clone(),
                    players: players.clone(),
                    computers: computers.clone(),
                    name: name.clone().unwrap_or_default(),
                };
                let sent = link.send(encode(&ToClient::Start(welcome)));
                sent.err().map(|_| Refusal::Lost)
            });
        if let Some(refusal) = refused {
            refuse(&mut link, refusal);
            return Err(refusal);
        }
        let id = self.next;
        self.next += 1;
        let name = name.expect("a client is let in with a name");
        self.clients.push(Seat {
            id,
            link,
            players,
            computers,
            name,
            trusted: false,
            diverged: false,
            lost: false,
            ready: false,
            asked: 0,
            answered: 0,
            heard: None,
            sent: self.now,
            played: 0,
            allowance: ALLOWANCE,
            vote: false,
            put_off_from: None,
            kicking: Vec::new(),
            kicked: false,
        });
        self.gathering &= self.wanted.is_some_and(|w| self.clients.len() < w);
        Ok(id)
    }

    /// Why a client wouldn't be let in to give the orders of `players`
    /// and run the computer players of `computers`, if it wouldn't.
    fn refuses(&self, players: &[u32], computers: &[u32]) -> Option<Refusal> {
        if self.begun {
            return Some(Refusal::Begun);
        }
        if self.wanted.is_some() && !self.gathering {
            return Some(Refusal::Full);
        }
        players.iter().chain(computers).find_map(|&p| {
            if self.world.sides.player(p).is_none() {
                return Some(Refusal::NoPlayer(p));
            }
            let taken = self.clients.iter().any(|c| c.plays(p));
            taken.then_some(Refusal::Taken(p))
        })
    }

    /// Takes a link to a client that has yet to say what it wants, which
    /// [`Host::seat`] lets in once it has.
    pub fn greet(&mut self, link: L) {
        greet(&mut self.greeted, link);
    }

    /// Lets in the clients greeted that have since asked to join, each as
    /// [`Host::admit`] would, returning what became of each that asked.
    pub fn seat(&mut self) -> Vec<Result<u32, Refusal>> {
        let mut seated = Vec::new();
        for mut link in std::mem::take(&mut self.greeted) {
            let asked = match link.receive() {
                Ok(None) if !self.begun => {
                    self.greeted.push(link);
                    continue;
                }
                // Too late to ask, as it's told on not being let in.
                Ok(None) => Ok((Vec::new(), String::new())),
                Ok(Some(message)) => joining(&message).map(|j| (j.players, j.name)),
                Err(_) => Err(Refusal::Lost),
            };
            seated.push(match asked {
                Ok((players, name)) => self.admit(link, players, &name),
                Err(refusal) => {
                    refuse(&mut link, refusal);
                    Err(refusal)
                }
            });
        }
        seated
    }

    /// Hears from the clients let in before the match has begun, without
    /// beginning it, as a turn hears them: which are ready, who's gone,
    /// who's for dropping or kicking whom, and what's said, everyone being
    /// told how they all stand. One that goes quiet holds the match, and
    /// can be dropped, as in play. What's asked of the match before it
    /// begins is dropped, as the client is told.
    pub fn wait(&mut self) -> Vec<Event> {
        self.go(None, false)
    }

    /// Lets go of the clients that have gone, telling those left of each.
    fn part(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        // Telling those left may find more gone.
        while self.clients.iter().any(|c| c.lost) {
            let lost: Vec<_> = self.clients.extract_if(.., |c| c.lost).collect();
            for mut seat in lost {
                let (client, kicked) = (seat.id, seat.kicked);
                for other in &mut self.clients {
                    other.kicking.retain(|&c| c != client);
                }
                if let Some(pauses) = &mut self.pauses {
                    let let_go = pauses.leave(client).into_iter().map(|a| (None, a));
                    self.put_off.extend(let_go);
                }
                // Its sides, its person's and its computer players', are
                // played on by new computer players once the match has
                // begun, but for those that have lost; before then, its
                // person's is unless another is to be let in for it.
                let mut sides: Vec<u32> = seat
                    .players
                    .iter()
                    .chain(&seat.computers)
                    .copied()
                    .collect();
                sides.sort_unstable();
                for player in sides {
                    if self.begun {
                        if self.world.outcome(player) != Outcome::Lost {
                            let stood = self.side_name(player, &seat.name);
                            let taken_over = self.take_over(player, &stood);
                            self.taking_over.push(taken_over);
                        }
                    } else if !self.gathering && seat.players.contains(&player) {
                        self.departed.push((player, seat.name.clone()));
                    }
                }
                let left = ToClient::Left(Gone {
                    client,
                    players: seat.players,
                    name: seat.name,
                    kicked,
                });
                if kicked {
                    let _ = seat.link.send(encode(&left));
                }
                events.push(Event::Left(client));
                self.parted = true;
                self.tell(&left);
            }
        }
        events
    }

    /// Has a new computer player take over side `player`, which went by
    /// `stood`, named after it unlike anyone else in the match.
    fn take_over(&mut self, player: u32, stood: &str) -> Action {
        let name = format!("{stood}{}", self.naming.taken_over);
        let name = crate::unique(&name, &self.naming.numbered, |n| self.named(n));
        let (computer, by) = (self.taker)(&self.setup, player);
        self.computers.insert(player, computer);
        let by = by.unwrap_or_else(|| name.clone());
        Action::TakeOver { player, name, by }
    }

    /// What side `player` goes by: its person's or computer player's name,
    /// a computer player's being its profile's, or `person`'s where the
    /// match has yet to name it.
    fn side_name(&self, player: u32, person: &str) -> String {
        let sides = &self.world.sides;
        let named = (sides.names.get(&player))
            .or_else(|| sides.computers.get(&player))
            .or_else(|| self.profile_names.get(&player))
            .or_else(|| self.naming.computers.get(&player));
        named.map_or_else(|| person.to_string(), Clone::clone)
    }

    /// Whether anyone in the match goes by `name`: a person, a computer
    /// player that took a side over or is to, or one playing a side
    /// nobody else does.
    fn named(&self, name: &str) -> bool {
        let sides = &self.world.sides;
        let to_take_over = |a: &Action| matches!(a, Action::TakeOver { name: n, .. } if n == name);
        let played = |p: &u32| {
            sides.names.contains_key(p) || sides.computers.contains_key(p) || self.seated(*p)
        };
        self.client_named(name)
            || sides.computers.values().any(|c| c == name)
            || self.taking_over.iter().any(to_take_over)
            || (self.naming.computers.iter()).any(|(p, c)| c == name && !played(p))
    }

    /// Whether a client goes by `name`.
    fn client_named(&self, name: &str) -> bool {
        self.clients.iter().any(|c| c.name == name)
    }

    /// Whether a client gives side `player`'s orders, for a person or a
    /// computer player its machine runs.
    fn seated(&self, player: u32) -> bool {
        self.clients.iter().any(|c| c.plays(player))
    }

    /// The orders of the computer player of each side still in the match
    /// that no client plays, making one for a side as it comes to need it.
    /// A side whose person leaves is the computer's from the turn after.
    fn think(&mut self) -> Vec<Action> {
        let theirs: Vec<u32> = (tgmap::sides(&self.world.sides.players))
            .map(|p| p.id)
            .filter(|&p| !self.seated(p) && self.world.outcome(p) == Outcome::Playing)
            .collect();
        let (setup, world, make) = (&self.setup, &self.world, &self.computer);
        self.computers.retain(|p, _| theirs.contains(p));
        for player in theirs {
            (self.computers.entry(player)).or_insert_with(|| make(setup, world, player));
        }
        let orders = self.computers.iter_mut().flat_map(|(&player, computer)| {
            let order = move |command| Action::Order { player, command };
            computer.think(world, player).into_iter().map(order)
        });
        orders.collect()
    }

    /// Tells each client that's asked for more since it was last told
    /// that all it's asked for is dealt with, short of what waits for a
    /// held match to go on.
    fn answer(&mut self) {
        for seat in &mut self.clients {
            let dealt_with = seat.put_off_from.unwrap_or(seat.asked);
            if seat.answered < dealt_with {
                seat.answered = dealt_with;
                seat.sent = self.now;
                let answered = encode(&ToClient::Answered(dealt_with));
                seat.lost |= seat.link.send(answered).is_err();
            }
        }
    }

    /// Takes what the clients have sent, has every copy of the match
    /// carry out what they asked that it lets them, and plays a tick
    /// unless the match is held still.
    pub fn turn(&mut self) -> Vec<Event> {
        self.go(None, true)
    }

    /// As [`Host::turn`], but playing no tick: for carrying out what's
    /// asked sooner than the next tick.
    pub fn relay(&mut self) -> Vec<Event> {
        self.go(Some(false), true)
    }

    /// As [`Host::turn`], but playing a tick whether or not the match is
    /// held still: for what keeps the match's time itself.
    pub fn tick(&mut self) -> Vec<Event> {
        self.go(Some(true), true)
    }

    /// A turn, beginning the match if `play` and it hasn't begun, and then
    /// playing a tick as `tick` says, or by whether the match is held
    /// still if it doesn't; or, unless `play`, one that plays nothing.
    fn go(&mut self, tick: Option<bool>, play: bool) -> Vec<Event> {
        assert!(
            self.built || !play,
            "a match begins once the host's copy is set up"
        );
        self.now = self.clock.now();
        let mut actions = Vec::new();
        if play && !self.begun {
            // Every side a person plays goes by their name from the first.
            for seat in &self.clients {
                let name = |&player| Action::Name {
                    player,
                    name: seat.name.clone(),
                };
                actions.extend(seat.players.iter().map(name).map(|a| (None, a)));
            }
            // A side left before then is a computer player's from the
            // first, unless someone let in since plays it. Its person is
            // named unlike those let in since, who weren't numbered by them.
            let departed = std::mem::take(&mut self.departed);
            let departed: Vec<_> = (departed.into_iter())
                .filter(|&(player, _)| !self.seated(player))
                .collect();
            for (player, name) in departed {
                let name = crate::unique(&name, &self.naming.numbered, |n| self.client_named(n));
                let taken_over = self.take_over(player, &name);
                self.taking_over
                    .extend([Action::Name { player, name }, taken_over]);
            }
        }
        self.begun |= play;
        let mut heard = self.hear();
        actions.append(&mut heard.actions);
        let mut events = Vec::new();
        for (client, step) in heard.diverged {
            events.push(Event::Diverged { client, step });
            self.tell(&ToClient::Diverged { client, step });
        }
        for said in heard.said {
            self.tell(&ToClient::Said(said));
        }
        // Whether anyone's side resigned, leaving them watching.
        let mut resigned = false;
        let votes_changed = heard.votes_changed | self.kick(heard.kicks);
        let was_held = self.held;
        self.judge();
        // Nothing's carried out before the match begins.
        // Computer players take sides over whether or not the match is held.
        let mut given = std::mem::take(&mut self.taking_over);
        // Whether to play a tick, if it's to be told, or else by whether
        // the match is held still; none for a turn that plays nothing.
        let ticking = if play && self.held {
            // What's asked while the match is held waits for it to go on,
            // and so isn't answered until it has.
            self.put_off.append(&mut actions);
            for seat in &mut self.clients {
                let first = heard.first_taken.iter().find(|&&(id, _)| id == seat.id);
                seat.put_off_from = seat.put_off_from.or(first.map(|&(_, asked)| asked));
            }
            Some(Some(false))
        } else if play {
            for seat in &mut self.clients {
                seat.put_off_from = None;
            }
            let mut put_off = std::mem::take(&mut self.put_off);
            put_off.append(&mut actions);
            given.extend(self.rule(put_off));
            Some(tick)
        } else {
            None
        };
        if ticking.is_some() {
            for action in &given {
                self.schedule.act(action.clone());
            }
        }
        let tick = ticking.map(|t| t.unwrap_or(!self.schedule.pausing()));
        // Judged once this turn's pausing is carried out, so orders aren't
        // decided for a match standing still, which would only have them
        // pile up or land on a stale match once it goes on.
        if tick == Some(true) {
            let orders = self.think();
            for order in &orders {
                self.schedule.act(order.clone());
            }
            given.extend(orders);
        }
        if let Some(tick) = tick.filter(|&tick| tick || !given.is_empty()) {
            self.tell(&ToClient::Step(Bundle {
                actions: given,
                tick,
            }));
            self.issued += u64::from(tick);
            match tick {
                true => _ = self.schedule.step(&mut self.world),
                false => _ = self.schedule.carry_out(&mut self.world),
            }
            // Nobody reads them here, and they'd only mount up.
            self.world.take_notices();
            // Whoever played a side that's resigned only watches now.
            for seat in &mut self.clients {
                let sides = seat.players.len();
                seat.players.retain(|&p| !self.world.resigned(p));
                resigned |= seat.players.len() != sides;
            }
        }
        self.answer();
        if self.held != was_held {
            events.push(Event::Held(self.held));
        }
        let parted = self.part();
        // Everyone hears at once that the match is held or let go, how
        // the votes stand, which a voter sees its own by, and who's gone.
        let due = self.told.is_none_or(|t| self.now >= t + TOLD);
        let changed = votes_changed || resigned || !parted.is_empty();
        if due || self.held != was_held || changed {
            self.told = Some(self.now);
            self.tell(&ToClient::Status(self.status()));
        }
        events.extend(parted);
        events.extend(self.part());
        self.keep_up();
        events
    }

    /// Takes what the clients have sent, as far as a turn takes from each.
    fn hear(&mut self) -> Heard {
        let mut heard = Heard::default();
        let checks = &self.schedule.record().checks;
        for seat in &mut self.clients {
            // One it trusts is heard out, having nobody to hold up but
            // those it's trusted by.
            let most = if seat.trusted {
                usize::MAX
            } else {
                HEARD_A_TURN
            };
            for _ in 0..most {
                let message = match seat.link.receive() {
                    Ok(None) => break,
                    Ok(Some(m)) => {
                        seat.heard = Some(self.now);
                        (m.len() <= LONGEST_HEARD).then(|| decode(&m)).flatten()
                    }
                    Err(_) => None,
                };
                match message {
                    Some(ToHost::Ask(action)) => {
                        if seat.may(&action) {
                            if !heard.first_taken.iter().any(|&(id, _)| id == seat.id) {
                                heard.first_taken.push((seat.id, seat.asked));
                            }
                            heard.actions.push((Some(seat.id), action));
                        }
                        seat.asked += 1;
                    }
                    Some(ToHost::Ready) => seat.ready = true,
                    Some(ToHost::Played(played)) => seat.played = seat.played.max(played),
                    Some(ToHost::Vote(vote)) => {
                        heard.votes_changed |= seat.vote != vote;
                        seat.vote = vote;
                    }
                    Some(ToHost::Kick { client, vote }) => {
                        heard.kicks.push((seat.id, client, vote))
                    }
                    Some(ToHost::Say(words)) => {
                        if worth_saying(&words) {
                            let from = seat.players.first().copied();
                            let name = seat.name.clone();
                            heard.said.push(Said { from, name, words });
                        }
                    }
                    // Too late to change how the match is set up.
                    Some(ToHost::Choose(_)) => {}
                    Some(ToHost::Check { step, hash }) => {
                        // The host has played every tick a client has.
                        let stands = checks.binary_search(&(step, hash)).is_ok();
                        if !stands && !seat.diverged {
                            seat.diverged = true;
                            heard.diverged.push((seat.id, step));
                        }
                    }
                    Some(ToHost::Leave) | None => {
                        seat.lost = true;
                        break;
                    }
                }
            }
        }
        heard
    }

    /// What `asked`, each with the client that asked if one did, has every
    /// copy of the match given, by the rules for pausing it.
    fn rule(&mut self, asked: Vec<(Option<u32>, Action)>) -> Vec<Action> {
        let pausing = |(_, a): &(_, Action)| matches!(a, Action::Pause(_));
        let Some(pauses) = (self.pauses.as_mut()).filter(|_| asked.iter().any(pausing)) else {
            return asked.into_iter().map(|(_, action)| action).collect();
        };
        let people: Vec<_> = (self.clients.iter())
            .filter(|c| !c.lost)
            .map(|c| Person {
                client: c.id,
                side: c.players.first().copied(),
            })
            .collect();
        let rule = |(client, action)| match (client, action) {
            (Some(client), Action::Pause(pause)) => pauses.ask(client, pause, self.now, &people),
            (_, action) => vec![action],
        };
        asked.into_iter().flat_map(rule).collect()
    }

    /// Casts or takes back each of `kicks`, a voter's vote for kicking a
    /// target out, telling everyone of each that changes anything; then,
    /// if any did or anyone has left since, kicks out those enough are
    /// for kicking out. Whether any vote changed.
    fn kick(&mut self, kicks: Vec<(u32, u32, bool)>) -> bool {
        let mut changed = false;
        let needed = self.kicks_needed();
        for (voter, target, vote) in kicks {
            let sided = |id| self.clients.iter().find(|c| c.id == id && c.sided());
            // Whoever runs the host is there for good: kicking it would
            // leave everyone else's match with nobody to play it on.
            let can = voter != target
                && sided(voter).is_some()
                && sided(target).is_some_and(|t| !t.trusted);
            if !can {
                continue;
            }
            let Some(seat) = self.clients.iter_mut().find(|c| c.id == voter) else {
                continue;
            };
            if seat.kicking.contains(&target) == vote {
                continue;
            }
            match vote {
                true => seat.kicking.push(target),
                false => seat.kicking.retain(|&c| c != target),
            }
            changed = true;
            let votes = self.kicks_against(target);
            self.tell(&ToClient::Kicking(KickVote {
                voter,
                target,
                vote,
                votes,
                needed,
            }));
        }
        if !changed && !std::mem::take(&mut self.parted) {
            return false;
        }
        // Those already going go as they would have.
        let out: Vec<u32> = (self.clients.iter())
            .filter(|c| !c.lost && self.kicks_against(c.id) >= needed)
            .map(|c| c.id)
            .collect();
        for seat in &mut self.clients {
            seat.kicked |= out.contains(&seat.id);
            seat.lost |= seat.kicked;
        }
        changed
    }

    /// How many of those staying with a side are for kicking `client`
    /// out: a vote cast before its voter's side resigned no longer counts.
    fn kicks_against(&self, client: u32) -> u32 {
        let voters = (self.clients.iter()).filter(|c| c.sided() && c.kicking.contains(&client));
        voters.count() as u32
    }

    /// How many it takes to kick a client out: more than half of those
    /// staying that give any player's orders, the one kicked among them.
    fn kicks_needed(&self) -> u32 {
        majority(self.clients.iter().filter(|c| c.sided()).count())
    }

    /// How `seat` stands at this turn.
    fn standing(&self, seat: &Seat<L>) -> Standing {
        // The client of whoever runs the host is never lost to it.
        // One yet to say anything since it was let in is still starting
        // up, and is found gone only if its link is.
        let unheard = seat.heard.is_some_and(|heard| self.now >= heard + UNHEARD);
        if !seat.trusted && unheard {
            Standing::Disconnected
        } else if seat.diverged {
            Standing::OutOfSync
        } else if self.held && !self.holding && seat.played < self.issued {
            Standing::CatchingUp
        } else {
            Standing::Ok
        }
    }

    /// Whether `seat` holds the match. Whoever runs the host is never
    /// dropped, so nor does its client hold the match for good.
    fn holds(&self, seat: &Seat<L>) -> bool {
        !seat.trusted && self.standing(seat).holds()
    }

    /// How many of those still heard from are for dropping those holding
    /// the match, and how many it takes: more than half of them.
    fn votes(&self) -> (u32, u32) {
        let heard: Vec<_> = (self.clients.iter())
            .filter(|c| self.standing(c) != Standing::Disconnected)
            .collect();
        let needed = majority(heard.len());
        let votes = heard.iter().filter(|c| c.vote).count() as u32;
        (votes, needed)
    }

    /// The longest any of those holding the match may yet hold it before
    /// they can be dropped, if any are.
    fn countdown(&self) -> Option<Duration> {
        let holding = self.clients.iter().filter(|c| self.holds(c));
        holding.map(|c| c.allowance).max()
    }

    /// Holds the match while any client's standing holds it, and then
    /// until every client has caught up; drains and refills the
    /// allowances; and drops those holding it once enough are for it and
    /// they've had their allowances.
    fn judge(&mut self) {
        let passed = self.counted.map_or(Duration::ZERO, |c| self.now - c);
        self.counted = Some(self.now);
        let holding: Vec<bool> = self.clients.iter().map(|c| self.holds(c)).collect();
        self.holding = holding.contains(&true);
        for (seat, &holds) in self.clients.iter_mut().zip(&holding) {
            seat.allowance = match holds {
                true => seat.allowance.saturating_sub(passed),
                false => (seat.allowance + passed / REFILLED).min(ALLOWANCE),
            };
        }
        if self.holding {
            self.held = true;
            let (votes, needed) = self.votes();
            let run_out = self.countdown().is_some_and(|left| left <= RUN_OUT);
            if votes >= needed && run_out {
                for (seat, &holds) in self.clients.iter_mut().zip(&holding) {
                    seat.lost |= holds;
                    seat.vote = false;
                }
            }
        } else if self.held {
            let issued = self.issued;
            let caught_up = self.clients.iter().all(|c| c.lost || c.played >= issued);
            self.held = !caught_up;
        }
    }

    /// How everyone stands, as every client is told.
    pub fn status(&self) -> Status {
        let seats = (self.clients.iter())
            .map(|c| Seated {
                client: c.id,
                players: c.players.clone(),
                name: c.name.clone(),
                host: c.trusted,
                standing: self.standing(c),
                vote: c.vote,
                kicking: c.kicking.clone(),
                ping: ping(&c.link),
            })
            .collect();
        let (votes, needed) = self.votes();
        let countdown = self
            .countdown()
            .map(|left| left.as_secs_f32().ceil() as u32);
        Status {
            seats,
            held: self.held,
            votes,
            needed,
            countdown,
            kicks_needed: self.kicks_needed(),
        }
    }

    /// Lets each client that's heard nothing for a while know the host is
    /// still there.
    fn keep_up(&mut self) {
        let alive = encode(&ToClient::Alive);
        for seat in &mut self.clients {
            if self.now >= seat.sent + QUIET {
                seat.sent = self.now;
                seat.lost |= seat.link.send(alive.clone()).is_err();
            }
        }
    }

    /// Sends `message` to every client.
    fn tell(&mut self, message: &ToClient) {
        let bytes = encode(message);
        for seat in &mut self.clients {
            seat.sent = self.now;
            seat.lost |= seat.link.send(bytes.clone()).is_err();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tgmap::{Domain, Formation, Member, Organization, Pathing};
    use tgsim::{Command, Given, Pausing, Record, Refused, Sides, Votes};

    use super::*;
    use crate::testing::player;
    use crate::{Client, Link, Local, NotJoined, pair, welcome};

    /// A match of players 1 to 4, the first two each with a soldier, the
    /// first's at 10,10 and the second's 10 further along y.
    fn fresh() -> World {
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
            players: (1..=4).map(|id| player(id, false, true)).collect(),
            teams: Vec::new(),
        };
        let pathing = Pathing::uniform(40, Domain::Land);
        World::from_organizations(sides, pathing, &[organization(0), organization(1)])
    }

    /// A client and the copy of the match it plays.
    struct Copy {
        client: Client<Local>,
        world: World,
    }

    impl std::ops::Deref for Copy {
        type Target = Client<Local>;

        fn deref(&self) -> &Client<Local> {
            &self.client
        }
    }

    impl std::ops::DerefMut for Copy {
        fn deref_mut(&mut self) -> &mut Client<Local> {
            &mut self.client
        }
    }

    impl Copy {
        fn world(&self) -> &World {
            &self.world
        }

        fn advance(&mut self, most: usize) -> Result<(), crate::Lost> {
            self.client.advance(&mut self.world, most)
        }
    }

    fn setup() -> Setup {
        Setup {
            map: "field".into(),
            seed: 3,
            ..Default::default()
        }
    }

    /// A host with a client for each of `seats`, giving the orders of
    /// the players it lists.
    fn hosted(seats: &[&[u32]]) -> (Host<Local>, Vec<Copy>) {
        hosted_by(&Clock::stopped(), seats)
    }

    /// As [`hosted`], everyone telling the time by `clock`.
    fn hosted_by(clock: &Clock, seats: &[&[u32]]) -> (Host<Local>, Vec<Copy>) {
        let mut host = Host::new(setup(), fresh());
        host.clock(clock.clone());
        let clients = seats.iter().enumerate().map(|(seat, players)| {
            let (ours, mut theirs) = pair();
            let name = named(seat as u32 + 1);
            let id = host.admit(ours, players.to_vec(), &name).unwrap();
            let welcome = welcome(&mut theirs).unwrap().unwrap();
            assert_eq!((welcome.client, &welcome.setup), (id, &setup()));
            assert_eq!(welcome.players, *players);
            let mut client = Client::new(theirs);
            client.clock(clock.clone());
            Copy {
                client,
                world: fresh(),
            }
        });
        let clients = clients.collect();
        (host, clients)
    }

    /// What [`hosted`] has the person at each client go by.
    fn named(client: u32) -> String {
        format!("Player {client}")
    }

    /// What a match was given but for the names the host gives sides as
    /// it begins.
    fn asked(record: &Record) -> Vec<Given> {
        let given = record.given.iter().cloned();
        given
            .filter(|g| !matches!(g.action, Action::Name { .. }))
            .collect()
    }

    fn gone(client: u32, players: &[u32]) -> Gone {
        let players = players.to_vec();
        Gone {
            client,
            players,
            name: named(client),
            kicked: false,
        }
    }

    fn go(player: u32, organization: usize, to: [f32; 2]) -> Action {
        let command = Command::Move { organization, to };
        Action::Order { player, command }
    }

    /// Plays `turns` turns, every client keeping up.
    fn play(host: &mut Host<Local>, clients: &mut [Copy], turns: usize) {
        for _ in 0..turns {
            assert_eq!(host.turn(), []);
            for c in clients.iter_mut() {
                c.advance(usize::MAX).unwrap();
            }
        }
    }

    #[test]
    fn every_copy_plays_what_the_clients_ask_the_same_way() {
        let (mut host, mut clients) = hosted(&[&[1], &[2], &[]]);
        clients[0].ask(go(1, 0, [30.0, 12.0])).unwrap();
        play(&mut host, &mut clients, 25);
        clients[1].ask(go(2, 1, [5.0, 30.0])).unwrap();
        play(&mut host, &mut clients, 25);
        assert_eq!(host.world().ticks, 50);
        assert!(host.world().units[0].position[0] > 12.0);
        assert!(host.world().units[1].position[0] < 9.0);
        let record = host.schedule().record();
        assert_eq!(asked(record).len(), 2);
        assert_eq!(record.checks.len(), 5);
        for c in &clients {
            assert_eq!(c.world().hash(), host.world().hash());
            assert_eq!(c.schedule().record(), record);
            assert_eq!(c.diverged(), []);
        }
        // And is a film of the match.
        let mut again = fresh();
        assert_eq!(record.replay(&mut again), Ok(()));
        assert_eq!(again.hash(), host.world().hash());
    }

    #[test]
    fn a_client_is_only_let_do_what_is_its_to_do() {
        let (mut host, mut clients) = hosted(&[&[1], &[]]);
        // Another player's orders, and anything from one that watches.
        clients[0].ask(go(2, 1, [5.0, 30.0])).unwrap();
        clients[1].ask(go(1, 0, [30.0, 12.0])).unwrap();
        clients[1].ask(Action::Pause(true)).unwrap();
        clients[0].ask(Action::Win(1)).unwrap();
        play(&mut host, &mut clients, 3);
        assert!(asked(host.schedule().record()).is_empty());
        assert_eq!(host.world().ticks, 3);
        host.trust(1);
        clients[0].ask(Action::Win(1)).unwrap();
        clients[1].ask(Action::Win(2)).unwrap();
        play(&mut host, &mut clients, 1);
        assert_eq!(asked(host.schedule().record()).len(), 1);
        // An order the match refuses is refused in every copy.
        clients[0].ask(go(1, 1, [5.0, 30.0])).unwrap();
        assert_eq!(host.turn(), []);
        assert_eq!(clients[0].advance(9), Ok(()));
        assert_eq!(clients[0].take_refused(), [(1, Refused::NotYours)]);
        assert_eq!(clients[0].take_refused(), []);
    }

    #[test]
    fn a_match_held_still_is_still_given_what_lets_it_go_on() {
        let (mut host, mut clients) = hosted(&[&[1], &[2]]);
        play(&mut host, &mut clients, 4);
        clients[0].ask(Action::Pause(true)).unwrap();
        play(&mut host, &mut clients, 10);
        assert_eq!(host.world().ticks, 4);
        assert!(clients.iter().all(|c| c.schedule().paused()));
        // Only whoever runs the host sets the speed.
        clients[1].ask(Action::Speed(4.0)).unwrap();
        host.trust(2);
        clients[1].ask(Action::Speed(2.0)).unwrap();
        clients[1].ask(Action::Pause(false)).unwrap();
        play(&mut host, &mut clients, 6);
        assert_eq!(host.world().ticks, 10);
        for c in &clients {
            assert_eq!(c.world().ticks, 10);
            assert_eq!((c.schedule().paused(), c.schedule().speed()), (false, 2.0));
            assert_eq!(c.schedule().record(), host.schedule().record());
        }
    }

    #[test]
    fn the_speed_it_is_given_is_every_copy_s_from_its_next_turn() {
        let (mut host, mut clients) = hosted(&[&[1]]);
        host.speed(2.0);
        play(&mut host, &mut clients, 1);
        assert_eq!(host.schedule().speed(), 2.0);
        assert_eq!(clients[0].schedule().speed(), 2.0);
    }

    #[test]
    fn between_machines_a_pause_is_let_go_by_its_pauser_or_by_vote() {
        let (mut host, mut clients) = hosted(&[&[1], &[2], &[3]]);
        host.multiplayer();
        play(&mut host, &mut clients, 2);
        let pausing = |host: &Host<Local>| -> Vec<Pausing> {
            let given = host.schedule().record().given.iter();
            let pausing = given.filter_map(|g| match g.action {
                Action::Pausing(p) => Some(p),
                _ => None,
            });
            pausing.collect()
        };
        // What pausing comes to is the host's to say.
        clients[0].ask(Action::Pausing(Pausing::VotedOff)).unwrap();
        clients[0].ask(Action::Pause(true)).unwrap();
        play(&mut host, &mut clients, 2);
        let needed = Pausing::Needed(Votes {
            votes: 0,
            needed: 2,
        });
        assert_eq!(pausing(&host), [Pausing::Paused { by: 1 }, needed]);
        assert!(clients.iter().all(|c| c.schedule().paused()));
        clients[1].ask(Action::Pause(false)).unwrap();
        clients[2].ask(Action::Pause(false)).unwrap();
        play(&mut host, &mut clients, 2);
        assert_eq!(pausing(&host)[4], Pausing::VotedOff);
        assert!(clients.iter().all(|c| !c.schedule().paused()));
        assert_eq!(host.world().ticks, 4);
        // A pauser who goes lets the match go with them.
        clients[1].ask(Action::Pause(true)).unwrap();
        play(&mut host, &mut clients, 1);
        assert!(host.schedule().paused());
        clients[1].leave().unwrap();
        clients.remove(1);
        host.turn();
        play(&mut host, &mut clients, 1);
        assert!(!host.schedule().paused() && !clients[0].schedule().paused());
        assert_eq!(pausing(&host).len(), 7);
    }

    #[test]
    fn a_side_resigned_leaves_its_player_watching() {
        let (mut host, mut clients) = hosted(&[&[1], &[2], &[3]]);
        host.multiplayer();
        play(&mut host, &mut clients, 2);
        // Only a side's own player resigns it.
        clients[0].ask(Action::Resign(2)).unwrap();
        clients[1].ask(Action::Resign(2)).unwrap();
        play(&mut host, &mut clients, 2);
        assert_eq!(asked(host.schedule().record()).len(), 1);
        assert!(clients.iter().all(|c| c.world().resigned(2)));
        let seat = |host: &Host<Local>, client| {
            let mut seats = host.status().seats.into_iter();
            seats.find(|s| s.client == client).unwrap().players
        };
        assert_eq!(seat(&host, 2), []);
        assert_eq!(clients[1].status().unwrap().seats[1].players, []);
        // Nor can they pause any more, or give orders.
        clients[1].ask(Action::Pause(true)).unwrap();
        clients[1].ask(go(2, 1, [5.0, 30.0])).unwrap();
        play(&mut host, &mut clients, 2);
        assert_eq!(asked(host.schedule().record()).len(), 1);
        assert!(!host.schedule().paused());
    }

    #[test]
    fn a_kick_vote_counts_only_while_its_voter_has_a_side() {
        let (mut host, mut clients) = hosted(&[&[1], &[2], &[3]]);
        play(&mut host, &mut clients, 2);
        clients[1].kick(3, true).unwrap();
        clients[1].ask(Action::Resign(2)).unwrap();
        play(&mut host, &mut clients, 2);
        // With two sides left two votes are needed, and the first is gone.
        clients[0].kick(3, true).unwrap();
        play(&mut host, &mut clients, 2);
        assert_eq!(host.clients(), 3);
    }

    #[test]
    fn a_client_that_falls_behind_catches_up() {
        let (mut host, mut clients) = hosted(&[&[1], &[2]]);
        let (quick, slow) = clients.split_at_mut(1);
        quick[0].ask(go(1, 0, [30.0, 12.0])).unwrap();
        play(&mut host, quick, 45);
        assert_eq!(slow[0].world().ticks, 0);
        // A part at a time, if that's all it's given time for.
        slow[0].advance(20).unwrap();
        assert_eq!(slow[0].world().ticks, 20);
        slow[0].advance(usize::MAX).unwrap();
        assert_eq!(slow[0].world().hash(), host.world().hash());
        // What it asks for is carried out when the host hears it.
        slow[0].ask(go(2, 1, [5.0, 30.0])).unwrap();
        play(&mut host, &mut clients, 5);
        assert_eq!(asked(host.schedule().record())[1].step, 45);
        assert_eq!(clients[1].world().hash(), host.world().hash());
        assert_eq!(clients[1].diverged(), []);
    }

    #[test]
    fn a_client_is_kicked_out_once_more_than_half_those_with_players_vote_for_it() {
        let (mut host, mut clients) = hosted(&[&[1], &[2], &[3], &[]]);
        play(&mut host, &mut clients, 2);
        let vote = |voter, vote, votes| KickVote {
            voter,
            target: 3,
            vote,
            votes,
            needed: 2,
        };
        // Only those with players vote, and not for kicking themselves.
        clients[3].kick(3, true).unwrap();
        clients[2].kick(3, true).unwrap();
        clients[0].kick(3, true).unwrap();
        clients[0].kick(3, true).unwrap();
        clients[0].kick(3, false).unwrap();
        play(&mut host, &mut clients, 1);
        for c in &mut clients {
            assert_eq!(c.take_kicking(), [vote(1, true, 1), vote(1, false, 0)]);
        }
        clients[0].kick(3, true).unwrap();
        clients[1].kick(3, true).unwrap();
        assert_eq!(host.turn(), [Event::Left(3)]);
        let kicked = Gone {
            kicked: true,
            ..gone(3, &[3])
        };
        // The one kicked out hears it, then nothing more.
        assert_eq!(clients[2].advance(usize::MAX), Err(crate::Lost));
        assert_eq!(clients[2].take_left(), std::slice::from_ref(&kicked));
        for i in [0, 1, 3] {
            clients[i].advance(usize::MAX).unwrap();
            assert_eq!(
                clients[i].take_kicking(),
                [vote(1, true, 1), vote(2, true, 2)]
            );
            assert_eq!(clients[i].take_left(), std::slice::from_ref(&kicked));
        }
        let status = host.status();
        assert!(status.seats.iter().all(|s| s.kicking.is_empty()));
        assert_eq!(status.kicks_needed, 2);
    }

    #[test]
    fn votes_cast_kick_out_once_those_leaving_bring_the_count_needed_down() {
        let (mut host, mut clients) = hosted(&[&[1], &[2], &[3], &[4]]);
        clients[0].kick(4, true).unwrap();
        clients[1].kick(4, true).unwrap();
        // The third would make it, but leaves, taking its vote with it.
        clients[2].kick(4, true).unwrap();
        clients[2].leave().unwrap();
        // Two of the three left are then enough.
        assert_eq!(host.turn(), [Event::Left(3), Event::Left(4)]);
        clients[0].advance(usize::MAX).unwrap();
        let kicked: Vec<_> = clients[0].take_left().iter().map(|g| g.kicked).collect();
        assert_eq!(kicked, [false, true]);
    }

    #[test]
    fn a_client_chats_only_as_itself() {
        let chat = |from| Action::Chat {
            from,
            to: tgsim::Channel::All,
            words: "hi".into(),
        };
        assert!(allows(&[1], &chat(Some(1))));
        assert!(!allows(&[1], &chat(Some(2))));
        assert!(!allows(&[1], &chat(None)));
        // One that watches speaks as one watching.
        assert!(allows(&[], &chat(None)));
        assert!(!allows(&[], &chat(Some(1))));
        let said = |words: String| Action::Chat {
            from: Some(1),
            to: tgsim::Channel::All,
            words,
        };
        assert!(allows(&[1], &said("a".repeat(tgsim::CHAT_LONGEST))));
        assert!(!allows(&[1], &said("a".repeat(tgsim::CHAT_LONGEST + 1))));
        assert!(!allows(&[1], &said("a\nb".into())));
    }

    #[test]
    fn whoever_runs_the_host_is_not_kicked_out() {
        let (mut host, mut clients) = hosted(&[&[1], &[2], &[3]]);
        host.trust(1);
        clients[1].kick(1, true).unwrap();
        clients[2].kick(1, true).unwrap();
        play(&mut host, &mut clients, 1);
        assert_eq!(host.clients(), 3);
        assert_eq!(clients[0].take_kicking(), []);
    }

    #[test]
    fn a_copy_that_stops_matching_is_found_and_everyone_told() {
        let (mut host, mut clients) = hosted(&[&[1], &[2]]);
        play(&mut host, &mut clients, 12);
        // The second client's copy goes its own way.
        clients[1].schedule_mut().act(go(2, 1, [5.0, 30.0]));
        let mut events = Vec::new();
        for _ in 0..30 {
            events.extend(host.turn());
            for c in clients.iter_mut() {
                c.advance(usize::MAX).unwrap();
            }
        }
        assert_ne!(clients[1].world().hash(), host.world().hash());
        // Once, at the first hash that differed.
        let found = Event::Diverged {
            client: 2,
            step: 20,
        };
        // It holds the match, which waits on it to be dropped.
        assert_eq!(events, [found, Event::Held(true)]);
        assert_eq!(host.world().ticks, 20);
        for c in &clients {
            assert_eq!(c.diverged(), [(2, 20)]);
        }
        assert_eq!(clients[0].world().hash(), host.world().hash());
        let standing: Vec<_> = host.status().seats.iter().map(|s| s.standing).collect();
        assert_eq!(standing, [Standing::Ok, Standing::OutOfSync]);
    }

    /// Plays turns a second apart, the clients but `silent` keeping up,
    /// returning what they brought.
    fn wait_out(
        clock: &Clock,
        host: &mut Host<Local>,
        clients: &mut [Copy],
        silent: usize,
        seconds: u32,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..seconds {
            clock.pass(Duration::from_secs(1));
            events.extend(host.turn());
            for (i, c) in clients.iter_mut().enumerate() {
                if i != silent {
                    c.advance(usize::MAX).unwrap();
                }
            }
        }
        events
    }

    #[test]
    fn a_client_unheard_from_holds_the_match_until_it_is_heard_again() {
        let clock = Clock::stopped();
        let (mut host, mut clients) = hosted_by(&clock, &[&[1], &[2]]);
        play(&mut host, &mut clients, 5);
        // Two seconds from when it was last heard, the turn after it last
        // sent.
        assert_eq!(wait_out(&clock, &mut host, &mut clients, 1, 2), []);
        assert_eq!(
            wait_out(&clock, &mut host, &mut clients, 1, 1),
            [Event::Held(true)]
        );
        let ticks = host.world().ticks;
        wait_out(&clock, &mut host, &mut clients, 1, 3);
        // Everyone stops at the same tick, and hears why.
        assert_eq!(host.world().ticks, ticks);
        let status = clients[0].status().unwrap().clone();
        assert!(status.held);
        assert_eq!(status.seats[1].standing, Standing::Disconnected);
        assert_eq!(clients[0].world().ticks, ticks);
        // Heard from again, it plays what it missed and the match goes on.
        clients[1].advance(usize::MAX).unwrap();
        assert_eq!(clients[1].world().ticks, ticks);
        assert_eq!(host.turn(), [Event::Held(false)]);
        assert_eq!(host.world().ticks, ticks + 1);
        play(&mut host, &mut clients, 1);
        assert!(!clients[0].status().unwrap().held);
    }

    #[test]
    fn those_holding_the_match_are_dropped_once_enough_vote_and_their_time_is_up() {
        let clock = Clock::stopped();
        let (mut host, mut clients) = hosted_by(&clock, &[&[1], &[2], &[]]);
        play(&mut host, &mut clients, 5);
        // The watcher goes quiet.
        wait_out(&clock, &mut host, &mut clients, 2, 3);
        assert!(host.held());
        // Two of the three, those still heard from, are enough.
        clients[0].vote(true).unwrap();
        clients[1].vote(true).unwrap();
        wait_out(&clock, &mut host, &mut clients, 2, 12);
        let status = clients[0].status().unwrap().clone();
        assert_eq!((status.votes, status.needed), (2, 2));
        assert_eq!(status.countdown, Some(2));
        assert_eq!(wait_out(&clock, &mut host, &mut clients, 2, 1), []);
        assert_eq!(host.clients(), 3);
        let events = wait_out(&clock, &mut host, &mut clients, 2, 2);
        assert_eq!(events, [Event::Left(3), Event::Held(false)]);
        assert_eq!(host.clients(), 2);
        assert_eq!(clients[0].take_left(), [gone(3, &[])]);
        // The votes go with the drop.
        let status = clients[0].status().unwrap();
        assert_eq!(status.votes, 0);
        let ticks = host.world().ticks;
        wait_out(&clock, &mut host, &mut clients, 2, 1);
        assert_eq!(host.world().ticks, ticks + 1);
    }

    #[test]
    fn those_holding_the_match_stay_without_enough_votes() {
        let clock = Clock::stopped();
        let (mut host, mut clients) = hosted_by(&clock, &[&[1], &[2], &[]]);
        play(&mut host, &mut clients, 5);
        clients[0].vote(true).unwrap();
        // What one votes before it's lost doesn't count once it is.
        clients[2].vote(true).unwrap();
        wait_out(&clock, &mut host, &mut clients, 2, 30);
        assert!(host.held());
        assert_eq!(host.clients(), 3);
        let status = host.status();
        assert_eq!(
            (status.votes, status.needed, status.countdown),
            (1, 2, Some(0))
        );
    }

    #[test]
    fn what_is_asked_while_the_match_is_held_waits_for_it_to_go_on() {
        let clock = Clock::stopped();
        let (mut host, mut clients) = hosted_by(&clock, &[&[1], &[2]]);
        play(&mut host, &mut clients, 5);
        wait_out(&clock, &mut host, &mut clients, 1, 3);
        assert!(host.held());
        let (first, then) = (go(1, 0, [5.0, 5.0]), go(1, 0, [6.0, 6.0]));
        clients[0].ask(first.clone()).unwrap();
        wait_out(&clock, &mut host, &mut clients, 1, 1);
        assert!(asked(host.schedule().record()).is_empty());
        // Not dealt with until it's carried out.
        assert_eq!(clients[0].answered(), 0);
        clients[1].advance(usize::MAX).unwrap();
        clients[0].ask(then.clone()).unwrap();
        host.turn();
        let given: Vec<_> = (asked(host.schedule().record()).into_iter())
            .map(|g| g.action)
            .collect();
        assert_eq!(given, [first, then]);
        clients[0].advance(usize::MAX).unwrap();
        assert_eq!(clients[0].answered(), 2);
    }

    #[test]
    fn what_is_said_for_everyone_is_passed_on_at_once_held_or_not() {
        let clock = Clock::stopped();
        let (mut host, mut clients) = hosted_by(&clock, &[&[1], &[2], &[]]);
        play(&mut host, &mut clients, 5);
        wait_out(&clock, &mut host, &mut clients, 1, 3);
        assert!(host.held());
        clients[0].say("wait for me".into()).unwrap();
        clients[2].say("hi".into()).unwrap();
        // Too long, or nothing at all, isn't passed on.
        clients[0].say("a".repeat(tgsim::CHAT_LONGEST + 1)).unwrap();
        clients[0].say(" ".into()).unwrap();
        host.turn();
        clients[0].advance(usize::MAX).unwrap();
        let said = |from, name: &str, words: &str| Said {
            from,
            name: name.into(),
            words: words.into(),
        };
        assert_eq!(
            clients[0].take_said(),
            [
                said(Some(1), "Player 1", "wait for me"),
                said(None, "Player 3", "hi")
            ]
        );
        // Nor is any of it in a step.
        assert!(asked(host.schedule().record()).is_empty());
    }

    #[test]
    fn a_client_unheard_from_by_the_host_finds_it_lost() {
        let clock = Clock::stopped();
        let (mut host, mut clients) = hosted_by(&clock, &[&[1]]);
        play(&mut host, &mut clients, 2);
        clock.pass(Duration::from_millis(2900));
        assert_eq!(clients[0].advance(usize::MAX), Ok(()));
        // A host with nothing to say still says so.
        host.turn();
        clock.pass(Duration::from_millis(2900));
        assert_eq!(clients[0].advance(usize::MAX), Ok(()));
        clock.pass(Duration::from_millis(3000));
        assert_eq!(clients[0].advance(usize::MAX), Err(crate::Lost));
    }

    #[test]
    fn clients_are_let_in_before_the_match_begins_to_players_it_has() {
        let (mut host, _clients) = hosted(&[&[1]]);
        let mut admit = |players: &[u32]| {
            let (ours, theirs) = pair();
            drop(theirs);
            host.admit(ours, players.to_vec(), "Bob")
        };
        assert_eq!(admit(&[1]), Err(Refusal::Taken(1)));
        assert_eq!(admit(&[2, 7]), Err(Refusal::NoPlayer(7)));
        // Its other end having gone already.
        assert_eq!(admit(&[2]), Err(Refusal::Lost));
        let (ours, _theirs) = pair();
        assert_eq!(host.admit(ours, vec![2], "Bob"), Ok(2));
        host.turn();
        let mut admit = |players: &[u32]| host.admit(pair().0, players.to_vec(), "Bob");
        assert_eq!(admit(&[]), Err(Refusal::Begun));
    }

    #[test]
    fn a_host_hears_which_clients_are_ready_before_the_match_begins() {
        let (mut host, mut clients) = hosted(&[&[1], &[2], &[]]);
        assert!(host.waiting());
        clients[1].ready().unwrap();
        clients[1].ready().unwrap();
        clients[0].ready().unwrap();
        assert_eq!(host.wait(), []);
        assert!(host.waiting());
        // One that goes isn't waited on, unless it's one of those wanted.
        drop(clients.remove(2));
        assert_eq!(host.wait(), [Event::Left(3)]);
        assert!(!host.waiting());
        host.want(3);
        assert!(host.waiting());
        let (ours, mut theirs) = pair();
        assert_eq!(host.admit(ours, vec![], "Bob"), Ok(4));
        welcome(&mut theirs).unwrap();
        let mut fourth = Client::new(theirs);
        assert!(host.waiting());
        fourth.ready().unwrap();
        host.wait();
        assert!(!host.waiting());
        // With those it wants, it lets in no more.
        let (ours, mut late) = pair();
        assert_eq!(host.admit(ours, vec![], "Bob"), Err(Refusal::Full));
        assert_eq!(welcome(&mut late), Err(NotJoined::Refused(Refusal::Full)));
        assert_eq!((host.clients(), host.world().ticks), (3, 0));
        // Nor is a match under way held up by hearing it.
        clients[0].ready().unwrap();
        play(&mut host, &mut clients, 2);
        assert_eq!(clients[1].world().ticks, 2);
    }

    #[test]
    fn while_copies_are_set_up_the_match_is_held_and_dropped_from_as_in_play() {
        let clock = Clock::stopped();
        let (mut host, mut clients) = hosted_by(&clock, &[&[1], &[2], &[]]);
        host.want(3);
        clients[0].ready().unwrap();
        clients[1].ready().unwrap();
        // The watcher's copy is still being set up when it goes quiet.
        let wait_out = |host: &mut Host<Local>, clients: &mut [Copy], seconds| {
            let mut events = Vec::new();
            for _ in 0..seconds {
                clock.pass(Duration::from_secs(1));
                events.extend(host.wait());
                for c in &mut clients[..2] {
                    c.wait().unwrap();
                }
            }
            events
        };
        // One yet to say anything since it was let in is still starting
        // up, and holds nothing.
        assert_eq!(wait_out(&mut host, &mut clients, 3), []);
        clients[2].wait().unwrap();
        assert_eq!(wait_out(&mut host, &mut clients, 3), [Event::Held(true)]);
        let status = clients[0].status().unwrap().clone();
        assert!(status.held);
        assert_eq!(status.seats[2].standing, Standing::Disconnected);
        clients[1].say("hurry up".into()).unwrap();
        clients[0].vote(true).unwrap();
        clients[1].vote(true).unwrap();
        let events = wait_out(&mut host, &mut clients, 15);
        assert_eq!(events, [Event::Left(3), Event::Held(false)]);
        assert_eq!(clients[0].take_left(), [gone(3, &[])]);
        let said: Vec<_> = clients[0]
            .take_said()
            .into_iter()
            .map(|s| s.words)
            .collect();
        assert_eq!(said, ["hurry up"]);
        // It has had all it wanted, so begins without the one gone, and
        // takes nobody in its place.
        assert!(!host.waiting());
        let (ours, _theirs) = pair();
        assert_eq!(host.admit(ours, vec![], "Bob"), Err(Refusal::Full));
        assert_eq!(host.world().ticks, 0);
    }

    #[test]
    fn a_client_is_told_when_what_it_asked_is_dealt_with() {
        let (mut host, mut clients) = hosted(&[&[1], &[2]]);
        // Dropped, before the match begins.
        clients[0].ask(Action::Pause(true)).unwrap();
        assert_eq!((clients[0].asked(), clients[0].answered()), (1, 0));
        assert_eq!(host.wait(), []);
        clients[0].advance(usize::MAX).unwrap();
        assert_eq!(clients[0].answered(), 1);
        assert!(!clients[0].schedule().paused());
        // Carried out or not, and only once its copy has done it.
        host.trust(2);
        clients[0].ask(Action::Win(1)).unwrap();
        clients[0].ask(Action::Speed(4.0)).unwrap();
        clients[1].ask(Action::Speed(2.0)).unwrap();
        host.turn();
        let Copy { client, world } = &mut clients[0];
        client.play(world, 0).unwrap();
        assert_eq!((client.answered(), client.schedule().speed()), (1, 1.0));
        client.play(world, 1).unwrap();
        assert_eq!((client.answered(), client.schedule().speed()), (3, 2.0));
        clients[1].advance(usize::MAX).unwrap();
        assert_eq!((clients[1].asked(), clients[1].answered()), (1, 1));
    }

    #[test]
    fn a_client_keeping_time_itself_plays_only_the_ticks_it_asks_for() {
        let (mut host, mut clients) = hosted(&[&[1], &[2]]);
        host.turn();
        host.turn();
        clients[0].ask(Action::Pause(true)).unwrap();
        host.turn();
        let Copy { client, world } = &mut clients[1];
        assert_eq!(client.behind(), 2);
        // Up to the tick after those asked for, and no further.
        client.play(world, 1).unwrap();
        assert_eq!((world.ticks, client.behind()), (1, 1));
        assert!(!client.schedule().paused());
        client.play(world, 0).unwrap();
        assert_eq!(world.ticks, 1);
        client.play(world, 5).unwrap();
        assert_eq!((world.ticks, client.behind()), (2, 0));
        // What comes after the last tick needs none.
        assert!(client.schedule().paused());
        assert_eq!(world.hash(), host.world().hash());
        // The host is only found gone once all it sent is played.
        clients[0].ask(Action::Pause(false)).unwrap();
        host.turn();
        drop(host);
        let Copy { client, world } = &mut clients[1];
        assert_eq!(client.play(world, 0), Ok(()));
        assert_eq!(client.play(world, 1), Err(crate::Lost));
        assert_eq!(world.ticks, 3);
    }

    #[test]
    fn a_client_that_goes_or_sends_nonsense_leaves_and_the_match_goes_on() {
        let (mut host, mut clients) = hosted(&[&[1], &[2], &[]]);
        play(&mut host, &mut clients, 3);
        let gone = clients.remove(1);
        drop(gone);
        clients[1].link().send(vec![250, 1]).unwrap();
        assert_eq!(host.turn(), [Event::Left(2), Event::Left(3)]);
        // The one that sent nonsense is cut off.
        assert_eq!(clients[1].advance(usize::MAX), Err(crate::Lost));
        clients[0].ask(go(1, 0, [30.0, 12.0])).unwrap();
        play(&mut host, &mut clients[..1], 10);
        assert_eq!(host.world().ticks, 14);
        assert_eq!(clients[0].world().hash(), host.world().hash());
        // And a client whose host has gone finds out.
        drop(host);
        assert_eq!(clients[0].advance(usize::MAX), Err(crate::Lost));
        assert_eq!(clients[0].ask(Action::Pause(true)), Err(crate::Lost));
    }

    #[test]
    fn clients_are_told_who_leaves_and_when_the_match_ends() {
        let (mut host, mut clients) = hosted(&[&[1], &[2], &[]]);
        play(&mut host, &mut clients, 2);
        // One says it's going, another just goes.
        clients[1].leave().unwrap();
        drop(clients.remove(2));
        assert_eq!(host.turn(), [Event::Left(2), Event::Left(3)]);
        assert_eq!(clients[1].advance(usize::MAX), Err(crate::Lost));
        clients.remove(1);
        let left = [gone(2, &[2]), gone(3, &[])];
        // In step with what the host played before it heard.
        let Copy { client, world } = &mut clients[0];
        client.play(world, 0).unwrap();
        assert_eq!(client.take_left(), []);
        client.play(world, 1).unwrap();
        assert_eq!((world.ticks, client.take_left()), (3, left.to_vec()));
        assert!(!client.ended());
        // Once the match is over, the host leaving it ends it.
        host.trust(1);
        clients[0].ask(Action::Win(1)).unwrap();
        play(&mut host, &mut clients, 1);
        assert!(host.world().over);
        let (_, record) = host.finish();
        assert_eq!(clients[0].advance(usize::MAX), Err(crate::Lost));
        assert!(clients[0].ended());
        assert_eq!(clients[0].world().ticks, record.steps);
    }

    #[test]
    fn a_host_leaving_a_match_still_played_is_heard_to_quit() {
        // The first is whoever runs the host.
        let (mut host, mut clients) = hosted(&[&[1], &[2]]);
        host.trust(1);
        play(&mut host, &mut clients, 2);
        host.finish();
        let theirs = &mut clients[1];
        assert_eq!(theirs.advance(usize::MAX), Err(crate::Lost));
        assert!(!theirs.ended());
        assert_eq!(theirs.take_left(), [gone(1, &[1])]);
    }

    #[test]
    fn a_host_leaving_a_match_that_is_over_ends_it_then_is_heard_to_quit() {
        let (mut host, mut clients) = hosted(&[&[1], &[2]]);
        host.trust(1);
        clients[0].ask(Action::Win(1)).unwrap();
        play(&mut host, &mut clients, 1);
        host.finish();
        let theirs = &mut clients[1];
        assert_eq!(theirs.advance(usize::MAX), Err(crate::Lost));
        assert!(theirs.ended());
        assert_eq!(theirs.take_left(), [gone(1, &[1])]);
    }

    #[test]
    fn a_client_is_heard_only_so_much_a_turn() {
        let (mut host, mut clients) = hosted(&[&[1], &[2]]);
        for _ in 0..HEARD_A_TURN + 5 {
            clients[0].ask(Action::Pause(false)).unwrap();
        }
        assert_eq!(host.turn(), []);
        assert_eq!(asked(host.schedule().record()).len(), HEARD_A_TURN);
        // The rest the turn after.
        assert_eq!(host.turn(), []);
        assert_eq!(asked(host.schedule().record()).len(), HEARD_A_TURN + 5);
        // But for one it trusts, which is heard out.
        host.trust(1);
        for _ in 0..HEARD_A_TURN + 5 {
            clients[0].ask(Action::Pause(false)).unwrap();
        }
        assert_eq!(host.turn(), []);
        assert_eq!(asked(host.schedule().record()).len(), 2 * HEARD_A_TURN + 10);
        // And a message too long to be anything it has to say, not at all.
        clients[1].link().send(vec![0; LONGEST_HEARD + 1]).unwrap();
        assert_eq!(host.turn(), [Event::Left(2)]);
    }

    #[test]
    fn a_trusted_client_is_let_do_anything() {
        let (mut host, mut clients) = hosted(&[&[], &[2]]);
        host.trust(1);
        clients[0].ask(go(2, 1, [5.0, 30.0])).unwrap();
        clients[0].ask(Action::Lose(1)).unwrap();
        clients[0].ask(Action::Pause(true)).unwrap();
        clients[1].ask(Action::Lose(1)).unwrap();
        play(&mut host, &mut clients, 1);
        assert_eq!(asked(host.schedule().record()).len(), 3);
    }

    #[test]
    fn a_host_that_keeps_time_itself_says_when_a_tick_is_played() {
        let (mut host, mut clients) = hosted(&[&[1]]);
        clients[0].ask(go(1, 0, [30.0, 12.0])).unwrap();
        clients[0].ask(Action::Pause(true)).unwrap();
        // What's asked is carried out without a tick going by.
        assert_eq!(host.relay(), []);
        clients[0].advance(usize::MAX).unwrap();
        assert_eq!(clients[0].world().ticks, 0);
        assert_eq!(asked(clients[0].schedule().record()).len(), 2);
        assert!(clients[0].schedule().paused());
        // And a tick is played though the match is held still.
        assert_eq!(host.tick(), []);
        clients[0].advance(usize::MAX).unwrap();
        assert_eq!((host.world().ticks, clients[0].world().ticks), (1, 1));
        assert_eq!(clients[0].world().hash(), host.world().hash());
    }

    #[test]
    fn clients_that_ask_to_join_are_let_in_or_told_why_not() {
        let mut host = Host::new(setup(), fresh());
        let mut ends = Vec::new();
        for _ in 0..4 {
            let (ours, theirs) = pair();
            host.greet(ours);
            ends.push(theirs);
        }
        // Nothing until they ask.
        assert_eq!(host.seat(), []);
        // A player asked for twice is asked for once.
        crate::join(&mut ends[0], vec![1, 1], "Bob".into()).unwrap();
        crate::join(&mut ends[1], vec![1, 2], "Bob".into()).unwrap();
        let older = Join {
            protocol: PROTOCOL + 1,
            players: vec![2],
            name: "Bob".into(),
            password: String::new(),
        };
        ends[2].send(encode(&older)).unwrap();
        let taken = Refusal::Taken(1);
        let spoken = Refusal::Protocol(PROTOCOL);
        assert_eq!(host.seat(), [Ok(1), Err(taken), Err(spoken)]);
        let welcomed = welcome(&mut ends[0]).unwrap().unwrap();
        assert_eq!((welcomed.client, welcomed.players), (1, vec![1]));
        assert_eq!(welcome(&mut ends[1]), Err(NotJoined::Refused(taken)));
        assert_eq!(welcome(&mut ends[2]), Err(NotJoined::Refused(spoken)));
        // One that hasn't asked by the time the match begins is too late.
        assert_eq!(welcome(&mut ends[3]), Ok(None));
        host.turn();
        assert_eq!(host.seat(), [Err(Refusal::Begun)]);
        let begun = NotJoined::Refused(Refusal::Begun);
        assert_eq!(welcome(&mut ends[3]), Err(begun));
        // As is one that comes after.
        let (ours, mut late) = pair();
        host.greet(ours);
        crate::join(&mut late, Vec::new(), "Bob".into()).unwrap();
        assert_eq!(host.seat(), [Err(Refusal::Begun)]);
        assert_eq!(welcome(&mut late), Err(begun));
    }

    #[test]
    fn each_side_a_person_plays_goes_by_their_name_unlike_anyone_else() {
        let mut host = Host::new(setup(), fresh());
        host.name_by(Naming {
            numbered: "%s #%d".into(),
            ..Naming::default()
        });
        let mut admit = |players: Vec<u32>, name: &str| {
            let (ours, theirs) = pair();
            let admitted = host.admit(ours, players, name);
            admitted.map(|_| theirs)
        };
        let mut ends = [
            admit(vec![2], " Bob ").unwrap(),
            admit(vec![], "Bob").unwrap(),
            admit(vec![1, 3], "Bob").unwrap(),
        ];
        // Each is told what it goes by.
        let told = ends
            .each_mut()
            .map(|end| welcome(end).unwrap().unwrap().name);
        assert_eq!(told, ["Bob", "Bob #1", "Bob #2"]);
        // What's no name isn't let in.
        assert!(matches!(admit(vec![4], "  "), Err(Refusal::Lost)));
        let named = |seat: &Seated| (seat.players.clone(), seat.name.clone());
        let seats: Vec<_> = host.status().seats.iter().map(named).collect();
        let bobs = [(vec![2], "Bob"), (vec![], "Bob #1"), (vec![1, 3], "Bob #2")];
        assert_eq!(seats, bobs.map(|(p, n)| (p, n.to_string())));
        host.turn();
        let names = [(1, "Bob #2"), (2, "Bob"), (3, "Bob #2")];
        let names = names.map(|(p, n)| (p, n.to_string())).into();
        assert_eq!(host.world().sides.names, names);
        // Nobody asks for them, and so nobody's answered.
        assert_eq!(asked(host.schedule().record()), []);
    }

    #[test]
    fn only_so_many_are_kept_waiting_to_ask() {
        let mut host = Host::new(setup(), fresh());
        let ends = (0..GREETED + 1).map(|_| {
            let (ours, theirs) = pair();
            host.greet(ours);
            theirs
        });
        let mut ends: Vec<Local> = ends.collect();
        let full = NotJoined::Refused(Refusal::Full);
        assert_eq!(welcome(&mut ends[GREETED]), Err(full));
        assert_eq!(welcome(&mut ends[0]), Ok(None));
        // What asks with nonsense is gone.
        ends[0].send(vec![250; 40]).unwrap();
        assert_eq!(host.seat(), [Err(Refusal::Lost)]);
    }

    #[test]
    fn the_sides_of_one_who_leaves_are_taken_over_by_computer_players() {
        let (mut host, mut clients) = hosted(&[&[1], &[2, 3]]);
        play(&mut host, &mut clients, 2);
        clients[1].leave().unwrap();
        assert_eq!(host.turn(), [Event::Left(2)]);
        clients.remove(1);
        play(&mut host, &mut clients, 2);
        let computers = [(2, "Player 2 AI"), (3, "Player 2 AI (1)")];
        let computers = computers.map(|(p, n)| (p, n.to_string())).into();
        assert_eq!(host.world().sides.computers, computers);
        let names = [(1, "Player 1".to_string())].into();
        assert_eq!(host.world().sides.names, names);
        assert_eq!(clients[0].world().sides.computers, computers);
        assert_eq!(clients[0].world().hash(), host.world().hash());
    }

    /// A computer player that sends its side's organization, if it's the
    /// second, along x, noting each side it thinks for.
    struct Marcher(Thought);

    type Thought = std::sync::Arc<std::sync::Mutex<Vec<u32>>>;

    impl ComputerPlayer for Marcher {
        fn think(&mut self, _: &World, player: u32) -> Vec<Command> {
            self.0.lock().unwrap().push(player);
            let to = [30.0, 20.0];
            (player == 2)
                .then_some(Command::Move {
                    organization: 1,
                    to,
                })
                .into_iter()
                .collect()
        }
    }

    /// Has `host` play the sides nobody else does, and take sides over, by
    /// [`Marcher`]s, giving what they note.
    fn marching(host: &mut Host<Local>) -> Thought {
        let thought = Thought::default();
        let noted = std::sync::Arc::clone(&thought);
        host.computers_by(move |_, _, _| Box::new(Marcher(noted.clone())));
        let noted = std::sync::Arc::clone(&thought);
        host.takers_by(move |_, _| (Box::new(Marcher(noted.clone())), Some("Marcher AI".into())));
        thought
    }

    #[test]
    fn computer_players_give_the_orders_of_the_sides_nobody_else_plays() {
        let (mut host, mut clients) = hosted(&[&[1], &[]]);
        let thought = marching(&mut host);
        play(&mut host, &mut clients, 2);
        // Each side but the person's, every turn.
        assert_eq!(*thought.lock().unwrap(), [2, 3, 4, 2, 3, 4]);
        let ordered = Action::Order {
            player: 2,
            command: Command::Move {
                organization: 1,
                to: [30.0, 20.0],
            },
        };
        let given: Vec<_> = asked(host.schedule().record());
        assert_eq!(
            given.iter().map(|g| &g.action).collect::<Vec<_>>(),
            [&ordered, &ordered]
        );
        play(&mut host, &mut clients, 20);
        assert!(host.world().units[1].position[0] > 10.0);
        for client in &clients {
            assert_eq!(client.world().hash(), host.world().hash());
        }
    }

    /// Lets a client into `host` giving the orders of `players` and
    /// running the computer players of `computers`.
    fn running(host: &mut Host<Local>, players: &[u32], computers: &[u32]) -> Copy {
        let (ours, mut theirs) = pair();
        let (players, computers) = (players.to_vec(), computers.to_vec());
        host.admit_running(ours, players, computers.clone(), "Bob")
            .unwrap();
        let welcome = welcome(&mut theirs).unwrap().unwrap();
        assert_eq!(welcome.computers, computers);
        Copy {
            client: Client::new(theirs),
            world: fresh(),
        }
    }

    #[test]
    fn a_client_gives_the_orders_of_the_computer_players_its_machine_runs() {
        let mut host = Host::new(setup(), fresh());
        let thought = marching(&mut host);
        let mut clients = vec![
            running(&mut host, &[1], &[]),
            running(&mut host, &[3], &[2]),
        ];
        play(&mut host, &mut clients, 1);
        // The host thinks only for the side no machine plays.
        assert_eq!(*thought.lock().unwrap(), [4]);
        let to = |x| Command::Move {
            organization: 1,
            to: [x, 20.0],
        };
        let order = |x| Action::Order {
            player: 2,
            command: to(x),
        };
        clients[0].ask(order(40.0)).unwrap();
        clients[1].ask(order(30.0)).unwrap();
        play(&mut host, &mut clients, 1);
        let given: Vec<_> = asked(host.schedule().record());
        // Only the client running it gives its orders.
        assert_eq!(
            given.iter().map(|g| &g.action).collect::<Vec<_>>(),
            [&order(30.0)]
        );
    }

    #[test]
    fn the_host_runs_the_computer_players_of_a_machine_that_leaves() {
        let mut host = Host::new(setup(), fresh());
        let thought = marching(&mut host);
        let mut clients = vec![
            running(&mut host, &[1], &[]),
            running(&mut host, &[3], &[2]),
        ];
        play(&mut host, &mut clients, 1);
        clients[1].leave().unwrap();
        assert_eq!(host.turn(), [Event::Left(2)]);
        clients.remove(1);
        thought.lock().unwrap().clear();
        play(&mut host, &mut clients, 1);
        assert_eq!(*thought.lock().unwrap(), [2, 3, 4]);
    }

    /// The takeovers `host` has had every copy of the match carry out.
    fn takeovers(host: &Host<Local>) -> Vec<(u32, String, String)> {
        let given = host.schedule().record().given.iter();
        let taken_over = given.filter_map(|g| match &g.action {
            Action::TakeOver { player, name, by } => Some((*player, name.clone(), by.clone())),
            _ => None,
        });
        taken_over.collect()
    }

    #[test]
    fn every_side_of_a_machine_that_leaves_is_taken_over_in_the_map_s_order() {
        let mut host = Host::new(setup(), fresh());
        let naming = Naming {
            computers: [(2, "Kesselring AI".to_string())].into(),
            ..Naming::default()
        };
        host.name_by(naming);
        marching(&mut host);
        let mut clients = vec![
            running(&mut host, &[1], &[]),
            running(&mut host, &[3], &[2]),
        ];
        play(&mut host, &mut clients, 1);
        clients[1].leave().unwrap();
        host.turn();
        clients.remove(1);
        play(&mut host, &mut clients, 1);
        // Each named after what it went by, the computer player's side
        // after its profile.
        let taken_over = |p, name: &str| (p, name.to_string(), "Marcher AI".to_string());
        assert_eq!(
            takeovers(&host),
            [
                taken_over(2, "Kesselring AI AI"),
                taken_over(3, "Bob (1) AI")
            ]
        );
        assert_eq!(clients[0].world().hash(), host.world().hash());
    }

    #[test]
    fn a_side_that_has_lost_is_not_taken_over() {
        let mut host = Host::new(setup(), fresh());
        marching(&mut host);
        let mut clients = vec![
            running(&mut host, &[1], &[]),
            running(&mut host, &[3], &[2]),
        ];
        host.trust(1);
        clients[0].ask(Action::Lose(2)).unwrap();
        play(&mut host, &mut clients, 1);
        clients[1].leave().unwrap();
        host.turn();
        clients.remove(1);
        play(&mut host, &mut clients, 1);
        let sides: Vec<u32> = takeovers(&host).into_iter().map(|(p, ..)| p).collect();
        assert_eq!(sides, [3]);
    }

    #[test]
    fn a_side_another_machine_runs_a_computer_player_for_is_taken() {
        let mut host = Host::new(setup(), fresh());
        let _bob = running(&mut host, &[], &[2]);
        let (ours, _theirs) = pair();
        assert_eq!(host.admit(ours, vec![2], "Al"), Err(Refusal::Taken(2)));
        let (ours, _theirs) = pair();
        let refused = host.admit_running(ours, vec![1], vec![2], "Al");
        assert_eq!(refused, Err(Refusal::Taken(2)));
    }

    #[test]
    fn computer_players_think_only_while_the_match_goes_on() {
        let (mut host, mut clients) = hosted(&[&[1]]);
        let thought = marching(&mut host);
        play(&mut host, &mut clients, 1);
        // Not on the turn it's paused, nor while it is.
        clients[0].ask(Action::Pause(true)).unwrap();
        play(&mut host, &mut clients, 3);
        assert_eq!(*thought.lock().unwrap(), [2, 3, 4]);
        // But on the turn it goes on.
        clients[0].ask(Action::Pause(false)).unwrap();
        play(&mut host, &mut clients, 1);
        assert_eq!(*thought.lock().unwrap(), [2, 3, 4, 2, 3, 4]);
    }

    #[test]
    fn computer_players_think_only_while_the_match_isnt_held() {
        let clock = Clock::stopped();
        let (mut host, mut clients) = hosted_by(&clock, &[&[1], &[2]]);
        let thought = marching(&mut host);
        play(&mut host, &mut clients, 1);
        wait_out(&clock, &mut host, &mut clients, 1, 3);
        assert!(host.held());
        thought.lock().unwrap().clear();
        wait_out(&clock, &mut host, &mut clients, 1, 2);
        assert!(host.held());
        assert_eq!(*thought.lock().unwrap(), []);
    }

    #[test]
    fn computer_players_stop_for_sides_out_of_the_match() {
        let (mut host, mut clients) = hosted(&[&[1]]);
        let thought = marching(&mut host);
        host.trust(1);
        clients[0].ask(Action::Lose(3)).unwrap();
        play(&mut host, &mut clients, 1);
        thought.lock().unwrap().clear();
        play(&mut host, &mut clients, 1);
        assert_eq!(*thought.lock().unwrap(), [2, 4]);
    }

    #[test]
    fn a_computer_player_plays_on_for_one_who_leaves() {
        let (mut host, mut clients) = hosted(&[&[1], &[2]]);
        let thought = marching(&mut host);
        play(&mut host, &mut clients, 1);
        clients[1].leave().unwrap();
        assert_eq!(host.turn(), [Event::Left(2)]);
        clients.remove(1);
        thought.lock().unwrap().clear();
        play(&mut host, &mut clients, 1);
        assert_eq!(*thought.lock().unwrap(), [2, 3, 4]);
    }

    #[test]
    fn the_side_of_one_who_leaves_while_the_match_is_set_up_is_taken_over_as_it_begins() {
        let (mut host, mut clients) = hosted(&[&[1], &[2]]);
        host.want(2);
        assert_eq!(host.wait(), []);
        drop(clients.remove(1));
        assert_eq!(host.wait(), [Event::Left(2)]);
        assert_eq!(host.world().sides.computers, [].into());
        play(&mut host, &mut clients, 1);
        let taken_over = Action::TakeOver {
            player: 2,
            name: "Player 2 AI".into(),
            // With no profile, it goes by its own name.
            by: "Player 2 AI".into(),
        };
        let given = host.schedule().record().given.iter().map(|g| &g.action);
        let theirs: Vec<_> = given
            .filter(|a| !matches!(a, Action::Name { player: 1, .. }))
            .collect();
        let named = Action::Name {
            player: 2,
            name: "Player 2".into(),
        };
        assert_eq!(theirs, [&named, &taken_over]);
        let names = [(1, "Player 1".to_string())].into();
        assert_eq!(clients[0].world().sides.names, names);
    }

    #[test]
    fn a_side_is_taken_over_at_once_while_someone_else_holds_the_match() {
        let clock = Clock::stopped();
        let (mut host, mut clients) = hosted_by(&clock, &[&[1], &[2], &[3]]);
        play(&mut host, &mut clients, 2);
        wait_out(&clock, &mut host, &mut clients, 2, 3);
        assert!(host.held());
        clients[1].leave().unwrap();
        host.turn();
        clients.remove(1);
        wait_out(&clock, &mut host, &mut clients, 1, 1);
        assert!(host.held());
        let computers = [(2, "Player 2 AI".to_string())].into();
        assert_eq!(host.world().sides.computers, computers);
        assert_eq!(clients[0].world().sides.computers, computers);
        assert_eq!(clients[0].world().ticks, host.world().ticks);
    }

    #[test]
    fn a_computer_player_taking_a_side_over_is_named_unlike_anyone() {
        let (mut host, mut clients) = hosted(&[&[1], &[2], &[]]);
        // The third person goes by the name the second's computer player
        // would, and the map's computer player on side 4 by the next.
        let mut computers = BTreeMap::new();
        computers.insert(4, "Player 2 AI (1)".to_string());
        // Side 3 is played by nobody but its computer player either, but
        // has none of its faction.
        host.name_by(Naming {
            computers,
            ..Naming::default()
        });
        let (ours, _theirs) = pair();
        host.admit(ours, vec![], "Player 2 AI").unwrap();
        play(&mut host, &mut clients, 1);
        clients[1].leave().unwrap();
        host.turn();
        clients.remove(1);
        play(&mut host, &mut clients[..1], 1);
        let name = &host.world().sides.computers[&2];
        assert_eq!(name, "Player 2 AI (2)");
    }

    #[test]
    fn a_person_who_left_before_the_match_began_is_named_unlike_those_let_in_since() {
        let (mut host, mut clients) = hosted(&[&[1], &[2]]);
        assert_eq!(host.wait(), []);
        drop(clients.remove(1));
        assert_eq!(host.wait(), [Event::Left(2)]);
        // With no say how many it wants, it lets in another, who goes by
        // the name the one gone did.
        let (ours, _theirs) = pair();
        host.admit(ours, vec![3], &named(2)).unwrap();
        host.turn();
        let given = host.schedule().record().given.iter().map(|g| &g.action);
        let taken_over = Action::TakeOver {
            player: 2,
            name: "Player 2 (1) AI".into(),
            // With no profile, it goes by its own name.
            by: "Player 2 (1) AI".into(),
        };
        assert!(given.clone().any(|a| *a == taken_over));
    }

    #[test]
    fn a_client_that_goes_before_the_match_begins_leaves_its_players_free() {
        let (mut host, mut clients) = hosted(&[&[1], &[2]]);
        assert_eq!(host.wait(), []);
        // What's asked before it begins comes to nothing.
        clients[1].ask(Action::Pause(true)).unwrap();
        drop(clients.remove(0));
        assert_eq!(host.wait(), [Event::Left(1)]);
        assert_eq!(host.clients(), 1);
        let (ours, _theirs) = pair();
        assert_eq!(host.admit(ours, vec![1], "Bob"), Ok(3));
        play(&mut host, &mut clients, 2);
        assert!(asked(host.schedule().record()).is_empty());
        assert_eq!(host.world().ticks, 2);
    }
}
