//! A client of a match: a copy that asks its host for what it wants done
//! and plays what the host tells every copy.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use tgsim::{Action, Refused, Schedule, World};

use crate::protocol::{
    Gone, Join, KickVote, PROTOCOL, Said, Status, ToClient, ToHost, Welcome, decode, encode,
};
use crate::{Clock, Link, Lost, Refusal};

/// Why a client hasn't joined a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotJoined {
    /// The host said nothing for as long as it was waited on.
    Unanswered,
    /// The link to the host was lost, or it sent what isn't a welcome.
    Lost,
    /// The host wouldn't let it in.
    Refused(Refusal),
}

/// Asks the host at the other end of `link` to be let in to give the
/// orders of `players`, or only to watch if there are none, for a person
/// going by `name`. Its answer is its [`welcome`].
pub fn join(link: &mut impl Link, players: Vec<u32>, name: String) -> Result<(), Lost> {
    let protocol = PROTOCOL;
    link.send(encode(&Join {
        protocol,
        players,
        name,
        password: String::new(),
    }))
}

/// What the host first tells a client at the other end of `link`, which
/// is what to set its copy of the match up from; `None` until it comes.
pub fn welcome(link: &mut impl Link) -> Result<Option<Welcome>, NotJoined> {
    let heard = link.receive().map_err(|_| NotJoined::Lost)?;
    match heard.map(|m| decode(&m)) {
        None => Ok(None),
        Some(Some(ToClient::Start(welcome))) => Ok(Some(welcome)),
        Some(Some(ToClient::Refused(refusal))) => Err(NotJoined::Refused(refusal)),
        Some(_) => Err(NotJoined::Lost),
    }
}

/// The most the host has said and its copy has yet to do that a client
/// takes in, leaving the rest to its link, which holds the host up in the
/// end rather than keep ever more.
const TAKEN_IN: usize = 256;

/// The most of what's been said for everyone kept until it's asked for.
const SAID_KEPT: usize = 64;

/// How long the host goes unheard from before it's taken as lost: longer
/// than it takes the host to find a client lost, so that it's the host
/// that holds the match for one cut off rather than the client giving up.
const UNHEARD: Duration = Duration::from_secs(3);

/// The longest the host goes without hearing from a client, so that it
/// knows the client is there.
const QUIET: Duration = Duration::from_millis(100);

/// How often [`enter`] looks for the host's answer.
const LOOKS: Duration = Duration::from_millis(10);

/// [`join`]s the match of the host at the other end of `link`, waiting no
/// longer than `wait` for its [`welcome`].
pub fn enter(
    link: &mut impl Link,
    players: Vec<u32>,
    name: String,
    wait: Duration,
) -> Result<Welcome, NotJoined> {
    join(link, players, name).map_err(|_| NotJoined::Lost)?;
    let asked = Instant::now();
    loop {
        match welcome(link)? {
            Some(welcome) => return Ok(welcome),
            None if asked.elapsed() < wait => std::thread::sleep(LOOKS),
            None => return Err(NotJoined::Unanswered),
        }
    }
}

/// What plays a copy of a match as its host says.
pub struct Client<L> {
    link: L,
    /// What the host has said that its copy has yet to do, in order.
    heard: VecDeque<ToClient>,
    /// How many of those play a tick.
    behind: usize,
    /// Whether nothing more will come from the host.
    lost: bool,
    schedule: Schedule,
    /// How many of its record's hashes the host has been sent.
    checked: usize,
    diverged: Vec<(u32, u64)>,
    /// The other clients its copy has heard have gone, with the players
    /// each gave the orders of, since this was last asked.
    left: Vec<Gone>,
    /// The kick votes its copy has heard cast or taken back since this
    /// was last asked.
    kicking: Vec<KickVote>,
    /// What's been said for everyone since it was last asked.
    said: Vec<Said>,
    /// Whether its copy has heard the host end the match.
    ended: bool,
    refused: Vec<(u32, Refused)>,
    /// How many actions it's asked the host for, and how many of those
    /// its copy has done all that will come of.
    asked: u64,
    answered: u64,
    clock: Clock,
    /// When the host was last heard from, and last told how many ticks
    /// its copy has played, which was this many.
    heard_at: Instant,
    sent: Instant,
    played: u64,
    status: Option<Status>,
}

impl<L: Link> Client<L> {
    /// A client with its host at the other end of `link`, to play a copy
    /// of the match its [`welcome`] set up.
    pub fn new(link: L) -> Client<L> {
        let clock = Clock::default();
        let now = clock.now();
        Client {
            clock,
            heard_at: now,
            sent: now,
            played: 0,
            status: None,
            link,
            heard: VecDeque::new(),
            behind: 0,
            lost: false,
            schedule: Schedule::default(),
            checked: 0,
            diverged: Vec::new(),
            left: Vec::new(),
            kicking: Vec::new(),
            said: Vec::new(),
            ended: false,
            refused: Vec::new(),
            asked: 0,
            answered: 0,
        }
    }

    /// Has it tell the time by `clock`.
    pub fn clock(&mut self, clock: Clock) {
        (self.heard_at, self.sent) = (clock.now(), clock.now());
        self.clock = clock;
    }

    /// How everyone stood when the host last said, once it has.
    pub fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    /// Tells the host whether it's for dropping those holding the match.
    pub fn vote(&mut self, drop: bool) -> Result<(), Lost> {
        self.link.send(encode(&ToHost::Vote(drop)))
    }

    /// What its copy of the match has been given, and the pause and speed
    /// that's left it at.
    pub fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    /// The clients whose copies of the match stopped standing as the
    /// host's does, with the ticks each had played when it was found.
    pub fn diverged(&self) -> &[(u32, u64)] {
        &self.diverged
    }

    /// The clients its copy of the match has heard have gone since this
    /// was last asked: this one too, if it's been kicked out.
    pub fn take_left(&mut self) -> Vec<Gone> {
        std::mem::take(&mut self.left)
    }

    /// The kick votes its copy of the match has heard cast or taken back
    /// since this was last asked.
    pub fn take_kicking(&mut self) -> Vec<KickVote> {
        std::mem::take(&mut self.kicking)
    }

    /// What's been said for everyone since this was last asked, as soon as
    /// it's heard: it waits for no step.
    pub fn take_said(&mut self) -> Vec<Said> {
        std::mem::take(&mut self.said)
    }

    /// Asks the host to pass `words` on to everyone at once.
    pub fn say(&mut self, words: String) -> Result<(), Lost> {
        self.link.send(encode(&ToHost::Say(words)))
    }

    /// Tells the host whether it's for kicking `client` out.
    pub fn kick(&mut self, client: u32, vote: bool) -> Result<(), Lost> {
        self.link.send(encode(&ToHost::Kick { client, vote }))
    }

    /// Whether its copy of the match has heard the host leave it once it
    /// was over, rather than only finding the host gone.
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Tells the host it's leaving the match, which it should then do.
    pub fn leave(&mut self) -> Result<(), Lost> {
        self.lost = true;
        self.link.send(encode(&ToHost::Leave))
    }

    /// Asks the host for `action`, which is carried out, if the host lets
    /// it be, when the host says so.
    pub fn ask(&mut self, action: Action) -> Result<(), Lost> {
        self.link.send(encode(&ToHost::Ask(action)))?;
        self.asked += 1;
        Ok(())
    }

    /// Tells the host that its copy of the match is set up, which a host
    /// may wait for before the match begins.
    pub fn ready(&mut self) -> Result<(), Lost> {
        self.link.send(encode(&ToHost::Ready))
    }

    /// How many actions it's asked the host for.
    pub fn asked(&self) -> u64 {
        self.asked
    }

    /// How many of the actions it's asked for are done with: carried out
    /// in its copy if the host let them be, and never to be if not.
    pub fn answered(&self) -> u64 {
        self.answered
    }

    /// The orders the match has refused since this was last asked, each
    /// with the player whose it was.
    pub fn take_refused(&mut self) -> Vec<(u32, Refused)> {
        std::mem::take(&mut self.refused)
    }

    /// How many ticks the host has played that its copy has yet to, as
    /// far as it's taken in.
    pub fn behind(&mut self) -> usize {
        self.hear();
        self.behind
    }

    /// Has `world`, its copy of the match and nobody else's to change, do
    /// what the host has said to since it last did: up to `most` steps of
    /// it, so that one far behind catches up a part at a time. The link
    /// is only lost once all the host said has been done.
    pub fn advance(&mut self, world: &mut World, most: usize) -> Result<(), Lost> {
        self.run(Some(world), most, usize::MAX)
    }

    /// As [`Client::advance`], but stopping short of the tick after the
    /// next `ticks` of them: for a copy that keeps time itself, and plays
    /// a tick only when one is due.
    pub fn play(&mut self, world: &mut World, ticks: usize) -> Result<(), Lost> {
        self.run(Some(world), usize::MAX, ticks)
    }

    /// Takes in what the host has said while its copy of the match is
    /// still being set up, so that the host hears from it all the while:
    /// how everyone stands, who's gone and what's said, but no step,
    /// which waits for a copy to play it.
    pub fn wait(&mut self) -> Result<(), Lost> {
        self.run(None, 0, 0)
    }

    /// Takes in what the host has sent.
    fn hear(&mut self) {
        let now = self.clock.now();
        while !self.lost && self.heard.len() < TAKEN_IN {
            let heard = self.link.receive().map(|m| m.map(|m| decode(&m)));
            if let Ok(Some(_)) = heard {
                self.heard_at = now;
            }
            match heard {
                Ok(None) => {
                    self.lost |= now >= self.heard_at + UNHEARD;
                    break;
                }
                // Nothing of a game being set up means anything once it's launched.
                Ok(Some(Some(
                    ToClient::Alive
                    | ToClient::Entered { .. }
                    | ToClient::Staging(_)
                    | ToClient::Told(_),
                ))) => {}
                Ok(Some(Some(ToClient::Said(said)))) => {
                    // Only the latest are kept for one that never asks.
                    if self.said.len() == SAID_KEPT {
                        self.said.remove(0);
                    }
                    self.said.push(said);
                }
                Ok(Some(Some(ToClient::Step(bundle)))) => {
                    self.behind += usize::from(bundle.tick);
                    self.heard.push_back(ToClient::Step(bundle));
                }
                Ok(Some(Some(
                    said @ (ToClient::Diverged { .. }
                    | ToClient::Answered(_)
                    | ToClient::Left(_)
                    | ToClient::Kicking(_)
                    | ToClient::Ended
                    | ToClient::Status(_)),
                ))) => self.heard.push_back(said),
                // It's already been let in.
                Ok(Some(Some(ToClient::Start(_) | ToClient::Refused(_)))) => self.lost = true,
                Ok(Some(None)) | Err(Lost) => self.lost = true,
            }
        }
    }

    /// Has `world` do what the host has said, up to `steps` steps of it
    /// of which `ticks` play a tick; with no `world`, no steps.
    fn run(
        &mut self,
        mut world: Option<&mut World>,
        mut steps: usize,
        mut ticks: usize,
    ) -> Result<(), Lost> {
        // Taking in what's come, as far as there's room, finds what's said
        // for everyone behind steps still to play.
        self.hear();
        loop {
            if self.heard.is_empty() {
                self.hear();
            }
            match self.heard.front() {
                Some(ToClient::Step(bundle))
                    if steps == 0 || world.is_none() || (bundle.tick && ticks == 0) =>
                {
                    break;
                }
                Some(_) => {}
                None if self.lost => return Err(Lost),
                None => break,
            }
            match self.heard.pop_front() {
                Some(ToClient::Step(bundle)) => {
                    let world = (world.as_deref_mut()).expect("a step waits for a copy to play it");
                    steps -= 1;
                    for action in bundle.actions {
                        self.schedule.act(action);
                    }
                    self.refused.extend(match bundle.tick {
                        true => {
                            ticks -= 1;
                            self.behind -= 1;
                            self.schedule.step(world)
                        }
                        false => self.schedule.carry_out(world),
                    });
                    let checks = &self.schedule.record().checks;
                    for &(step, hash) in &checks[self.checked..] {
                        // A host that's gone is found so once what it
                        // said before it went has been done.
                        let _ = self.link.send(encode(&ToHost::Check { step, hash }));
                    }
                    self.checked = checks.len();
                }
                Some(ToClient::Diverged { client, step }) => self.diverged.push((client, step)),
                Some(ToClient::Left(gone)) => self.left.push(gone),
                Some(ToClient::Kicking(vote)) => self.kicking.push(vote),
                Some(ToClient::Ended) => self.ended = true,
                // As things stood once what came before it was done.
                Some(ToClient::Status(status)) => self.status = Some(status),
                // Never more than it's asked, whatever the host says.
                Some(ToClient::Answered(asked)) => {
                    self.answered = asked.clamp(self.answered, self.asked)
                }
                Some(
                    ToClient::Start(_)
                    | ToClient::Refused(_)
                    | ToClient::Alive
                    | ToClient::Said(_)
                    | ToClient::Entered { .. }
                    | ToClient::Staging(_)
                    | ToClient::Told(_),
                )
                | None => {}
            }
        }
        self.keep_up();
        Ok(())
    }

    /// Tells the host how many ticks its copy has played, when that's
    /// changed or it's said nothing for a while.
    fn keep_up(&mut self) {
        let (now, played) = (self.clock.now(), self.schedule.record().steps);
        if played != self.played || now >= self.sent + QUIET {
            (self.sent, self.played) = (now, played);
            // A host that's gone is found so as what it said is done.
            let _ = self.link.send(encode(&ToHost::Played(played)));
        }
    }
}

#[cfg(test)]
impl<L> Client<L> {
    /// Its end of the link, to send what a client wouldn't.
    pub(crate) fn link(&mut self) -> &mut L {
        &mut self.link
    }

    /// Its schedule, to have its copy go its own way.
    pub(crate) fn schedule_mut(&mut self) -> &mut Schedule {
        &mut self.schedule
    }
}
