//! A game being set up before it's launched: who's in it, the side each
//! person takes and what they're to play, the same for everyone in it.
//! Every change goes through the host, which tells everyone what the game
//! has come to, and at launch hands each person's link on to the host of
//! the match.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tgdata::{Id, RuleValues};

use crate::client::{ask_in, first_answer};
use crate::host::{LONGEST_HEARD, greet, joining, worth_saying};
use crate::presets::Preset;
use crate::protocol::{Gone, Hearers, Said, ToClient, ToHost, Welcome, decode, encode};
use crate::slots::Slots;
use crate::{Clock, Host, Link, Lost, Naming, NotJoined, Refusal, Setup, ping, refuse};

/// The seconds a launch counts down, unless the game's data says
/// otherwise.
pub const COUNTDOWN: u32 = 5;

/// How often everyone is told what the game has come to, changed or not,
/// so that they know the host is still there.
const TOLD: Duration = Duration::from_secs(1);

/// What a game being set up has chosen so far.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Staging {
    /// What the game is called.
    pub name: String,
    /// Whether it takes a password to come in.
    pub locked: bool,
    /// The most people it lets in, observers among them.
    pub most: usize,
    /// Whether people may come in to watch.
    pub observers: bool,
    /// The map to play, by its name in the game's data.
    pub map: String,
    /// The sides of the map a person may take, by player id.
    pub sides: Vec<u32>,
    /// How the map's sides are put in teams.
    pub preset: Preset,
    /// The world rules changed from their defaults.
    pub rules: RuleValues,
    /// What's chosen for each side.
    pub slots: Slots,
    /// The machine each computer player belongs to, by player id, as the
    /// host knows it; whoever runs the host's where none is.
    pub owners: BTreeMap<u32, u32>,
    /// Everyone in the game, in the order they came in.
    pub guests: Vec<Guest>,
    /// Offers to trade sides that stand, from one person to another, by
    /// what the host knows each as.
    pub offers: BTreeSet<(u32, u32)>,
    /// The seconds left before the game launches, while it counts down.
    pub countdown: Option<u32>,
}

/// A person in a game being set up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Guest {
    /// What the host knows their machine as.
    pub client: u32,
    pub name: String,
    /// The side they take, by player id; none to watch.
    pub side: Option<u32>,
    /// Whether they've marked themselves ready.
    pub ready: bool,
    /// Whether it's whoever runs the host.
    pub host: bool,
    /// Whether their machine has the map chosen, which they need to be
    /// ready.
    pub holds: bool,
    /// How long, in whole milliseconds, a message takes the host to get
    /// to them and back, where it measures that.
    pub ping: Option<u32>,
}

impl Guest {
    /// Whether they count as ready, as launching takes: whoever runs the
    /// host and anyone watching always do, and anyone else once they've
    /// said so and have the map.
    pub fn counts_ready(&self) -> bool {
        self.host || self.side.is_none() || (self.ready && self.holds)
    }
}

impl Staging {
    pub fn guest(&self, client: u32) -> Option<&Guest> {
        self.guests.iter().find(|g| g.client == client)
    }

    fn guest_mut(&mut self, client: u32) -> Option<&mut Guest> {
        self.guests.iter_mut().find(|g| g.client == client)
    }

    /// Whether someone takes side `player`.
    pub fn taken(&self, player: u32) -> bool {
        self.guests.iter().any(|g| g.side == Some(player))
    }

    /// What the machine side `player`'s computer player would belong to is
    /// known as.
    pub fn owner(&self, player: u32) -> Option<u32> {
        let host = || self.guests.iter().find(|g| g.host).map(|g| g.client);
        self.owners.get(&player).copied().or_else(host)
    }

    /// Whether a computer player belonging to `client`'s machine plays side
    /// `player`.
    pub fn owns(&self, client: u32, player: u32) -> bool {
        let computer = !self.taken(player) && !self.slots.is_open(player);
        computer && self.owner(player) == Some(client)
    }

    /// Whether the people at `a` and `b` have an offer to trade sides
    /// between them, made by either.
    pub fn offered(&self, a: u32, b: u32) -> bool {
        self.offers.contains(&(a, b)) || self.offers.contains(&(b, a))
    }

    /// Whether the person at `client` is on a side.
    fn seated(&self, client: u32) -> bool {
        self.guest(client).is_some_and(|g| g.side.is_some())
    }

    /// Whether side `player` is free for someone to take: open, and nobody
    /// takes it.
    fn free(&self, player: u32) -> bool {
        self.slots.is_open(player) && !self.taken(player)
    }

    /// Whether everyone counts as ready.
    pub fn ready(&self) -> bool {
        self.guests.iter().all(Guest::counts_ready)
    }

    /// Whether it has room for someone else to come in: fewer people than
    /// its most, and a side free for them unless they may watch.
    pub fn room(&self) -> bool {
        let free = self.sides.iter().any(|&p| self.free(p));
        self.guests.len() < self.most && (self.observers || free)
    }

    /// Whether the person at `client` may have `choice` made, as things
    /// stand: whoever runs the host makes the game's choices, and each
    /// person those for their own side and their machine's computer
    /// players. While the launch counts down, only being ready or not is
    /// chosen, and stopping it.
    pub fn allows(&self, client: u32, choice: &Choice) -> bool {
        let Some(guest) = self.guest(client) else {
            return false;
        };
        if self.countdown.is_some() {
            return match choice {
                Choice::Ready(_) => guest.side.is_some(),
                Choice::Holds(_, _) => true,
                Choice::Launch(_) => guest.host,
                _ => false,
            };
        }
        let mine = |&player: &u32| self.owns(client, player);
        match choice {
            Choice::Map { .. } | Choice::Rules(_) | Choice::Preset { .. } | Choice::Launch(_) => {
                guest.host
            }
            Choice::Kick(client) => guest.host && self.guest(*client).is_some_and(|g| !g.host),
            Choice::Nation(player, _) | Choice::Faction(player, _) => {
                guest.side == Some(*player) || mine(player)
            }
            // People have no handicap or profile.
            Choice::Handicap(player, _) => mine(player),
            Choice::Computer(player, _) => self.free(*player) || mine(player),
            // Whoever runs the host puts out anyone's.
            Choice::Open(player) => {
                let computer = !self.taken(*player) && !self.slots.is_open(*player);
                mine(player) || (guest.host && computer)
            }
            Choice::Side(None) => guest.side.is_some() && self.observers,
            // Trading places only with their own machine's computer player.
            Choice::Side(Some(player)) => {
                self.sides.contains(player) && (self.free(*player) || mine(player))
            }
            // Whoever runs the host moves its own computer players among the
            // sides a person may take.
            Choice::Shift(from, to) => {
                let taken = |p| self.sides.contains(p);
                let to_ok = self.free(*to) || mine(to);
                guest.host && from != to && taken(from) && taken(to) && mine(from) && to_ok
            }
            // Nobody is ready without the map.
            Choice::Ready(ready) => guest.side.is_some() && (!ready || guest.holds || guest.host),
            Choice::Holds(_, _) => true,
            // Only between two people on sides.
            Choice::Offer(to, true) => *to != client && guest.side.is_some() && self.seated(*to),
            Choice::Offer(to, false) => self.offers.contains(&(client, *to)),
            Choice::Answer(from, accept) => {
                let both = guest.side.is_some() && self.seated(*from);
                self.offers.contains(&(*from, client)) && (both || !accept)
            }
        }
    }

    /// Clears every offer to and from the person at `client`.
    fn withdraw(&mut self, client: u32) {
        self.offers
            .retain(|&(from, to)| from != client && to != client);
    }

    /// Puts the sides in teams as `preset` has it, `sides` being those a
    /// person may then take: anyone on another goes to watch.
    fn arrange(&mut self, preset: Preset, sides: Vec<u32>) {
        self.preset = preset;
        self.sides = sides;
        for guest in &mut self.guests {
            guest.side = guest.side.filter(|s| self.sides.contains(s));
        }
    }

    /// Makes `choice`, which it allows, for the person at `client`.
    fn make(&mut self, client: u32, choice: Choice) {
        let chosen = |map: &mut BTreeMap<u32, Id>, player: u32, id: Option<Id>| match id {
            Some(id) => _ = map.insert(player, id),
            None => _ = map.remove(&player),
        };
        let slots = &mut self.slots;
        match choice {
            Choice::Map {
                map,
                preset,
                sides,
                rules,
            } => {
                self.map = map;
                self.rules = rules;
                // What was chosen for the old map's sides means nothing on
                // the new one's, and each machine has yet to say whether it
                // has the new one.
                *slots = Slots::left_open(&sides);
                self.owners.clear();
                for guest in &mut self.guests {
                    guest.holds = guest.host;
                }
                self.arrange(preset, sides);
                // Those still on a side hold it on the new map.
                for guest in &self.guests {
                    if let Some(side) = guest.side {
                        self.slots.open.remove(&side);
                    }
                }
            }
            Choice::Rules(rules) => self.rules = rules,
            Choice::Preset { preset, sides } => self.arrange(preset, sides),
            Choice::Holds(map, holds) => {
                if map == self.map
                    && let Some(guest) = self.guest_mut(client)
                {
                    guest.holds = holds;
                }
            }
            Choice::Nation(player, id) => {
                chosen(&mut slots.nations, player, id);
                // A faction is one of its nation's.
                slots.factions.remove(&player);
            }
            Choice::Faction(player, id) => chosen(&mut slots.factions, player, id),
            Choice::Handicap(player, id) => chosen(&mut slots.handicaps, player, id),
            Choice::Computer(player, id) => {
                slots.computer(player, id);
                self.owners.insert(player, client);
            }
            Choice::Open(player) => {
                slots.empty(player);
                self.owners.remove(&player);
            }
            Choice::Shift(from, to) => {
                slots.exchange(from, to);
                let (on_from, on_to) = (self.owners.remove(&from), self.owners.remove(&to));
                for (player, owner) in [(from, on_to), (to, on_from)] {
                    if let Some(owner) = owner {
                        self.owners.insert(player, owner);
                    }
                }
            }
            Choice::Side(side) => {
                let Some(guest) = self.guest_mut(client) else {
                    return;
                };
                let left = std::mem::replace(&mut guest.side, side);
                // A computer player they trade places with goes on
                // belonging to its machine.
                let owner = side.and_then(|s| self.owners.remove(&s));
                self.slots.seat(left, side);
                if let (Some(left), Some(owner)) = (left, owner)
                    && !self.slots.is_open(left)
                {
                    self.owners.insert(left, owner);
                }
            }
            Choice::Ready(ready) => {
                if let Some(guest) = self.guest_mut(client) {
                    guest.ready = ready;
                }
            }
            Choice::Offer(to, true) => _ = self.offers.insert((client, to)),
            Choice::Offer(to, false) => _ = self.offers.remove(&(client, to)),
            Choice::Answer(from, false) => _ = self.offers.remove(&(from, client)),
            Choice::Answer(from, true) => {
                let side = |s: &Staging, c| s.guest(c).and_then(|g| g.side);
                let (Some(theirs), Some(mine)) = (side(self, from), side(self, client)) else {
                    return;
                };
                self.slots.trade(theirs, mine);
                for (c, s) in [(from, mine), (client, theirs)] {
                    if let Some(guest) = self.guest_mut(c) {
                        guest.side = Some(s);
                    }
                }
                self.withdraw(from);
                self.withdraw(client);
            }
            // Whoever runs the host launches and puts people out.
            Choice::Launch(_) | Choice::Kick(_) => {}
        }
    }
}

/// A change to what a game being set up has chosen, asked of its host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Choice {
    /// Play `map`, its sides in teams as `preset` has them, `sides` being
    /// those a person may take, under the world `rules` changed from
    /// their defaults.
    Map {
        map: String,
        preset: Preset,
        sides: Vec<u32>,
        rules: RuleValues,
    },
    /// Play the map under these world rules changed from their defaults,
    /// keeping all else that's chosen.
    Rules(RuleValues),
    /// Put the sides in teams as `preset` has them, `sides` being those a
    /// person may then take.
    Preset {
        preset: Preset,
        sides: Vec<u32>,
    },
    /// Whether the person's machine has the map, by its name.
    Holds(String, bool),
    /// The nation, faction or handicap of a side, by player id; none to
    /// leave it to chance or the map.
    Nation(u32, Option<Id>),
    Faction(u32, Option<Id>),
    Handicap(u32, Option<Id>),
    /// Put a computer player on a side nobody takes, by player id, playing
    /// by a profile, or with none, one at random.
    Computer(u32, Option<Id>),
    /// Leave a side nobody takes for nobody, by player id.
    Open(u32),
    /// Move the computer player on the first side, by player id, to the
    /// second, trading places with any computer player there.
    Shift(u32, u32),
    /// Take a side, by player id, or none to watch.
    Side(Option<u32>),
    Ready(bool),
    /// Offer the person at this client to trade sides with them, or
    /// withdraw the offer.
    Offer(u32, bool),
    /// Answer the offer of the person at this client: trade sides with
    /// them, or turn it down.
    Answer(u32, bool),
    /// Launch, counting down if everyone is ready and else asking them to
    /// become so; or stop the countdown.
    Launch(bool),
    /// Put the person at this client out of the game.
    Kick(u32),
}

/// Something everyone in a game being set up is told, or the one it's
/// about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Told {
    /// Someone has come in, going by this name.
    Joined(String),
    /// Whoever runs the host asked everyone to become ready.
    AskedReady,
    /// The launch has begun counting down.
    Counting,
    /// The seconds left before the launch.
    Count(u32),
    /// The launch has stopped counting down.
    Aborted,
    /// What this person asked for was refused, while the launch counts
    /// down if `counting`.
    Refused { choice: Choice, counting: bool },
}

/// The host of a game being set up.
pub struct Lobby<L> {
    staging: Staging,
    /// The link to each guest, by what it knows them as.
    links: Vec<(u32, L)>,
    /// Links to those that have yet to ask to come in.
    greeted: Vec<L>,
    next: u32,
    password: String,
    naming: Naming,
    /// How many seconds the launch counts down from.
    countdown: u32,
    clock: Clock,
    /// When the countdown last went down, and when everyone was last told
    /// what the game has come to.
    counted: Option<Instant>,
    told: Option<Instant>,
}

impl<L: Link> Lobby<L> {
    /// A host of a game set up as `staging` has it, which people need
    /// `password` to come in to, if it isn't empty.
    pub fn new(staging: Staging, password: String) -> Lobby<L> {
        Lobby {
            staging: Staging {
                locked: !password.is_empty(),
                guests: Vec::new(),
                countdown: None,
                ..staging
            },
            links: Vec::new(),
            greeted: Vec::new(),
            next: 1,
            password,
            naming: Naming::default(),
            countdown: COUNTDOWN,
            clock: Clock::default(),
            counted: None,
            told: None,
        }
    }

    pub fn staging(&self) -> &Staging {
        &self.staging
    }

    /// Has it tell the time by `clock`.
    pub fn clock(&mut self, clock: Clock) {
        self.clock = clock;
    }

    /// Has it name those in the game by `naming`.
    pub fn name_by(&mut self, naming: Naming) {
        self.naming = naming;
    }

    /// Has the launch count down from `seconds` rather than
    /// [`COUNTDOWN`].
    pub fn count_down_from(&mut self, seconds: u32) {
        self.countdown = seconds.max(1);
    }

    /// Lets in a person at the other end of `link` going by `name`,
    /// numbered if anyone in the game does already, who asked with
    /// `password`; one that isn't let in is told why. The first let in is
    /// whoever runs the host. Returns what it's known as.
    pub fn admit(&mut self, mut link: L, name: &str, password: &str) -> Result<u32, Refusal> {
        let taken = |name: &str| self.staging.guests.iter().any(|g| g.name == name);
        let name = crate::name(name).map(|n| crate::unique(&n, &self.naming.numbered, taken));
        let host = self.links.is_empty();
        let refused = match &name {
            None => Some(Refusal::Lost),
            Some(_) if self.staging.countdown.is_some() => Some(Refusal::Begun),
            Some(_) if !host && password != self.password => Some(Refusal::Password),
            Some(_) if !host && !self.staging.room() => Some(Refusal::Full),
            Some(name) => {
                let name = name.clone();
                let entered = link.send(encode(&ToClient::Entered {
                    client: self.next,
                    name,
                }));
                entered.err().map(|_| Refusal::Lost)
            }
        };
        if let Some(refusal) = refused {
            refuse(&mut link, refusal);
            return Err(refusal);
        }
        let client = self.next;
        self.next += 1;
        let name = name.expect("a guest is let in with a name");
        // Where nobody may watch, they take the first side free.
        let free = self.staging.sides.iter().find(|&&p| self.staging.free(p));
        let side = free.copied().filter(|_| !self.staging.observers && !host);
        if side.is_some() {
            self.staging.slots.seat(None, side);
        }
        self.staging.guests.push(Guest {
            client,
            name: name.clone(),
            side,
            ready: false,
            host,
            holds: host,
            ping: None,
        });
        self.links.push((client, link));
        self.tell_staging();
        self.tell(&ToClient::Told(Told::Joined(name)));
        Ok(client)
    }

    /// Takes a link to someone who has yet to say what they want, which
    /// [`Lobby::seat`] lets in once they have.
    pub fn greet(&mut self, link: L) {
        greet(&mut self.greeted, link);
    }

    /// Lets in those greeted that have since asked to come in, each as
    /// [`Lobby::admit`] would, returning what became of each that asked.
    pub fn seat(&mut self) -> Vec<Result<u32, Refusal>> {
        let mut seated = Vec::new();
        for mut link in std::mem::take(&mut self.greeted) {
            let asked = match link.receive() {
                Ok(None) => {
                    self.greeted.push(link);
                    continue;
                }
                Ok(Some(message)) => joining(&message),
                Err(_) => Err(Refusal::Lost),
            };
            seated.push(match asked {
                Ok(join) => self.admit(link, &join.name, &join.password),
                Err(refusal) => {
                    refuse(&mut link, refusal);
                    Err(refusal)
                }
            });
        }
        seated
    }

    /// Hears from everyone in the game, making what they choose that
    /// they may and passing on what they say, and counts the launch down.
    /// Whether the game is to launch now.
    pub fn turn(&mut self) -> bool {
        let now = self.clock.now();
        let counting = self.staging.countdown.is_some();
        let mut changed = false;
        let mut gone = Vec::new();
        for at in 0..self.links.len() {
            let client = self.links[at].0;
            loop {
                let heard = match self.links[at].1.receive() {
                    Ok(None) => break,
                    Ok(Some(m)) if m.len() <= LONGEST_HEARD => decode::<ToHost>(&m),
                    Ok(Some(_)) | Err(_) => None,
                };
                match heard {
                    Some(ToHost::Choose(choice)) => {
                        changed |= self.choose(client, choice, &mut gone);
                    }
                    Some(ToHost::Say(words)) if worth_saying(&words) => {
                        self.pass_on(client, Hearers::All, words);
                    }
                    Some(ToHost::Tell { to, words }) if worth_saying(&words) => {
                        self.pass_on(client, to, words);
                    }
                    Some(ToHost::Leave) | None => {
                        gone.push((client, false));
                        break;
                    }
                    // Nothing else means anything before launch.
                    Some(_) => {}
                }
            }
        }
        for (at, (_, link)) in self.links.iter().enumerate() {
            let ping = ping(link);
            changed |= std::mem::replace(&mut self.staging.guests[at].ping, ping) != ping;
        }
        let left = !gone.is_empty();
        for (client, kicked) in gone {
            changed |= self.part(client, kicked);
        }
        // Anyone going or no longer ready stops it.
        if counting && self.staging.countdown.is_some() && (left || !self.staging.ready()) {
            self.abort();
            changed = true;
        }
        changed |= self.count_down(now);
        let due = self.told.is_none_or(|t| now >= t + TOLD);
        if changed || due {
            self.told = Some(now);
            self.tell_staging();
        }
        self.staging.countdown == Some(0)
    }

    /// Has the person at `client` have `choice` made, if they may, or
    /// tells them it's refused, adding anyone put out to `gone`. Whether
    /// anything changed.
    fn choose(&mut self, client: u32, choice: Choice, gone: &mut Vec<(u32, bool)>) -> bool {
        let counting = self.staging.countdown.is_some();
        if !self.staging.allows(client, &choice) {
            let refused = Told::Refused { choice, counting };
            self.tell_one(client, &ToClient::Told(refused));
            return false;
        }
        match choice {
            // Pressed again while it counts down, nothing.
            Choice::Launch(true) if counting => false,
            Choice::Launch(true) if !self.staging.ready() => {
                self.tell(&ToClient::Told(Told::AskedReady));
                false
            }
            Choice::Launch(true) => {
                self.staging.countdown = Some(self.countdown);
                self.counted = Some(self.clock.now());
                self.tell(&ToClient::Told(Told::Counting));
                self.tell(&ToClient::Told(Told::Count(self.countdown)));
                true
            }
            Choice::Launch(false) => {
                let stopped = counting;
                if stopped {
                    self.abort();
                }
                stopped
            }
            Choice::Kick(target) => {
                gone.push((target, true));
                false
            }
            choice => {
                self.staging.make(client, choice);
                // An offer is for the places the two hold, so it goes
                // once either holds none.
                let staging = &self.staging;
                let seated: Vec<u32> = (staging.guests.iter())
                    .filter(|g| g.side.is_some())
                    .map(|g| g.client)
                    .collect();
                let both = |&(a, b): &(u32, u32)| seated.contains(&a) && seated.contains(&b);
                self.staging.offers.retain(both);
                true
            }
        }
    }

    /// Tells everyone what the person at `client` said for those `to`
    /// names.
    fn pass_on(&mut self, client: u32, to: Hearers, words: String) {
        let guest = self.staging.guest(client);
        let name = guest.map(|g| g.name.clone()).unwrap_or_default();
        let from = guest.and_then(|g| g.side);
        self.tell(&ToClient::Said(Said {
            from,
            name,
            words,
            to,
        }));
    }

    /// Stops the launch counting down, or launching once it has, telling
    /// everyone.
    pub fn stop(&mut self) {
        if self.staging.countdown.is_some() {
            self.abort();
            self.tell_staging();
        }
    }

    fn abort(&mut self) {
        self.staging.countdown = None;
        self.counted = None;
        self.tell(&ToClient::Told(Told::Aborted));
    }

    /// Counts the launch down a second at a time by the host's clock,
    /// telling everyone each second left. Whether it went down.
    fn count_down(&mut self, now: Instant) -> bool {
        let (Some(left), Some(counted)) = (self.staging.countdown, self.counted) else {
            return false;
        };
        if now < counted + Duration::from_secs(1) {
            return false;
        }
        self.counted = Some(counted + Duration::from_secs(1));
        let left = left.saturating_sub(1);
        self.staging.countdown = Some(left);
        // It launches at nought, which isn't told.
        if left > 0 {
            self.tell(&ToClient::Told(Told::Count(left)));
        }
        true
    }

    /// Lets go of the guest at `client`, telling everyone, the one put out
    /// too if it was. Whether there was one.
    fn part(&mut self, client: u32, kicked: bool) -> bool {
        let Some(at) = self.links.iter().position(|(c, _)| *c == client) else {
            return false;
        };
        let (_, mut link) = self.links.remove(at);
        let guest = self.staging.guests.remove(at);
        self.staging.withdraw(client);
        if let Some(side) = guest.side {
            self.staging.slots.empty(side);
        }
        // Their machine's computer players go with them.
        let theirs: Vec<u32> = (self.staging.owners.iter())
            .filter(|&(_, &c)| c == client)
            .map(|(&p, _)| p)
            .collect();
        for player in theirs {
            self.staging.owners.remove(&player);
            self.staging.slots.empty(player);
        }
        let left = ToClient::Left(Gone {
            client,
            players: guest.side.into_iter().collect(),
            name: guest.name,
            kicked,
        });
        if kicked {
            let _ = link.send(encode(&left));
        }
        self.tell(&left);
        true
    }

    fn tell_staging(&mut self) {
        let staging = ToClient::Staging(self.staging.clone());
        self.tell(&staging);
    }

    /// Tells everyone in the game `message`, letting go of any whose link
    /// is lost on the next turn.
    fn tell(&mut self, message: &ToClient) {
        let message = encode(message);
        for (_, link) in &mut self.links {
            let _ = link.send(message.clone());
        }
    }

    fn tell_one(&mut self, client: u32, message: &ToClient) {
        if let Some((_, link)) = self.links.iter_mut().find(|(c, _)| *c == client) {
            let _ = link.send(encode(message));
        }
    }

    /// Launches the game as `setup`, between the sides `sides`: hands
    /// everyone on to the match's host, each to play the side they took or
    /// watch and to run the computer players their machine added, whoever
    /// ran this one trusted to run that one too. Everyone,
    /// whoever runs it included, sets up their copy of the match from
    /// here, and the host is given its own with [`Host::build`].
    pub fn launch(self, setup: Setup, sides: tgsim::Sides) -> Host<L> {
        let mut host = Host::awaiting(setup, sides);
        host.name_by(self.naming);
        host.multiplayer();
        let staging = &self.staging;
        for ((_, link), guest) in self.links.into_iter().zip(&staging.guests) {
            let players = guest.side.into_iter().collect();
            // The host's own run with the host, as do those of no machine.
            let theirs = |&(&p, &c): &(&u32, &u32)| c == guest.client && staging.owns(c, p);
            let computers = match guest.host {
                true => Vec::new(),
                false => staging
                    .owners
                    .iter()
                    .filter(theirs)
                    .map(|(&p, _)| p)
                    .collect(),
            };
            let Ok(id) = host.admit_running(link, players, computers, &guest.name) else {
                continue;
            };
            if guest.host {
                host.trust(id);
            }
        }
        host.want(host.clients());
        host
    }
}

/// Asks the host of a game being set up, at the other end of `link`, to
/// let in a person going by `name` with the game's `password`.
pub fn enter_lobby(link: &mut impl Link, name: String, password: String) -> Result<(), Lost> {
    ask_in(link, Vec::new(), name, password)
}

/// A person's end of a game being set up.
pub struct Entry<L> {
    link: L,
    /// What the host knows them as, and what they go by.
    me: u32,
    name: String,
    staging: Staging,
    /// How many times the host has said what the game has come to.
    staged: u64,
    said: Vec<Said>,
    left: Vec<Gone>,
    told: Vec<Told>,
}

impl<L: Link> Entry<L> {
    /// What the host of a game at the other end of `link` first says to
    /// one who asked to come in: the person's end of the game, once it's
    /// let them in, or else the link back while it has yet to say.
    pub fn entered(mut link: L) -> Result<Result<Entry<L>, L>, NotJoined> {
        match first_answer(&mut link)? {
            None => Ok(Err(link)),
            Some(ToClient::Entered { client, name }) => Ok(Ok(Entry {
                link,
                me: client,
                name,
                staging: Staging::default(),
                staged: 0,
                said: Vec::new(),
                left: Vec::new(),
                told: Vec::new(),
            })),
            Some(_) => Err(NotJoined::Lost),
        }
    }

    pub fn me(&self) -> u32 {
        self.me
    }

    /// What the person goes by in the game.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn staging(&self) -> &Staging {
        &self.staging
    }

    /// How many times the host has said what the game has come to, which
    /// goes up each time it says.
    pub fn staged(&self) -> u64 {
        self.staged
    }

    /// Asks the host for `choice`, which it makes if the person may.
    pub fn choose(&mut self, choice: Choice) -> Result<(), Lost> {
        self.send(ToHost::Choose(choice))
    }

    /// Says `words` to everyone in the game.
    pub fn say(&mut self, words: String) -> Result<(), Lost> {
        self.send(ToHost::Say(words))
    }

    /// Says `words` for those `to` names.
    pub fn say_to(&mut self, to: Hearers, words: String) -> Result<(), Lost> {
        self.send(ToHost::Tell { to, words })
    }

    /// Leaves the game.
    pub fn leave(mut self) {
        let _ = self.send(ToHost::Leave);
    }

    fn send(&mut self, message: ToHost) -> Result<(), Lost> {
        self.link.send(encode(&message))
    }

    /// What's been said since it was last asked.
    pub fn take_said(&mut self) -> Vec<Said> {
        std::mem::take(&mut self.said)
    }

    /// Who has gone since it was last asked.
    pub fn take_left(&mut self) -> Vec<Gone> {
        std::mem::take(&mut self.left)
    }

    /// What the host has told since it was last asked.
    pub fn take_told(&mut self) -> Vec<Told> {
        std::mem::take(&mut self.told)
    }

    /// Hears what the host has said: the match to set up, and the link to
    /// play it over, once the game has launched; until then, `None`.
    pub fn listen(&mut self) -> Result<Option<Welcome>, NotJoined> {
        loop {
            let heard = self.link.receive().map_err(|_| NotJoined::Lost)?;
            let Some(message) = heard else {
                return Ok(None);
            };
            match decode(&message) {
                Some(ToClient::Staging(staging)) => {
                    self.staging = staging;
                    self.staged += 1;
                }
                Some(ToClient::Said(said)) => self.said.push(said),
                Some(ToClient::Told(told)) => self.told.push(told),
                Some(ToClient::Left(gone)) if gone.client == self.me => {
                    return Err(NotJoined::Refused(Refusal::Kicked));
                }
                Some(ToClient::Left(gone)) => self.left.push(gone),
                Some(ToClient::Start(welcome)) => return Ok(Some(welcome)),
                Some(ToClient::Refused(refusal)) => return Err(NotJoined::Refused(refusal)),
                Some(_) => {}
                None => return Err(NotJoined::Lost),
            }
        }
    }

    /// The link to the host, to play the match over once it's launched.
    pub fn into_link(self) -> L {
        self.link
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Client, Local, pair};

    fn staging() -> Staging {
        Staging {
            name: "Host's Game".into(),
            most: 3,
            observers: true,
            map: "field".into(),
            sides: vec![1, 2],
            slots: Slots::left_open(&[1, 2]),
            ..Default::default()
        }
    }

    /// A lobby with whoever runs it in, and the clock it tells the time by.
    /// World rules for a game at night with no fog.
    fn night() -> RuleValues {
        [
            ("FogOfWar".into(), "NONE".into()),
            ("Sun".into(), "sun_night".into()),
        ]
        .into()
    }

    fn hosted() -> (Lobby<Local>, Entry<Local>, Clock) {
        let clock = Clock::stopped();
        let mut lobby = Lobby::new(staging(), "sesame".into());
        lobby.clock(clock.clone());
        let (ours, theirs) = pair();
        assert_eq!(lobby.admit(ours, "Host", ""), Ok(1));
        let mut entry = entered(theirs);
        lobby.turn();
        entry.listen().unwrap();
        entry.take_told();
        (lobby, entry, clock)
    }

    fn entered(link: Local) -> Entry<Local> {
        match Entry::entered(link) {
            Ok(Ok(entry)) => entry,
            _ => panic!("not let in"),
        }
    }

    /// Someone asking to come in as `name` with `password`.
    fn ask(
        lobby: &mut Lobby<Local>,
        name: &str,
        password: &str,
    ) -> Result<Entry<Local>, NotJoined> {
        let (ours, mut theirs) = pair();
        lobby.greet(ours);
        enter_lobby(&mut theirs, name.into(), password.into()).unwrap();
        lobby.seat();
        Entry::entered(theirs).map(|e| e.unwrap_or_else(|_| panic!("unanswered")))
    }

    fn turn(lobby: &mut Lobby<Local>, entries: &mut [&mut Entry<Local>]) -> bool {
        let launch = lobby.turn();
        for entry in entries {
            assert_eq!(entry.listen(), Ok(None));
        }
        launch
    }

    #[test]
    fn people_come_in_with_the_password_while_there_is_room_each_named_unlike_the_rest() {
        let (mut lobby, mut host, _) = hosted();
        let refused = |r| Err::<(), _>(NotJoined::Refused(r));
        assert_eq!(
            ask(&mut lobby, "Bob", "").err(),
            refused(Refusal::Password).err()
        );
        let mut bob = ask(&mut lobby, "Bob", "sesame").unwrap();
        let mut other = ask(&mut lobby, "Bob", "sesame").unwrap();
        assert_eq!((bob.name(), other.name()), ("Bob", "Bob (1)"));
        assert_eq!(
            ask(&mut lobby, "Carol", "sesame").err(),
            refused(Refusal::Full).err()
        );
        turn(&mut lobby, &mut [&mut host, &mut bob, &mut other]);
        let names: Vec<_> = other.staging().guests.iter().map(|g| &g.name).collect();
        assert_eq!(names, ["Host", "Bob", "Bob (1)"]);
        assert!(other.staging().guests[0].host && other.staging().locked);
        let joined = [Told::Joined("Bob".into()), Told::Joined("Bob (1)".into())];
        assert_eq!(host.take_told(), joined);
    }

    #[test]
    fn without_observers_a_game_is_full_once_its_sides_are_taken() {
        let mut lobby = Lobby::new(
            Staging {
                observers: false,
                ..staging()
            },
            String::new(),
        );
        let (ours, mut host) = pair();
        lobby.admit(ours, "Host", "").unwrap();
        let mut bob = ask(&mut lobby, "Bob", "").unwrap();
        bob.choose(Choice::Side(Some(1))).unwrap();
        // Nor may anyone stop playing to watch.
        bob.choose(Choice::Side(None)).unwrap();
        host.send(encode(&ToHost::Choose(Choice::Side(Some(2)))))
            .unwrap();
        lobby.turn();
        assert_eq!(lobby.staging().guest(2).unwrap().side, Some(1));
        let refused = |r| Err::<(), _>(NotJoined::Refused(r));
        assert_eq!(
            ask(&mut lobby, "Carol", "").err(),
            refused(Refusal::Full).err()
        );
    }

    #[test]
    fn newcomers_take_a_free_side_and_free_it_and_their_computer_players_on_leaving() {
        let mut lobby = Lobby::new(
            Staging {
                observers: false,
                most: 2,
                ..staging()
            },
            String::new(),
        );
        let (ours, mut host) = pair();
        lobby.admit(ours, "Host", "").unwrap();
        host.send(encode(&ToHost::Choose(Choice::Computer(1, None))))
            .unwrap();
        lobby.turn();
        // Not the computer player's.
        let mut bob = ask(&mut lobby, "Bob", "").unwrap();
        assert_eq!(lobby.staging().guest(2).unwrap().side, Some(2));
        assert!(!lobby.staging().room());
        bob.choose(Choice::Nation(2, Some(Id::from("usa"))))
            .unwrap();
        lobby.turn();
        assert_eq!(lobby.staging().slots.nations.len(), 1);
        host.send(encode(&ToHost::Choose(Choice::Open(1)))).unwrap();
        lobby.turn();
        bob.choose(Choice::Computer(1, None)).unwrap();
        lobby.turn();
        assert_eq!(lobby.staging().owner(1), Some(2));
        bob.leave();
        lobby.turn();
        let staging = lobby.staging();
        assert!(staging.slots.nations.is_empty() && staging.owners.is_empty());
        assert!(staging.slots.is_open(1) && staging.slots.is_open(2));
    }

    #[test]
    fn each_makes_their_own_choices_and_the_host_the_game_s() {
        let (mut lobby, mut host, _) = hosted();
        let mut bob = ask(&mut lobby, "Bob", "sesame").unwrap();
        bob.choose(Choice::Side(Some(2))).unwrap();
        bob.choose(Choice::Nation(2, Some(Id::from("usa"))))
            .unwrap();
        // Not the host's to choose, nor a side someone else takes.
        let free_for_all = Choice::Preset {
            preset: Preset::FreeForAll(2),
            sides: vec![1, 2],
        };
        bob.choose(free_for_all.clone()).unwrap();
        bob.choose(Choice::Nation(1, Some(Id::from("usa"))))
            .unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        host.choose(Choice::Side(Some(2))).unwrap();
        host.choose(Choice::Nation(2, Some(Id::from("japan"))))
            .unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let staging = bob.staging();
        assert_eq!(staging.guest(2).unwrap().side, Some(2));
        assert_eq!(staging.guest(1).unwrap().side, None);
        assert_eq!(staging.preset, Preset::Custom);
        assert_eq!(staging.slots.nations, [(2, Id::from("usa"))].into());
        let refused = |choice| Told::Refused {
            choice,
            counting: false,
        };
        let told = bob.take_told();
        assert!(told.contains(&refused(free_for_all)));
        assert_eq!(host.take_told().len(), 3);
        // Nothing is chosen for an open side but who holds it.
        host.choose(Choice::Nation(1, Some(Id::from("japan"))))
            .unwrap();
        host.choose(Choice::Handicap(1, Some(Id::from("easy"))))
            .unwrap();
        // A computer player belongs to the machine that put it there,
        // which makes its choices.
        host.choose(Choice::Computer(1, Some(Id::from("konev_sai"))))
            .unwrap();
        host.choose(Choice::Nation(1, Some(Id::from("japan"))))
            .unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        bob.choose(Choice::Computer(1, None)).unwrap();
        bob.choose(Choice::Handicap(1, Some(Id::from("easy"))))
            .unwrap();
        bob.choose(Choice::Side(Some(1))).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let staging = bob.staging();
        assert_eq!(staging.slots.computers, [(1, Id::from("konev_sai"))].into());
        assert!(staging.slots.handicaps.is_empty());
        assert_eq!(staging.guest(2).unwrap().side, Some(2));
        assert_eq!(staging.owner(1), Some(1));
        let refusals = bob.take_told();
        assert_eq!(refusals.len(), 3);
        // Whoever runs the host puts out anyone's, and anyone may put
        // their own on an open side.
        host.choose(Choice::Open(1)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        bob.choose(Choice::Computer(1, Some(Id::from("rommel_sai"))))
            .unwrap();
        bob.choose(Choice::Nation(1, Some(Id::from("japan"))))
            .unwrap();
        bob.choose(Choice::Handicap(1, Some(Id::from("easy"))))
            .unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        assert_eq!(bob.staging().owner(1), Some(2));
        assert!(bob.take_told().is_empty());
        // The host's choices for it are refused, as were those for the
        // open side.
        host.choose(Choice::Nation(1, Some(Id::from("usa"))))
            .unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        assert_eq!(host.take_told().len(), 3);
        // Taking their own computer player's side trades places with it,
        // each keeping their own nation, and it its handicap, profile and
        // machine.
        bob.choose(Choice::Side(Some(1))).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let staging = bob.staging();
        assert_eq!(staging.slots.handicaps, [(2, Id::from("easy"))].into());
        assert_eq!(
            staging.slots.computers,
            [(2, Id::from("rommel_sai"))].into()
        );
        let nations = [(1, Id::from("usa")), (2, Id::from("japan"))];
        assert_eq!(staging.slots.nations, nations.into());
        assert_eq!(staging.owners, [(2, 2)].into());
        // Left for nobody, a side has nothing chosen.
        bob.choose(Choice::Open(2)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let staging = bob.staging();
        assert_eq!(staging.slots.nations, [(1, Id::from("usa"))].into());
        assert!(staging.slots.is_open(2) && staging.slots.computers.is_empty());
        assert!(staging.owners.is_empty());
        // A new map's sides are chosen afresh.
        let map = Choice::Map {
            map: "hills".into(),
            preset: Preset::Custom,
            sides: vec![1, 2],
            rules: night(),
        };
        host.choose(map).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let staging = bob.staging();
        // Bob keeps a side the new map has too, still his.
        assert_eq!(
            (staging.map.as_str(), staging.guest(2).unwrap().side),
            ("hills", Some(1))
        );
        assert!(!staging.slots.is_open(1));
        assert!(staging.slots.nations.is_empty());
        assert_eq!(staging.rules, night());
        assert!(staging.slots.is_open(2));
        // Changing only the rules keeps what's chosen for the sides.
        host.choose(Choice::Computer(2, None)).unwrap();
        host.choose(Choice::Nation(2, Some(Id::from("usa"))))
            .unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let dusk: RuleValues = [("Sun".into(), "sun_dusk".into())].into();
        host.choose(Choice::Rules(dusk.clone())).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let staging = bob.staging();
        assert_eq!(staging.slots.nations, [(2, Id::from("usa"))].into());
        assert_eq!(staging.rules, dusk);
    }

    #[test]
    fn launching_asks_everyone_to_be_ready_until_they_are_then_counts_down() {
        let (mut lobby, mut host, clock) = hosted();
        lobby.count_down_from(2);
        let mut bob = ask(&mut lobby, "Bob", "sesame").unwrap();
        bob.choose(Choice::Side(Some(1))).unwrap();
        // Not ready without the map.
        bob.choose(Choice::Ready(true)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        assert!(!bob.staging().guest(2).unwrap().ready);
        bob.choose(Choice::Holds("field".into(), true)).unwrap();
        host.take_told();
        host.choose(Choice::Launch(true)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        assert_eq!(bob.staging().countdown, None);
        assert_eq!(host.take_told(), [Told::AskedReady]);
        bob.choose(Choice::Ready(true)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        host.choose(Choice::Launch(true)).unwrap();
        assert!(!turn(&mut lobby, &mut [&mut host, &mut bob]));
        assert_eq!(bob.staging().countdown, Some(2));
        assert_eq!(host.take_told(), [Told::Counting, Told::Count(2)]);
        // Nothing is chosen while it counts down but whether to be ready.
        bob.choose(Choice::Side(Some(2))).unwrap();
        clock.pass(Duration::from_secs(1));
        assert!(!turn(&mut lobby, &mut [&mut host, &mut bob]));
        assert_eq!(bob.staging().countdown, Some(1));
        assert_eq!(bob.staging().guest(2).unwrap().side, Some(1));
        let refused = Told::Refused {
            choice: Choice::Side(Some(2)),
            counting: true,
        };
        assert!(bob.take_told().contains(&refused));
        assert_eq!(host.take_told(), [Told::Count(1)]);
        clock.pass(Duration::from_secs(1));
        assert!(lobby.turn());
        bob.listen().unwrap();
        // No nought.
        assert!(bob.take_told().is_empty());
    }

    #[test]
    fn the_countdown_stops_when_the_host_aborts_someone_unreadies_or_anyone_goes() {
        let (mut lobby, mut host, _) = hosted();
        let mut bob = ask(&mut lobby, "Bob", "sesame").unwrap();
        let carol = ask(&mut lobby, "Carol", "sesame").unwrap();
        bob.choose(Choice::Side(Some(1))).unwrap();
        bob.choose(Choice::Holds("field".into(), true)).unwrap();
        bob.choose(Choice::Ready(true)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let count = |host: &mut Entry<Local>, lobby: &mut Lobby<Local>, bob: &mut Entry<Local>| {
            host.choose(Choice::Launch(true)).unwrap();
            turn(lobby, &mut [host, bob]);
            assert!(bob.staging().countdown.is_some());
            host.take_told();
        };
        count(&mut host, &mut lobby, &mut bob);
        host.choose(Choice::Launch(false)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        assert_eq!(host.take_told(), [Told::Aborted]);
        count(&mut host, &mut lobby, &mut bob);
        bob.choose(Choice::Ready(false)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        assert_eq!(host.take_told(), [Told::Aborted]);
        bob.choose(Choice::Ready(true)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        count(&mut host, &mut lobby, &mut bob);
        // Nobody comes in while it counts down.
        let refused = |r| Err::<(), _>(NotJoined::Refused(r));
        assert_eq!(
            ask(&mut lobby, "Dan", "sesame").err(),
            refused(Refusal::Begun).err()
        );
        carol.leave();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        assert_eq!(host.take_told(), [Told::Aborted]);
        assert_eq!(bob.staging().countdown, None);
    }

    #[test]
    fn a_new_map_or_team_setup_takes_people_off_sides_it_leaves_out() {
        let (mut lobby, mut host, _) = hosted();
        let mut bob = ask(&mut lobby, "Bob", "sesame").unwrap();
        bob.choose(Choice::Side(Some(2))).unwrap();
        bob.choose(Choice::Holds("field".into(), true)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let free_for_one = Choice::Preset {
            preset: Preset::FreeForAll(1),
            sides: vec![1],
        };
        host.choose(free_for_one).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        assert_eq!(bob.staging().guest(2).unwrap().side, None);
        // A new map has to be had afresh, but for by the host.
        let map = Choice::Map {
            map: "hills".into(),
            preset: Preset::Custom,
            sides: vec![1, 2],
            rules: RuleValues::new(),
        };
        host.choose(map).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let staging = bob.staging();
        assert!(staging.guest(1).unwrap().holds && !staging.guest(2).unwrap().holds);
    }

    #[test]
    fn people_on_sides_trade_them_by_offer_each_taking_their_nation() {
        let (mut lobby, mut host, _) = hosted();
        let mut bob = ask(&mut lobby, "Bob", "sesame").unwrap();
        let mut carol = ask(&mut lobby, "Carol", "sesame").unwrap();
        host.choose(Choice::Side(Some(1))).unwrap();
        host.choose(Choice::Nation(1, Some(Id::from("usa"))))
            .unwrap();
        bob.choose(Choice::Side(Some(2))).unwrap();
        bob.choose(Choice::Nation(2, Some(Id::from("japan"))))
            .unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob, &mut carol]);
        for entry in [&mut host, &mut bob, &mut carol] {
            entry.take_told();
        }
        // Two offering to each other leaves both standing; either may
        // withdraw their own.
        bob.choose(Choice::Offer(1, true)).unwrap();
        host.choose(Choice::Offer(2, true)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob, &mut carol]);
        assert_eq!(carol.staging().offers, [(1, 2), (2, 1)].into());
        assert!(carol.staging().offered(1, 2));
        bob.choose(Choice::Offer(1, false)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob, &mut carol]);
        assert_eq!(carol.staging().offers, [(1, 2)].into());
        // Not with someone watching, nor by them, nor answering what
        // wasn't offered.
        bob.choose(Choice::Offer(3, true)).unwrap();
        carol.choose(Choice::Offer(2, true)).unwrap();
        host.choose(Choice::Answer(2, true)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob, &mut carol]);
        assert_eq!(bob.take_told().len(), 1);
        assert_eq!(carol.take_told().len(), 1);
        assert_eq!(host.take_told().len(), 1);
        // Accepting trades them at once, and clears what they offered.
        bob.choose(Choice::Answer(1, true)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob, &mut carol]);
        let staging = carol.staging();
        assert_eq!(staging.guest(1).unwrap().side, Some(2));
        assert_eq!(staging.guest(2).unwrap().side, Some(1));
        let nations = [(1, Id::from("japan")), (2, Id::from("usa"))];
        assert_eq!(staging.slots.nations, nations.into());
        assert!(staging.offers.is_empty());
        // Turned down, or by one who goes, an offer is gone.
        host.choose(Choice::Offer(2, true)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob, &mut carol]);
        bob.choose(Choice::Answer(1, false)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob, &mut carol]);
        assert!(carol.staging().offers.is_empty());
        bob.choose(Choice::Offer(1, true)).unwrap();
        bob.choose(Choice::Side(None)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob, &mut carol]);
        assert!(carol.staging().offers.is_empty());
        bob.choose(Choice::Side(Some(1))).unwrap();
        bob.choose(Choice::Offer(1, true)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob, &mut carol]);
        assert!(!carol.staging().offers.is_empty());
        bob.leave();
        lobby.turn();
        assert!(lobby.staging().offers.is_empty());
    }

    #[test]
    fn the_host_moves_its_own_computer_players_between_sides() {
        let mut lobby = Lobby::new(
            Staging {
                sides: vec![1, 2, 3],
                slots: Slots::left_open(&[1, 2, 3]),
                ..staging()
            },
            String::new(),
        );
        let (ours, theirs) = pair();
        lobby.admit(ours, "Host", "").unwrap();
        let mut host = entered(theirs);
        let mut bob = ask(&mut lobby, "Bob", "").unwrap();
        host.choose(Choice::Computer(1, Some(Id::from("konev_sai"))))
            .unwrap();
        host.choose(Choice::Nation(1, Some(Id::from("rus"))))
            .unwrap();
        bob.choose(Choice::Computer(3, None)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        for entry in [&mut host, &mut bob] {
            entry.take_told();
        }
        // Onto an open side, taking all chosen for it along.
        host.choose(Choice::Shift(1, 2)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let staging = bob.staging();
        assert_eq!(staging.slots.computers, [(2, Id::from("konev_sai"))].into());
        assert_eq!(staging.slots.nations, [(2, Id::from("rus"))].into());
        assert!(staging.slots.is_open(1));
        assert_eq!(staging.owner(2), Some(1));
        // Not another machine's, nor by anyone else.
        host.choose(Choice::Shift(3, 1)).unwrap();
        host.choose(Choice::Shift(2, 3)).unwrap();
        bob.choose(Choice::Shift(3, 1)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        assert_eq!((host.take_told().len(), bob.take_told().len()), (2, 1));
        // Onto another of its own, the two trade places.
        host.choose(Choice::Computer(1, Some(Id::from("rommel_sai"))))
            .unwrap();
        host.choose(Choice::Shift(1, 2)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let computers = [(1, Id::from("konev_sai")), (2, Id::from("rommel_sai"))];
        assert_eq!(bob.staging().slots.computers, computers.into());
        assert_eq!(bob.staging().owners, [(1, 1), (2, 1), (3, 2)].into());
    }

    #[test]
    fn where_nobody_may_watch_a_newcomer_takes_a_free_side() {
        let staging = Staging {
            observers: false,
            ..staging()
        };
        let mut lobby = Lobby::new(staging, String::new());
        let (ours, _host) = pair();
        lobby.admit(ours, "Host", "").unwrap();
        let _bob = ask(&mut lobby, "Bob", "").unwrap();
        assert_eq!(lobby.staging().guest(2).unwrap().side, Some(1));
        assert_eq!(lobby.staging().guest(1).unwrap().side, None);
    }

    #[test]
    fn a_launch_the_host_stops_after_counting_down_does_not_happen() {
        let (mut lobby, mut host, clock) = hosted();
        lobby.count_down_from(1);
        host.choose(Choice::Launch(true)).unwrap();
        lobby.turn();
        clock.pass(Duration::from_secs(1));
        assert!(lobby.turn());
        lobby.stop();
        assert!(!lobby.turn());
        host.listen().unwrap();
        assert_eq!(host.staging().countdown, None);
        assert!(host.take_told().contains(&Told::Aborted));
    }

    #[test]
    fn what_is_said_is_passed_on_and_who_goes_or_is_put_out_is_told() {
        let (mut lobby, mut host, _) = hosted();
        let mut bob = ask(&mut lobby, "Bob", "sesame").unwrap();
        let carol = ask(&mut lobby, "Carol", "sesame").unwrap();
        bob.say("hi".into()).unwrap();
        // Everyone is told what's for some, saying who it's for.
        bob.say_to(Hearers::People(vec![1]), "psst".into()).unwrap();
        bob.say_to(Hearers::Team, " ".into()).unwrap();
        carol.leave();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let said = host.take_said();
        assert_eq!(
            (said[0].name.as_str(), said[0].words.as_str()),
            ("Bob", "hi")
        );
        assert_eq!(said.len(), 2);
        assert_eq!(
            (said[1].words.as_str(), &said[1].to),
            ("psst", &Hearers::People(vec![1]))
        );
        assert_eq!(bob.take_said().len(), 2);
        let left = host.take_left();
        assert_eq!((left[0].name.as_str(), left[0].kicked), ("Carol", false));
        // Not whoever runs the host, nor by anyone else.
        bob.choose(Choice::Kick(1)).unwrap();
        host.choose(Choice::Kick(1)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        host.choose(Choice::Kick(2)).unwrap();
        lobby.turn();
        assert_eq!(bob.listen(), Err(NotJoined::Refused(Refusal::Kicked)));
        host.listen().unwrap();
        assert!(host.take_left()[0].kicked);
        assert_eq!(host.staging().guests.len(), 1);
    }

    #[test]
    fn at_launch_everyone_is_handed_on_to_play_the_side_they_took() {
        let (mut lobby, mut host, _) = hosted();
        let mut bob = ask(&mut lobby, "Bob", "sesame").unwrap();
        bob.choose(Choice::Side(Some(2))).unwrap();
        bob.choose(Choice::Computer(1, None)).unwrap();
        turn(&mut lobby, &mut [&mut host, &mut bob]);
        let sides = tgsim::Sides {
            players: (1..=2)
                .map(|id| crate::testing::player(id, false, true))
                .collect(),
            ..Default::default()
        };
        let pathing = tgmap::Pathing::uniform(4, tgmap::Domain::Land);
        let world = tgsim::World::from_organizations(sides, pathing, &[]);
        let setup = Setup {
            map: "field".into(),
            seed: 7,
            ..Default::default()
        };
        let mut match_host = lobby.launch(setup.clone(), world.sides.clone());
        assert_eq!(match_host.clients(), 2);
        let theirs = bob.listen().unwrap().expect("launched");
        assert_eq!(
            (theirs.players.as_slice(), &theirs.setup),
            (&[2][..], &setup)
        );
        // With the computer player Bob's machine added, to run there.
        assert_eq!(theirs.computers, [1]);
        let ours = host.listen().unwrap().expect("launched");
        assert!(ours.players.is_empty() && ours.computers.is_empty());
        let mut client = Client::new(bob.into_link());
        client.ready().unwrap();
        let mut ours = Client::new(host.into_link());
        ours.ready().unwrap();
        match_host.wait();
        // Not before its own copy is set up.
        assert!(match_host.waiting());
        match_host.build(world);
        assert!(!match_host.waiting());
    }
}
