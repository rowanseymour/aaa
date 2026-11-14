//! Plays a match with no window, as fast as it will go.

use std::path::Path;
use std::process::ExitCode;

use aaa_data::GameData;
use aaa_server::Setup;
use aaa_sim::{Schedule, TICK};

const USAGE: &str = "\
usage:
  aaa-server <game dir> <map> [options]    play a map, e.g. \"allies10_berlin\"

options:
  --seed <number>     what the match's chances are drawn from, rather than
                      a new one
  --seconds <number>  how long to play it for, 0 to 36000; 60 unless given
  --fog <setting>     none, explored or full: how much of the map the fog of
                      war hides, rather than the game's default";

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

/// The game's folder, the match to play and how long for, as `args` ask.
fn asked(mut args: Vec<String>) -> Result<(String, Setup, f32), ()> {
    let seed = match take_option(&mut args, "--seed")? {
        Some(n) => n.parse().map_err(|_| ())?,
        None => aaa_server::fresh_seed(),
    };
    let seconds = match take_option(&mut args, "--seconds")? {
        Some(s) => s.parse().ok().filter(|s| (0.0..=36000.0).contains(s)),
        None => Some(60.0),
    };
    let fog = match take_option(&mut args, "--fog")? {
        Some(f) => Some(aaa_map::FogSetting::named(&f).ok_or(())?),
        None => None,
    };
    let [dir, map] = <[String; 2]>::try_from(args).map_err(|_| ())?;
    let setup = Setup {
        map,
        seed,
        fog,
        ..Default::default()
    };
    Ok((dir, setup, seconds.ok_or(())?))
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
    let Ok((dir, setup, seconds)) = asked(std::env::args().skip(1).collect()) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if log::set_logger(&Warnings).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    let started = GameData::open(Path::new(&dir)).and_then(|game| {
        let found = game.find_map(&setup.map);
        let map = found.ok_or_else(|| format!("no map \"{}\"", setup.map))?;
        aaa_server::start(
            &game,
            &Setup {
                map,
                ..setup.clone()
            },
            || {},
        )
    });
    let mut world = match started {
        Ok(started) => started.world,
        Err(e) => {
            eprintln!("aaa-server: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut schedule = Schedule::default();
    for _ in 0..(seconds / TICK).round() as u64 {
        schedule.step(&mut world);
    }
    println!("seed {}", setup.seed);
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
        let asked = asked(args(&["--seed", "7", "game", "--seconds", "5", "berlin"]));
        let (dir, setup, seconds) = asked.unwrap();
        assert_eq!((dir.as_str(), setup.map.as_str()), ("game", "berlin"));
        assert_eq!((setup.seed, seconds), (7, 5.0));
        let (_, setup, seconds) = super::asked(args(&["game", "berlin"])).unwrap();
        assert!(setup.fog.is_none());
        assert_eq!(seconds, 60.0);
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
        ] {
            assert!(asked(args(words)).is_err(), "{words:?}");
        }
    }
}
