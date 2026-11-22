//! The console: the key left of 1 opens it, and it takes script commands in
//! the game's own style, which `--exec` also reads from files. A line holds
//! commands split by `;`, each a case-insensitive verb and its words, with
//! "quotes" around a word that has spaces; `#` or `//` makes the rest of a
//! line a comment. `help` lists the verbs. [`crate::remote`] takes lines
//! from a socket too.
//!
//! Commands run in turn, each once the one before has finished: a
//! screenshot is on disk, and the screen a click or a key leads to is up.

mod camera;
mod interface;
mod parse;
mod play;
mod remote;
mod verbs;

pub(crate) use interface::shot_taken;
pub(crate) use parse::{commands, complete};
pub(crate) use verbs::run_command;

use std::collections::VecDeque;
use std::sync::mpsc::SyncSender;

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::time::Real;

use crate::ui::{Font, Ink, Surface, backed, label};
use crate::{Orders, Screen};
pub(crate) use remote::Remote;

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
                (
                    keys.run_if(crate::pause::closed).run_if(crate::keys_free),
                    run,
                    show,
                )
                    .chain(),
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
        .is_some_and(|s| *s.get() == Screen::Loading)
        // A match its host has yet to begin is as good as still loading.
        || world.get_resource::<Orders>().is_some_and(Orders::waiting);
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

    use tgrender::shot::Taken;

    use super::verbs::{DEV_ONLY, VERBS, developing};
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

    /// docs/console.md is written from [`VERBS`], so the two can't drift;
    /// `UPDATE_DOCS=1` rewrites it.
    #[test]
    fn docs_list_the_verbs() {
        let mut page = String::from(
            "# Console commands\n\n\
             <!-- Written from the verbs in crates/tgclient/src/console/verbs.rs by the \
             docs_list_the_verbs test; run it with UPDATE_DOCS=1 to rewrite. -->\n\n\
             How lines are written, and how scripts and a port feed them in, is in \
             [the console module](../crates/tgclient/src/console/mod.rs). Words in \
             [brackets] are optional, and (a | b) means either.\n\n\
             - **Original**: the original games have a command of that name, though \
             not always taking the same words.\n\
             - **Match**: only during a match.\n",
        );
        let tick = |on: bool| if on { "✓" } else { "" };
        let dev = DEV_ONLY.trim_start_matches("; ");
        for (heading, dev_only) in [("Playing", false), ("Developing", true)] {
            page += &format!("\n## {heading}\n\n");
            if dev_only {
                page += &format!("These run {dev}.\n\n");
            }
            page += "| Command | What it does | Original | Match |\n|---|---|:-:|:-:|\n";
            for v in VERBS.iter().filter(|v| v.dev == dev_only) {
                let usage = format!("{} {}", v.name, v.args);
                page += &format!(
                    "| `{}` | {} | {} | {} |\n",
                    usage.trim_end().replace('|', "\\|"),
                    v.does.replace('|', "\\|"),
                    tick(v.original),
                    tick(v.in_match),
                );
            }
        }
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/console.md");
        if std::env::var_os("UPDATE_DOCS").is_some() {
            std::fs::write(path, &page).unwrap();
        } else {
            // A Windows checkout may have given the page CRLF line endings.
            let on_disk = std::fs::read_to_string(path)
                .unwrap_or_default()
                .replace("\r\n", "\n");
            assert!(
                on_disk == page,
                "docs/console.md is stale; rerun this test with UPDATE_DOCS=1"
            );
        }
    }

    #[test]
    fn dev_verbs_run_only_with_dev() {
        let mut world = console();
        enter(&mut world, "objects");
        run(&mut world);
        assert_eq!(said(&world), ["objects: only with --dev"]);
        world.insert_resource(crate::Dev(true));
        assert_eq!(developing(&world), Ok(()));
        let (_host, joined, _) = crate::orders::testing::joined(&[1]);
        world.insert_resource(joined);
        assert_eq!(
            developing(&world),
            Err("only in a match no other machine shares".into())
        );
    }
}
