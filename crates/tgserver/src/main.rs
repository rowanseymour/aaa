//! Plays a match with no window: alone as fast as it will go, as the host
//! of one for clients elsewhere, or as such a client.

use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use tgdata::GameData;
use tgserver::{Client, Event, Film, Host, Naming, Setup, quic};
use tgsim::{Record, Schedule, TICK, World};

const USAGE: &str = "\
usage:
  tgserver <game dir> <map> [options]    play a map, by its name in the
                                         game's data
  tgserver <game dir> --replay <file>    play a film again, checking that it
                                         plays out as it did
  tgserver <game dir> --join <address>   play a copy of the match hosted at
                                         an address, e.g. 127.0.0.1:4000,
                                         until its host goes

options:
  --seed <number>     what the match's chances are drawn from, rather than
                      a new one
  --seconds <number>  how long to play it for, 0 to 36000, which is also the
                      longest film it plays again; 60 unless given
  --fog <setting>     none, explored or full: how much of the map the fog of
                      war hides, rather than the game's default
  --film <file>       where to keep the match as a film
  --host <address>    host the match at an address, e.g. 0.0.0.0:4000, for
                      clients that join, playing it at its own speed
  --clients <number>  how many clients a hosted match waits for, each to join
                      and be ready to play, before it begins; 1 unless given
  --speed <times>     how many times as fast a hosted match is played, within
                      the game's speeds, e.g. 2; 1 unless given
  --players <ids>     with --join, the players to give the orders of, by id,
                      e.g. 1,2; none unless given
  --name <name>       with --join, what the others know this player as";

/// The most seconds of a match it plays, so that a film whose length was
/// damaged doesn't keep it playing for good.
const LONGEST: f32 = 36000.0;

/// What to do with the game in a folder.
#[derive(Debug)]
enum Asked {
    /// Play the match for this many seconds, keeping it as a film in a
    /// file if one's named, and hosting it if it's to be.
    Play(Setup, f32, Option<String>, Option<Hosting>),
    /// Play a copy of the match hosted at this address, asking for these
    /// players, going by this name if one's given.
    Join(SocketAddr, Vec<u32>, Option<String>),
    /// Play the film in this file again.
    Replay(String),
}

/// Where a match is hosted, and how many clients it waits for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Hosting {
    at: SocketAddr,
    clients: usize,
    /// How many times as fast it's played, if not as fast as the game's
    /// own time.
    speed: Option<f32>,
}

/// Removes `flag` and the value after it from `args`; an error if the
/// value is missing.
fn take_option(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, ()> {
    match args.iter().position(|a| a == flag) {
        Some(i) if i + 1 < args.len() => {
            let value = args.remove(i + 1);
            args.remove(i);
            Ok(Some(value))
        }
        Some(_) => Err(()),
        None => Ok(None),
    }
}

/// The game's folder and what to do with it, as `args` ask.
fn asked(mut args: Vec<String>) -> Result<(String, Asked), ()> {
    if let Some(film) = take_option(&mut args, "--replay")? {
        let [dir] = <[String; 1]>::try_from(args).map_err(|_| ())?;
        return Ok((dir, Asked::Replay(film)));
    }
    if let Some(at) = take_option(&mut args, "--join")? {
        let players = (take_option(&mut args, "--players")?)
            .map(|ids| ids.split(',').map(str::parse).collect::<Result<_, _>>())
            .transpose()
            .map_err(|_| ())?
            .unwrap_or_default();
        let name = (take_option(&mut args, "--name")?)
            .map(|typed| tgserver::name(&typed).ok_or(()))
            .transpose()?;
        let [dir] = <[String; 1]>::try_from(args).map_err(|_| ())?;
        let at = at.parse().map_err(|_| ())?;
        return Ok((dir, Asked::Join(at, players, name)));
    }
    let clients = (take_option(&mut args, "--clients")?)
        .map(|n| n.parse().ok().filter(|&n| n > 0).ok_or(()))
        .transpose()?;
    let speed = (take_option(&mut args, "--speed")?)
        .map(|n| {
            n.parse()
                .ok()
                .filter(|n| tgsim::SPEEDS.contains(n))
                .ok_or(())
        })
        .transpose()?;
    let hosting = match (take_option(&mut args, "--host")?, clients, speed) {
        (Some(at), clients, speed) => Some(Hosting {
            at: at.parse().map_err(|_| ())?,
            clients: clients.unwrap_or(1),
            speed,
        }),
        (None, None, None) => None,
        (None, ..) => return Err(()),
    };
    let seed = (take_option(&mut args, "--seed")?)
        .map(|n| n.parse().map_err(|_| ()))
        .transpose()?
        .unwrap_or_else(tgserver::fresh_seed);
    let seconds = match take_option(&mut args, "--seconds")? {
        Some(s) => s.parse().ok().filter(|s| (0.0..=LONGEST).contains(s)),
        None => Some(60.0),
    };
    let fog = (take_option(&mut args, "--fog")?)
        .map(|f| tgmap::FogSetting::named(&f).ok_or(()))
        .transpose()?;
    let rules = fog.map(|f| (tgdata::Id::from(tgdata::FOG_OF_WAR), f.id().to_string()));
    let film = take_option(&mut args, "--film")?;
    let [dir, map] = <[String; 2]>::try_from(args).map_err(|_| ())?;
    let setup = Setup {
        map,
        seed,
        rules: rules.into_iter().collect(),
        ..Default::default()
    };
    Ok((dir, Asked::Play(setup, seconds.ok_or(())?, film, hosting)))
}

/// How long the host of a match to join is waited on to say whether
/// it's let in.
const ANSWERED: Duration = Duration::from_secs(10);

/// How long it sleeps between looks at what's come over a link, with
/// nothing else to do.
const IDLE: Duration = Duration::from_millis(10);

/// Hosts the match `world`, which `setup` set up, as `hosting` says: once
/// its clients have joined, plays `ticks` ticks of it in step with them at
/// the speed it's given, or until they've all gone. Returns its copy as it
/// ends up and what it was given.
fn host(
    setup: Setup,
    world: World,
    profiles: tgserver::Profiles,
    ticks: u64,
    hosting: Hosting,
    naming: Naming,
) -> Result<(World, Record), String> {
    let mut listener = quic::listen(hosting.at)?;
    eprintln!("hosting at {}", listener.at()?);
    let mut host = Host::new(setup, world);
    host.computers_from(profiles);
    host.name_by(naming);
    host.multiplayer();
    if let Some(speed) = hosting.speed {
        host.speed(speed);
    }
    host.want(hosting.clients);
    let mut greet = |host: &mut Host<quic::Remote>| {
        while let Some(link) = listener.accept() {
            host.greet(link);
        }
        for seated in host.seat() {
            match seated {
                Ok(client) => eprintln!("client {client} joined"),
                Err(refusal) => eprintln!("a client wasn't let in: {refusal:?}"),
            }
        }
    };
    let said = |event| match event {
        Event::Left(client) => eprintln!("client {client} left"),
        Event::Diverged { client, step } => {
            eprintln!("client {client}'s copy stopped matching by tick {step}")
        }
        Event::Held(true) => eprintln!("the match is held"),
        Event::Held(false) => eprintln!("the match goes on"),
    };
    while host.waiting() {
        greet(&mut host);
        host.wait().into_iter().for_each(said);
        std::thread::sleep(IDLE);
    }
    let (mut began, mut due) = (Instant::now(), Duration::ZERO);
    while host.world().ticks < ticks && host.clients() > 0 {
        host.turn().into_iter().for_each(said);
        // Only to tell those that come now that they're too late.
        greet(&mut host);
        if host.held() {
            // The ticks a hold put off aren't made up for.
            (began, due) = (Instant::now(), Duration::ZERO);
        }
        due += Duration::from_secs_f32(TICK / host.schedule().speed());
        std::thread::sleep(due.saturating_sub(began.elapsed()));
    }
    // Its links go before what carries them, which sees that what they
    // were sent gets there.
    let finished = host.finish();
    drop(listener);
    Ok(finished)
}

/// Does what's `asked` with the game in `dir`, returning the match as it
/// ends up and the seed it was played from.
fn run(dir: &str, asked: Asked) -> Result<(World, u64), String> {
    let game = GameData::open(Path::new(dir))?;
    match asked {
        Asked::Play(setup, seconds, film, hosting) => {
            let found = game.find_map(&setup.map);
            let map = found.ok_or_else(|| format!("no map \"{}\"", setup.map))?;
            let setup = Setup { map, ..setup };
            let started = tgserver::start(&game, &setup, || {})?;
            let ticks = (seconds / TICK).round() as u64;
            let (world, record) = match hosting {
                Some(hosting) => {
                    let speeds = game.game_speeds();
                    let (slowest, fastest) = (speeds.start().exp2(), speeds.end().exp2());
                    if let Some(speed) = hosting.speed.filter(|s| !(slowest..=fastest).contains(s))
                    {
                        return Err(format!(
                            "speed {speed}: the game plays from {slowest} to {fastest}"
                        ));
                    }
                    let naming = Naming::of(&game.strings(), &started.factions);
                    host(
                        setup.clone(),
                        started.world,
                        started.profiles,
                        ticks,
                        hosting,
                        naming,
                    )?
                }
                None => {
                    let (mut world, mut schedule) = (started.world, Schedule::default());
                    for _ in 0..ticks {
                        schedule.step(&mut world);
                    }
                    (world, schedule.record().clone())
                }
            };
            let seed = setup.seed;
            if let Some(path) = film {
                Film { setup, record }.write(Path::new(&path))?;
            }
            Ok((world, seed))
        }
        Asked::Join(at, players, name) => {
            let gone = |_| format!("{at}: the host has gone");
            let mut link = quic::connect(at)?;
            let name = name.unwrap_or_else(|| tgserver::default_name(&game.strings()));
            let welcome = tgserver::enter(&mut link, players, name, ANSWERED)
                .map_err(|e| format!("{at}: not let in: {e:?}"))?;
            eprintln!("joined as client {}", welcome.client);
            let mut client = Client::new(link);
            // Kept in touch with the host while it's set up, which would
            // otherwise take the client for lost.
            let setup = welcome.setup.clone();
            let starting = std::thread::spawn(move || tgserver::start(&game, &setup, || {}));
            while !starting.is_finished() {
                client.wait().map_err(gone)?;
                std::thread::sleep(IDLE);
            }
            let started =
                (starting.join()).unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            let mut world = started?.world;
            client.ready().map_err(gone)?;
            let mut played = Ok(());
            while played.is_ok() {
                played = client.advance(&mut world, usize::MAX);
                for gone in client.take_left() {
                    let (id, players) = (gone.client, gone.players);
                    let how = if gone.kicked { "was kicked" } else { "left" };
                    eprintln!("client {id} {how}, giving the orders of {players:?}");
                }
                std::thread::sleep(IDLE);
            }
            match client.ended() {
                true => eprintln!("the host ended the match"),
                false => eprintln!("the host has gone"),
            }
            for (client, step) in client.diverged() {
                eprintln!("client {client}'s copy stopped matching the host's by tick {step}");
            }
            Ok((world, welcome.setup.seed))
        }
        Asked::Replay(path) => {
            let film = Film::read(Path::new(&path))?;
            let steps = film.record.steps;
            if steps > (LONGEST / TICK) as u64 {
                return Err(format!("{path}: {steps} ticks is too long a film"));
            }
            let mut world = tgserver::start(&game, &film.setup, || {})?.world;
            let replayed = film.record.replay(&mut world);
            replayed.map_err(|d| format!("{path}: plays out another way from step {}", d.step))?;
            Ok((world, film.setup.seed))
        }
    }
}

/// Says on stderr what the game's data and the match warn of.
struct Warnings;

impl log::Log for Warnings {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("{}: {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

fn main() -> ExitCode {
    let Ok((dir, asked)) = asked(std::env::args().skip(1).collect()) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if log::set_logger(&Warnings).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    let (world, seed) = match run(&dir, asked) {
        Ok(played) => played,
        Err(e) => {
            eprintln!("tgserver: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("seed {seed}");
    println!("{} ticks, hash {:016x}", world.ticks, world.hash());
    for player in &world.sides.players {
        println!(
            "{} ({}): {:?}",
            player.name,
            player.id,
            world.outcome(player.id)
        );
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn options_come_out_of_the_arguments_in_any_place() {
        let asked = asked(args(&[
            "--seed",
            "7",
            "game",
            "--seconds",
            "5",
            "berlin",
            "--film",
            "out.film",
        ]));
        let (dir, Asked::Play(setup, seconds, film, hosting)) = asked.unwrap() else {
            panic!("not a match to play");
        };
        assert_eq!((dir.as_str(), setup.map.as_str()), ("game", "berlin"));
        assert_eq!((setup.seed, seconds), (7, 5.0));
        assert_eq!(film.as_deref(), Some("out.film"));
        assert_eq!(hosting, None);
        let (_, Asked::Play(setup, seconds, film, hosting)) =
            super::asked(args(&["game", "berlin"])).unwrap()
        else {
            panic!("not a match to play");
        };
        assert!(setup.rules.is_empty() && film.is_none() && hosting.is_none());
        assert_eq!(seconds, 60.0);
    }

    #[test]
    fn a_film_to_play_again_takes_the_place_of_a_map() {
        let (dir, asked) = asked(args(&["--replay", "out.film", "game"])).unwrap();
        assert!(matches!(asked, Asked::Replay(film) if film == "out.film"));
        assert_eq!(dir, "game");
    }

    #[test]
    fn a_match_is_hosted_at_an_address_or_joined_at_one() {
        let hosted = |words: &[&str]| match asked(args(words)) {
            Ok((_, Asked::Play(_, _, _, hosting))) => hosting,
            _ => panic!("not a match to play"),
        };
        let at: SocketAddr = "127.0.0.1:4000".parse().unwrap();
        let hosting = hosted(&["game", "berlin", "--host", "127.0.0.1:4000"]);
        let hosted_at = |clients, speed| Some(Hosting { at, clients, speed });
        assert_eq!(hosting, hosted_at(1, None));
        let words = [
            "game",
            "--clients",
            "3",
            "berlin",
            "--host",
            "127.0.0.1:4000",
        ];
        assert_eq!(hosted(&words), hosted_at(3, None));
        let words = ["game", "berlin", "--host", "127.0.0.1:4000", "--speed", "2"];
        assert_eq!(hosted(&words), hosted_at(1, Some(2.0)));
        let joined = |words: &[&str]| match asked(args(words)) {
            Ok((_, Asked::Join(at, players, name))) => (at, players, name),
            _ => panic!("not a match to join"),
        };
        let words = ["game", "--join", "127.0.0.1:4000"];
        assert_eq!(joined(&words), (at, vec![], None));
        let words = ["--players", "1,12", "game", "--join", "127.0.0.1:4000"];
        assert_eq!(joined(&words), (at, vec![1, 12], None));
        let words = ["game", "--join", "127.0.0.1:4000", "--name", " Bob "];
        assert_eq!(joined(&words), (at, vec![], Some("Bob".into())));
    }

    #[test]
    fn what_is_missing_or_malformed_is_refused() {
        for words in [
            &["game"][..],
            &["game", "berlin", "extra"],
            &["game", "berlin", "--seed"],
            &["game", "berlin", "--seed", "many"],
            &["game", "berlin", "--seconds", "-1"],
            &["game", "berlin", "--fog", "thick"],
            &["game", "berlin", "--film"],
            &["game", "berlin", "--replay", "out.film"],
            &["--replay", "out.film"],
            &["game", "berlin", "--host", "nowhere"],
            &["game", "berlin", "--clients", "2"],
            &["game", "berlin", "--speed", "2"],
            &["game", "berlin", "--host", "127.0.0.1:4000", "--speed", "0"],
            &[
                "game",
                "berlin",
                "--host",
                "127.0.0.1:4000",
                "--clients",
                "0",
            ],
            &["game", "berlin", "--join", "127.0.0.1:4000"],
            &["game", "--join", "127.0.0.1:4000", "--players", "1,two"],
            &["game", "--join", "127.0.0.1:4000", "--seed", "3"],
            &["game", "--join", "127.0.0.1:4000", "--name", "  "],
            &["game", "berlin", "--name", "Bob"],
        ] {
            assert!(asked(args(words)).is_err(), "{words:?}");
        }
    }
}
