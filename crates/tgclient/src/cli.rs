//! The command line a game's program is run with.

use std::net::SocketAddr;

use bevy::prelude::*;

use crate::Program;

pub(crate) fn usage(program: &Program) -> String {
    let Program {
        name, example_map, ..
    } = program;
    format!(
        "\
usage:
  {name} <game dir> [options]
      set up a game from the menus
  {name} <game dir> <map> [options]
      play a map, e.g. \"{example_map}\"
  {name} <game dir> --join <address> [options]
      play in the match hosted at an address, e.g. 127.0.0.1:4000

options:
  --players <ids>           with --join, the players to give the orders of, by
                            id, e.g. 1,2, playing the first; none, to watch,
                            unless given
  --name <name>             what others in the match know this player as
  --side <n>                play the map's nth player instead of the first one
                            a person may play; 0 to watch computer players play
                            every side
  --exec <script.txt>       run a file of console commands once the match starts,
                            or from the first menu without a map; may be given
                            more than once
  --console <port>          take console lines on a port of this machine, each
                            answered with what the console says, then an empty line
  --screenshot <file.png>   save a frame to a file and exit, without a window
                            or sound, once any scripts have run
  --shot-after <seconds>    with --screenshot, play that long into the match first
  --headless                run without a window or sound, for the console to
                            drive and take screenshots of
  --options <file.toml>     keep the options chosen in this file rather than
                            the usual one, which runs for testing, e.g. with
                            --console, leave alone
  --fog <setting>           none, explored or full: how much of the map the fog
                            of war hides, rather than the game's default
  --seed <number>           what the match's chances are drawn from, to play one
                            out the same way again, rather than a new one
  --log <filter>            what to log, as for RUST_LOG, e.g. \"debug\" or
                            \"tgclient=debug\"; RUST_LOG still applies on top
  --dev                     let the console's verbs for developing the game run,
                            in a match no other machine shares"
    )
}

/// Removes `flag` and the value after it from `args`; `None` if the value is
/// missing.
fn take_option(args: &mut Vec<String>, flag: &str) -> Option<Option<String>> {
    match args.iter().position(|a| a == flag) {
        Some(i) if i + 1 < args.len() => {
            let value = args.remove(i + 1);
            args.remove(i);
            Some(Some(value))
        }
        Some(_) => None,
        None => Some(None),
    }
}

/// Removes `flag` from `args`, saying whether it was there.
fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let at = args.iter().position(|a| a == flag);
    at.map(|at| args.remove(at)).is_some()
}

/// What the command line asks for besides the game and map.
pub(crate) struct Options {
    pub(crate) side: Option<usize>,
    pub(crate) scripts: Vec<String>,
    pub(crate) console: Option<u16>,
    pub(crate) screenshot: Option<String>,
    pub(crate) shot_after: f64,
    pub(crate) headless: bool,
    pub(crate) dev: bool,
    /// Where the options chosen are kept, if not where they usually are.
    pub(crate) saved: Option<String>,
    pub(crate) fog: Option<tgmap::FogSetting>,
    pub(crate) seed: Option<u64>,
    pub(crate) log: Option<String>,
    /// Where the match to play in is hosted, and the players to ask its
    /// host for.
    pub(crate) join: Option<(SocketAddr, Vec<u32>)>,
    /// What the person goes by, if not by the game's default.
    pub(crate) name: Option<String>,
}

/// Takes the options out of `args`; `None` if any is malformed.
pub(crate) fn parse(args: &mut Vec<String>) -> Option<Options> {
    let side = match take_option(args, "--side")? {
        Some(n) => Some(n.parse().ok()?),
        None => None,
    };
    let mut scripts = Vec::new();
    while let Some(script) = take_option(args, "--exec")? {
        scripts.push(script);
    }
    let console = match take_option(args, "--console")? {
        Some(p) => Some(p.parse().ok()?),
        None => None,
    };
    let screenshot = take_option(args, "--screenshot")?;
    let shot_after = match take_option(args, "--shot-after")? {
        Some(s) => s.parse().ok().filter(|s: &f64| (0.0..=600.0).contains(s))?,
        None => 0.0,
    };
    let headless = take_flag(args, "--headless");
    let dev = take_flag(args, "--dev");
    let saved = take_option(args, "--options")?;
    let fog = match take_option(args, "--fog")? {
        Some(f) => Some(tgmap::FogSetting::named(&f)?),
        None => None,
    };
    let seed = match take_option(args, "--seed")? {
        Some(n) => Some(n.parse().ok()?),
        None => None,
    };
    let log = take_option(args, "--log")?;
    let name = match take_option(args, "--name")? {
        Some(typed) => Some(tgserver::name(&typed)?),
        None => None,
    };
    let players = match take_option(args, "--players")? {
        Some(ids) => Some(
            (ids.split(',').map(str::parse))
                .collect::<Result<_, _>>()
                .ok()?,
        ),
        None => None,
    };
    let join = match (take_option(args, "--join")?, players) {
        (Some(at), players) => Some((at.parse().ok()?, players.unwrap_or_default())),
        (None, Some(_)) => return None,
        (None, None) => None,
    };
    // Its host set the match up.
    if join.is_some() && (side.is_some() || fog.is_some() || seed.is_some()) {
        return None;
    }
    Some(Options {
        side,
        scripts,
        console,
        screenshot,
        shot_after,
        headless,
        dev,
        saved,
        fog,
        seed,
        log,
        join,
        name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_match_to_join_is_asked_for_by_its_hosts_address() {
        let asked = |words: &[&str]| {
            let mut args = words.iter().map(|w| w.to_string()).collect();
            parse(&mut args).map(|o| (o.join, args))
        };
        let at: SocketAddr = "127.0.0.1:4000".parse().unwrap();
        let (join, left) = asked(&["game", "--join", "127.0.0.1:4000"]).unwrap();
        assert_eq!((join, left), (Some((at, vec![])), vec!["game".to_string()]));
        let (join, _) = asked(&["--players", "2,1", "game", "--join", "127.0.0.1:4000"]).unwrap();
        assert_eq!(join, Some((at, vec![2, 1])));
        assert_eq!(asked(&["game", "berlin"]).unwrap().0, None);
        // Its host set the match up, and says who plays what.
        for words in [
            &["game", "--join", "nowhere"][..],
            &["game", "--join", "127.0.0.1:4000", "--players", "one"],
            &["game", "--join", "127.0.0.1:4000", "--seed", "5"],
            &["game", "--join", "127.0.0.1:4000", "--side", "2"],
            &["game", "--join", "127.0.0.1:4000", "--fog", "none"],
            &["game", "berlin", "--players", "1"],
        ] {
            assert!(asked(words).is_none(), "{words:?}");
        }
    }
}
