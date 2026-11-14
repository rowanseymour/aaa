//! A client of a match: a copy that asks its host for what it wants done
//! and plays what the host tells every copy.

use aaa_sim::{Action, Refused, Schedule, World};

use crate::protocol::{ToClient, ToHost, Welcome, decode, encode};
use crate::{Link, Lost};

/// What the host first tells a client at the other end of `link`, which
/// is what to set its copy of the match up from; `None` until it comes.
pub fn welcome(link: &mut impl Link) -> Result<Option<Welcome>, Lost> {
    match link.receive()?.map(|m| decode(&m)) {
        None => Ok(None),
        Some(Some(ToClient::Start(welcome))) => Ok(Some(welcome)),
        Some(_) => Err(Lost),
    }
}

/// What plays a copy of a match as its host says.
pub struct Client<L> {
    link: L,
    schedule: Schedule,
    /// How many of its record's hashes the host has been sent.
    checked: usize,
    diverged: Vec<(u32, u64)>,
    refused: Vec<(u32, Refused)>,
}

impl<L: Link> Client<L> {
    /// A client with its host at the other end of `link`, to play a copy
    /// of the match its [`welcome`] set up.
    pub fn new(link: L) -> Client<L> {
        Client {
            link,
            schedule: Schedule::default(),
            checked: 0,
            diverged: Vec::new(),
            refused: Vec::new(),
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

    /// Asks the host for `action`, which is carried out, if the host lets
    /// it be, when the host says so.
    pub fn ask(&mut self, action: Action) -> Result<(), Lost> {
        self.link.send(encode(&ToHost::Ask(action)))
    }

    /// The orders the match has refused since this was last asked, each
    /// with the player whose it was.
    pub fn take_refused(&mut self) -> Vec<(u32, Refused)> {
        std::mem::take(&mut self.refused)
    }

    /// Has `world`, its copy of the match and nobody else's to change, do
    /// what the host has said to since it last did: up to `most` steps of
    /// it, so that one far behind catches up a part at a time.
    pub fn advance(&mut self, world: &mut World, most: usize) -> Result<(), Lost> {
        let mut steps = 0;
        while steps < most {
            let Some(message) = self.link.receive()? else {
                break;
            };
            match decode(&message).ok_or(Lost)? {
                ToClient::Step(bundle) => {
                    steps += 1;
                    for action in bundle.actions {
                        self.schedule.act(action);
                    }
                    self.refused.extend(match bundle.tick {
                        true => self.schedule.step(world),
                        false => self.schedule.carry_out(world),
                    });
                    let checks = &self.schedule.record().checks;
                    for &(step, hash) in &checks[self.checked..] {
                        self.link.send(encode(&ToHost::Check { step, hash }))?;
                    }
                    self.checked = checks.len();
                }
                ToClient::Diverged { client, step } => self.diverged.push((client, step)),
                // The match is already set up.
                ToClient::Start(_) => return Err(Lost),
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
