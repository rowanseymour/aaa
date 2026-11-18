//! Finding games on the local network: a host answers queries broadcast to
//! [`QUERY_PORT`] with what its game is, and a search broadcasts one and
//! gathers the answers.

use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};

use crate::protocol::{PROTOCOL, decode, encode};

/// The port hosts listen for queries on, as the game's `GameSpyQueryPort`.
pub const QUERY_PORT: u16 = 8850;

/// What starts every query and answer, so that nothing else sent to the
/// port is taken for one.
const MAGIC: [u8; 4] = *b"tglq";

/// The longest answer taken.
const LONGEST: usize = 2048;

/// What a host says of its game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Advert {
    pub name: String,
    /// The people in it, observers counted, and the most it lets in.
    pub people: u32,
    pub most: u32,
    /// Whether it takes a password to come in.
    pub locked: bool,
    /// The map chosen, by its name in the game's data.
    pub map: String,
    /// The protocol its host speaks, which a joiner must too.
    pub protocol: u32,
    /// Whether its match is under way, and so it's too late to join.
    pub playing: bool,
    /// The port its host takes joins on.
    pub port: u16,
}

impl Advert {
    /// Whether a game of this program could join it, as far as versions
    /// go.
    pub fn compatible(&self) -> bool {
        self.protocol == PROTOCOL
    }
}

/// A socket on the query port, shared with any other on this machine.
fn shared(port: u16) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.set_broadcast(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port).into())?;
    Ok(socket.into())
}

/// A host's answer to queries for its game.
pub struct Advertiser {
    socket: UdpSocket,
    advert: Advert,
}

impl Advertiser {
    /// Answers queries with `advert` until dropped.
    pub fn new(advert: Advert) -> std::io::Result<Advertiser> {
        Ok(Advertiser {
            socket: shared(QUERY_PORT)?,
            advert,
        })
    }

    /// Has it say `advert` from now on.
    pub fn advertise(&mut self, advert: Advert) {
        self.advert = advert;
    }

    /// Has it say from now on that its match is under way.
    pub fn playing(&mut self) {
        self.advert.playing = true;
    }

    /// Answers the queries that have come.
    pub fn answer(&mut self) {
        let mut heard = [0; 64];
        loop {
            let from = match self.socket.recv_from(&mut heard) {
                Ok((n, from)) if heard[..n] == query() => from,
                Ok(_) => continue,
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                Err(_) => return,
            };
            let mut answer = MAGIC.to_vec();
            answer.extend(encode(&self.advert));
            let _ = self.socket.send_to(&answer, from);
        }
    }
}

fn query() -> Vec<u8> {
    let mut query = MAGIC.to_vec();
    query.extend(PROTOCOL.to_le_bytes());
    query
}

/// A search for games on the local network.
pub struct Search {
    socket: UdpSocket,
    sent: Instant,
}

/// A game found, at the address its host takes joins at, with how long
/// its answer took.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub at: SocketAddr,
    pub advert: Advert,
    pub ping: Duration,
}

impl Search {
    /// Broadcasts a query for games, and asks this machine's own too,
    /// which a broadcast may not reach where the system keeps the program
    /// off the local network.
    pub fn start() -> std::io::Result<Search> {
        let here = SocketAddrV4::new(Ipv4Addr::LOCALHOST, QUERY_PORT).into();
        let mut search = Search::to(here)?;
        let everywhere = SocketAddrV4::new(Ipv4Addr::BROADCAST, QUERY_PORT);
        if let Err(e) = search.socket.send_to(&query(), everywhere) {
            log::warn!("can't search the local network for games: {e}");
        }
        search.sent = Instant::now();
        Ok(search)
    }

    /// Sends a query to `at`, a broadcast address or a host's.
    pub fn to(at: SocketAddr) -> std::io::Result<Search> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_broadcast(true)?;
        socket.set_nonblocking(true)?;
        socket.send_to(&query(), at)?;
        Ok(Search {
            socket,
            sent: Instant::now(),
        })
    }

    /// The games that have answered since this was last asked.
    pub fn found(&mut self) -> Vec<Found> {
        let mut found = Vec::new();
        let mut heard = [0; LONGEST];
        loop {
            match self.socket.recv_from(&mut heard) {
                Ok((n, from)) => {
                    let Some(answer) = heard[..n].strip_prefix(&MAGIC) else {
                        continue;
                    };
                    let Some(advert) = decode::<Advert>(answer) else {
                        continue;
                    };
                    found.push(Found {
                        at: SocketAddr::new(from.ip(), advert.port),
                        advert,
                        ping: self.sent.elapsed(),
                    });
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return found,
                Err(_) => return found,
            }
        }
    }

    /// How long since the query went out.
    pub fn age(&self) -> Duration {
        self.sent.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_answers_a_query_with_its_game() {
        let advert = Advert {
            name: "Bob's Game".into(),
            people: 1,
            most: 4,
            locked: false,
            map: "(2v2)Berlin".into(),
            protocol: PROTOCOL,
            playing: false,
            port: 7290,
        };
        let Ok(mut host) = Advertiser::new(advert.clone()) else {
            // Nowhere to listen in this environment.
            return;
        };
        let at = SocketAddrV4::new(Ipv4Addr::LOCALHOST, QUERY_PORT).into();
        let mut search = Search::to(at).unwrap();
        let mut found = Vec::new();
        for _ in 0..100 {
            host.answer();
            found.extend(search.found());
            if !found.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let ours = found.iter().find(|f| f.advert == advert).expect("answered");
        assert_eq!(ours.at, SocketAddr::from(([127, 0, 0, 1], 7290)));
        assert!(ours.advert.compatible());
    }
}
