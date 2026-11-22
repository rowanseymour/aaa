//! The verbs the console knows, and carrying one out.

use bevy::prelude::*;

use crate::actions::Aim;
use crate::{Orders, Screen, Sim};

use super::camera::{camera, look_at, pitch, with_camera, yaw, zoom};
use super::interface::{
    ask, click, game_pause, game_speed, press, resolution, screenshot, ui, ui_scale, ui_tint, wait,
};
use super::parse::commands;
use super::play::{
    advance, attach, attach_hero, build, building_order, commission, create, decide,
    destroy_selected, detach_hero, disband, heroes, objects, order, outcome, placement_kingdom,
    player_kingdom, record, repair, research, select, settlements, sun, unpack,
    use_faction_ability,
};
use super::{Console, MAX_DEPTH, Step};

/// A verb, the words it takes and what it does.
pub(crate) struct Verb {
    pub(crate) name: &'static str,
    pub(crate) args: &'static str,
    pub(crate) does: &'static str,
    /// Carries it out, given its words and how deep in files it's run.
    run: fn(&mut World, &[&str], usize) -> Reply,
    /// It's for developing the game, so runs only where [`developing`] lets it.
    pub(crate) dev: bool,
    /// It runs only during a match.
    pub(crate) in_match: bool,
    /// The original games have a command of this name, in their console.txt
    /// or their data files, though not always taking the same words.
    pub(crate) original: bool,
}

pub(super) const fn verb(
    name: &'static str,
    args: &'static str,
    does: &'static str,
    run: fn(&mut World, &[&str], usize) -> Reply,
) -> Verb {
    Verb {
        name,
        args,
        does,
        run,
        dev: false,
        in_match: false,
        original: false,
    }
}

impl Verb {
    const fn dev(self) -> Verb {
        Verb { dev: true, ..self }
    }

    const fn original(self) -> Verb {
        Verb {
            original: true,
            ..self
        }
    }

    const fn in_match(self) -> Verb {
        Verb {
            in_match: true,
            ..self
        }
    }
}

pub(crate) const VERBS: &[Verb] = &[
    verb(
        "help",
        "[verb]",
        "lists the verbs, or says how to use one; also ?",
        |_, args, _| help(args),
    )
    .original(),
    verb(
        "echo",
        "text",
        "prints the text",
        |_, args, _| Ok(args.join(" ")),
    )
    .original(),
    verb(
        "exec",
        "file",
        "runs the commands in a file, a line at a time",
        exec,
    )
    .original(),
    verb(
        "quit",
        "",
        "ends the program",
        |world, _, _| {
            world.write_message(AppExit::Success);
            Ok(String::new())
        },
    )
    .original(),
    verb(
        "screenshot",
        "file.png",
        "saves what the window shows to a file, answering once it's written",
        |world, args, _| screenshot(world, args),
    )
    .original(),
    verb(
        "ui",
        "",
        "lists the ids of the buttons on screen that can be pressed, and which are chosen",
        |world, args, _| no_args(args).map(|_| ui(world)),
    ),
    verb(
        "click",
        "button",
        "presses a button, by its id; ui lists them",
        |world, args, _| click(world, args),
    ),
    verb(
        "press",
        "key",
        "presses a key as a hotkey file writes it, e.g. esc, f1 or ctrl-s",
        |world, args, _| press(world, args),
    ),
    verb(
        "ask",
        "intent",
        "asks for what a hotkey file names, as its key would, e.g. ActorCommand stop",
        |world, args, _| ask(world, args),
    ),
    verb(
        "wait",
        "seconds",
        "lets that long go by before the next command",
        |world, args, _| wait(world, args),
    )
    .original(),
    verb(
        "lookat",
        "[x y]",
        "points the camera at a place on the map, in metres; without one, says where it points",
        |world, args, _| look_at(world, args),
    )
    .original()
    .in_match(),
    verb(
        "camera",
        "x y [height] [pitch]",
        "points the camera at x y from a height above the ground in metres, looking a pitch in degrees down",
        |world, args, _| camera(world, args),
    )
    .original()
    .in_match(),
    verb(
        "zoom",
        "[0-1]",
        "zooms from 0, all the way out, to 1, all the way in; without a level, says it",
        |world, args, _| zoom(world, args),
    )
    .original()
    .in_match(),
    verb(
        "yaw",
        "[degrees]",
        "turns the camera to look that far counter-clockwise from the map's x axis; without, back to the game's",
        |world, args, _| yaw(world, args),
    )
    .original()
    .in_match(),
    verb(
        "pitch",
        "[degrees]",
        "tilts the camera that far down from level; without, back to the game's",
        |world, args, _| pitch(world, args),
    )
    .original()
    .in_match(),
    verb(
        "defaultangles",
        "",
        "puts the camera back to the game's own view",
        |world, args, _| no_args(args).and_then(|_| {
            with_camera(world, |cam, _| {
                cam.default_angles();
                Ok(String::new())
            })
        }),
    )
    .original()
    .in_match(),
    verb(
        "create",
        "template [x y]",
        "raises an organization for the placement kingdom where the camera points, or at x y, and says its id",
        |world, args, _| create(world, args),
    )
    .original()
    .dev()
    .in_match(),
    verb(
        "placementkingdom",
        "[kingdom]",
        "sets, by name or id, who owns what create raises; without one, lists the kingdoms",
        |world, args, _| placement_kingdom(world, args),
    )
    .original()
    .dev()
    .in_match(),
    verb(
        "playerkingdom",
        "[kingdom]",
        "plays as a kingdom, by name or id; without one, lists them",
        |world, args, _| player_kingdom(world, args),
    )
    .original()
    .dev()
    .in_match(),
    verb(
        "outcome",
        "",
        "says whether the match is over, and how it's come out for each kingdom",
        |world, args, _| no_args(args).map(|_| outcome(&world.resource::<Sim>().0)),
    )
    .in_match(),
    verb(
        "missionwon",
        "kingdom",
        "has a kingdom, by name or id, win the match",
        |world, args, _| decide(world, args, true),
    )
    .dev()
    .in_match(),
    verb(
        "missionlost",
        "kingdom",
        "has a kingdom, by name or id, lose the match",
        |world, args, _| decide(world, args, false),
    )
    .dev()
    .in_match(),
    verb(
        "move",
        "organization (x y | other)",
        "sends an organization, by its id, to x y, or to keep near another organization, by its id",
        |world, args, _| order(world, args, Aim::Move),
    )
    .original()
    .in_match(),
    verb(
        "attack",
        "organization (x y | enemy)",
        "sends an organization, by its id, to x y in its combat formation, taking on enemies that come near on the way, or after an enemy organization, by its id",
        |world, args, _| order(world, args, Aim::Attack),
    )
    .original()
    .in_match(),
    verb(
        "guard",
        "organization (x y | other)",
        "posts an organization, by its id, at x y or beside another organization, by its id, to come back to after every fight",
        |world, args, _| order(world, args, Aim::Guard),
    )
    .original()
    .in_match(),
    verb(
        "heroes",
        "[kingdom]",
        "lists the heroes a kingdom, by name or id, holds, and what each is doing; without one, the kingdom played",
        |world, args, _| heroes(world, args),
    )
    .in_match(),
    verb(
        "settlements",
        "",
        "lists the settlements, with each one's side and the id of its centre",
        |world, args, _| no_args(args).map(|_| settlements(&world.resource::<Sim>().0)),
    )
    .in_match(),
    verb(
        "objects",
        "[kingdom | template ...]",
        "lists the buildings and organizations on the map, each with its template, id, kingdom and place; given kingdoms, by name or id, or templates, only those, a word that names a kingdom taken as one",
        |world, args, _| Ok(objects(&world.resource::<Sim>().0, args)),
    )
    .dev()
    .in_match(),
    verb(
        "select",
        "[id ...]",
        "selects organizations, or one building, of any side by their ids; without any, says what's selected",
        |world, args, _| select(world, args),
    )
    .in_match(),
    verb(
        "commission",
        "template [x y | hero]",
        "asks the selected building to commission a template it lists, or with x y, one that's flown in, to deliver it there; a settlement's company can be led by a hero from the pool",
        |world, args, _| commission(world, args),
    )
    .in_match(),
    verb(
        "research",
        "technology",
        "asks the selected building to research a technology it lists",
        |world, args, _| research(world, args),
    )
    .in_match(),
    verb(
        "usefactionability",
        "n x y",
        "uses the nth of the faction's special operations, from 0 as the panel lists them, on the ground at x y",
        |world, args, _| use_faction_ability(world, args),
    )
    .original()
    .in_match(),
    verb(
        "pack",
        "",
        "packs up the selected building",
        |world, args, _| {
            building_order(world, args, |building| tgsim::Command::Pack { building })
        },
    )
    .in_match(),
    verb(
        "sell",
        "",
        "sells the selected building",
        |world, args, _| {
            building_order(world, args, |building| tgsim::Command::Sell { building })
        },
    )
    .in_match(),
    verb(
        "disband",
        "organization",
        "disbands an organization, by its id",
        |world, args, _| disband(world, args),
    )
    .in_match(),
    verb(
        "attach",
        "organization",
        "attaches an organization, by its id, to an HQ with a place for it, or moves it on to the next",
        |world, args, _| attach(world, args, true),
    )
    .in_match(),
    verb(
        "detach",
        "organization",
        "detaches an organization, by its id, from its HQ",
        |world, args, _| attach(world, args, false),
    )
    .in_match(),
    verb(
        "attachhero",
        "organization hero",
        "puts a hero from its side's pool at the head of a company, by the company's id",
        |world, args, _| attach_hero(world, args),
    )
    .in_match(),
    verb(
        "detachhero",
        "organization",
        "sends the hero leading a company, by its id, back to its side's pool",
        |world, args, _| detach_hero(world, args),
    )
    .in_match(),
    verb(
        "unpack",
        "x y [degrees]",
        "sends the selected packed building to unpack at x y, facing that far counter-clockwise from the map's x axis",
        |world, args, _| unpack(world, args),
    )
    .in_match(),
    verb(
        "build",
        "organization template x y [degrees]",
        "sends an organization, by its id, to put up a building of template at x y, facing that far counter-clockwise from the map's x axis",
        |world, args, _| build(world, args),
    )
    .in_match(),
    verb(
        "repair",
        "organization building",
        "sends an organization, by its id, to repair a building, by its id",
        |world, args, _| repair(world, args),
    )
    .in_match(),
    verb(
        "cancelsite",
        "",
        "calls off the selected building site",
        |world, args, _| building_order(world, args, |building| {
            tgsim::Command::CancelConstruction { building }
        }),
    )
    .in_match(),
    verb(
        "destroyselected",
        "",
        "destroys the selected organizations",
        |world, args, _| destroy_selected(world, args),
    )
    .original()
    .dev()
    .in_match(),
    verb(
        "advance",
        "seconds",
        "plays that much of the game at once, paused or not",
        |world, args, _| advance(world, args),
    )
    .dev()
    .in_match(),
    verb(
        "record",
        "[file]",
        "says how many ticks the match has played, how many orders it's carried out and its hash, which copies of a match share while they've played out the same way; given a file, keeps the match so far there as a film, for tgserver to play again",
        |world, args, _| record(world, args),
    )
    .in_match(),
    verb(
        "fog",
        "",
        "lifts the fog of war from the screen, or lays it again",
        |world, args, _| no_args(args).map(|_| {
            let mut fog = world.resource_mut::<crate::fog::Fog>();
            fog.lifted = !fog.lifted;
            String::new()
        }),
    )
    .original()
    .dev()
    .in_match(),
    verb(
        "sun",
        "[preset]",
        "lights the map with a lighting preset's colours at once, by its id, e.g. sun_dusk, as a trigger setting the sun's colours would; without one, lists them",
        |world, args, _| sun(world, args),
    )
    .dev()
    .in_match(),
    verb(
        "gamespeed",
        "[speed]",
        "runs the game that many times as fast, 0.05 to 16; without, says how fast",
        |world, args, _| game_speed(world, args),
    )
    .original(),
    verb(
        "gamepause",
        "[1/0]",
        "pauses or resumes the game; without, switches",
        |world, args, _| game_pause(world, args),
    )
    .original(),
    verb(
        "toggleshowzonesupply",
        "",
        "shows or hides the supply border",
        |world, args, _| no_args(args).map(|_| {
            let mut border = world.resource_mut::<crate::supply::Border>();
            border.0 = !border.0;
            String::new()
        }),
    )
    .original(),
    verb(
        "uiscale",
        "[scale]",
        "draws the interface that many times as big, 0.5 to 2, though smaller if the window's too narrow for it; without, says how big it's asked to be and is",
        |world, args, _| ui_scale(world, args),
    ),
    verb(
        "uitint",
        "[r g b [r g b]]",
        "shades the interface's backgrounds, borders and text from a dark colour toward a light one, each part 0 to 1; without, says them",
        |world, args, _| ui_tint(world, args),
    ),
    verb(
        "resolution",
        "[width height]",
        "resizes the window; without, says its size",
        |world, args, _| resolution(world, args),
    ),
];

pub(crate) fn find(name: &str) -> Option<&'static Verb> {
    let name = if name == "?" { "help" } else { name };
    VERBS.iter().find(|v| v.name.eq_ignore_ascii_case(name))
}

/// Runs a command, saying what it answers.
pub(crate) fn run_command(world: &mut World, words: &[String], depth: usize) {
    let text = match execute(world, words, depth) {
        Ok(t) => t,
        Err(e) => format!("{}: {e}", words[0].to_ascii_lowercase()),
    };
    if !text.is_empty() {
        world.resource_mut::<Console>().say(&text);
    }
}

pub(super) type Reply = Result<String, String>;

fn execute(world: &mut World, words: &[String], depth: usize) -> Reply {
    let Some(verb) = find(&words[0]) else {
        return Err("no such command; help lists them".into());
    };
    if verb.dev {
        developing(world)?;
    }
    let playing =
        (world.get_resource::<State<Screen>>()).is_some_and(|s| *s.get() == Screen::Match);
    if verb.in_match && !playing {
        return Err("only during a match".into());
    }
    let args: Vec<&str> = words[1..].iter().map(String::as_str).collect();
    (verb.run)(world, &args, depth)
}

pub(crate) fn usage(name: &str) -> String {
    let v = find(name).expect("a verb");
    format!("usage: {} {}", v.name, v.args)
}

pub(super) fn no_args(args: &[&str]) -> Result<(), String> {
    match args {
        [] => Ok(()),
        _ => Err("takes no words".into()),
    }
}

pub(crate) fn number(word: &str) -> Result<f32, String> {
    word.parse::<f32>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{word:?} isn't a number"))
}

pub(crate) fn help(args: &[&str]) -> Reply {
    match args {
        [] => {
            let names: Vec<&str> = VERBS.iter().map(|v| v.name).collect();
            Ok(format!(
                "{}\nhelp <verb> says how to use one",
                names.join(" ")
            ))
        }
        [name] => {
            let v = find(name).ok_or("no such command")?;
            let dev = if v.dev { DEV_ONLY } else { "" };
            Ok(format!("{} {}\n  {}{dev}", v.name, v.args, v.does))
        }
        _ => Err(usage("help")),
    }
}

pub(crate) fn exec(world: &mut World, args: &[&str], depth: usize) -> Reply {
    let [path] = args else {
        return Err(usage("exec"));
    };
    if depth >= MAX_DEPTH {
        return Err(format!(
            "scripts run {MAX_DEPTH} deep; is {path} running itself?"
        ));
    }
    let script = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    // Ahead of whatever follows the exec.
    let mut console = world.resource_mut::<Console>();
    for words in script.lines().flat_map(commands).rev() {
        console.queued.push_front(Step::Command(words, depth + 1));
    }
    Ok(String::new())
}

/// What `help` adds to what a verb for developing the game does.
pub(crate) const DEV_ONLY: &str = "; only with --dev, in a match no other machine shares";

/// Why a verb for developing the game can't run, if it can't: only with
/// `--dev`, and never in a match another machine shares, so no real match
/// is changed or seen through by one.
pub(crate) fn developing(world: &World) -> Result<(), String> {
    if !world.get_resource::<crate::Dev>().is_some_and(|d| d.0) {
        return Err("only with --dev".into());
    }
    if (world.get_resource::<Orders>()).is_some_and(Orders::joined_to) {
        return Err("only in a match no other machine shares".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_verbs_ignoring_case() {
        assert_eq!(find("LookAt").map(|v| v.name), Some("lookat"));
        assert_eq!(find("?").map(|v| v.name), Some("help"));
        assert!(find("wireframe").is_none());
    }
}
