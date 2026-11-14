//! Plays a match with no window, as fast as it will go.

use std::path::Path;
use std::process::ExitCode;

use aaa_data::GameData;
use aaa_server::{Film, Setup};
use aaa_sim::{Schedule, TICK, World};

const USAGE: &str = "\
usage:
  aaa-server <game dir> <map> [options]    play a map, e.g. \"allies10_berlin\"
  aaa-server <game dir> --replay <file>    play a film again, checking that it
                                           plays out as it did

options:
  --seed <number>     what the match's chances are drawn from, rather than
                      a new one
  --seconds <number>  how long to play it for, 0 to 36000, which is also the
                      longest film it plays again; 60 unless given
  --fog <setting>     none, explored or full: how much of the map the fog of
                      war hides, rather than the game's default
  --film <file>       where to keep the match as a film";

/// The most seconds of a match it plays, so that a film whose length was
/// damaged doesn't keep it playing for good.
const LONGEST: f32 = 36000.0;

/// What to do with the game in a folder.
#[derive(Debug)]
enum Asked {
    /// Play the match for this many seconds, keeping it as a film in a
    /// file if one's named.
    Play(Setup, f32, Option<String>),
    /// Play the film in this file again.
    Replay(String),
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
    let seed = match take_option(&mut args, "--seed")? {
        Some(n) => n.parse().map_err(|_| ())?,
        None => aaa_server::fresh_seed(),
    };
    let seconds = match take_option(&mut args, "--seconds")? {
        Some(s) => s.parse().ok().filter(|s| (0.0..=LONGEST).contains(s)),
        None => Some(60.0),
    };
    let fog = match take_option(&mut args, "--fog")? {
        Some(f) => Some(aaa_map::FogSetting::named(&f).ok_or(())?),
        None => None,
    };
    let film = take_option(&mut args, "--film")?;
    let [dir, map] = <[String; 2]>::try_from(args).map_err(|_| ())?;
    let setup = Setup {
        map,
        seed,
        fog,
        ..Default::default()
    };
    Ok((dir, Asked::Play(setup, seconds.ok_or(())?, film)))
}

/// Does what's `asked` with the game in `dir`, returning the match as it
/// ends up and the seed it was played from.
fn run(dir: &str, asked: Asked) -> Result<(World, u64), String> {
    let game = GameData::open(Path::new(dir))?;
    match asked {
        Asked::Play(setup, seconds, film) => {
            let found = game.find_map(&setup.map);
            let map = found.ok_or_else(|| format!("no map \"{}\"", setup.map))?;
            let setup = Setup { map, ..setup };
            let mut world = aaa_server::start(&game, &setup, || {})?.world;
            let mut schedule = Schedule::default();
            for _ in 0..(seconds / TICK).round() as u64 {
                schedule.step(&mut world);
            }
            let seed = setup.seed;
            if let Some(path) = film {
                let record = schedule.record().clone();
                Film { setup, record }.write(Path::new(&path))?;
            }
            Ok((world, seed))
        }
        Asked::Replay(path) => {
            let film = Film::read(Path::new(&path))?;
            let steps = film.record.steps;
            if steps > (LONGEST / TICK) as u64 {
                return Err(format!("{path}: {steps} ticks is too long a film"));
            }
            let mut world = aaa_server::start(&game, &film.setup, || {})?.world;
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
            eprintln!("aaa-server: {e}");
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
        let (dir, Asked::Play(setup, seconds, film)) = asked.unwrap() else {
            panic!("not a match to play");
        };
        assert_eq!((dir.as_str(), setup.map.as_str()), ("game", "berlin"));
        assert_eq!((setup.seed, seconds), (7, 5.0));
        assert_eq!(film.as_deref(), Some("out.film"));
        let (_, Asked::Play(setup, seconds, film)) =
            super::asked(args(&["game", "berlin"])).unwrap()
        else {
            panic!("not a match to play");
        };
        assert!(setup.fog.is_none() && film.is_none());
        assert_eq!(seconds, 60.0);
    }

    #[test]
    fn a_film_to_play_again_takes_the_place_of_a_map() {
        let (dir, asked) = asked(args(&["--replay", "out.film", "game"])).unwrap();
        assert!(matches!(asked, Asked::Replay(film) if film == "out.film"));
        assert_eq!(dir, "game");
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
        ] {
            assert!(asked(args(words)).is_err(), "{words:?}");
        }
    }
}
