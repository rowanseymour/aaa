//! What a host and its clients say to each other, as `postcard` encodes
//! it: one message to a [`Link`](crate::Link) message.
//!
//! Every copy of a match plays it for itself. A client asks the host for
//! what it wants done; the host alone decides what's done and when, and
//! tells every client the same, so they all play the same match.

use aaa_sim::Action;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{Refusal, Setup};

/// The version of all this that this build speaks: a host lets in only
/// clients that speak its own.
pub const PROTOCOL: u32 = 4;

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
    /// Another client has gone, whether it left or was lost, with the
    /// players it gave the orders of.
    Left { client: u32, players: Vec<u32> },
    /// The host is leaving a match that's over; who leaves with it
    /// follows, then nothing more.
    Ended,
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
    use aaa_sim::Command;

    use super::*;

    #[test]
    fn messages_read_back_as_they_were_written() {
        let order = Action::Order {
            player: 2,
            command: Command::Stop { regiment: 3 },
        };
        let to_host = [
            ToHost::Ready,
            ToHost::Leave,
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
            ToClient::Left {
                client: 2,
                players: vec![1],
            },
            ToClient::Ended,
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
