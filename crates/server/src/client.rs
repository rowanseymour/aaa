//! A client of a match: a copy that asks its host for what it wants done
//! and plays what the host tells every copy.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use aaa_sim::{Action, Refused, Schedule, World};

use crate::protocol::{Join, PROTOCOL, ToClient, ToHost, Welcome, decode, encode};
use crate::{Link, Lost, Refusal};

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
/// orders of `players`, or only to watch if there are none. Its answer is
/// its [`welcome`].
pub fn join(link: &mut impl Link, players: Vec<u32>) -> Result<(), Lost> {
    let protocol = PROTOCOL;
    link.send(encode(&Join { protocol, players }))
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

/// How often [`enter`] looks for the host's answer.
const LOOKS: Duration = Duration::from_millis(10);

/// [`join`]s the match of the host at the other end of `link`, waiting no
/// longer than `wait` for its [`welcome`].
pub fn enter(
    link: &mut impl Link,
    players: Vec<u32>,
    wait: Duration,
) -> Result<Welcome, NotJoined> {
    join(link, players).map_err(|_| NotJoined::Lost)?;
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
    left: Vec<(u32, Vec<u32>)>,
    /// Whether its copy has heard the host end the match.
    ended: bool,
    refused: Vec<(u32, Refused)>,
    /// How many actions it's asked the host for, and how many of those
    /// its copy has done all that will come of.
    asked: u64,
    answered: u64,
}

impl<L: Link> Client<L> {
    /// A client with its host at the other end of `link`, to play a copy
    /// of the match its [`welcome`] set up.
    pub fn new(link: L) -> Client<L> {
        Client {
            link,
            heard: VecDeque::new(),
            behind: 0,
            lost: false,
            schedule: Schedule::default(),
            checked: 0,
            diverged: Vec::new(),
            left: Vec::new(),
            ended: false,
            refused: Vec::new(),
            asked: 0,
            answered: 0,
        }
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

    /// The other clients its copy of the match has heard have gone since
    /// this was last asked, each with the players it gave the orders of.
    pub fn take_left(&mut self) -> Vec<(u32, Vec<u32>)> {
        std::mem::take(&mut self.left)
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
        self.run(world, most, usize::MAX)
    }

    /// As [`Client::advance`], but stopping short of the tick after the
    /// next `ticks` of them: for a copy that keeps time itself, and plays
    /// a tick only when one is due.
    pub fn play(&mut self, world: &mut World, ticks: usize) -> Result<(), Lost> {
        self.run(world, usize::MAX, ticks)
    }

    /// Takes in what the host has sent.
    fn hear(&mut self) {
        while !self.lost && self.heard.len() < TAKEN_IN {
            match self.link.receive().map(|m| m.map(|m| decode(&m))) {
                Ok(None) => break,
                Ok(Some(Some(ToClient::Step(bundle)))) => {
                    self.behind += usize::from(bundle.tick);
                    self.heard.push_back(ToClient::Step(bundle));
                }
                Ok(Some(Some(
                    said @ (ToClient::Diverged { .. }
                    | ToClient::Answered(_)
                    | ToClient::Left { .. }
                    | ToClient::Ended),
                ))) => self.heard.push_back(said),
                // It's already been let in.
                Ok(Some(Some(ToClient::Start(_) | ToClient::Refused(_)))) => self.lost = true,
                Ok(Some(None)) | Err(Lost) => self.lost = true,
            }
        }
    }

    /// Has `world` do what the host has said, up to `steps` steps of it
    /// of which `ticks` play a tick.
    fn run(&mut self, world: &mut World, mut steps: usize, mut ticks: usize) -> Result<(), Lost> {
        loop {
            if self.heard.is_empty() {
                self.hear();
            }
            match self.heard.front() {
                Some(ToClient::Step(bundle)) if steps == 0 || (bundle.tick && ticks == 0) => break,
                Some(_) => {}
                None if self.lost => return Err(Lost),
                None => break,
            }
            match self.heard.pop_front() {
                Some(ToClient::Step(bundle)) => {
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
                Some(ToClient::Left { client, players }) => self.left.push((client, players)),
                Some(ToClient::Ended) => self.ended = true,
                // Never more than it's asked, whatever the host says.
                Some(ToClient::Answered(asked)) => {
                    self.answered = asked.clamp(self.answered, self.asked)
                }
                Some(ToClient::Start(_) | ToClient::Refused(_)) | None => {}
            }
        }
        Ok(())
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
