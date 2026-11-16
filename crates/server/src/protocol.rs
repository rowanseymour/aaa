//! What a host and its clients say to each other, as `postcard` encodes
//! it: one message to a [`Link`](crate::Link) message.
//!
//! Every copy of a match plays it for itself. A client asks the host for
//! what it wants done; the host alone decides what's done and when, and
//! tells every client the same, so they all play the same match.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tgsim::Action;

use crate::{Refusal, Setup};

/// The version of all this that this build speaks: a host lets in only
/// clients that speak its own.
pub const PROTOCOL: u32 = 9;

/// What a client first says to a host: that it wants in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Join {
    /// The [`PROTOCOL`] the client speaks.
    pub protocol: u32,
    /// The players it wants to give the orders of; none to only watch.
    pub players: Vec<u32>,
}

/// From a client to the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToHost {
    /// Asks for an action to be carried out.
    Ask(Action),
    /// The client has set its copy of the match up, and plays it as soon
    /// as the host says to.
    Ready,
    /// How the client's copy of the match stood once it had played
    /// `step` ticks.
    Check { step: u64, hash: u64 },
    /// The client is leaving the match, and nothing more follows.
    Leave,
    /// How many ticks the client's copy of the match has played, which
    /// also tells the host the client is still there.
    Played(u64),
    /// Whether the client is for dropping those holding the match.
    Vote(bool),
    /// Whether the client is for kicking `client` out of the match.
    Kick { client: u32, vote: bool },
    /// Words for everyone, to pass on at once, outside the steps.
    Say(String),
}

/// From the host to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToClient {
    /// The match to set up, and what the client is in it.
    Start(Welcome),
    /// The client isn't let in, and nothing more follows.
    Refused(Refusal),
    /// What every copy of the match does next.
    Step(Bundle),
    /// A client's copy of the match stopped standing as the host's does.
    Diverged { client: u32, step: u64 },
    /// How many of the actions this client has asked for the host has
    /// dealt with: every step before this has those it carried out, and
    /// the rest it never will.
    Answered(u64),
    /// A client has gone. The one kicked out hears it too, and then
    /// nothing more.
    Left(Gone),
    /// A client is for kicking another out, or no longer is.
    Kicking(KickVote),
    /// The host is leaving a match that's over; who leaves with it
    /// follows, then nothing more.
    Ended,
    /// How everyone stands.
    Status(Status),
    /// Nothing but that the host is still there.
    Alive,
    /// Words a client said for everyone, passed on at once, held match or
    /// not, and so in no step.
    Said(Said),
}

/// Words a client said for everyone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Said {
    /// The side of the person who said them, or none for one watching.
    pub from: Option<u32>,
    pub words: String,
}

/// How the match and its clients stand, as the host sees them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub seats: Vec<Seated>,
    /// Whether the match is held for everyone, its ticks waiting on those
    /// whose standing holds it and then on the rest catching up.
    pub held: bool,
    /// How many are for dropping those holding the match, and how many it
    /// takes.
    pub votes: u32,
    pub needed: u32,
    /// The seconds, rounded up, before those holding the match can be
    /// dropped, while it's held by any.
    pub countdown: Option<u32>,
    /// How many it takes to kick a client out.
    pub kicks_needed: u32,
}

/// A client as the host sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Seated {
    pub client: u32,
    pub players: Vec<u32>,
    /// Whether it's the client of whoever runs the host.
    pub host: bool,
    pub standing: Standing,
    /// Whether it's for dropping those holding the match.
    pub vote: bool,
    /// The clients it's for kicking out.
    pub kicking: Vec<u32>,
    /// How long, in whole milliseconds, a message takes the host to get to
    /// it and back, where the host measures that.
    pub ping: Option<u32>,
}

/// What's become of a client's part in the match.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Standing {
    #[default]
    Ok,
    /// Unheard from for long enough to hold the match.
    Disconnected,
    /// Its copy of the match stopped standing as the host's does, which
    /// holds the match too.
    OutOfSync,
    /// Behind on a match let go after being held, which waits for it.
    CatchingUp,
}

impl Standing {
    /// Whether it holds the match.
    pub fn holds(self) -> bool {
        matches!(self, Standing::Disconnected | Standing::OutOfSync)
    }
}

/// A client that's gone, whether it left, was lost or was kicked out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gone {
    pub client: u32,
    /// The players it gave the orders of.
    pub players: Vec<u32>,
    pub kicked: bool,
}

/// A vote for kicking a client out cast or taken back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KickVote {
    pub voter: u32,
    pub target: u32,
    /// Whether it was cast, rather than taken back.
    pub vote: bool,
    /// How many are then for kicking `target` out, and how many it takes.
    pub votes: u32,
    pub needed: u32,
}

/// What a client is told as it's let in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    /// What the host knows the client as.
    pub client: u32,
    pub setup: Setup,
    /// The players the client gives orders for; none for one that only
    /// watches.
    pub players: Vec<u32>,
}

/// The actions to carry out next, in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    pub actions: Vec<Action>,
    /// Whether the match then plays a tick: it doesn't while it's held
    /// still, when actions still have to get through to let it go on.
    pub tick: bool,
}

pub(crate) fn encode(message: &impl Serialize) -> Vec<u8> {
    // Writing to memory, which nothing a message holds can fail at.
    postcard::to_stdvec(message).expect("a message encodes")
}

/// The message `bytes` are, if they're all of one.
pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    match postcard::take_from_bytes(bytes) {
        Ok((message, [])) => Some(message),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use tgsim::Command;

    use super::*;

    #[test]
    fn messages_read_back_as_they_were_written() {
        let order = Action::Order {
            player: 2,
            command: Command::Stop { organization: 3 },
        };
        let to_host = [
            ToHost::Ready,
            ToHost::Leave,
            ToHost::Played(12),
            ToHost::Vote(true),
            ToHost::Kick {
                client: 3,
                vote: true,
            },
            ToHost::Say("hi".into()),
            ToHost::Ask(order.clone()),
            ToHost::Check {
                step: 10,
                hash: u64::MAX,
            },
        ];
        for message in to_host {
            assert_eq!(decode(&encode(&message)), Some(message));
        }
        let to_client = [
            ToClient::Start(Welcome {
                client: 1,
                setup: Setup {
                    map: "berlin".into(),
                    seed: 7,
                    ..Default::default()
                },
                players: vec![1, 2],
            }),
            ToClient::Step(Bundle {
                actions: vec![order, Action::Pause(true)],
                tick: false,
            }),
            ToClient::Diverged {
                client: 2,
                step: 30,
            },
            ToClient::Refused(Refusal::Protocol(PROTOCOL)),
            ToClient::Answered(3),
            ToClient::Left(Gone {
                client: 2,
                players: vec![1],
                kicked: true,
            }),
            ToClient::Kicking(KickVote {
                voter: 1,
                target: 2,
                vote: false,
                votes: 1,
                needed: 2,
            }),
            ToClient::Ended,
            ToClient::Alive,
            ToClient::Said(Said {
                from: Some(1),
                words: "hi".into(),
            }),
            ToClient::Status(Status {
                seats: vec![Seated {
                    client: 2,
                    players: vec![1],
                    host: false,
                    standing: Standing::Disconnected,
                    vote: true,
                    kicking: vec![3],
                    ping: Some(80),
                }],
                held: true,
                votes: 1,
                needed: 2,
                countdown: Some(9),
                kicks_needed: 2,
            }),
        ];
        let join = Join {
            protocol: PROTOCOL,
            players: vec![3],
        };
        assert_eq!(decode(&encode(&join)), Some(join));
        for message in to_client {
            assert_eq!(decode(&encode(&message)), Some(message));
        }
    }

    #[test]
    fn what_is_not_one_whole_message_is_none() {
        let bytes = encode(&ToHost::Check { step: 1, hash: 2 });
        assert_eq!(decode::<ToHost>(&bytes[..bytes.len() - 1]), None);
        assert_eq!(decode::<ToHost>(&[&bytes[..], &[0]].concat()), None);
        assert_eq!(decode::<ToHost>(&[200]), None);
    }
}
