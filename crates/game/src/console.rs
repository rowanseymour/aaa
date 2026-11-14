//! The console: the key left of 1 opens it, and it takes script commands in
//! the game's own style, which `--exec` also reads from files. A line holds
//! commands split by `;`, each a case-insensitive verb and its words, with
//! "quotes" around a word that has spaces; `#` or `//` makes the rest of a
//! line a comment. `help` lists the verbs. [`crate::remote`] takes lines
//! from a socket too.
//!
//! Commands run in turn, each once the one before has finished: a
//! screenshot is on disk, and the screen a click or a key leads to is up.

use std::collections::VecDeque;
use std::f32::consts::FRAC_PI_2;
use std::sync::mpsc::SyncSender;

use aaa_render::camera::RtsCamera;
use aaa_render::shot::Taken;
use aaa_render::terrain::{map_point, world as point};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::time::Real;
use bevy::window::PrimaryWindow;

use crate::actions::Aim;
use crate::lobby::Lineup;
use crate::remote::Remote;
use crate::ui::{Font, Ink, Surface, backed, label};
use crate::waypoints::Sent;
use crate::{Loaded, Local, Orders, Raising, Screen, Selected, SelectedBuilding, Sim};

/// Lines kept for scrolling back through.
const SCROLLBACK: usize = 200;
/// Lines shown above the prompt.
const SHOWN: usize = 16;
/// How deep scripts may run scripts, so one that runs itself stops.
const MAX_DEPTH: usize = 8;
/// Frames a click or a key is given to change the screen, and what it
/// changes to to be laid out.
const SETTLE_FRAMES: u32 = 3;
/// Seconds a screenshot is waited for, a window that isn't being drawn
/// never giving one.
const SHOT_WAIT: f64 = 60.0;

pub struct ConsolePlugin;

impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Console>()
            .add_observer(shot_taken)
            .add_systems(Startup, spawn)
            // Not while the pause menu is up, whose pause the console could undo.
            .add_systems(
                Update,
                (keys.run_if(crate::pause::closed), run, show).chain(),
            );
    }
}

/// Whether the console is closed, for the systems that read the keyboard
/// and mouse to wait on.
pub fn closed(console: Res<Console>) -> bool {
    !console.open
}

/// Whether the console has run everything it was given.
pub fn idle(console: Res<Console>) -> bool {
    console.queued.is_empty() && console.scripts.is_empty() && !console.waiting()
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
    /// What's left to run, in order.
    queued: VecDeque<Step>,
    /// What a client's line has said so far, for answering it with.
    capture: Option<String>,
    /// Screenshots asked for and not yet saved, which the next command
    /// waits for.
    shots: usize,
    /// When, on the clock that never pauses, the last was asked for.
    shot_asked: f64,
    /// Frames the next command waits for the screen to settle.
    settling: u32,
    /// Keys `press` holds down, to let go of.
    held: Vec<KeyCode>,
    /// When, on the clock that never pauses, `wait` is over.
    until: Option<f64>,
    /// Scripts held until a match starts, or the first menu is up.
    scripts: Vec<String>,
    /// Tab was pressed and the input wants completing.
    complete: bool,
    /// Who owns what `create` raises; the player's kingdom if `None`.
    placement: Option<u32>,
}

impl Console {
    /// Runs a script once [`start_scripts`] says to.
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

impl Console {
    /// Whether the command that last ran is still at work.
    fn waiting(&self) -> bool {
        self.shots > 0 || self.settling > 0 || self.until.is_some()
    }
}

/// Something to run.
enum Step {
    /// A line as typed or sent, with where its answer goes if a client's.
    Line(String, Option<SyncSender<String>>),
    /// A command's words, and how deep in scripts it is.
    Command(Vec<String>, usize),
    /// Answers a client, its line's commands having finished.
    Answer(SyncSender<String>),
}

/// Queues the held scripts; added to where they start.
pub fn start_scripts(mut console: ResMut<Console>) {
    for path in std::mem::take(&mut console.scripts) {
        console
            .queued
            .push_back(Step::Command(vec!["exec".into(), path], 0));
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
        "ui",
        "",
        "lists the ids of the buttons on screen that can be pressed, and which are chosen",
    ),
    verb(
        "click",
        "button",
        "presses a button, by its id; ui lists them",
    ),
    verb(
        "press",
        "key",
        "presses a key as a hotkey file writes it, e.g. esc, f1 or ctrl-s",
    ),
    verb(
        "wait",
        "seconds",
        "lets that long go by before the next command",
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
    verb(
        "attack",
        "regiment (x y | enemy)",
        "sends a regiment, by its id, to x y in its combat formation, taking on enemies that come near on the way, or after an enemy regiment, by its id",
    ),
    verb(
        "guard",
        "regiment (x y | other)",
        "posts a regiment, by its id, at x y or beside another regiment, by its id, to come back to after every fight",
    ),
    verb(
        "select",
        "[id ...]",
        "selects the player's regiments, or one of its buildings, by their ids; without any, says what's selected",
    ),
    verb(
        "commission",
        "template",
        "asks the selected building to commission a template it lists",
    ),
    verb(
        "research",
        "technology",
        "asks the selected building to research a technology it lists",
    ),
    verb("pack", "", "packs up the selected building"),
    verb("sell", "", "sells the selected building"),
    verb("disband", "regiment", "disbands a regiment, by its id"),
    verb(
        "attach",
        "regiment",
        "attaches a regiment, by its id, to an HQ with a place for it, or moves it on to the next",
    ),
    verb(
        "detach",
        "regiment",
        "detaches a regiment, by its id, from its HQ",
    ),
    verb(
        "unpack",
        "x y [degrees]",
        "sends the selected packed building to unpack at x y, facing that far counter-clockwise from the map's x axis",
    ),
    verb("destroyselected", "", "destroys the selected regiments"),
    verb(
        "advance",
        "seconds",
        "plays that much of the game at once, paused or not",
    ),
    verb(
        "fog",
        "",
        "lifts the fog of war from the screen, or lays it again",
    ),
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
    verb(
        "toggleshowzonesupply",
        "",
        "shows or hides the supply border",
    ),
    verb(
        "uiscale",
        "[scale]",
        "draws the interface that many times as big, 0.5 to 2, though smaller if the window's too narrow for it; without, says how big it's asked to be and is",
    ),
    verb(
        "uitint",
        "[r g b [r g b]]",
        "shades the interface's backgrounds, borders and text from a dark colour toward a light one, each part 0 to 1; without, says them",
    ),
    verb(
        "resolution",
        "[width height]",
        "resizes the window; without, says its size",
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

/// Runs a command, saying what it answers.
fn run_command(world: &mut World, words: &[String], depth: usize) {
    let text = match execute(world, words, depth) {
        Ok(t) => t,
        Err(e) => format!("{}: {e}", words[0].to_ascii_lowercase()),
    };
    if !text.is_empty() {
        world.resource_mut::<Console>().say(&text);
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
        "ui" => no_args(&args).map(|_| ui(world)),
        "click" => click(world, &args),
        "press" => press(world, &args),
        "wait" => wait(world, &args),
        "gamespeed" => game_speed(world, &args),
        "gamepause" => game_pause(world, &args),
        "toggleshowzonesupply" => no_args(&args).map(|_| {
            let mut border = world.resource_mut::<crate::supply::Border>();
            border.0 = !border.0;
            String::new()
        }),
        "uiscale" => ui_scale(world, &args),
        "uitint" => ui_tint(world, &args),
        "resolution" => resolution(world, &args),
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
                "move" => order(world, &args, Aim::Move),
                "attack" => order(world, &args, Aim::Attack),
                "guard" => order(world, &args, Aim::Guard),
                "select" => select(world, &args),
                "commission" => commission(world, &args),
                "research" => research(world, &args),
                "unpack" => unpack(world, &args),
                "pack" => {
                    building_order(world, &args, |building| aaa_sim::Command::Pack { building })
                }
                "sell" => {
                    building_order(world, &args, |building| aaa_sim::Command::Sell { building })
                }
                "disband" => disband(world, &args),
                "attach" => attach(world, &args, true),
                "detach" => attach(world, &args, false),
                "destroyselected" => destroy_selected(world, &args),
                "advance" => advance(world, &args),
                "fog" => no_args(&args).map(|_| {
                    let mut fog = world.resource_mut::<crate::fog::Fog>();
                    fog.lifted = !fog.lifted;
                    String::new()
                }),
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
    // Ahead of whatever follows the exec.
    let mut console = world.resource_mut::<Console>();
    for words in script.lines().flat_map(commands).rev() {
        console.queued.push_front(Step::Command(words, depth + 1));
    }
    Ok(String::new())
}

fn screenshot(world: &mut World, args: &[&str]) -> Reply {
    let [path] = args else {
        return Err(usage("screenshot"));
    };
    let path = std::path::PathBuf::from(path);
    // A windowless run draws to an image, which is saved once it's settled.
    if !aaa_render::shot::take(world, path.clone()) {
        world.spawn(Screenshot::primary_window()).observe(
            move |captured: On<ScreenshotCaptured>, mut commands: Commands| {
                let saved = aaa_render::shot::save(&captured.image, &path)
                    .map(|_| path.clone())
                    .map_err(|e| format!("{}: {e}", path.display()));
                commands.trigger(Taken(saved));
            },
        );
    }
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    let mut console = world.resource_mut::<Console>();
    console.shots += 1;
    console.shot_asked = now;
    Ok(String::new())
}

/// Says how a screenshot went, and lets the next command run.
fn shot_taken(taken: On<Taken>, mut console: ResMut<Console>) {
    let said = match &taken.0 {
        Ok(path) => format!("saved {}", path.display()),
        Err(e) => format!("screenshot: {e}"),
    };
    console.say(&said);
    console.shots = console.shots.saturating_sub(1);
}

fn ui(world: &mut World) -> String {
    let (buttons, unnamed) = crate::drive::buttons(world);
    let line = |b: &crate::drive::Seen| match b.chosen {
        true => format!("{}  # chosen", b.id),
        false => b.id.clone(),
    };
    let mut lines: Vec<String> = buttons.iter().map(line).collect();
    if unnamed > 0 {
        lines.push(format!("# and {unnamed} with no id"));
    }
    lines.join("\n")
}

fn click(world: &mut World, args: &[&str]) -> Reply {
    let [id] = args else {
        return Err(usage("click"));
    };
    crate::drive::click(world, id)?;
    world.resource_mut::<Console>().settling = SETTLE_FRAMES;
    Ok(String::new())
}

fn press(world: &mut World, args: &[&str]) -> Reply {
    let [chord] = args else {
        return Err(usage("press"));
    };
    let held = crate::drive::press(world, chord)?;
    let mut console = world.resource_mut::<Console>();
    console.held = held;
    console.settling = SETTLE_FRAMES;
    Ok(String::new())
}

fn wait(world: &mut World, args: &[&str]) -> Reply {
    let [seconds] = args else {
        return Err(usage("wait"));
    };
    let seconds = number(seconds).ok().filter(|s| (0.0..=600.0).contains(s));
    let seconds = seconds.ok_or("takes 0 to 600 seconds")?;
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    world.resource_mut::<Console>().until = Some(now + f64::from(seconds));
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

fn ui_scale(world: &mut World, args: &[&str]) -> Reply {
    let drawn = world.resource::<UiScale>().0;
    let mut scale = world.resource_mut::<crate::UiScaleSetting>();
    match args {
        [] => Ok(format!("{} (drawn at {drawn})", scale.0)),
        [n] => {
            let n = number(n)?;
            if !crate::UI_SCALES.contains(&n) {
                let (low, high) = crate::UI_SCALES.into_inner();
                return Err(format!("takes {low} to {high}"));
            }
            scale.0 = n;
            Ok(String::new())
        }
        _ => Err(usage("uiscale")),
    }
}

fn ui_tint(world: &mut World, args: &[&str]) -> Reply {
    let mut theme = world.resource_mut::<crate::ui::Theme>();
    match args {
        [] => {
            let [r, g, b, _] = theme.tint.to_srgba().to_f32_array();
            let [lr, lg, lb, _] = theme.light.to_srgba().to_f32_array();
            Ok(format!("{r} {g} {b} {lr} {lg} {lb}"))
        }
        [_, _, _] | [_, _, _, _, _, _] => {
            let parts = args
                .iter()
                .map(|a| number(a))
                .collect::<Result<Vec<_>, _>>()?;
            if !parts.iter().all(|c| (0.0..=1.0).contains(c)) {
                return Err("takes each part from 0 to 1".into());
            }
            theme.tint = Color::srgb(parts[0], parts[1], parts[2]);
            if let [_, _, _, r, g, b] = parts[..] {
                theme.light = Color::srgb(r, g, b);
            }
            Ok(String::new())
        }
        _ => Err(usage("uitint")),
    }
}

fn resolution(world: &mut World, args: &[&str]) -> Reply {
    let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
    let Ok(mut window) = windows.single_mut(world) else {
        return Err("no window".into());
    };
    match args {
        [] => Ok(format!("{} {}", window.width(), window.height())),
        [w, h] => {
            let (w, h) = (number(w)?, number(h)?);
            if w < 1.0 || h < 1.0 {
                return Err("takes a width and height of at least 1".into());
            }
            window.resolution.set(w, h);
            Ok(String::new())
        }
        _ => Err(usage("resolution")),
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
    if pause && let Some(mut display) = world.get_resource_mut::<crate::displays::Displays>() {
        display.keep_paused();
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

fn look_at(world: &mut World, args: &[&str]) -> Reply {
    with_camera(world, |cam, ground| match args {
        [] => {
            let [x, y] = map_point(cam.focus);
            Ok(format!("{x:.1} {y:.1}"))
        }
        [x, y] => {
            let [x, y] = place(ground, x, y)?;
            cam.focus = point(x, y, ground.height_at([x, y]));
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
        cam.focus = point(x, y, ground.height_at([x, y]));
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
            if let Some(mut lineup) = world.get_resource_mut::<Lineup>() {
                lineup.seat(Some(id));
            }
            world.resource_mut::<Selected>().0.clear();
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
        (map_point(cam.focus), cam.yaw)
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
    // Facing the way the camera looks, so it's seen from behind.
    let facing = [-yaw.sin(), yaw.cos()];
    let regiment = world
        .resource::<Raising>()
        .0
        .raise(template, 0, owner, position, facing)?;
    let sim = &mut world.resource_mut::<Sim>().0;
    let r = sim.add_new_regiment(regiment);
    let regiment = &sim.regiments[r];
    Ok(format!(
        "{} {} of {} units at {:.1} {:.1}",
        regiment.template,
        regiment.id,
        regiment.units.len(),
        position[0],
        position[1]
    ))
}

/// Gives a regiment an order at a place, marked as if clicked there.
fn order(world: &mut World, args: &[&str], aim: Aim) -> Reply {
    match (aim, args) {
        (Aim::Attack, [id, enemy]) => return after(world, id, enemy, aim),
        (Aim::Guard, [id, other]) => return after(world, id, other, aim),
        _ => {}
    }
    let [id, x, y] = args else {
        return Err(usage(match aim {
            Aim::Attack => "attack",
            Aim::Guard => "guard",
            _ => "move",
        }));
    };
    let point = place(&world.resource::<Loaded>().map.ground, x, y)?;
    let sim = &world.resource::<Sim>().0;
    let r = regiment_by_id(sim, id)?;
    let owner = sim.regiments[r].owner;
    let (regiment, to) = (r, point);
    let command = match aim {
        Aim::Move => aaa_sim::Command::Move { regiment, to },
        Aim::Attack => aaa_sim::Command::Attack { regiment, to },
        Aim::Run => aaa_sim::Command::Run { regiment, to },
        Aim::Guard => aaa_sim::Command::Guard { regiment, to },
        Aim::Unpack => aaa_sim::Command::Unpack {
            regiment,
            at: to,
            facing: crate::actions::UNPACK_FACING,
        },
    };
    give(world, owner, command).map_err(|e| match e {
        aaa_sim::Refused::NotYours => format!("regiment {id} is destroyed"),
        e => format!("refused: {e:?}"),
    })?;
    world.write_message(Sent {
        to: Some(point),
        aim,
        regiments: vec![(r, point)],
    });
    Ok(String::new())
}

/// Sends a regiment after an enemy regiment, or to guard another, both by
/// their ids.
fn after(world: &mut World, id: &str, enemy: &str, aim: Aim) -> Reply {
    let sim = &world.resource::<Sim>().0;
    let find = |id: &str| {
        sim.regiments
            .iter()
            .position(|r| r.id.to_string() == id)
            .ok_or_else(|| format!("no regiment {id}"))
    };
    let (r, e) = (find(id)?, find(enemy)?);
    let there = sim.centre(e);
    let owner = sim.regiments[r].owner;
    let command = match aim {
        Aim::Guard => aaa_sim::Command::GuardPiece {
            regiment: r,
            piece: aaa_sim::Piece::Regiment(e),
        },
        _ => aaa_sim::Command::Pursue {
            regiment: r,
            enemy: e,
        },
    };
    give(world, owner, command).map_err(|e| format!("refused: {e:?}"))?;
    world.write_message(Sent {
        to: None,
        aim,
        regiments: vec![(r, there)],
    });
    Ok(String::new())
}

fn select(world: &mut World, args: &[&str]) -> Reply {
    let sim = &world.resource::<Sim>().0;
    if args.is_empty() {
        let selected = &world.resource::<Selected>().0;
        let mut ids: Vec<String> = selected
            .iter()
            .map(|&r| sim.regiments[r].id.to_string())
            .collect();
        if let Some(b) = world.resource::<SelectedBuilding>().0 {
            ids.push(sim.buildings[b].id.to_string());
        }
        return Ok(match ids.is_empty() {
            true => "nothing selected".into(),
            false => ids.join(" "),
        });
    }
    let player = world.resource::<Local>().0.ok_or("nobody to play")?;
    let scenery = world.resource::<crate::banners::Scenery>();
    if let [id] = args
        // A building packed up has handed its id to its truck.
        && let Some(b) = sim
            .buildings
            .iter()
            .position(|b| !b.gone() && b.id.to_string() == *id)
    {
        if sim.buildings[b].owner != player || !sim.buildings[b].standing() {
            return Err(format!("building {id} isn't the player's to select"));
        }
        world.resource_mut::<Selected>().0.clear();
        world.resource_mut::<SelectedBuilding>().0 = Some(b);
        return Ok(String::new());
    }
    let mut ids: Vec<&str> = Vec::new();
    for id in args {
        if !ids.contains(id) {
            ids.push(id);
        }
    }
    if ids.len() > crate::object_display::MAX_SELECTED {
        return Err(format!(
            "takes up to {} regiments",
            crate::object_display::MAX_SELECTED
        ));
    }
    let picked = ids
        .iter()
        .map(|id| {
            let r = regiment_by_id(sim, id)?;
            match sim.commands(player, r) && !scenery.has(sim, r) {
                true => Ok(r),
                false => Err(format!("regiment {id} isn't the player's to select")),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    world.resource_mut::<Selected>().0 = picked;
    world.resource_mut::<SelectedBuilding>().0 = None;
    Ok(String::new())
}

fn commission(world: &mut World, args: &[&str]) -> Reply {
    let [template] = args else {
        return Err(usage("commission"));
    };
    let player = world.resource::<Local>().0.ok_or("nobody to play")?;
    let b = selected_building(world)?;
    let raising = &world.resource::<Raising>().0;
    let hq = &world.resource::<Sim>().0.buildings[b].template;
    let listed = raising
        .list(hq.as_str())
        .iter()
        .find(|t| t.as_str().eq_ignore_ascii_case(template));
    let template = listed.ok_or_else(|| format!("{hq} doesn't commission {template}"))?;
    let command = aaa_sim::Command::Commission {
        building: b,
        template: template.clone(),
    };
    give(world, player, command).map_err(|e| format!("refused: {e:?}"))?;
    Ok(String::new())
}

fn research(world: &mut World, args: &[&str]) -> Reply {
    let [technology] = args else {
        return Err(usage("research"));
    };
    let player = world.resource::<Local>().0.ok_or("nobody to play")?;
    let b = selected_building(world)?;
    let sim = &world.resource::<Sim>().0;
    let listed = (sim.research_list(b).iter())
        .find(|r| r.technology.as_str().eq_ignore_ascii_case(technology));
    let Some(item) = listed else {
        let template = &sim.buildings[b].template;
        return Err(format!("{template} doesn't research {technology}"));
    };
    let command = aaa_sim::Command::Research {
        building: b,
        technology: item.technology.clone(),
    };
    give(world, player, command).map_err(|e| format!("refused: {e:?}"))?;
    Ok(String::new())
}

/// Gives the selected building the order `order` makes for it.
fn building_order(world: &mut World, args: &[&str], order: fn(usize) -> aaa_sim::Command) -> Reply {
    no_args(args)?;
    let player = world.resource::<Local>().0.ok_or("nobody to play")?;
    let b = selected_building(world)?;
    give(world, player, order(b)).map_err(|e| format!("refused: {e:?}"))?;
    Ok(String::new())
}

fn disband(world: &mut World, args: &[&str]) -> Reply {
    let [id] = args else {
        return Err(usage("disband"));
    };
    let sim = &world.resource::<Sim>().0;
    let regiment = regiment_by_id(sim, id)?;
    let owner = sim.regiments[regiment].owner;
    give(world, owner, aaa_sim::Command::Disband { regiment })
        .map_err(|e| format!("refused: {e:?}"))?;
    Ok(String::new())
}

fn attach(world: &mut World, args: &[&str], attach: bool) -> Reply {
    let [id] = args else {
        return Err(usage(if attach { "attach" } else { "detach" }));
    };
    let sim = &world.resource::<Sim>().0;
    let regiment = regiment_by_id(sim, id)?;
    let owner = sim.regiments[regiment].owner;
    let command = match attach {
        true => aaa_sim::Command::Attach { regiment },
        false => aaa_sim::Command::Detach { regiment },
    };
    give(world, owner, command).map_err(|e| format!("refused: {e:?}"))?;
    Ok(String::new())
}

fn unpack(world: &mut World, args: &[&str]) -> Reply {
    let (x, y, facing) = match args {
        [x, y] => (x, y, crate::actions::UNPACK_FACING),
        [x, y, degrees] => {
            let a = number(degrees)?.to_radians();
            (x, y, [a.cos(), a.sin()])
        }
        _ => return Err(usage("unpack")),
    };
    let point = place(&world.resource::<Loaded>().map.ground, x, y)?;
    let player = world.resource::<Local>().0.ok_or("nobody to play")?;
    let &[r] = &world.resource::<Selected>().0[..] else {
        return Err("select one packed building".into());
    };
    let command = aaa_sim::Command::Unpack {
        regiment: r,
        at: point,
        facing,
    };
    give(world, player, command).map_err(|e| format!("refused: {e:?}"))?;
    let at = aaa_sim::unpack_site(point);
    world.write_message(crate::waypoints::Sent {
        to: Some(at),
        aim: crate::actions::Aim::Unpack,
        regiments: vec![(r, at)],
    });
    Ok(format!("unpacking at {:.0} {:.0}", at[0], at[1]))
}

/// Queues `command` from `player` for the next tick, as a click would.
/// The regiment with object id `id`, by index.
fn regiment_by_id(sim: &aaa_sim::World, id: &str) -> Result<usize, String> {
    (sim.regiments.iter())
        .position(|r| r.id.to_string() == id)
        .ok_or_else(|| format!("no regiment {id}"))
}

fn selected_building(world: &World) -> Result<usize, String> {
    (world.resource::<SelectedBuilding>().0).ok_or_else(|| "no building selected".into())
}

fn give(world: &mut World, player: u32, command: aaa_sim::Command) -> Result<(), aaa_sim::Refused> {
    world.resource_scope(|world, mut orders: Mut<Orders>| {
        orders.give(&world.resource::<Sim>().0, player, command)
    })
}

fn advance(world: &mut World, args: &[&str]) -> Reply {
    let [seconds] = args else {
        return Err(usage("advance"));
    };
    let seconds = number(seconds)?;
    if !(0.0..=600.0).contains(&seconds) {
        return Err("takes 0 to 600 seconds".into());
    }
    let mut selected = std::mem::take(&mut world.resource_mut::<Selected>().0);
    let mut chosen = world.resource::<SelectedBuilding>().0;
    let mut orders = world.remove_resource::<Orders>().unwrap_or_default();
    let sim = &mut world.resource_mut::<Sim>().0;
    for _ in 0..(seconds / aaa_sim::TICK).round() as u32 {
        crate::step_world(sim, &mut orders, &mut selected, &mut chosen);
    }
    world.insert_resource(orders);
    world.resource_mut::<Selected>().0 = selected;
    world.resource_mut::<SelectedBuilding>().0 = chosen;
    Ok(String::new())
}

fn destroy_selected(world: &mut World, args: &[&str]) -> Reply {
    no_args(args)?;
    let selected = std::mem::take(&mut world.resource_mut::<Selected>().0);
    if selected.is_empty() {
        return Err("nothing selected".into());
    }
    for r in selected {
        world.resource_mut::<Sim>().0.destroy(r);
    }
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
        [verb] if verb.eq_ignore_ascii_case("create") && in_match => world
            .resource::<Raising>()
            .0
            .templates()
            .iter()
            .map(|t| t.to_string())
            .collect(),
        [verb] if verb.eq_ignore_ascii_case("commission") && in_match => {
            let sim = &world.resource::<Sim>().0;
            let building = world.resource::<SelectedBuilding>().0;
            let template = building.map_or("", |b| sim.buildings[b].template.as_str());
            let raising = &world.resource::<Raising>().0;
            raising
                .list(template)
                .iter()
                .map(|t| t.to_string())
                .collect()
        }
        [verb] if verb.eq_ignore_ascii_case("research") && in_match => {
            let sim = &world.resource::<Sim>().0;
            let building = world.resource::<SelectedBuilding>().0;
            (building.map_or(&[][..], |b| sim.research_list(b)).iter())
                .map(|r| r.technology.to_string())
                .collect()
        }
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
                console.queued.push_back(Step::Line(line, None));
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
pub(crate) fn run(world: &mut World) {
    if std::mem::take(&mut world.resource_mut::<Console>().complete) {
        complete(world);
    }
    if let Some(remote) = world.get_resource::<Remote>() {
        let asked = remote.take();
        let mut console = world.resource_mut::<Console>();
        for (line, answer) in asked {
            console.queued.push_back(Step::Line(line, Some(answer)));
        }
    }
    // A frame after they went down, so they were seen held together.
    let held = std::mem::take(&mut world.resource_mut::<Console>().held);
    crate::drive::release(world, &held);
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    let loading = world
        .get_resource::<State<Screen>>()
        .is_some_and(|s| *s.get() == Screen::Loading);
    let mut console = world.resource_mut::<Console>();
    if console.until.is_some_and(|until| now >= until) {
        console.until = None;
    }
    if console.shots > 0 && now - console.shot_asked > SHOT_WAIT {
        console.shots = 0;
        console.say("screenshot: no frame came to save");
    }
    // Nor while a map loads, when what most commands are about isn't there,
    // nor until the match's interface is laid out.
    if loading {
        console.settling = SETTLE_FRAMES;
        return;
    }
    if console.settling > 0 {
        console.settling -= 1;
        return;
    }
    while !world.resource::<Console>().waiting() {
        let mut console = world.resource_mut::<Console>();
        let Some(step) = console.queued.pop_front() else {
            break;
        };
        match step {
            Step::Line(line, answer) => {
                console.say(&format!("] {line}"));
                if let Some(answer) = answer {
                    console.capture = Some(String::new());
                    console.queued.push_front(Step::Answer(answer));
                }
                for words in commands(&line).into_iter().rev() {
                    console.queued.push_front(Step::Command(words, 0));
                }
            }
            Step::Command(words, depth) => run_command(world, &words, depth),
            Step::Answer(answer) => {
                let _ = answer.send(console.capture.take().unwrap_or_default());
            }
        }
    }
}

#[derive(Component)]
struct Panel;

#[derive(Component)]
struct PanelText;

fn spawn(mut commands: Commands, mut console: ResMut<Console>) {
    // The first screen isn't laid out yet.
    console.settling = SETTLE_FRAMES;
    commands.spawn((
        Panel,
        backed(
            Surface::Popup,
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                width: percent(100),
                padding: UiRect::all(px(8)),
                ..default()
            },
        ),
        GlobalZIndex(crate::ui::layer::CONSOLE),
        children![(PanelText, label("", Font::Hud, Ink::Help))],
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
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;

    use super::*;

    /// A world the console can run in, and what it has said since.
    fn console() -> World {
        let mut world = World::new();
        world.init_resource::<Console>();
        world.init_resource::<Time<Real>>();
        world.init_resource::<Messages<KeyboardInput>>();
        world.add_observer(shot_taken);
        world
    }

    fn enter(world: &mut World, line: &str) {
        let mut console = world.resource_mut::<Console>();
        console.queued.push_back(Step::Line(line.into(), None));
    }

    fn said(world: &World) -> Vec<&str> {
        let lines = world.resource::<Console>().lines.iter();
        lines
            .map(String::as_str)
            .filter(|l| !l.starts_with(']'))
            .collect()
    }

    fn pass(world: &mut World, seconds: f64) {
        let mut time = world.resource_mut::<Time<Real>>();
        time.advance_by(Duration::from_secs_f64(seconds));
    }

    #[test]
    fn a_script_runs_before_what_follows_its_exec() {
        let script = std::env::temp_dir().join(format!("console-{}.txt", std::process::id()));
        std::fs::write(&script, "echo b; echo c\necho d\n").unwrap();
        let mut world = console();
        enter(
            &mut world,
            &format!("echo a; exec \"{}\"; echo e", script.display()),
        );
        run(&mut world);
        std::fs::remove_file(&script).unwrap();
        assert_eq!(said(&world), ["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn a_key_is_let_go_and_the_screen_settles_before_the_next_command() {
        let mut world = console();
        enter(&mut world, "press esc; echo after");
        run(&mut world);
        assert!(said(&world).is_empty());
        for _ in 0..SETTLE_FRAMES {
            run(&mut world);
        }
        assert!(said(&world).is_empty(), "still settling");
        run(&mut world);
        assert_eq!(said(&world), ["after"]);
        let mut keys = world.resource_mut::<Messages<KeyboardInput>>();
        let keys = keys.drain();
        let states: Vec<ButtonState> = keys.map(|k| k.state).collect();
        assert_eq!(states, [ButtonState::Pressed, ButtonState::Released]);
    }

    #[test]
    fn wait_holds_the_next_command() {
        let mut world = console();
        enter(&mut world, "wait 2; echo after");
        run(&mut world);
        pass(&mut world, 1.0);
        run(&mut world);
        assert!(said(&world).is_empty());
        pass(&mut world, 1.5);
        run(&mut world);
        assert_eq!(said(&world), ["after"]);
    }

    #[test]
    fn a_client_is_answered_once_its_screenshot_is_saved() {
        let mut world = console();
        let (tx, rx) = sync_channel(1);
        let line = Step::Line("screenshot a.png; echo after".into(), Some(tx));
        world.resource_mut::<Console>().queued.push_back(line);
        run(&mut world);
        run(&mut world);
        assert!(rx.try_recv().is_err(), "the file isn't written yet");
        world.trigger(Taken(Ok("a.png".into())));
        run(&mut world);
        assert_eq!(rx.try_recv().unwrap(), "saved a.png\nafter\n");
    }

    #[test]
    fn a_screenshot_that_never_comes_is_given_up_on() {
        let mut world = console();
        enter(&mut world, "screenshot a.png; echo after");
        run(&mut world);
        pass(&mut world, SHOT_WAIT + 1.0);
        run(&mut world);
        assert_eq!(said(&world), ["screenshot: no frame came to save", "after"]);
    }

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

    /// docs/console.md is written from [`VERBS`], so the two can't drift;
    /// `UPDATE_DOCS=1` rewrites it.
    #[test]
    fn docs_list_the_verbs() {
        let mut page = String::from(
            "# Console commands\n\n\
             <!-- Written from the verbs in crates/game/src/console.rs by its \
             docs_list_the_verbs test; run it with UPDATE_DOCS=1 to rewrite. -->\n\n\
             How lines are written, and how scripts and a port feed them in, is in \
             [console.rs](../crates/game/src/console.rs). Words in [brackets] are \
             optional, and (a | b) means either.\n\n",
        );
        for v in VERBS {
            let usage = format!("{} {}", v.name, v.args);
            page += &format!("- `{}`: {}\n", usage.trim_end(), v.does);
        }
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/console.md");
        if std::env::var_os("UPDATE_DOCS").is_some() {
            std::fs::write(path, &page).unwrap();
        } else {
            let on_disk = std::fs::read_to_string(path).unwrap_or_default();
            assert!(
                on_disk == page,
                "docs/console.md is stale; rerun this test with UPDATE_DOCS=1"
            );
        }
    }

    #[test]
    fn finds_verbs_ignoring_case() {
        assert_eq!(find("LookAt").map(|v| v.name), Some("lookat"));
        assert_eq!(find("?").map(|v| v.name), Some("help"));
        assert!(find("wireframe").is_none());
    }
}
