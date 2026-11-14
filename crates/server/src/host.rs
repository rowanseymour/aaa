//! The host of a match: the one copy that decides what the match is
//! given and when, and tells every client.

use aaa_sim::{Action, Schedule, World};

use crate::protocol::{Bundle, ToClient, ToHost, Welcome, decode, encode};
use crate::{Link, Setup};

/// The most messages taken from a client in one turn, so that one sending
/// without end holds nobody else up and puts only so much in a turn; the
/// rest wait for the next.
const HEARD_A_TURN: usize = 64;

/// The longest message taken from a client, in bytes, which is well over
/// what anything it has to say comes to. A longer one is taken as
/// nonsense, so that what every copy of the match keeps of it is small.
const LONGEST_HEARD: usize = 1024;

/// A match and the clients playing and watching it.
pub struct Host<L> {
    setup: Setup,
    world: World,
    schedule: Schedule,
    clients: Vec<Seat<L>>,
    /// What the next client let in is known as.
    next: u32,
    begun: bool,
}

/// A client as the host has it.
struct Seat<L> {
    id: u32,
    link: L,
    players: Vec<u32>,
    /// Whether it's let do anything.
    trusted: bool,
    diverged: bool,
    lost: bool,
}

/// Why a client wasn't let in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The match has begun, and what it's been given so far isn't passed
    /// on.
    Begun,
    /// The match hasn't this player.
    NoPlayer(u32),
    /// Another client gives this player's orders.
    Taken(u32),
    /// Its link was lost.
    Lost,
}

/// Something a turn brought that those running the host may want to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A client's link was lost, or it sent what isn't a message.
    Left(u32),
    /// A client's copy of the match stopped standing as the host's does.
    Diverged { client: u32, step: u64 },
}

impl<L> Seat<L> {
    /// Whether the host carries out `action` for it.
    fn may(&self, action: &Action) -> bool {
        self.trusted
            || match action {
                Action::Order { player, .. } => self.players.contains(player),
                Action::Pause(_) | Action::Speed(_) => !self.players.is_empty(),
                Action::Win(_) | Action::Lose(_) | Action::Raise { .. } | Action::Destroy(_) => {
                    false
                }
            }
    }
}

impl<L: Link> Host<L> {
    /// A host of the match `world`, which `setup` set up and which has
    /// yet to be played.
    pub fn new(setup: Setup, world: World) -> Host<L> {
        Host {
            setup,
            world,
            schedule: Schedule::default(),
            clients: Vec::new(),
            next: 1,
            begun: false,
        }
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

    /// Lets in a client at the other end of `link` to give the orders of
    /// `players`, or only to watch if there are none, returning what it's
    /// known as.
    pub fn admit(&mut self, mut link: L, players: Vec<u32>) -> Result<u32, Refusal> {
        if self.begun {
            return Err(Refusal::Begun);
        }
        for &p in &players {
            if self.world.sides.player(p).is_none() {
                return Err(Refusal::NoPlayer(p));
            }
            if self.clients.iter().any(|c| c.players.contains(&p)) {
                return Err(Refusal::Taken(p));
            }
        }
        let id = self.next;
        let welcome = Welcome {
            client: id,
            setup: self.setup.clone(),
            players: players.clone(),
        };
        let sent = link.send(encode(&ToClient::Start(welcome)));
        sent.map_err(|_| Refusal::Lost)?;
        self.next += 1;
        self.clients.push(Seat {
            id,
            link,
            players,
            trusted: false,
            diverged: false,
            lost: false,
        });
        Ok(id)
    }

    /// Takes what the clients have sent, has every copy of the match
    /// carry out what they asked that it lets them, and plays a tick
    /// unless the match is held still.
    pub fn turn(&mut self) -> Vec<Event> {
        self.go(None)
    }

    /// As [`Host::turn`], but playing no tick: for carrying out what's
    /// asked sooner than the next tick.
    pub fn relay(&mut self) -> Vec<Event> {
        self.go(Some(false))
    }

    /// As [`Host::turn`], but playing a tick whether or not the match is
    /// held still: for what keeps the match's time itself.
    pub fn tick(&mut self) -> Vec<Event> {
        self.go(Some(true))
    }

    /// A turn, playing a tick as `tick` says, or by whether the match is
    /// held still if it doesn't.
    fn go(&mut self, tick: Option<bool>) -> Vec<Event> {
        self.begun = true;
        let mut events = Vec::new();
        let mut actions = Vec::new();
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
                let heard = seat.link.receive();
                let heard =
                    heard.map(|m| m.map(|m| (m.len() <= LONGEST_HEARD).then(|| decode(&m))));
                let heard = heard.map(|m| m.map(Option::flatten));
                match heard {
                    Ok(None) => break,
                    Ok(Some(Some(ToHost::Ask(action)))) => {
                        if seat.may(&action) {
                            actions.push(action);
                        }
                    }
                    Ok(Some(Some(ToHost::Check { step, hash }))) => {
                        // The host has played every tick a client has.
                        let stands = checks.binary_search(&(step, hash)).is_ok();
                        if !stands && !seat.diverged {
                            seat.diverged = true;
                            let client = seat.id;
                            events.push(Event::Diverged { client, step });
                        }
                    }
                    Ok(Some(None)) | Err(_) => {
                        seat.lost = true;
                        break;
                    }
                }
            }
        }
        for action in &actions {
            self.schedule.act(action.clone());
        }
        let tick = tick.unwrap_or(!self.schedule.pausing());
        if tick || !actions.is_empty() {
            self.tell(&ToClient::Step(Bundle { actions, tick }));
        }
        for &event in &events {
            if let Event::Diverged { client, step } = event {
                self.tell(&ToClient::Diverged { client, step });
            }
        }
        match tick {
            true => _ = self.schedule.step(&mut self.world),
            false => _ = self.schedule.carry_out(&mut self.world),
        }
        // Nobody reads them here, and they'd only mount up.
        self.world.take_notices();
        let lost = self.clients.extract_if(.., |c| c.lost);
        events.extend(lost.map(|c| Event::Left(c.id)));
        events
    }

    /// Sends `message` to every client.
    fn tell(&mut self, message: &ToClient) {
        let bytes = encode(message);
        for seat in &mut self.clients {
            seat.lost |= seat.link.send(bytes.clone()).is_err();
        }
    }
}

#[cfg(test)]
mod tests {
    use aaa_map::{Domain, Formation, Member, Pathing, Regiment};
    use aaa_sim::{Command, Refused, Sides};

    use super::*;
    use crate::testing::player;
    use crate::{Client, Local, pair, welcome};

    /// A match of players 1 and 2, each with a soldier, the first's at
    /// 10,10 and the second's 10 further along y.
    fn fresh() -> World {
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
            players: vec![player(1, false, true), player(2, false, true)],
            teams: Vec::new(),
        };
        let pathing = Pathing::uniform(40, Domain::Land);
        World::from_regiments(sides, pathing, &[regiment(0), regiment(1)])
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
        let mut host = Host::new(setup(), fresh());
        let clients = seats.iter().map(|players| {
            let (ours, mut theirs) = pair();
            let id = host.admit(ours, players.to_vec()).unwrap();
            let welcome = welcome(&mut theirs).unwrap().unwrap();
            assert_eq!((welcome.client, &welcome.setup), (id, &setup()));
            assert_eq!(welcome.players, *players);
            Copy {
                client: Client::new(theirs),
                world: fresh(),
            }
        });
        let clients = clients.collect();
        (host, clients)
    }

    fn go(player: u32, regiment: usize, to: [f32; 2]) -> Action {
        let command = Command::Move { regiment, to };
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
        assert_eq!(record.given.len(), 2);
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
        assert!(host.schedule().record().given.is_empty());
        assert_eq!(host.world().ticks, 3);
        host.trust(1);
        clients[0].ask(Action::Win(1)).unwrap();
        clients[1].ask(Action::Win(2)).unwrap();
        play(&mut host, &mut clients, 1);
        assert_eq!(host.schedule().record().given.len(), 1);
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
        assert_eq!(host.schedule().record().given[1].step, 45);
        assert_eq!(clients[1].world().hash(), host.world().hash());
        assert_eq!(clients[1].diverged(), []);
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
        assert_eq!(events, [found]);
        for c in &clients {
            assert_eq!(c.diverged(), [(2, 20)]);
        }
        assert_eq!(clients[0].world().hash(), host.world().hash());
    }

    #[test]
    fn clients_are_let_in_before_the_match_begins_to_players_it_has() {
        let (mut host, _clients) = hosted(&[&[1]]);
        let mut admit = |players: &[u32]| {
            let (ours, theirs) = pair();
            drop(theirs);
            host.admit(ours, players.to_vec())
        };
        assert_eq!(admit(&[1]), Err(Refusal::Taken(1)));
        assert_eq!(admit(&[2, 7]), Err(Refusal::NoPlayer(7)));
        // Its other end having gone already.
        assert_eq!(admit(&[2]), Err(Refusal::Lost));
        let (ours, _theirs) = pair();
        assert_eq!(host.admit(ours, vec![2]), Ok(2));
        host.turn();
        let mut admit = |players: &[u32]| host.admit(pair().0, players.to_vec());
        assert_eq!(admit(&[]), Err(Refusal::Begun));
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
    fn a_client_is_heard_only_so_much_a_turn() {
        let (mut host, mut clients) = hosted(&[&[1], &[2]]);
        for _ in 0..HEARD_A_TURN + 5 {
            clients[0].ask(Action::Pause(false)).unwrap();
        }
        assert_eq!(host.turn(), []);
        assert_eq!(host.schedule().record().given.len(), HEARD_A_TURN);
        // The rest the turn after.
        assert_eq!(host.turn(), []);
        assert_eq!(host.schedule().record().given.len(), HEARD_A_TURN + 5);
        // But for one it trusts, which is heard out.
        host.trust(1);
        for _ in 0..HEARD_A_TURN + 5 {
            clients[0].ask(Action::Pause(false)).unwrap();
        }
        assert_eq!(host.turn(), []);
        assert_eq!(host.schedule().record().given.len(), 2 * HEARD_A_TURN + 10);
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
        assert_eq!(host.schedule().record().given.len(), 3);
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
        assert_eq!(clients[0].schedule().record().given.len(), 2);
        assert!(clients[0].schedule().paused());
        // And a tick is played though the match is held still.
        assert_eq!(host.tick(), []);
        clients[0].advance(usize::MAX).unwrap();
        assert_eq!((host.world().ticks, clients[0].world().ticks), (1, 1));
        assert_eq!(clients[0].world().hash(), host.world().hash());
    }
}
