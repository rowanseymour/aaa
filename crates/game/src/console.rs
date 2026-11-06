//! The console: the key left of 1 opens it, and it takes script commands in
//! the game's own style, which `--exec` also reads from files. A line holds
//! commands split by `;`, each a case-insensitive verb and its words, with
//! "quotes" around a word that has spaces; `#` or `//` makes the rest of a
//! line a comment. `help` lists the verbs. [`crate::remote`] takes lines
//! from a socket too.

use std::collections::VecDeque;
use std::f32::consts::FRAC_PI_2;
use std::sync::mpsc::SyncSender;

use aaa_map::Recruits;
use aaa_render::MapAssets;
use aaa_render::camera::RtsCamera;
use aaa_render::terrain::world as point;
use bevy::ecs::system::SystemState;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::PrimaryWindow;

use crate::remote::Remote;
use crate::{Drawn, Loaded, Local, Screen, Selected, Sim, UnitView};

/// Lines kept for scrolling back through.
const SCROLLBACK: usize = 200;
/// Lines shown above the prompt.
const SHOWN: usize = 16;
/// How deep scripts may run scripts, so one that runs itself stops.
const MAX_DEPTH: usize = 8;

pub struct ConsolePlugin;

impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Console>()
            .add_systems(Startup, spawn)
            .add_systems(Update, (keys, run, show).chain());
    }
}

/// Whether the console is closed, for the systems that read the keyboard
/// and mouse to wait on.
pub fn closed(console: Res<Console>) -> bool {
    !console.open
}

#[derive(Resource, Default)]
pub struct Console {
    open: bool,
    /// What it has said, oldest first.
    lines: VecDeque<String>,
    input: String,
    /// Lines entered, oldest first, for the arrow keys to bring back.
    history: Vec<String>,
    /// Where in `history` the arrow keys are; `None` for a fresh line.
    recall: Option<usize>,
    /// Lines to run, with where they came from.
    queued: VecDeque<(String, Source)>,
    /// What the line running says, for answering a client with.
    capture: Option<String>,
    /// Where the answer to the line running goes, if it came from a client.
    answer: Option<SyncSender<String>>,
    /// Scripts held until a match starts, since most commands need one.
    scripts: Vec<String>,
    /// Tab was pressed and the input wants completing.
    complete: bool,
    /// Who owns what `create` raises; the player's kingdom if `None`.
    placement: Option<u32>,
    /// The id the next object raised takes.
    next_id: Option<u32>,
}

impl Console {
    /// Runs a script once a match has started.
    pub fn exec_on_start(&mut self, path: &str) {
        self.scripts.push(path.to_string());
    }

    fn say(&mut self, text: &str) {
        for line in text.lines() {
            info!("console: {line}");
            if let Some(capture) = &mut self.capture {
                capture.push_str(line);
                capture.push('\n');
            }
            self.lines.push_back(line.to_string());
            if self.lines.len() > SCROLLBACK {
                self.lines.pop_front();
            }
        }
    }
}

/// Where a line to run came from.
enum Source {
    Typed,
    Script,
    Client(SyncSender<String>),
}

/// Queues the held scripts; added to the match's start.
pub fn start_scripts(mut console: ResMut<Console>) {
    for path in std::mem::take(&mut console.scripts) {
        console
            .queued
            .push_back((format!("exec \"{path}\""), Source::Script));
    }
}

/// A verb, the words it takes and what it does.
struct Verb {
    name: &'static str,
    args: &'static str,
    does: &'static str,
}

const fn verb(name: &'static str, args: &'static str, does: &'static str) -> Verb {
    Verb { name, args, does }
}

const VERBS: &[Verb] = &[
    verb(
        "help",
        "[verb]",
        "lists the verbs, or says how to use one; also ?",
    ),
    verb("echo", "text", "prints the text"),
    verb(
        "exec",
        "file",
        "runs the commands in a file, a line at a time",
    ),
    verb("quit", "", "ends the program"),
    verb(
        "screenshot",
        "file.png",
        "saves what the window shows to a file, answering once it's written",
    ),
    verb(
        "lookat",
        "[x y]",
        "points the camera at a place on the map, in metres; without one, says where it points",
    ),
    verb(
        "camera",
        "x y [height] [pitch]",
        "points the camera at x y from a height above the ground in metres, looking a pitch in degrees down",
    ),
    verb(
        "zoom",
        "[0-1]",
        "zooms from 0, all the way out, to 1, all the way in; without a level, says it",
    ),
    verb(
        "yaw",
        "[degrees]",
        "turns the camera to look that far counter-clockwise from the map's x axis; without, back to the game's",
    ),
    verb(
        "pitch",
        "[degrees]",
        "tilts the camera that far down from level; without, back to the game's",
    ),
    verb(
        "defaultangles",
        "",
        "puts the camera back to the game's own view",
    ),
    verb(
        "create",
        "template [x y]",
        "raises a regiment for the placement kingdom where the camera points, or at x y, and says its id",
    ),
    verb(
        "placementkingdom",
        "[kingdom]",
        "sets, by name or id, who owns what create raises; without one, lists the kingdoms",
    ),
    verb(
        "playerkingdom",
        "[kingdom]",
        "plays as a kingdom, by name or id; without one, lists them",
    ),
    verb(
        "move",
        "regiment x y",
        "sends a regiment, by its id, to x y",
    ),
    verb("destroyselected", "", "destroys the selected regiment"),
    verb(
        "gamespeed",
        "[speed]",
        "runs the game that many times as fast, 0.05 to 16; without, says how fast",
    ),
    verb(
        "gamepause",
        "[1/0]",
        "pauses or resumes the game; without, switches",
    ),
];

fn find(name: &str) -> Option<&'static Verb> {
    let name = if name == "?" { "help" } else { name };
    VERBS.iter().find(|v| v.name.eq_ignore_ascii_case(name))
}

/// A line's commands, each split into words.
fn commands(line: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    // A word was started, so `""` still counts as one.
    let mut started = false;
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        let comment = c == '#' || (c == '/' && chars.peek() == Some(&'/'));
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            _ if quoted => word.push(c),
            _ if comment && !started => break,
            ';' => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
                if !words.is_empty() {
                    out.push(std::mem::take(&mut words));
                }
            }
            c if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    if !words.is_empty() {
        out.push(words);
    }
    out
}

/// Runs a line, saying what it answers.
fn run_line(world: &mut World, line: &str, depth: usize) {
    for words in commands(line) {
        let reply = execute(world, &words, depth);
        let text = match reply {
            Ok(t) => t,
            Err(e) => format!("{}: {e}", words[0].to_ascii_lowercase()),
        };
        if !text.is_empty() {
            world.resource_mut::<Console>().say(&text);
        }
    }
}

type Reply = Result<String, String>;

fn execute(world: &mut World, words: &[String], depth: usize) -> Reply {
    let Some(verb) = find(&words[0]) else {
        return Err("no such command; help lists them".into());
    };
    let args: Vec<&str> = words[1..].iter().map(String::as_str).collect();
    match verb.name {
        "help" => help(&args),
        "echo" => Ok(args.join(" ")),
        "exec" => exec(world, &args, depth),
        "quit" => {
            world.write_message(AppExit::Success);
            Ok(String::new())
        }
        "screenshot" => screenshot(world, &args),
        "gamespeed" => game_speed(world, &args),
        "gamepause" => game_pause(world, &args),
        name => {
            let playing = world
                .get_resource::<State<Screen>>()
                .is_some_and(|s| *s.get() == Screen::Match);
            if !playing {
                return Err("only during a match".into());
            }
            match name {
                "lookat" => look_at(world, &args),
                "camera" => camera(world, &args),
                "zoom" => zoom(world, &args),
                "yaw" => yaw(world, &args),
                "pitch" => pitch(world, &args),
                "defaultangles" => no_args(&args).and_then(|_| {
                    with_camera(world, |cam, _| {
                        cam.default_angles();
                        Ok(String::new())
                    })
                }),
                "create" => create(world, &args),
                "placementkingdom" => placement_kingdom(world, &args),
                "playerkingdom" => player_kingdom(world, &args),
                "move" => move_regiment(world, &args),
                "destroyselected" => destroy_selected(world, &args),
                _ => unreachable!("{name} isn't handled"),
            }
        }
    }
}

fn usage(name: &str) -> String {
    let v = find(name).expect("a verb");
    format!("usage: {} {}", v.name, v.args)
}

fn no_args(args: &[&str]) -> Result<(), String> {
    match args {
        [] => Ok(()),
        _ => Err("takes no words".into()),
    }
}

fn number(word: &str) -> Result<f32, String> {
    word.parse::<f32>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{word:?} isn't a number"))
}

fn help(args: &[&str]) -> Reply {
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
            Ok(format!("{} {}\n  {}", v.name, v.args, v.does))
        }
        _ => Err(usage("help")),
    }
}

fn exec(world: &mut World, args: &[&str], depth: usize) -> Reply {
    let [path] = args else {
        return Err(usage("exec"));
    };
    if depth >= MAX_DEPTH {
        return Err(format!(
            "scripts run {MAX_DEPTH} deep; is {path} running itself?"
        ));
    }
    let script = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    for line in script.lines() {
        run_line(world, line, depth + 1);
    }
    Ok(String::new())
}

fn screenshot(world: &mut World, args: &[&str]) -> Reply {
    let [path] = args else {
        return Err(usage("screenshot"));
    };
    let windows = world
        .query_filtered::<(), With<PrimaryWindow>>()
        .iter(world)
        .count();
    if windows == 0 {
        return Err("no window to save; --screenshot saves a windowless run's".into());
    }
    let path = std::path::PathBuf::from(path);
    // A client's answer waits for the file, with what the line said so far.
    let mut console = world.resource_mut::<Console>();
    let mut waiting = console
        .answer
        .take()
        .map(|answer| (answer, console.capture.take().unwrap_or_default()));
    world.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>, mut console: ResMut<Console>| {
            let said = match aaa_render::shot::save(&captured.image, &path) {
                Ok(()) => format!("saved {}", path.display()),
                Err(e) => format!("screenshot: {}: {e}", path.display()),
            };
            console.say(&said);
            if let Some((answer, before)) = waiting.take() {
                let _ = answer.send(before + &said + "\n");
            }
        },
    );
    Ok(String::new())
}

fn game_speed(world: &mut World, args: &[&str]) -> Reply {
    let mut time = world.resource_mut::<Time<Virtual>>();
    match args {
        [] => Ok(format!("{}", time.relative_speed())),
        [speed] => {
            let speed = number(speed)?;
            if !(0.05..=16.0).contains(&speed) {
                return Err("takes 0.05 to 16".into());
            }
            time.set_relative_speed(speed);
            Ok(String::new())
        }
        _ => Err(usage("gamespeed")),
    }
}

fn game_pause(world: &mut World, args: &[&str]) -> Reply {
    let mut time = world.resource_mut::<Time<Virtual>>();
    let pause = match args {
        [] => !time.is_paused(),
        ["1"] => true,
        ["0"] => false,
        _ => return Err(usage("gamepause")),
    };
    match pause {
        true => time.pause(),
        false => time.unpause(),
    }
    Ok(String::new())
}

/// Runs `f` on the match's camera and moves it there, since its controls,
/// which otherwise would, wait while the console is open.
fn with_camera(
    world: &mut World,
    f: impl FnOnce(&mut RtsCamera, &aaa_map::Ground) -> Reply,
) -> Reply {
    world.resource_scope(|world, loaded: Mut<Loaded>| {
        let mut cameras = world.query::<(&mut RtsCamera, &mut Transform, &mut Projection)>();
        let (mut cam, mut transform, mut projection) =
            cameras.single_mut(world).map_err(|_| "no camera")?;
        let reply = f(&mut cam, &loaded.map.ground)?;
        *transform = cam.transform();
        *projection = cam.projection();
        Ok(reply)
    })
}

/// A place on the map, kept within it.
fn place(ground: &aaa_map::Ground, x: &str, y: &str) -> Result<[f32; 2], String> {
    let (x, y) = (number(x)?, number(y)?);
    let size = ground.size();
    if !(0.0..=size).contains(&x) || !(0.0..=size).contains(&y) {
        return Err(format!("{x} {y} is off the map, which runs 0 to {size}"));
    }
    Ok([x, y])
}

fn focus(cam: &RtsCamera) -> [f32; 2] {
    [cam.focus.x, -cam.focus.z]
}

fn look_at(world: &mut World, args: &[&str]) -> Reply {
    with_camera(world, |cam, ground| match args {
        [] => {
            let [x, y] = focus(cam);
            Ok(format!("{x:.1} {y:.1}"))
        }
        [x, y] => {
            let [x, y] = place(ground, x, y)?;
            cam.focus = point(x, y, ground.height_at(x, y));
            Ok(String::new())
        }
        _ => Err(usage("lookat")),
    })
}

fn camera(world: &mut World, args: &[&str]) -> Reply {
    with_camera(world, |cam, ground| {
        let (x, y, rest) = match args {
            [x, y, rest @ ..] if rest.len() <= 2 => (x, y, rest),
            _ => return Err(usage("camera")),
        };
        let [x, y] = place(ground, x, y)?;
        if let Some(pitch) = rest.get(1) {
            set_pitch(cam, number(pitch)?)?;
        }
        if let Some(height) = rest.first() {
            let (near, far) = cam.distances();
            cam.distance = (number(height)? / cam.pitch.sin()).clamp(near, far);
        }
        cam.focus = point(x, y, ground.height_at(x, y));
        Ok(String::new())
    })
}

fn zoom(world: &mut World, args: &[&str]) -> Reply {
    with_camera(world, |cam, _| {
        let (near, far) = cam.distances();
        // Evenly in ratio, as the mouse wheel zooms.
        match args {
            [] => Ok(format!(
                "{:.2}",
                (cam.distance / far).ln() / (near / far).ln()
            )),
            [level] => {
                let level = number(level)?;
                if !(0.0..=1.0).contains(&level) {
                    return Err("takes 0 to 1".into());
                }
                cam.distance = far * (near / far).powf(level);
                Ok(String::new())
            }
            _ => Err(usage("zoom")),
        }
    })
}

fn yaw(world: &mut World, args: &[&str]) -> Reply {
    with_camera(world, |cam, _| {
        // At no yaw the camera looks along the map's y axis.
        cam.yaw = match args {
            [] => RtsCamera::new(cam.focus).yaw,
            [degrees] => number(degrees)?.to_radians() - FRAC_PI_2,
            _ => return Err(usage("yaw")),
        };
        Ok(String::new())
    })
}

fn set_pitch(cam: &mut RtsCamera, degrees: f32) -> Result<(), String> {
    if !(5.0..=89.0).contains(&degrees) {
        return Err("pitch takes 5 to 89 degrees".into());
    }
    cam.pitch = degrees.to_radians();
    Ok(())
}

fn pitch(world: &mut World, args: &[&str]) -> Reply {
    with_camera(world, |cam, _| {
        match args {
            [] => cam.pitch = RtsCamera::new(cam.focus).pitch,
            [degrees] => set_pitch(cam, number(degrees)?)?,
            _ => return Err(usage("pitch")),
        }
        Ok(String::new())
    })
}

/// The player a word names, by name or id.
fn kingdom(sim: &aaa_sim::World, word: &str) -> Result<u32, String> {
    let players = &sim.sides.players;
    players
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(word) || p.id.to_string() == word)
        .map(|p| p.id)
        .ok_or_else(|| format!("no kingdom {word:?}; {}", kingdoms(sim, None)))
}

/// The map's kingdoms, marking `current`.
fn kingdoms(sim: &aaa_sim::World, current: Option<u32>) -> String {
    let names: Vec<String> = sim
        .sides
        .players
        .iter()
        .map(|p| {
            let mark = if Some(p.id) == current { "*" } else { "" };
            format!("{mark}\"{}\" ({})", p.name, p.id)
        })
        .collect();
    format!("kingdoms: {}", names.join(", "))
}

fn placement_kingdom(world: &mut World, args: &[&str]) -> Reply {
    let sim = &world.resource::<Sim>().0;
    let current = world
        .resource::<Console>()
        .placement
        .or(world.resource::<Local>().0);
    match args {
        [] => Ok(kingdoms(sim, current)),
        [word] => {
            let id = kingdom(sim, word)?;
            world.resource_mut::<Console>().placement = Some(id);
            Ok(String::new())
        }
        _ => Err(usage("placementkingdom")),
    }
}

fn player_kingdom(world: &mut World, args: &[&str]) -> Reply {
    let sim = &world.resource::<Sim>().0;
    match args {
        [] => Ok(kingdoms(sim, world.resource::<Local>().0)),
        [word] => {
            let id = kingdom(sim, word)?;
            world.resource_mut::<Local>().0 = Some(id);
            world.resource_mut::<Selected>().0 = None;
            Ok(String::new())
        }
        _ => Err(usage("playerkingdom")),
    }
}

fn create(world: &mut World, args: &[&str]) -> Reply {
    let (template, at) = match args {
        [template] => (template, None),
        [template, x, y] => (template, Some((x, y))),
        _ => return Err(usage("create")),
    };
    let (focus, yaw) = {
        let cam = world
            .query::<&RtsCamera>()
            .single(world)
            .map_err(|_| "no camera")?;
        (focus(cam), cam.yaw)
    };
    let position = match at {
        Some((x, y)) => place(&world.resource::<Loaded>().map.ground, x, y)?,
        None => focus,
    };
    let owner = world
        .resource::<Console>()
        .placement
        .or(world.resource::<Local>().0)
        .ok_or("no placement kingdom; set one with placementkingdom")?;
    let id = next_id(world);
    let recruits = recruits(world);
    // Facing the way the camera looks, so it's seen from behind.
    let facing = [-yaw.sin(), yaw.cos()];
    let regiment = recruits.raise(template, id, owner, position, facing)?;
    world.resource_mut::<Console>().next_id = Some(id + 1 + regiment.members.len() as u32);
    let r = world.resource_mut::<Sim>().0.add_regiment(&regiment);
    let units = world.resource::<Sim>().0.regiments[r].units.clone();
    let mut state: SystemState<(Commands, MapAssets)> = SystemState::new(world);
    let missing = world.resource_scope(|world, mut loaded: Mut<Loaded>| {
        world.resource_scope(|world, mut drawn: Mut<Drawn>| {
            let (mut commands, mut assets) =
                state.get_mut(world).expect("the render plugin's assets");
            let mut missing = 0;
            for (m, unit) in regiment.members.iter().zip(units) {
                let models = &mut drawn.0;
                let spawned = models.spawn_unit(
                    &mut commands,
                    &mut loaded.game,
                    &mut assets.models(),
                    &m.thing,
                    m.speed,
                );
                match spawned {
                    Some(e) => {
                        commands.entity(e).insert(UnitView {
                            unit,
                            shot: None,
                            dead: false,
                            conform: models.conform(&m.thing.template),
                        });
                    }
                    None => missing += 1,
                }
            }
            missing
        })
    });
    state.apply(world);
    let mut reply = format!(
        "{} {} of {} units at {:.1} {:.1}",
        regiment.template,
        regiment.id,
        regiment.members.len(),
        position[0],
        position[1]
    );
    if missing > 0 {
        reply += &format!(", {missing} without a model");
    }
    Ok(reply)
}

/// An id no object on the map has.
fn next_id(world: &mut World) -> u32 {
    if let Some(id) = world.resource::<Console>().next_id {
        return id;
    }
    let sim = &world.resource::<Sim>().0;
    let things = world.resource::<Loaded>().map.things.iter().map(|t| t.id);
    let id = things
        .chain(sim.regiments.iter().map(|r| r.id))
        .chain(sim.units.iter().map(|u| u.id))
        .chain(sim.buildings.iter().map(|b| b.id))
        .max()
        .map_or(1, |id| id + 1);
    world.resource_mut::<Console>().next_id = Some(id);
    id
}

/// What raising regiments takes, read from the game's data the first time.
fn recruits(world: &mut World) -> &Recruits {
    if !world.contains_resource::<Raising>() {
        let recruits = Recruits::new(&mut world.resource_mut::<Loaded>().game);
        world.insert_resource(Raising(recruits));
    }
    &world.resource::<Raising>().0
}

#[derive(Resource)]
struct Raising(Recruits);

fn move_regiment(world: &mut World, args: &[&str]) -> Reply {
    let [id, x, y] = args else {
        return Err(usage("move"));
    };
    let point = place(&world.resource::<Loaded>().map.ground, x, y)?;
    let sim = &mut world.resource_mut::<Sim>().0;
    let r = sim
        .regiments
        .iter()
        .position(|r| r.id.to_string() == *id)
        .ok_or_else(|| format!("no regiment {id}"))?;
    let owner = sim.regiments[r].owner;
    sim.order_move(owner, r, point)
        .map_err(|_| format!("regiment {id} is destroyed"))?;
    Ok(String::new())
}

fn destroy_selected(world: &mut World, args: &[&str]) -> Reply {
    no_args(args)?;
    let r = world.resource::<Selected>().0.ok_or("nothing selected")?;
    world.resource_mut::<Sim>().0.destroy(r);
    world.resource_mut::<Selected>().0 = None;
    Ok(String::new())
}

/// What tab offers after `before` on the line: verbs first, then the
/// templates `create` can raise or the kingdoms.
fn completions(world: &mut World, before: &str) -> Vec<String> {
    // A line's last command is the one being typed.
    let last = before.rsplit(';').next().unwrap_or("");
    let words = commands(last).pop().unwrap_or_default();
    let in_match = world.contains_resource::<Sim>();
    match words.as_slice() {
        [] => VERBS.iter().map(|v| v.name.to_string()).collect(),
        [verb] if verb.eq_ignore_ascii_case("help") => {
            VERBS.iter().map(|v| v.name.to_string()).collect()
        }
        [verb] if verb.eq_ignore_ascii_case("create") && in_match => recruits(world)
            .templates()
            .iter()
            .map(|t| t.to_string())
            .collect(),
        [verb] if verb.to_ascii_lowercase().ends_with("kingdom") && in_match => {
            let players = &world.resource::<Sim>().0.sides.players;
            players.iter().map(|p| quote(&p.name)).collect()
        }
        _ => Vec::new(),
    }
}

fn quote(word: &str) -> String {
    match word.contains(char::is_whitespace) {
        true => format!("\"{word}\""),
        false => word.to_string(),
    }
}

/// Completes the last word as far as the choices agree, or lists them.
fn complete(world: &mut World) {
    let input = world.resource::<Console>().input.clone();
    let (before, partial) = match input.rfind(|c: char| c.is_whitespace() || c == ';') {
        Some(i) => input.split_at(i + 1),
        None => ("", input.as_str()),
    };
    let (before, partial) = (before.to_string(), partial.to_string());
    let lower = partial.trim_start_matches('"').to_ascii_lowercase();
    let choices: Vec<String> = completions(world, &before)
        .into_iter()
        .filter(|c| {
            c.trim_start_matches('"')
                .to_ascii_lowercase()
                .starts_with(&lower)
        })
        .collect();
    let mut console = world.resource_mut::<Console>();
    match choices.as_slice() {
        [] => {}
        [one] => console.input = format!("{before}{one} "),
        many => {
            let mut lead = many[0].clone();
            for c in &many[1..] {
                let n = lead
                    .chars()
                    .zip(c.chars())
                    .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                    .count();
                lead = lead.chars().take(n).collect();
            }
            if lead.len() > partial.len() {
                console.input = format!("{before}{lead}");
            } else {
                console.say(&many.join("  "));
            }
        }
    }
}

/// Opens and closes the console, and edits the line being typed.
fn keys(mut presses: MessageReader<KeyboardInput>, mut console: ResMut<Console>) {
    for press in presses.read() {
        if press.state != ButtonState::Pressed {
            continue;
        }
        if press.key_code == KeyCode::Backquote {
            console.open = !console.open;
            continue;
        }
        if !console.open {
            continue;
        }
        match press.key_code {
            KeyCode::Escape => console.input.clear(),
            KeyCode::Backspace => {
                console.input.pop();
                console.recall = None;
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                let line = std::mem::take(&mut console.input);
                console.recall = None;
                if line.trim().is_empty() {
                    continue;
                }
                if console.history.last() != Some(&line) {
                    console.history.push(line.clone());
                }
                console.queued.push_back((line, Source::Typed));
            }
            KeyCode::ArrowUp => {
                let back = match console.recall {
                    Some(at) => at.saturating_sub(1),
                    None if console.history.is_empty() => continue,
                    None => console.history.len() - 1,
                };
                console.recall = Some(back);
                console.input = console.history[back].clone();
            }
            KeyCode::ArrowDown => {
                let Some(at) = console.recall else { continue };
                match console.history.get(at + 1) {
                    Some(line) => {
                        console.input = line.clone();
                        console.recall = Some(at + 1);
                    }
                    None => {
                        console.input.clear();
                        console.recall = None;
                    }
                }
            }
            KeyCode::Tab => console.complete = true,
            _ => {
                let typed = match &press.logical_key {
                    Key::Character(c) => c.as_str(),
                    Key::Space => " ",
                    _ => continue,
                };
                console
                    .input
                    .extend(typed.chars().filter(|c| !c.is_control()));
                console.recall = None;
            }
        }
    }
}

/// Runs what's been entered or queued, and completes what's being typed.
fn run(world: &mut World) {
    if std::mem::take(&mut world.resource_mut::<Console>().complete) {
        complete(world);
    }
    if let Some(remote) = world.get_resource::<Remote>() {
        let asked = remote.take();
        let mut console = world.resource_mut::<Console>();
        for (line, answer) in asked {
            console.queued.push_back((line, Source::Client(answer)));
        }
    }
    while let Some((line, source)) = world.resource_mut::<Console>().queued.pop_front() {
        let mut console = world.resource_mut::<Console>();
        if !matches!(source, Source::Script) {
            console.say(&format!("] {line}"));
        }
        if let Source::Client(answer) = source {
            console.capture = Some(String::new());
            console.answer = Some(answer);
        }
        run_line(world, &line, 0);
        let mut console = world.resource_mut::<Console>();
        let said = console.capture.take().unwrap_or_default();
        // Unless a command took it, to answer once its work is done.
        if let Some(answer) = console.answer.take() {
            let _ = answer.send(said);
        }
    }
}

#[derive(Component)]
struct Panel;

#[derive(Component)]
struct PanelText;

fn spawn(mut commands: Commands) {
    commands.spawn((
        Panel,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            width: percent(100),
            padding: UiRect::all(px(8)),
            ..default()
        },
        GlobalZIndex(100),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
        children![(
            PanelText,
            Text::default(),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(Color::srgb(0.85, 0.85, 0.85)),
        )],
    ));
}

fn show(
    console: Res<Console>,
    mut panels: Query<&mut Node, With<Panel>>,
    mut texts: Query<&mut Text, With<PanelText>>,
) {
    if !console.is_changed() {
        return;
    }
    for mut node in &mut panels {
        node.display = if console.open {
            Display::Flex
        } else {
            Display::None
        };
    }
    let skip = console.lines.len().saturating_sub(SHOWN);
    let mut text: Vec<&str> = console
        .lines
        .iter()
        .skip(skip)
        .map(String::as_str)
        .collect();
    let prompt = format!("] {}_", console.input);
    text.push(&prompt);
    for mut t in &mut texts {
        t.0 = text.join("\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<Vec<&str>> {
        let c = commands(line);
        // Leaked for the comparison's sake.
        c.into_iter()
            .map(|w| w.into_iter().map(|s| &*s.leak()).collect())
            .collect()
    }

    #[test]
    fn splits_commands_and_words() {
        assert_eq!(words("lookat 10 20"), [["lookat", "10", "20"]]);
        assert_eq!(
            words("  +left;  wireframe ;lookat 1 2;"),
            [vec!["+left"], vec!["wireframe"], vec!["lookat", "1", "2"]]
        );
        assert_eq!(
            words("placementkingdom \"Free French\"; echo \"a;b\" \"\""),
            [
                vec!["placementkingdom", "Free French"],
                vec!["echo", "a;b", ""]
            ]
        );
    }

    #[test]
    fn drops_comments() {
        assert!(words("# lookat 1 2").is_empty());
        assert!(words("   // lookat 1 2").is_empty());
        assert!(words("#changeres 640 480 1").is_empty());
        assert_eq!(words("zoom 1; // the rest"), [["zoom", "1"]]);
        assert_eq!(words("echo a#b"), [["echo", "a#b"]]);
    }

    #[test]
    fn finds_verbs_ignoring_case() {
        assert_eq!(find("LookAt").map(|v| v.name), Some("lookat"));
        assert_eq!(find("?").map(|v| v.name), Some("help"));
        assert!(find("wireframe").is_none());
    }
}
