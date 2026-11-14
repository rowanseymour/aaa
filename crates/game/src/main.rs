//! Plays Axis & Allies (2004) matches, set up in a custom game's lobby or on
//! a map given on the command line.
//!
//! You play one of the map's sides, or watch, and the computer plays the rest. Click one of your units or its
//! banner to select its regiment, Shift+click to add a regiment to the
//! selection or take it out, or drag a box over several, with Shift to add
//! them, up to six at once. Click one of your buildings to select it
//! instead; the [`object_display`] along the bottom shows what's selected. The selection
//! passes from a truck to the building it unpacks into, and back as the
//! building packs up. Right-click the ground to send the selection there, keeping its
//! regiments' places relative to each other and stopping to fight enemies on
//! the way. The number keys pick the selection's formation, in the order each regiment's
//! layout lists them, and the buttons in the corner give the selection the
//! orders in [`actions`]. Clicking the minimap moves the camera there, or
//! gives an aimed order there, and the key left of 1 opens a console for
//! script commands. Units and buildings make their sounds as they're
//! selected, ordered, move, fire and die, and show their [`effects`] as they
//! fire, are hurt, die, and pack or unpack. Escape opens a menu that holds
//! the match still while it's up. [`supply`] is drawn on the map. The game's
//! own hotkeys work what they're bound to, by way of [`keys`], so the
//! arrows, not letters, move the camera.

mod actions;
mod backdrop;
mod banners;
mod confirm;
mod console;
mod deliveries;
mod displays;
mod drive;
mod effects;
mod fog;
mod ghost;
mod keys;
mod lobby;
mod menu;
mod messages;
mod minimap;
mod music;
mod object_display;
mod objectives;
mod options;
mod orders;
mod outcome;
mod pause;
mod presets;
mod remote;
mod resource_bar;
mod selection;
mod sound;
mod special_ops;
mod supply;
mod technologies;
mod ui;
mod waypoints;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use aaa_data::{Footing, GameData};
use aaa_map::{Ground, Map, Recruits, Shape};
use aaa_render::MapObject;
use aaa_render::camera::{self, RtsCamera};
use aaa_render::models::{Footprint, ModelAssets, Models, pose};
use aaa_render::overlay::{
    self, BUILDING_HEALTH_BAR, HEALTH_BAR, Mark, OverlayImages, SQUARE_INSET,
};
use aaa_render::shot::{self, ShotPlugin};
use aaa_render::terrain::{map_point, world};
use aaa_render::{Edge, MapAssets, MapRenderPlugin, Playing};
use aaa_sim::{Relation, State, TICK, Target, World};
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::window::PrimaryWindow;
use orders::Orders;
use tgck::map::{Detail, Thing};

const USAGE: &str = "\
usage:
  aaa-game <game dir> [options]          set up a game from the menus
  aaa-game <game dir> <map> [options]    play a map, e.g. \"allies10_berlin\"

options:
  --side <n>                play the map's nth player (\"Side n\" in its lobby)
                            instead of the first one a person may play
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
  --ui-scale <scale>        draw the interface that many times as big, 0.5 to 2,
                            though smaller if the window's too narrow for it
  --fog <setting>           none, explored or full: how much of the map the fog
                            of war hides, rather than the game's default
  --seed <number>           what the match's chances are drawn from, to play one
                            out the same way again, rather than a new one
  --log <filter>            what to log, as for RUST_LOG, e.g. \"debug\" or
                            \"aaa_game=debug\"; RUST_LOG still applies on top";

#[derive(States, Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Screen {
    MainMenu,
    Options,
    Lobby,
    Loading,
    Match,
}

/// The game's data, which a match shares as it's loaded and played.
#[derive(Resource)]
struct Data(Arc<GameData>);

/// The map to play, which [`load`] sets up off the main thread.
#[derive(Resource)]
struct Pending {
    setup: aaa_server::Setup,
    /// What the loading screen calls it.
    title: String,
    /// What the loading screen says of it.
    description: String,
    /// The player the person plays, or none to watch, when launched from
    /// the lobby.
    person: Option<u32>,
    /// Whether it was launched from the lobby, which failing to load it goes
    /// back to and which waits for the person to continue once it's loaded,
    /// rather than given on the command line, which it exits or starts.
    from_menu: bool,
}

/// The loading screen, which stays over the match while it's set up.
#[derive(Component)]
pub(crate) struct LoadingScreen;

/// What the loading screen says it's doing.
#[derive(Component)]
struct LoadingStatus;

/// How far loading a match has got, which the loading screen says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    Data,
    Objects,
    Resources,
    Finished,
}

/// What the loading screen says for each [`Stage`], in the game's words.
#[derive(Resource)]
struct StageWords([String; 4]);

impl StageWords {
    fn of(strings: &aaa_data::Strings) -> Self {
        let said = ui::said(strings);
        StageWords([
            said("app_status_loading_data", "Loading Data"),
            said("app_status_loading_objects", "Loading Objects"),
            said("app_status_loading_resources", "Loading Resources"),
            said("app_status_finished", "Finished Loading"),
        ])
    }

    fn of_stage(&self, stage: Stage) -> &str {
        &self.0[stage as usize]
    }
}

/// A match set up behind the loading screen; whether it waits there for the
/// person to continue.
#[derive(Resource)]
struct Readying {
    wait: bool,
}

/// Where the loading screen puts its Continue button.
#[derive(Component)]
struct ContinueSlot;

#[derive(Resource)]
struct Loaded {
    game: Arc<GameData>,
    map: Map,
}

/// A match being set up off the main thread, which would otherwise stop the
/// window responding. A thread of its own rather than a task pool's, which
/// would hold up quitting mid-load until it finished.
#[derive(Resource)]
struct Preparing {
    thread: std::thread::JoinHandle<Result<Prepared, String>>,
    /// Where the thread has got to.
    stage: Arc<Mutex<Stage>>,
    /// What the loading screen last said.
    shown: Stage,
    from_menu: bool,
}

/// What [`prepare`] sets a match up with.
struct Prepared {
    loaded: Loaded,
    orders: Orders,
    sim: World,
    raising: Recruits,
    person: Option<u32>,
    leaders: Leaders,
    effects: effects::Effects,
}

#[derive(Resource)]
struct Sim(World);

/// Says which of the orders carried out were refused.
fn log_refused(refused: Vec<(u32, aaa_sim::Refused)>) {
    for (player, e) in refused {
        info!("player {player}'s order refused: {e:?}");
    }
}

/// Carries out orders as soon as they're given, ticking or not, so that
/// what shows them sees them taken up.
fn carry_out(mut sim: ResMut<Sim>, mut orders: ResMut<Orders>) {
    log_refused(orders.carry_out(&mut sim.0));
}

/// Runs the game's clock as the match has been told to play: held while
/// it's paused, and at its speed. The loading screen holds the clock
/// itself until the match is started.
fn keep_time(
    orders: Res<Orders>,
    mut time: ResMut<Time<Virtual>>,
    loading: Query<(), With<LoadingScreen>>,
) {
    if !loading.is_empty() {
        return;
    }
    match (orders.paused(), time.is_paused()) {
        (true, false) => time.pause(),
        (false, true) => time.unpause(),
        _ => {}
    }
    if time.relative_speed() != orders.speed() {
        time.set_relative_speed(orders.speed());
    }
}

/// The models objects are drawn with, for drawing more mid-match.
#[derive(Resource)]
struct Drawn(Models);

/// Which of the map's sides to play, counting from 1, as asked on the command
/// line.
#[derive(Resource, Clone, Copy)]
struct Side(Option<usize>);

/// What matches' chances are drawn from, as asked on the command line.
#[derive(Resource, Clone, Copy)]
struct Seed(Option<u64>);

impl Seed {
    /// The one asked for, else one no match has had.
    fn get(self) -> u64 {
        self.0.unwrap_or_else(aaa_server::fresh_seed)
    }
}

/// The general who leads each side, by player id; none for sides without a
/// nation.
#[derive(Resource, Default)]
struct Leaders(HashMap<u32, aaa_data::General>);

impl Leaders {
    /// What a side's things call their owner: the person's own side is the
    /// player, the computer's its general, and one without a general by
    /// the name the map gives it.
    fn owner(
        &self,
        player: &aaa_map::Player,
        local: Option<u32>,
        map: &aaa_data::Strings,
    ) -> String {
        if local == Some(player.id) {
            return "Player".into();
        }
        if let Some(general) = self.0.get(&player.id) {
            return general.name.clone();
        }
        map.get(&player.name)
            .cloned()
            .unwrap_or_else(|| player.name.clone())
    }
}

/// The player orders come from; none where the map has nobody to play.
#[derive(Resource)]
struct Local(Option<u32>);

/// The regiments orders go to, in the order they were picked.
#[derive(Resource, Default)]
struct Selected(Vec<usize>);

/// The player's building that's selected, whose orders the buttons give
/// while no regiments are.
#[derive(Resource, Default)]
struct SelectedBuilding(Option<usize>);

/// What raising regiments and commissioning things takes, read from the
/// game's data as the match starts.
#[derive(Resource)]
struct Raising(Recruits);

/// How many of the sim's units and buildings are drawn, so those it adds
/// mid-match get drawn too, and how much of the scenery it has cleared is
/// gone from view.
#[derive(Resource, Default)]
struct Viewed {
    units: usize,
    buildings: usize,
    cleared: usize,
    /// The map's objects by id, for clearing scenery without searching.
    objects: HashMap<u32, Entity>,
}

/// What the cursor is over.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    #[default]
    Nothing,
    Regiment(usize),
    Building(usize),
}

/// The flat square marks are drawn on and their materials, one per
/// [`TINTS`].
#[derive(Resource)]
struct Marks {
    square: Handle<Mesh>,
    rings: [Handle<StandardMaterial>; 3],
    squares: [Handle<StandardMaterial>; 3],
}

/// The box drawn while dragging to select regiments.
#[derive(Component)]
struct SelectionBox;

/// The square under a focused building, draped over the ground in a mesh of
/// its own: which building and how big.
#[derive(Component)]
struct DrapedMark(usize, Vec2);

/// One of the health bars drawn over the screen.
#[derive(Component)]
struct HealthBar;

/// Draws one of the simulation's units.
#[derive(Component)]
struct UnitView {
    /// Index into the sim's units.
    unit: usize,
    /// The last of its shots shown.
    shot: Option<(u64, usize)>,
    dead: bool,
    moving: bool,
    /// The outline its model tilts on to follow the ground, if it does.
    conform: Option<Shape>,
    footing: Footing,
}

/// Draws the trailer one of the sim's units tows.
#[derive(Component)]
struct TrailerView {
    /// Index into the sim's units.
    unit: usize,
    conform: Option<Shape>,
    footing: Footing,
}

/// Spawns the model of the sim's `unit`, placed as `thing`, moving at up to
/// `speed`, and of any `trailer` it tows; false if the unit has none.
#[allow(clippy::too_many_arguments)]
fn spawn_unit_view(
    commands: &mut Commands,
    models: &mut Models,
    assets: &mut ModelAssets,
    ground: &Ground,
    thing: &Thing,
    speed: f32,
    trailer: Option<&aaa_sim::Trailer>,
    owner: u32,
    unit: usize,
) -> bool {
    if let Some(t) = trailer {
        let footing = models.footing(t.template.as_str());
        let towed = thing_at(
            ground,
            footing,
            thing.id,
            t.template.as_str(),
            t.position,
            t.facing,
        );
        if let Some(e) = models.spawn_object(commands, assets, &towed, Some(owner)) {
            let conform = models.conform(t.template.as_str());
            commands.entity(e).insert(TrailerView {
                unit,
                conform,
                footing,
            });
        }
    }
    let Some(e) = models.spawn_unit(commands, assets, thing, owner, speed) else {
        return false;
    };
    commands.entity(e).insert(UnitView {
        unit,
        shot: None,
        dead: false,
        moving: false,
        conform: models.conform(&thing.template),
        footing: models.footing(&thing.template),
    });
    true
}

/// Places trailers between where the last two ticks left them, hiding
/// them along with what towed them.
fn show_trailers(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    fog: Res<fog::Fog>,
    fixed: Res<Time<Fixed>>,
    mut views: Query<(&TrailerView, &mut Transform, &mut Visibility)>,
) {
    let t = fixed.overstep_fraction();
    for (view, mut transform, mut visibility) in &mut views {
        let unit = &sim.0.units[view.unit];
        let Some(trailer) = unit.trailer.as_ref().filter(|_| unit.alive()) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(match fog.shows_unit(view.unit) {
            true => Visibility::Inherited,
            false => Visibility::Hidden,
        });
        (transform.translation, transform.rotation) = pose(
            &loaded.map.ground,
            trailer.drawn_at(t),
            trailer.facing,
            view.conform,
            view.footing,
        );
    }
}

/// Draws one of the simulation's buildings.
#[derive(Component)]
struct BuildingView {
    /// Index into the sim's buildings.
    building: usize,
    /// Where the map placed it.
    placed: Vec3,
    /// The last of its shots shown.
    shot: Option<(u64, usize)>,
    destroyed: bool,
    /// The model it plays while it packs or unpacks, shown in its place,
    /// and whether it's unpacking.
    packing: Option<(Entity, bool)>,
}

/// Seconds of a match to play before its capture.
#[derive(Resource)]
struct ShotAfter(f64);

/// When the match started.
#[derive(Resource)]
struct MatchStarted(f64);

fn note_start(mut commands: Commands, time: Res<Time>) {
    commands.insert_resource(MatchStarted(time.elapsed_secs_f64()));
}

fn shot_due(after: Res<ShotAfter>, started: Option<Res<MatchStarted>>, time: Res<Time>) -> bool {
    started.is_some_and(|s| time.elapsed_secs_f64() - s.0 >= after.0)
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

/// What the command line asks for besides the game and map.
struct Options {
    side: Option<usize>,
    scripts: Vec<String>,
    console: Option<u16>,
    screenshot: Option<String>,
    shot_after: f64,
    headless: bool,
    ui_scale: f32,
    fog: Option<aaa_map::FogSetting>,
    seed: Option<u64>,
    log: Option<String>,
}

/// Takes the options out of `args`; `None` if any is malformed.
fn options(args: &mut Vec<String>) -> Option<Options> {
    let side = match take_option(args, "--side")? {
        Some(n) => Some(n.parse().ok().filter(|&n| n >= 1)?),
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
    let headless = match args.iter().position(|a| a == "--headless") {
        Some(at) => {
            args.remove(at);
            true
        }
        None => false,
    };
    let ui_scale = match take_option(args, "--ui-scale")? {
        Some(n) => n.parse().ok().filter(|n| UI_SCALES.contains(n))?,
        None => 1.0,
    };
    let fog = match take_option(args, "--fog")? {
        Some(f) => Some(aaa_map::FogSetting::named(&f)?),
        None => None,
    };
    let seed = match take_option(args, "--seed")? {
        Some(n) => Some(n.parse().ok()?),
        None => None,
    };
    let log = take_option(args, "--log")?;
    Some(Options {
        side,
        scripts,
        console,
        screenshot,
        shot_after,
        headless,
        ui_scale,
        fog,
        seed,
        log,
    })
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let Some(Options {
        side,
        scripts,
        console,
        screenshot,
        shot_after,
        headless,
        ui_scale,
        fog,
        seed,
        log,
    }) = options(&mut args)
    else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let log = match aaa_render::log::plugin(log.as_deref()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("aaa-game: {e}");
            return ExitCode::from(2);
        }
    };
    let remote = match console.map(remote::Remote::listen).transpose() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("aaa-game: {e}");
            return ExitCode::FAILURE;
        }
    };
    let once = screenshot.is_some();
    let windowless = once || headless;
    let screenshot = windowless.then(|| ShotPlugin {
        path: screenshot.map(Into::into),
    });
    let (dir, map) = match args.as_slice() {
        [dir] => (dir, None),
        [dir, map] => (dir, Some(map)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let opened = GameData::open(&PathBuf::from(dir)).and_then(|game| {
        let Some(map) = map else {
            return Ok((game, None));
        };
        let name = game
            .find_map(map)
            .ok_or_else(|| format!("no map named {map:?}; run without one to pick from a menu"))?;
        Ok((game, Some(name)))
    });
    let (game, map) = match opened {
        Ok(o) => o,
        Err(e) => {
            eprintln!("aaa-game: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut app = App::new();
    let mut window = shot::window("aaa-game".into(), screenshot.is_some());
    // Its size, and whether it fills the screen, are chosen in the options.
    if let Some(window) = &mut window.primary_window {
        window.resizable = false;
        window.enabled_buttons.maximize = false;
    }
    let plugins = DefaultPlugins.set(window).set(log);
    match windowless {
        // Muted, it has no use for the machine's audio device, only for
        // somewhere its systems can keep the audio they'd read.
        true => app
            .add_plugins(plugins.disable::<bevy::audio::AudioPlugin>())
            .init_asset::<AudioSource>(),
        false => app.add_plugins(plugins),
    };
    if let Some(s) = screenshot {
        app.add_plugins(s);
        let frame = std::time::Duration::from_secs_f64(1.0 / 60.0);
        match once {
            // Every frame takes the same time, so a shot some way into a
            // match catches the same moment however fast the machine draws.
            true => _ = app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(frame)),
            // Nothing else holds a run without a window to the speed one is
            // drawn at, and so the match to the speed it's played at.
            false => {
                _ = app.add_systems(
                    Last,
                    move |mut drawn: bevy::ecs::system::Local<Option<Instant>>| {
                        let left = drawn.and_then(|at| frame.checked_sub(at.elapsed()));
                        std::thread::sleep(left.unwrap_or_default());
                        *drawn = Some(Instant::now());
                    },
                )
            }
        }
        app.configure_sets(
            Update,
            shot::Capture
                .run_if(console::idle)
                .run_if(backdrop::settled),
        );
        // Of the match, not the loading screen before it.
        if once && map.is_some() {
            app.insert_resource(ShotAfter(shot_after))
                .add_systems(OnEnter(Screen::Match), note_start)
                .configure_sets(
                    Update,
                    shot::Capture
                        .run_if(in_state(Screen::Match))
                        .run_if(shot_due),
                );
        }
    }
    let mut console = console::Console::default();
    for s in &scripts {
        console.exec_on_start(s);
    }
    let render = MapRenderPlugin {
        edge: Edge::Fade,
        pickable: false,
    };
    app.add_plugins(render)
        .add_plugins(console::ConsolePlugin)
        .add_plugins(sound::SoundPlugin)
        .add_plugins(special_ops::SpecialOpsPlugin)
        .add_plugins(music::MusicPlugin)
        .add_plugins(effects::EffectsPlugin)
        .insert_resource(console)
        .insert_resource(UiScaleSetting(ui_scale))
        .add_systems(PreUpdate, fit_ui)
        // The menu's backdrop has a camera too, which stays put.
        .configure_sets(
            Update,
            camera::Controls
                .run_if(in_state(Screen::Match))
                .run_if(takes_input),
        )
        .insert_resource(Time::<Fixed>::from_seconds(TICK.into()))
        .add_message::<waypoints::Sent>()
        .add_message::<messages::Refused>()
        .add_message::<messages::Failed>()
        .init_resource::<Selected>()
        .init_resource::<SelectedBuilding>()
        .init_resource::<Focus>()
        .init_resource::<ghost::Ghost>()
        .init_resource::<ghost::Landing>()
        .init_resource::<deliveries::Flights>()
        .init_resource::<supply::Border>()
        .init_resource::<supply::Outline>()
        .init_resource::<actions::Page>()
        .insert_resource(Side(side))
        .insert_resource(Seed(seed))
        .add_plugins((
            ui::UiPlugin,
            confirm::ConfirmPlugin,
            menu::MenuPlugin,
            backdrop::BackdropPlugin,
            options::OptionsPlugin,
            lobby::LobbyPlugin,
            pause::PausePlugin,
            displays::DisplaysPlugin,
            technologies::TechnologyPlugin,
            objectives::ObjectivesPlugin,
            outcome::OutcomePlugin,
        ))
        .add_systems(OnEnter(Screen::MainMenu), console::start_scripts)
        .add_systems(OnEnter(Screen::Loading), loading)
        .add_systems(Update, load.run_if(in_state(Screen::Loading)))
        .add_systems(
            OnEnter(Screen::Match),
            (
                setup,
                deliveries::reset,
                fog::setup,
                setup_overlays,
                keys::load,
                banners::setup_banners,
                minimap::spawn,
                actions::spawn,
                object_display::spawn,
                resource_bar::spawn,
                messages::spawn,
                supply::spawn,
                waypoints::load,
                console::start_scripts,
                ready,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                keys::load.run_if(resource_changed::<options::Options>),
                supply::mark_border.run_if(resource_exists_and_changed::<keys::Bindings>),
            )
                .chain()
                .run_if(in_state(Screen::Match)),
        )
        .add_plugins(HoldFirstTick)
        .add_systems(
            FixedUpdate,
            step.in_set(Ticks).run_if(in_state(Screen::Match)),
        )
        .add_systems(
            Update,
            (
                (
                    fog::look,
                    (fog::shade_ground, fog::hide_scenery),
                    spawn_views,
                    (show_units, show_trailers, show_buildings, show_packing)
                        .before(aaa_render::Animate),
                    deliveries::show,
                    fog::hide_buildings,
                )
                    .chain(),
                keep_selection,
                clear_scenery,
                selection_sounds,
                show_focus,
                (
                    supply::show_border,
                    supply::tint_buildings,
                    supply::show_cycle_button,
                    supply::show_border_button,
                ),
                show_health.after(camera::Controls),
                (
                    banners::add_banners,
                    banners::badge_banners,
                    banners::add_hq_banners,
                    (banners::show_banners, banners::show_hq_banners),
                )
                    .chain()
                    .after(camera::Controls),
            )
                .run_if(in_state(Screen::Match)),
        )
        .add_systems(
            Update,
            (
                minimap::steer
                    .before(camera::Controls)
                    .before(actions::aim)
                    .run_if(takes_input),
                minimap::update.after(camera::Controls).after(fog::look),
                minimap::tint,
                (actions::back_out, actions::show).chain(),
                (
                    object_display::lay_out,
                    (object_display::refresh, object_display::portray),
                )
                    .chain(),
                resource_bar::show.after(actions::show),
                (messages::tell, messages::refuse, messages::show).chain(),
                messages::list.after(messages::tell).after(displays::Keys),
                (ghost::show, ghost::land, ghost::redden).chain(),
                waypoints::cursor,
                (waypoints::mark, waypoints::clear).chain().after(carry_out),
                waypoints::rally,
            )
                .run_if(in_state(Screen::Match)),
        )
        .add_systems(
            Update,
            (
                (
                    focus,
                    (
                        select,
                        command
                            .run_if(minimap::pointer_away)
                            .run_if(actions::pointer_away),
                    )
                        .run_if(not(actions::aiming)),
                    actions::aim.run_if(actions::pointer_away),
                )
                    .chain(),
                choose_formation,
                actions::hotkeys,
                supply::cycle_detached,
                messages::cycle,
                supply::toggle_border,
                fog::cheat,
            )
                .run_if(in_state(Screen::Match))
                .run_if(takes_input)
                .before(carry_out),
        )
        .add_systems(
            Update,
            (carry_out, keep_time)
                .chain()
                .after(console::run)
                .run_if(in_state(Screen::Match)),
        )
        // So that a pause asked for by a key holds the match from this
        // frame, not the next.
        .configure_sets(Update, (pause::Toggle, displays::Keys).before(carry_out));
    if let Some(remote) = remote {
        app.insert_resource(remote);
    }
    let strings = game.strings();
    // Nobody is at a run without a window to hear it.
    let volumes = sound::Volumes::from(game.volumes()).muted(windowless);
    app.insert_resource(lobby::Nations(game.nations()))
        .insert_resource(lobby::Handicaps::new(game.handicaps()))
        .insert_resource(lobby::Leaving::of(&strings))
        .insert_resource(confirm::Words::of(&strings))
        .insert_resource(outcome::Words::of(&strings))
        .insert_resource(StageWords::of(&strings))
        .insert_resource(options::Options::new(&game))
        .insert_resource(sound::Sounds::new(&game, &ui::CLICKS))
        .insert_resource(music::Music::new(&game))
        .insert_resource(volumes)
        .insert_resource(Data(Arc::new(game)));
    match map {
        Some(name) => {
            app.insert_resource(Pending {
                title: aaa_data::map_title(&name).to_string(),
                description: String::new(),
                person: None,
                from_menu: false,
                setup: aaa_server::Setup {
                    map: name,
                    seed: Seed(seed).get(),
                    fog,
                    ..Default::default()
                },
            })
            .insert_state(Screen::Loading);
        }
        None => {
            app.insert_state(Screen::MainMenu);
        }
    }
    match app.run() {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

/// Whether none of the console, the pause menu and the loading screen is
/// up, so the match takes the keyboard and mouse.
fn takes_input(
    console: Res<console::Console>,
    menu: Res<pause::PauseMenu>,
    displays: Res<displays::Displays>,
    loading: Query<(), With<LoadingScreen>>,
) -> bool {
    console::closed(console)
        && pause::closed(menu)
        && displays::closed(Some(displays))
        && loading.is_empty()
}

/// Loads a map and sets up the sim and recruits for a match on it.
fn prepare(
    game: Arc<GameData>,
    pending: Pending,
    side: Side,
    stage: &Mutex<Stage>,
) -> Result<Prepared, String> {
    let reached = |s| *stage.lock().unwrap_or_else(|e| e.into_inner()) = s;
    let Pending {
        setup,
        person,
        from_menu,
        ..
    } = pending;
    info!("seed {}", setup.seed);
    let aaa_server::Started {
        map,
        raising,
        world: sim,
        generals,
    } = aaa_server::start(&game, &setup, || reached(Stage::Objects))?;
    let leaders = Leaders(generals);
    let person = match from_menu {
        true => person,
        false => lobby::default_person(&map.players, side),
    };
    if !from_menu && side.0.is_some() && person.is_none() {
        warn!("{}: no such side to play", map.title);
    }
    let effects = effects::Effects::new(&game, &map);
    // Built here, off the main thread, and kept for the interface to read
    // as the match starts.
    game.strings();
    game.listings();
    game.technologies();
    game.property_looks();
    Ok(Prepared {
        loaded: Loaded { game, map },
        // The host's copy, made here to keep it off the window's thread.
        orders: Orders::hosting(setup, sim.clone()),
        sim,
        raising,
        person,
        leaders,
        effects,
    })
}

/// Puts a [`prepare`]d match in place.
fn begin(world: &mut bevy::ecs::world::World, prepared: Prepared) {
    let Prepared {
        loaded,
        orders,
        sim,
        raising,
        person,
        leaders,
        effects,
    } = prepared;
    let lineup = lobby::Lineup::new(&loaded.map.players, person);
    for &(id, control) in &lineup.0 {
        if let Some(p) = sim.sides.player(id) {
            let team = p
                .team
                .map_or("no team".into(), |t| format!("team {}", t + 1));
            let nation = p.nation.as_deref().unwrap_or("no nation");
            let general = leaders.0.get(&id).map_or("no general", |g| g.id.as_str());
            info!(
                "{} ({}, {team}, {nation}, {general}): {control:?}",
                p.name, p.color
            );
        }
    }
    world.insert_resource(Local(lineup.person()));
    world.insert_resource(leaders);
    world.insert_resource(lineup);
    world.insert_resource(orders);
    world.insert_resource(Sim(sim));
    world.insert_resource(Raising(raising));
    world.insert_resource(effects);
    world.insert_resource(loaded);
}

/// Shows the map being loaded, as the game does: its name over what it's
/// about, and how loading goes.
fn loading(mut commands: Commands, pending: Res<Pending>, words: Res<StageWords>) {
    use ui::{Edge, Font, Ink, Surface, label, text};
    commands.spawn((Camera2d, DespawnOnExit(Screen::Loading)));
    let root = commands
        .spawn((
            LoadingScreen,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(40)),
                row_gap: px(24),
                ..default()
            },
            Surface::Solid,
            GlobalZIndex(10),
        ))
        .id();
    commands.spawn((ChildOf(root), text(pending.title.clone(), Font::Title)));
    if !pending.description.is_empty() {
        commands.spawn((
            ChildOf(root),
            Node {
                width: percent(50),
                min_height: percent(40),
                padding: UiRect::all(px(16)),
                border: UiRect::all(px(2)),
                ..default()
            },
            Edge::Accent,
            children![text(pending.description.clone(), Font::Body)],
        ));
    }
    commands.spawn((
        ChildOf(root),
        Node {
            flex_grow: 1.0,
            ..default()
        },
    ));
    commands.spawn((
        ChildOf(root),
        Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        },
        children![
            (
                LoadingStatus,
                label(words.of_stage(Stage::Data), Font::Body, Ink::Help)
            ),
            (
                ContinueSlot,
                Node {
                    width: px(240),
                    justify_content: JustifyContent::End,
                    ..default()
                },
            ),
        ],
    ));
}

/// Starts [`prepare`]ing the pending map, then once it's ready starts the
/// match on it behind the loading screen, or goes back to the lobby or exits
/// if it can't be loaded.
fn load(world: &mut bevy::ecs::world::World) {
    if let Some(pending) = world.remove_resource::<Pending>() {
        let Some(game) = world.get_resource::<Data>().map(|d| d.0.clone()) else {
            return;
        };
        let side = *world.resource::<Side>();
        let from_menu = pending.from_menu;
        let stage = Arc::new(Mutex::new(Stage::Data));
        let reached = stage.clone();
        let thread = std::thread::spawn(move || prepare(game, pending, side, &reached));
        world.insert_resource(Preparing {
            thread,
            stage,
            shown: Stage::Data,
            from_menu,
        });
        return;
    }
    let Some(mut preparing) = world.get_resource_mut::<Preparing>() else {
        return;
    };
    if !preparing.thread.is_finished() {
        let stage = *preparing.stage.lock().unwrap_or_else(|e| e.into_inner());
        if stage != preparing.shown {
            preparing.shown = stage;
            show_stage(world, stage);
        }
        return;
    }
    let Some(Preparing {
        thread, from_menu, ..
    }) = world.remove_resource::<Preparing>()
    else {
        return;
    };
    let prepared = thread
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    match prepared {
        Ok(prepared) => {
            // Shown this frame, before the match is set up on the next.
            show_stage(world, Stage::Resources);
            begin(world, prepared);
            world.insert_resource(Readying { wait: from_menu });
            world.resource_mut::<NextState<Screen>>().set(Screen::Match);
        }
        Err(e) if from_menu => {
            error!("{e}");
            let mut screens = world.query_filtered::<Entity, With<LoadingScreen>>();
            for screen in screens.iter(world).collect::<Vec<_>>() {
                world.despawn(screen);
            }
            world.resource_mut::<NextState<Screen>>().set(Screen::Lobby);
        }
        Err(e) => {
            error!("{e}");
            world.write_message(AppExit::error());
        }
    }
}

/// Says on the loading screen how far loading has got.
fn show_stage(world: &mut bevy::ecs::world::World, stage: Stage) {
    let said = world.resource::<StageWords>().of_stage(stage).to_string();
    let mut status = world.query_filtered::<&mut Text, With<LoadingStatus>>();
    for mut text in status.iter_mut(world) {
        text.0.clone_from(&said);
    }
}

/// Once the match is set up, says so and holds it until the person
/// continues, or starts it straight away.
fn ready(
    mut commands: Commands,
    readying: Option<Res<Readying>>,
    mut time: ResMut<Time<Virtual>>,
    mut status: Query<&mut Text, With<LoadingStatus>>,
    words: Res<StageWords>,
    slot: Query<Entity, With<ContinueSlot>>,
    screen: Query<Entity, With<LoadingScreen>>,
) {
    let Some(readying) = readying else {
        return;
    };
    commands.remove_resource::<Readying>();
    if !readying.wait {
        screen.iter().for_each(|e| commands.entity(e).despawn());
        return;
    }
    time.pause();
    for mut text in &mut status {
        text.0 = words.of_stage(Stage::Finished).into();
    }
    for slot in &slot {
        commands
            .spawn((
                ChildOf(slot),
                ui::button("Continue", ui::Font::Large),
                ui::called("loading.continue"),
            ))
            .observe(start);
    }
}

/// Takes the loading screen away and starts the match.
fn start(
    _: On<bevy::ui_widgets::Activate>,
    mut commands: Commands,
    mut time: ResMut<Time<Virtual>>,
    screen: Query<Entity, With<LoadingScreen>>,
) {
    screen.iter().for_each(|e| commands.entity(e).despawn());
    time.unpause();
}

/// Loads what [`show_focus`] and [`show_health`] draw with, and the box
/// [`select`] draws.
fn setup_overlays(
    mut commands: Commands,
    loaded: Res<Loaded>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let images = OverlayImages::load(&loaded.game, &mut images, formats.0);
    let mut tinted = |image: &Option<Handle<Image>>| {
        TINTS.map(|c| materials.add(overlay::mark_material(image.clone(), c)))
    };
    commands.insert_resource(Marks {
        square: meshes.add(overlay::flat_square()),
        rings: tinted(&images.ring),
        squares: tinted(&images.square),
    });
    commands.insert_resource(images);
    commands.spawn((
        SelectionBox,
        Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(px(1)),
            ..default()
        },
        BorderColor::all(OWN),
        BackgroundColor(OWN.with_alpha(0.1)),
        Visibility::Hidden,
        Pickable::IGNORE,
        DespawnOnExit(Screen::Match),
    ));
}

fn setup(
    mut commands: Commands,
    loaded: Res<Loaded>,
    sim: Res<Sim>,
    local: Res<Local>,
    mut assets: MapAssets,
    mut sounds: ResMut<sound::Sounds>,
    mut window: Option<Single<&mut Window, With<PrimaryWindow>>>,
) {
    let Loaded { game, map } = &*loaded;
    if let Some(window) = &mut window {
        window.title = format!("{} - aaa-game", map.title);
    }
    // The sim's units and buildings are drawn below, trailers with what
    // tows them; what else a Division HQ holds would be left standing when
    // it packs.
    let leave_out: HashSet<u32> = (map.roles())
        .filter(|(_, role)| *role != aaa_map::Role::Other)
        .map(|(t, _)| t.id)
        .collect();
    let mut models = aaa_render::spawn_map(&mut commands, &mut assets, game, map, false, |t| {
        leave_out.contains(&t.id)
    });
    // Units are drawn as the map stores them, with its variations and
    // scales; members a regiment has lost aren't stored, so stand on the
    // ground.
    let stored: HashMap<u32, &Thing> = map.standing().map(|t| (t.id, t)).collect();
    let members = map.regiments.iter().flat_map(|r| &r.members);
    let mut missing = 0;
    for (i, m) in members.enumerate() {
        debug_assert_eq!(sim.0.units[i].id, m.id);
        let owner = sim.0.regiments[sim.0.units[i].regiment].owner;
        let thing = match stored.get(&m.id) {
            Some(&t) => Cow::Borrowed(t),
            None => {
                let footing = models.footing(m.template.as_str());
                let at = thing_at(
                    &map.ground,
                    footing,
                    m.id,
                    m.template.as_str(),
                    m.position,
                    m.facing,
                );
                Cow::Owned(at)
            }
        };
        let mut model_assets = assets.models();
        if !spawn_unit_view(
            &mut commands,
            &mut models,
            &mut model_assets,
            &map.ground,
            &thing,
            m.speed,
            sim.0.units[i].trailer.as_ref(),
            owner,
            i,
        ) {
            missing += 1;
        }
    }
    info!(
        "{} regiments of {} units, {missing} without a model",
        sim.0.regiments.len(),
        sim.0.units.len()
    );
    let mut missing = 0;
    for (i, b) in map.buildings.iter().enumerate() {
        debug_assert_eq!(sim.0.buildings[i].id, b.id);
        let thing = stored[&b.id];
        match models.spawn_object(
            &mut commands,
            &mut assets.models(),
            thing,
            aaa_map::owner(thing),
        ) {
            Some(e) => {
                let [x, y, z] = thing.position;
                commands.entity(e).insert(BuildingView {
                    building: i,
                    placed: world(x, y, z),
                    shot: None,
                    destroyed: false,
                    packing: None,
                });
            }
            None => missing += 1,
        }
    }
    info!(
        "{} buildings, {missing} without a model",
        sim.0.buildings.len()
    );
    // Looking at the person's side: a regiment of theirs, else a building,
    // else where they start.
    let own = |owner: u32| local.0 == Some(owner);
    let regiment = (0..sim.0.regiments.len())
        .find(|&r| local.0.is_some_and(|p| sim.0.commands(p, r)))
        .map(|r| sim.0.centre(r));
    let building = || {
        let mut buildings = sim.0.buildings.iter();
        buildings.find(|b| own(b.owner)).map(|b| b.position)
    };
    let start = || {
        let mut things = map.things.iter();
        let start =
            things.find(|t| t.template == "start_loc" && aaa_map::owner(t).is_some_and(own));
        start.map(|t| [t.position[0], t.position[1]])
    };
    let cam = match regiment.or_else(building).or_else(start) {
        Some([x, y]) => RtsCamera::new(world(x, y, map.ground.height_at([x, y]))),
        None => RtsCamera::centred(&map.ground),
    };
    // The letters are the game's hotkeys.
    let cam = RtsCamera {
        letter_keys: false,
        ..cam
    };
    camera::spawn(&mut commands, cam);
    commands.insert_resource(Viewed {
        units: sim.0.units.len(),
        buildings: sim.0.buildings.len(),
        cleared: sim.0.cleared.len(),
        objects: HashMap::new(),
    });
    commands.insert_resource(Drawn(models));
    sounds.load_objects(game);
}

/// Plays the callout of a regiment of the player's newly added to the
/// selection: others' are selected in silence.
fn selection_sounds(
    selected: Res<Selected>,
    sim: Res<Sim>,
    local: Res<Local>,
    time: Res<Time>,
    mut sounds: ResMut<sound::Sounds>,
    mut before: bevy::prelude::Local<Vec<usize>>,
) {
    if !selected.is_changed() {
        return;
    }
    let fresh = selected.0.iter().find(|r| !before.contains(r));
    if let Some(&r) = fresh.filter(|&&r| Some(sim.0.regiments[r].owner) == local.0) {
        let regiment = &sim.0.regiments[r];
        let at = sim.0.centre(r);
        sounds.reply(
            &time,
            regiment.template.as_str(),
            |a| &a.selected,
            at,
            regiment.owner,
        );
    }
    before.clone_from(&selected.0);
}

/// Where a shot's target stands.
fn target_position(sim: &World, target: Target) -> [f32; 2] {
    match target {
        Target::Unit(u) => sim.units[u].position,
        Target::Building(b) => sim.buildings[b].position,
    }
}

/// The match has been shown once, so the sim may step. Until then what's
/// shown is how the match begins, however long its first frame took.
#[derive(Resource)]
struct Shown;

/// Holds the sim's ticks off until the match has been shown once.
struct HoldFirstTick;

impl Plugin for HoldFirstTick {
    fn build(&self, app: &mut App) {
        app.configure_sets(FixedUpdate, Ticks.run_if(resource_exists::<Shown>))
            .add_systems(
                Update,
                show_first
                    .run_if(in_state(Screen::Match))
                    .run_if(not(resource_exists::<Shown>)),
            )
            .add_systems(OnExit(Screen::Match), |mut commands: Commands| {
                commands.remove_resource::<Shown>()
            });
    }
}

/// Lets the sim step from the next frame, its first tick a tick's time
/// from now rather than after whatever time loading left over.
fn show_first(mut commands: Commands, mut fixed: ResMut<Time<Fixed>>) {
    let over = fixed.overstep();
    fixed.discard_overstep(over);
    commands.insert_resource(Shown);
}

/// What advances the sim a tick.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct Ticks;

fn step(
    mut sim: ResMut<Sim>,
    mut orders: ResMut<Orders>,
    mut selected: ResMut<Selected>,
    mut chosen: ResMut<SelectedBuilding>,
) {
    step_world(&mut sim.0, &mut orders, &mut selected.0, &mut chosen.0);
}

/// Plays a tick of the game after carrying out the orders waiting,
/// dropping destroyed regiments from the selection. A packed building
/// selected on its own that unpacks hands the selection to the building,
/// and a building that packs to what it packed into.
fn step_world(
    sim: &mut World,
    orders: &mut Orders,
    selected: &mut Vec<usize>,
    chosen: &mut Option<usize>,
) {
    log_refused(orders.step(sim));
    if let &[r] = &selected[..]
        && let Some(b) = sim.regiments[r].unpacked_into
    {
        *chosen = Some(b);
    }
    selected.retain(|&r| !sim.destroyed(r));
    if let Some(b) = *chosen
        && let State::Packed(_, r) = sim.buildings[b].state
    {
        *chosen = None;
        *selected = vec![r];
    }
}

/// A placed object standing on the ground, for drawing what the sim adds.
fn thing_at(
    ground: &Ground,
    footing: Footing,
    id: u32,
    template: &str,
    [x, y]: [f32; 2],
    [fx, fy]: [f32; 2],
) -> Thing {
    Thing {
        template: template.to_string(),
        class: tgck::map::ACTOR,
        id,
        name: String::new(),
        position: [x, y, ground.stand_at(footing, [x, y])],
        facing: [fx, fy, 0.0],
        flags: 0,
        scale: 1.0,
        variation: -1,
        seen_by: 0,
        detail: Detail::None,
    }
}

/// Draws the units and buildings the sim has added since last time, such
/// as those commissioned or unpacked; a building still unpacking plays
/// its unpacking model in its place.
fn spawn_views(
    mut commands: Commands,
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    mut drawn: ResMut<Drawn>,
    mut viewed: ResMut<Viewed>,
    mut assets: MapAssets,
) {
    let map = &loaded.map;
    for u in viewed.units..sim.0.units.len() {
        let unit = &sim.0.units[u];
        let regiment = &sim.0.regiments[unit.regiment];
        let thing = thing_at(
            &map.ground,
            drawn.0.footing(unit.template.as_str()),
            unit.id,
            unit.template.as_str(),
            unit.position,
            unit.facing,
        );
        let mut models = assets.models();
        spawn_unit_view(
            &mut commands,
            &mut drawn.0,
            &mut models,
            &map.ground,
            &thing,
            regiment.speed,
            unit.trailer.as_ref(),
            regiment.owner,
            u,
        );
    }
    viewed.units = sim.0.units.len();
    for (b, building) in sim.0.buildings.iter().enumerate().skip(viewed.buildings) {
        let thing = thing_at(
            &map.ground,
            drawn.0.footing(building.template.as_str()),
            building.id,
            building.template.as_str(),
            building.position,
            building.facing,
        );
        let mut models = assets.models();
        let Some(e) =
            drawn
                .0
                .spawn_object(&mut commands, &mut models, &thing, Some(building.owner))
        else {
            continue;
        };
        let [x, y, z] = thing.position;
        commands.entity(e).insert((
            BuildingView {
                building: b,
                placed: world(x, y, z),
                shot: None,
                destroyed: false,
                packing: None,
            },
            DespawnOnExit(Screen::Match),
        ));
    }
    viewed.buildings = sim.0.buildings.len();
}

/// Takes away the scenery the sim has cleared from under new buildings.
fn clear_scenery(
    mut commands: Commands,
    sim: Res<Sim>,
    mut viewed: ResMut<Viewed>,
    added: Query<(Entity, &MapObject), Added<MapObject>>,
) {
    let viewed = &mut *viewed;
    for (e, object) in &added {
        viewed.objects.insert(object.0.id, e);
    }
    for id in &sim.0.cleared[viewed.cleared..] {
        if let Some(e) = viewed.objects.remove(id) {
            commands.entity(e).despawn();
        }
    }
    viewed.cleared = sim.0.cleared.len();
}

/// Plays a building's packing or unpacking model in its place while it
/// packs or unpacks, and hides it once it's packed up.
fn show_packing(
    mut commands: Commands,
    sim: Res<Sim>,
    mut drawn: ResMut<Drawn>,
    mut assets: MapAssets,
    mut views: Query<(&mut BuildingView, &MapObject, &mut Visibility)>,
    fog: Res<fog::Fog>,
) {
    for (mut view, object, mut visibility) in &mut views {
        if fog.building(view.building) != fog::Look::Shown {
            continue;
        }
        let building = &sim.0.buildings[view.building];
        let wanted = match (building.unpacking, building.packing) {
            (Some((start, _)), _) => Some((start, true)),
            (None, Some((start, _))) => Some((start, false)),
            _ => None,
        };
        if view.packing.map(|(_, u)| u) != wanted.map(|(_, u)| u)
            && let Some((e, _)) = view.packing.take()
        {
            commands.entity(e).despawn();
        }
        let gone = matches!(building.state, State::Packed(..) | State::Sold(_));
        if let (Some((start, unpacking)), None) = (wanted, view.packing) {
            let into = (sim.0.ticks - start) as f32 * TICK;
            let mut models = assets.models();
            let spawned = drawn.0.spawn_packing(
                &mut commands,
                &mut models,
                &object.0,
                building.owner,
                unpacking,
                into,
            );
            if let Some(e) = spawned {
                commands.entity(e).insert(DespawnOnExit(Screen::Match));
                view.packing = Some((e, unpacking));
            }
        }
        let shown = match view.packing.is_some() || gone {
            true => Visibility::Hidden,
            false => Visibility::Inherited,
        };
        if !matches!(building.state, State::Destroyed(_)) {
            visibility.set_if_neq(shown);
        }
    }
}

/// Lets go of what the player can no longer see of others' things, and of
/// a building that's fallen or packed up. Whose it is doesn't matter: a
/// building captured while selected stays selected.
fn keep_selection(
    sim: Res<Sim>,
    fog: Res<fog::Fog>,
    mut selected: ResMut<Selected>,
    mut chosen: ResMut<SelectedBuilding>,
) {
    let sim = &sim.0;
    // The game keeps a building that drops into the fog as its remembered
    // picture, but the display would show it as it is now, so it goes.
    if let Some(b) = chosen.0
        && (!sim.buildings[b].standing() || fog.building(b) != fog::Look::Shown)
    {
        chosen.0 = None;
    }
    if selected.0.iter().any(|&r| !fog.shows_regiment(sim, r)) {
        selected.0.retain(|&r| fog.shows_regiment(sim, r));
    }
}

/// Places units between where the last two ticks left them, playing their
/// moving animation while they move, their attack as they fire and their
/// death as they die; the dead without one are hidden.
fn show_units(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    fog: Res<fog::Fog>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    mut sounds: ResMut<sound::Sounds>,
    mut views: Query<(
        &mut UnitView,
        &mut Transform,
        &mut Visibility,
        Option<&mut Playing>,
    )>,
) {
    let t = fixed.overstep_fraction();
    for (mut view, mut transform, mut visibility, mut playing) in &mut views {
        let unit = &sim.0.units[view.unit];
        let owner = sim.0.regiments[unit.regiment].owner;
        if unit.left.is_some() || unit.awaited {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        // Neither seen nor heard in the fog.
        let seen = fog.shows_unit(view.unit);
        // A truck that packed back up from its building is seen again, as
        // is a member that's arrived or been replaced.
        if unit.alive() {
            visibility.set_if_neq(match seen {
                true => Visibility::Inherited,
                false => Visibility::Hidden,
            });
            if view.dead {
                view.dead = false;
                if let Some(p) = playing.as_mut() {
                    p.revive();
                }
            }
        }
        if !unit.alive() && !view.dead {
            view.dead = true;
            if seen {
                sounds.cue(
                    &time,
                    unit.template.as_str(),
                    |a| &a.destroyed,
                    unit.position,
                    owner,
                );
            }
            let dies = playing.as_mut().is_some_and(|p| p.die(unit.id as usize));
            if !dies || !seen {
                *visibility = Visibility::Hidden;
            }
        }
        if unit.shot != view.shot {
            view.shot = unit.shot;
            let weapon = unit
                .shot
                .and_then(|(_, w)| unit.fighter.as_ref()?.weapons.get(w));
            if let (Some(w), Some(target), true) = (weapon, unit.target, seen) {
                let to = target_position(&sim.0, target);
                sounds.shot(&time, &w.sounds, unit.position, to, owner);
            }
            if let (Some(p), Some(ids)) = (
                playing.as_mut(),
                weapon.and_then(|w| w.animation.as_deref()),
            ) {
                p.use_ability(ids);
            }
        }
        (transform.translation, transform.rotation) = pose(
            &loaded.map.ground,
            unit.drawn_at(t),
            unit.facing,
            view.conform,
            view.footing,
        );
        if unit.alive() && unit.moving() != view.moving {
            view.moving = unit.moving();
            let event: fn(&aaa_data::ObjectAudio) -> &Vec<_> = match view.moving {
                true => |a| &a.start_moving,
                false => |a| &a.stop_moving,
            };
            if seen {
                sounds.cue(&time, unit.template.as_str(), event, unit.position, owner);
            }
        }
        if let Some(mut playing) = playing.filter(|_| unit.alive()) {
            let speed = sim.0.regiments[unit.regiment].speed;
            playing.set_moving(unit.moving().then_some(speed));
        }
    }
}

/// Plays buildings' attacks as they fire, and sinks the destroyed into the
/// ground.
fn show_buildings(
    sim: Res<Sim>,
    fog: Res<fog::Fog>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    mut sounds: ResMut<sound::Sounds>,
    mut views: Query<(
        &mut BuildingView,
        &mut Transform,
        &mut Visibility,
        Option<&mut Playing>,
    )>,
) {
    const SINK_TIME: f32 = 3.0;
    const SINK_DEPTH: f32 = 12.0;
    for (mut view, mut transform, mut visibility, playing) in &mut views {
        // Left as last seen until seen again.
        if fog.building(view.building) != fog::Look::Shown {
            continue;
        }
        let building = &sim.0.buildings[view.building];
        if let (State::Destroyed(fell), false) = (building.state, view.destroyed) {
            view.destroyed = true;
            // Not for one that fell out of sight, first seen fallen now.
            let since = (sim.0.ticks - fell) as f32 * TICK;
            if since < SINK_TIME {
                let (at, owner) = (building.position, building.owner);
                sounds.cue(
                    &time,
                    building.template.as_str(),
                    |a| &a.destroyed,
                    at,
                    owner,
                );
            }
        }
        if let State::Destroyed(fell) = building.state {
            let since = (sim.0.ticks - fell) as f32 + fixed.overstep_fraction();
            let sunk = (since * TICK / SINK_TIME).min(1.0);
            transform.translation = view.placed - Vec3::Y * SINK_DEPTH * sunk;
            if sunk >= 1.0 {
                *visibility = Visibility::Hidden;
            }
            continue;
        }
        if building.shot != view.shot {
            view.shot = building.shot;
            let weapon = building
                .shot
                .and_then(|(_, w)| building.fighter.weapons.get(w));
            if let (Some(w), Some(target)) = (weapon, building.target) {
                let to = target_position(&sim.0, target);
                sounds.shot(&time, &w.sounds, building.position, to, building.owner);
            }
            if let (Some(mut p), Some(ids)) = (playing, weapon.and_then(|w| w.animation.as_deref()))
            {
                p.use_ability(ids);
            }
        }
    }
}

/// What the cursor points at on the map, on the ground or through the
/// minimap, and not through the action grid.
#[derive(SystemParam)]
struct Pointer<'w, 's> {
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    cameras: Query<
        'w,
        's,
        (
            &'static Camera,
            &'static GlobalTransform,
            &'static RtsCamera,
        ),
    >,
    grid: Query<
        'w,
        's,
        (
            &'static RelativeCursorPosition,
            &'static InheritedVisibility,
        ),
        With<actions::Blocks>,
    >,
    minimap: Res<'w, minimap::Minimap>,
    views: Query<'w, 's, &'static RelativeCursorPosition, With<minimap::MinimapView>>,
}

/// A point on the map the cursor's at, in world units.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Pointing {
    Ground([f32; 2]),
    Minimap([f32; 2]),
}

impl Pointing {
    fn point(self) -> [f32; 2] {
        match self {
            Pointing::Ground(p) | Pointing::Minimap(p) => p,
        }
    }
}

impl Pointer<'_, '_> {
    fn at(&self, ground: &Ground) -> Option<Pointing> {
        if actions::over_grid(&self.grid) {
            return None;
        }
        match self.minimap.spot(&self.views) {
            minimap::Spot::Off => {
                pointed(&self.windows, &self.cameras, ground).map(Pointing::Ground)
            }
            minimap::Spot::OffMap => None,
            minimap::Spot::At(p) => Some(Pointing::Minimap(p)),
        }
    }
}

/// The regiment, of those `taking` takes, with the living unit nearest
/// `point`, if one's within `reach`.
fn nearest_regiment(
    sim: &World,
    point: [f32; 2],
    reach: f32,
    taking: impl Fn(usize) -> bool,
) -> Option<usize> {
    nearest(
        sim.units
            .iter()
            .filter(|u| u.alive() && taking(u.regiment))
            .map(|u| (u.regiment, u.position)),
        point,
        reach,
    )
}

fn nearest(
    units: impl Iterator<Item = (usize, [f32; 2])>,
    point: [f32; 2],
    reach: f32,
) -> Option<usize> {
    let near = |p: [f32; 2]| (p[0] - point[0]).hypot(p[1] - point[1]);
    units
        .map(|(r, p)| (r, near(p)))
        .filter(|(_, d)| *d < reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r)
}

/// The point on the ground under the cursor, in world units.
fn pointed(
    windows: &Query<&Window, With<PrimaryWindow>>,
    cameras: &Query<(&Camera, &GlobalTransform, &RtsCamera)>,
    ground: &Ground,
) -> Option<[f32; 2]> {
    let (Ok(window), Ok((camera, transform, _))) = (windows.single(), cameras.single()) else {
        return None;
    };
    let ray = camera
        .viewport_to_world(transform, window.cursor_position()?)
        .ok()?;
    pick_ground(ground, ray)
}

/// Finds what the cursor is over: the regiment or HQ whose banner it's on,
/// else the regiment of the nearest unit within reach of the ground it
/// points at, else the building standing there; nothing while it's on the
/// minimap or the interface's panels, which cover the map.
#[allow(clippy::too_many_arguments)]
fn focus(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &RtsCamera)>,
    banners: Query<banners::Pointed<banners::Banner>>,
    hq_banners: Query<banners::Pointed<banners::HqBanner>>,
    minimap: Query<&RelativeCursorPosition, With<minimap::MinimapView>>,
    grid: Query<(&RelativeCursorPosition, &InheritedVisibility), With<actions::Blocks>>,
    loaded: Res<Loaded>,
    sim: Res<Sim>,
    scenery: Res<banners::Scenery>,
    fog: Res<fog::Fog>,
    mut focus: ResMut<Focus>,
) {
    const REACH: f32 = 2.0;
    if !minimap::pointer_away(minimap) || actions::over_grid(&grid) {
        focus.set_if_neq(Focus::Nothing);
        return;
    }
    // An HQ's banner may outlast it by a frame.
    let flying = |found: &Focus| match *found {
        Focus::Building(b) => !sim.0.buildings[b].gone() && sim.0.hq_of(b).is_some(),
        _ => true,
    };
    if let Some(found) = banners::under_cursor(&banners, &hq_banners).filter(flying) {
        focus.set_if_neq(found);
        return;
    }
    let Some(point) = pointed(&windows, &cameras, &loaded.map.ground) else {
        focus.set_if_neq(Focus::Nothing);
        return;
    };
    let unit = nearest(
        (sim.0.units.iter().enumerate())
            .filter(|&(u, unit)| unit.alive() && fog.shows_unit(u))
            .filter(|(_, unit)| !scenery.has(&sim.0, unit.regiment))
            .map(|(_, unit)| (unit.regiment, unit.position)),
        point,
        REACH,
    );
    let building = || {
        sim.0.buildings.iter().enumerate().position(|(i, b)| {
            !b.gone()
                && fog.building(i) == fog::Look::Shown
                && b.obstacle
                    .as_ref()
                    .is_some_and(|o| o.distance(point) == 0.0)
        })
    };
    let found = match unit {
        Some(r) => Focus::Regiment(r),
        None => building().map_or(Focus::Nothing, Focus::Building),
    };
    focus.set_if_neq(found);
}

/// Left click selects what's under the cursor, whoever's it is, and
/// dragging what the box covers, by [`selection`]'s rules; with Shift or
/// Ctrl held, a click adds to the selection or takes back out what's in it,
/// and a box adds to it.
#[allow(clippy::too_many_arguments)]
fn select(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &RtsCamera)>,
    minimap: Query<&RelativeCursorPosition, With<minimap::MinimapView>>,
    grid: Query<(&RelativeCursorPosition, &InheritedVisibility), With<actions::Blocks>>,
    (loaded, drawn): (Res<Loaded>, Res<Drawn>),
    local: Res<Local>,
    focus: Res<Focus>,
    (sim, scenery, fog): (Res<Sim>, Res<banners::Scenery>, Res<fog::Fog>),
    mut selected: ResMut<Selected>,
    mut chosen: ResMut<SelectedBuilding>,
    mut start: bevy::prelude::Local<Option<Vec2>>,
    ui: Res<UiScale>,
    mut shown: Single<(&mut Node, &mut Visibility), With<SelectionBox>>,
) {
    // How far the cursor goes before a click becomes a drag, in pixels.
    const DRAG: f32 = 4.0;
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());
    if buttons.just_pressed(MouseButton::Left)
        && minimap::pointer_away(minimap)
        && actions::pointer_away(grid)
    {
        *start = cursor;
    }
    let released = buttons.just_released(MouseButton::Left);
    // Presses can end unseen, such as while the console is open.
    if !released && !buttons.pressed(MouseButton::Left) {
        *start = None;
    }
    let dragged = start
        .zip(cursor)
        .map(|(a, b)| Rect::from_corners(a, b))
        .filter(|r| r.size().max_element() > DRAG);
    let (node, visibility) = &mut *shown;
    match dragged.filter(|_| !released) {
        Some(r) => {
            place_on_map(node, r.min, r.size(), &ui);
            visibility.set_if_neq(Visibility::Inherited);
        }
        None => {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
    if !released || start.take().is_none() {
        return;
    }
    let sim = &sim.0;
    let rank = |t| selection::rank(sim, local.0, t);
    let adding = keys.any_pressed([
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    let mut selection = selection::Selection {
        regiments: &mut selected.0,
        building: &mut chosen.0,
    };
    let Some(rect) = dragged else {
        let thing = match *focus {
            Focus::Regiment(r) if !sim.destroyed(r) && !scenery.has(sim, r) => {
                Some(selection::Thing::Regiment(r))
            }
            Focus::Building(b) if sim.buildings[b].standing() => {
                Some(selection::picked(sim, selection::Thing::Building(b)))
            }
            _ => None,
        };
        match (thing, adding) {
            (Some(thing), true) => selection.toggle(rank, thing),
            (Some(thing), false) => selection.replace(thing),
            (None, true) => {}
            (None, false) => selection.clear(),
        }
        return;
    };
    let Ok((camera, transform, _)) = cameras.single() else {
        return;
    };
    let ground = &loaded.map.ground;
    let drawn_at = |at: [f32; 2], z: f32| {
        camera
            .world_to_viewport(transform, world(at[0], at[1], z))
            .ok()
    };
    let unit_at = |u: usize| {
        let unit = &sim.units[u];
        let z = ground.stand_at(drawn.0.footing(unit.template.as_str()), unit.position);
        drawn_at(unit.position, z)
    };
    let mut inside: Vec<(selection::Thing, Vec2)> = (0..sim.regiments.len())
        .filter(|&r| !scenery.has(sim, r))
        .filter_map(|r| {
            let at = (sim.survivors(r))
                .filter(|&u| fog.shows_unit(u))
                .filter_map(unit_at)
                .find(|&p| rect.contains(p))?;
            Some((selection::Thing::Regiment(r), at))
        })
        .collect();
    for (b, building) in sim.buildings.iter().enumerate() {
        let seen = building.standing() && fog.building(b) == fog::Look::Shown;
        let at = drawn_at(building.position, ground.height_at(building.position));
        let Some(at) = at.filter(|&p| seen && rect.contains(p)) else {
            continue;
        };
        // A city once, however many of its buildings the box covers.
        let thing = selection::picked(sim, selection::Thing::Building(b));
        if !inside.iter().any(|&(t, _)| t == thing) {
            inside.push((thing, at));
        }
    }
    if !adding {
        selection.clear();
    }
    // Nearest the middle of what's selected, else of the box.
    let held: Vec<Vec2> = (selection.regiments.iter())
        .filter_map(|&r| sim.survivors(r).find_map(unit_at))
        .collect();
    let middle = match held.len() {
        0 => rect.center(),
        n => held.iter().sum::<Vec2>() / n as f32,
    };
    selection.boxed(rank, inside, middle);
}

/// Right click sends the selected regiments where it points, each keeping
/// its place relative to the rest, or after the enemy regiment it's on,
/// those that can't fight it going there instead. With a building that
/// commissions selected, it sets its rally point there.
#[allow(clippy::too_many_arguments)]
fn command(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &RtsCamera)>,
    loaded: Res<Loaded>,
    local: Res<Local>,
    focus: Res<Focus>,
    looks: Res<waypoints::Looks>,
    sim: Res<Sim>,
    mut orders: ResMut<Orders>,
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    time: Res<Time>,
    mut sounds: ResMut<sound::Sounds>,
    mut sent: MessageWriter<waypoints::Sent>,
) {
    let Some(player) = local.0 else {
        return;
    };
    // Others' things take no orders.
    if !buttons.just_pressed(MouseButton::Right)
        || !selection::commanded(&sim.0, local.0, &selected.0, chosen.0)
    {
        return;
    }
    let ground = &loaded.map.ground;
    let Some(point) = pointed(&windows, &cameras, ground) else {
        return;
    };
    if selected.0.is_empty() {
        // Buildings that don't commission have no rally point to set.
        if let Some(b) = chosen.0 {
            let rally = aaa_sim::Command::Rally {
                building: b,
                at: Some(point),
            };
            let _ = orders.give(&sim.0, player, rally);
        }
        return;
    }
    acknowledge(&sim.0, &mut sounds, &time, player, &selected.0);
    let mut moving = selected.0.clone();
    if let Some(e) = enemy_at(&sim.0, player, *focus) {
        let (pursuing, rest) = pursue(&sim.0, &mut orders, player, &selected.0, e);
        sent.write(waypoints::Sent {
            to: None,
            aim: actions::Aim::Attack,
            regiments: pursuing,
        });
        moving = rest;
    }
    let regiments = send(
        &sim.0,
        &mut orders,
        player,
        &moving,
        point,
        (actions::Aim::Move, looks.together(actions::Aim::Move)),
        ground.size(),
    );
    sent.write(waypoints::Sent {
        to: Some(point),
        aim: actions::Aim::Move,
        regiments,
    });
}

/// The enemy regiment of `player`'s that `focus` is on, if it's one.
fn enemy_at(sim: &World, player: u32, focus: Focus) -> Option<usize> {
    let Focus::Regiment(e) = focus else {
        return None;
    };
    let owner = sim.regiments[e].owner;
    (sim.sides.relation(player, owner) == aaa_sim::Relation::Enemy).then_some(e)
}

/// Sends regiments after the enemy regiment `e`. Returns those that went,
/// each with where the enemy is, and those that can't.
fn pursue(
    sim: &World,
    orders: &mut Orders,
    player: u32,
    regiments: &[usize],
    e: usize,
) -> (Vec<(usize, [f32; 2])>, Vec<usize>) {
    let there = sim.centre(e);
    let (went, rest): (Vec<usize>, Vec<usize>) = regiments.iter().partition(|&&r| {
        let command = aaa_sim::Command::Pursue {
            regiment: r,
            enemy: e,
        };
        orders.give(sim, player, command).is_ok()
    });
    (went.into_iter().map(|r| (r, there)).collect(), rest)
}

/// Plays the first of the regiments answering an order.
fn acknowledge(
    sim: &World,
    sounds: &mut sound::Sounds,
    time: &Time,
    player: u32,
    regiments: &[usize],
) {
    if let Some(&first) = regiments.first() {
        let template = &sim.regiments[first].template;
        sounds.reply(
            time,
            template.as_str(),
            |a| &a.commanded,
            sim.centre(first),
            player,
        );
    }
}

/// Gives regiments an order at a point, each keeping its place relative to
/// the rest within a map `size` across, unless they all go `together` to
/// the point itself. Returns those that went, each with where to.
fn send(
    sim: &World,
    orders: &mut Orders,
    player: u32,
    regiments: &[usize],
    [px, py]: [f32; 2],
    (aim, together): (actions::Aim, bool),
    size: f32,
) -> Vec<(usize, [f32; 2])> {
    let centres: Vec<[f32; 2]> = regiments.iter().map(|&r| sim.centre(r)).collect();
    let n = centres.len() as f32;
    let [cx, cy] = centres
        .iter()
        .fold([0.0, 0.0], |[x, y], [a, b]| [x + a / n, y + b / n]);
    let mut sent = Vec::new();
    for (&r, [x, y]) in regiments.iter().zip(centres) {
        let target = match together {
            true => [px, py],
            false => [
                (px + x - cx).clamp(0.0, size),
                (py + y - cy).clamp(0.0, size),
            ],
        };
        let (regiment, to) = (r, target);
        let command = match aim {
            actions::Aim::Move => aaa_sim::Command::Move { regiment, to },
            actions::Aim::Attack => aaa_sim::Command::Attack { regiment, to },
            actions::Aim::Run => aaa_sim::Command::Run { regiment, to },
            actions::Aim::Guard => aaa_sim::Command::Guard { regiment, to },
            actions::Aim::Unpack => aaa_sim::Command::Unpack {
                regiment,
                at: to,
                facing: actions::UNPACK_FACING,
            },
        };
        if orders.give(sim, player, command).is_ok() {
            sent.push((r, target));
        }
    }
    sent
}

fn choose_formation(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Res<Selected>,
    local: Res<Local>,
    sim: Res<Sim>,
    mut orders: ResMut<Orders>,
) {
    const KEYS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    let (Some(f), Some(player)) = (KEYS.iter().position(|&k| keys.just_pressed(k)), local.0) else {
        return;
    };
    for &r in &selected.0 {
        let regiment = &sim.0.regiments[r];
        let Some(formation) = regiment.formations.get(f) else {
            continue;
        };
        let command = aaa_sim::Command::Formation {
            regiment: r,
            formation: f,
        };
        if orders.give(&sim.0, player, command).is_ok() {
            debug!("{} to formation {}", regiment.template, formation.id);
        }
    }
}

/// Where a ray from the camera first meets the ground, in world units.
fn pick_ground(ground: &Ground, ray: Ray3d) -> Option<[f32; 2]> {
    let above = |p: Vec3| {
        let [x, y] = map_point(p);
        p.y - ground.height_at([x, y])
    };
    let step = 0.5;
    let mut t = 0.0;
    let mut was = ray.origin;
    let limit = ground.size() * 4.0;
    while t < limit {
        t += step;
        let p = ray.get_point(t);
        if above(p) <= 0.0 {
            // Halve the step down to where it crossed.
            let (mut a, mut b) = (was, p);
            for _ in 0..16 {
                let mid = (a + b) / 2.0;
                if above(mid) > 0.0 { a = mid } else { b = mid }
            }
            let size = ground.size();
            let [x, y] = map_point(b);
            return ((0.0..=size).contains(&x) && (0.0..=size).contains(&y)).then_some([x, y]);
        }
        was = p;
    }
    None
}

/// What [`UiScaleSetting`] may be: smaller for a small window, bigger to
/// suit a large one.
const UI_SCALES: std::ops::RangeInclusive<f32> = 0.5..=2.0;
/// Between the screen's edges and the minimap, object display and action
/// grid along its bottom, in logical pixels.
const MARGIN: f32 = 12.0;

/// How big the player wants the interface drawn; [`UiScale`] is this, less
/// as a narrow window needs to fit the bottom of the screen in.
#[derive(Resource)]
struct UiScaleSetting(f32);

/// How much of the window's height the bottom of the screen may take.
const UI_HEIGHT_SHARE: f32 = 1.0 / 3.0;

/// Sets [`UiScale`] from [`UiScaleSetting`], shrunk until the object
/// display, centred, clears the minimap and action grid either side of it, and
/// they take no more than [`UI_HEIGHT_SHARE`] of the window's height.
fn fit_ui(
    setting: Res<UiScaleSetting>,
    windows: Query<Ref<Window>, With<PrimaryWindow>>,
    mut scale: ResMut<UiScale>,
) {
    let window = windows.single().ok();
    if !setting.is_changed() && !window.as_ref().is_some_and(|w| w.is_changed()) {
        return;
    }
    // Without a window, a screenshot's image.
    let size = window.map_or(shot::SIZE.as_vec2(), |w| w.size());
    if size.min_element() <= 0.0 {
        return;
    }
    let side = MARGIN + minimap::DISC.max(actions::SIZE.x) + MARGIN;
    let wide = size.x / (2.0 * side + object_display::SIZE.x);
    let tallest = minimap::DISC
        .max(actions::SIZE.y)
        .max(object_display::SIZE.y);
    let tall = size.y * UI_HEIGHT_SHARE / (MARGIN + tallest);
    let wanted = setting.0.min(wide).min(tall).max(*UI_SCALES.start());
    if scale.0 != wanted {
        scale.0 = wanted;
    }
}

/// Places a node over the map at a rectangle in viewport pixels, which
/// [`UiScale`] mustn't scale.
fn place_on_map(node: &mut Node, corner: Vec2, size: Vec2, ui: &UiScale) {
    node.left = px(corner.x / ui.0);
    node.top = px(corner.y / ui.0);
    node.width = px(size.x / ui.0);
    node.height = px(size.y / ui.0);
}

const OWN: Color = Color::srgb(0.1, 0.9, 0.1);
const FRIEND: Color = Color::srgb(0.2, 0.55, 1.0);
const OTHER: Color = Color::srgb(0.95, 0.15, 0.1);
/// What marks things by how their owner stands toward the player, by
/// [`tint`].
const TINTS: [Color; 3] = [OWN, FRIEND, OTHER];

/// Which of [`Marks`]' tints suits what `owner` owns.
fn tint(sim: &World, local: Option<u32>, owner: u32) -> usize {
    match local.map(|p| sim.sides.relation(p, owner)) {
        Some(Relation::Itself) => 0,
        Some(Relation::Ally) => 1,
        _ => 2,
    }
}

type ShownMark = (
    &'static mut Transform,
    &'static mut MeshMaterial3d<StandardMaterial>,
    &'static mut Visibility,
);

/// Rings the units of the regiment under the cursor and squares the
/// building under it, each mark sized to the model's footprint.
#[allow(clippy::too_many_arguments)]
fn show_focus(
    mut commands: Commands,
    focus: Res<Focus>,
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    local: Res<Local>,
    marks: Res<Marks>,
    fixed: Res<Time<Fixed>>,
    units: Query<(&UnitView, Option<&Footprint>)>,
    buildings: Query<(&BuildingView, &MapObject, Option<&Footprint>)>,
    mut shown: Query<ShownMark, (With<Mark>, Without<DrapedMark>)>,
    mut draped: Query<(Entity, &DrapedMark, &mut MeshMaterial3d<StandardMaterial>)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    // The ring fills its image but for a thin margin.
    const RING_FILL: f32 = 238.0 / 256.0;
    // Infantry are about this wide.
    const SMALLEST: Vec2 = Vec2::splat(0.4);
    let ground = &loaded.map.ground;
    let t = fixed.overstep_fraction();
    let mut wanted = Vec::new();
    let mut square = None;
    match *focus {
        Focus::Nothing => {}
        Focus::Regiment(r) => {
            let material = &marks.rings[tint(&sim.0, local.0, sim.0.regiments[r].owner)];
            for (view, footprint) in &units {
                let unit = &sim.0.units[view.unit];
                if unit.regiment != r || !unit.alive() {
                    continue;
                }
                let at = unit.drawn_at(t);
                // An oval through the footprint's corners.
                let size =
                    footprint.map_or(SMALLEST, |f| f.0) * std::f32::consts::SQRT_2 / RING_FILL;
                let transform = overlay::mark_transform(ground, at, unit.facing, size);
                wanted.push((transform, material.clone()));
            }
        }
        Focus::Building(b) => {
            let building = &sim.0.buildings[b];
            let material = &marks.squares[tint(&sim.0, local.0, building.owner)];
            if let Some((_, object, footprint)) = buildings.iter().find(|(v, ..)| v.building == b) {
                let [fx, fy, _] = object.0.facing;
                let size = claimed_size(building, footprint) / (1.0 - 2.0 * SQUARE_INSET);
                square = Some((b, size, [fx, fy], material.clone()));
            }
        }
    }
    let mut kept = false;
    for (entity, mark, mut material) in &mut draped {
        match &square {
            Some((b, size, _, m)) if !kept && mark.0 == *b && mark.1 == *size => {
                if material.0 != *m {
                    material.0 = m.clone();
                }
                kept = true;
            }
            _ => commands.entity(entity).despawn(),
        }
    }
    if !kept && let Some((b, size, facing, material)) = square {
        let mesh = overlay::draped_square(ground, sim.0.buildings[b].position, facing, size);
        commands.spawn((
            Mark,
            DrapedMark(b, size),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            DespawnOnExit(Screen::Match),
        ));
    }
    let mut wanted = wanted.into_iter();
    for (mut transform, mut material, mut visibility) in &mut shown {
        match wanted.next() {
            Some((t, m)) => {
                *transform = t;
                if material.0 != m {
                    material.0 = m;
                }
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
    for (transform, material) in wanted {
        commands.spawn((
            Mark,
            Mesh3d(marks.square.clone()),
            MeshMaterial3d(material),
            transform,
            DespawnOnExit(Screen::Match),
        ));
    }
}

/// The ground a building's template claims, which takes in its pad and
/// yard; the model's own box is about its walls.
fn claimed_size(building: &aaa_sim::Building, footprint: Option<&Footprint>) -> Vec2 {
    let claimed = match building.obstacle.as_ref().map(|o| o.shape) {
        Some(Shape::Rect { width, height }) => Some(Vec2::new(width, height)),
        Some(Shape::Circle { radius }) => Some(Vec2::splat(2.0 * radius)),
        _ => None,
    };
    claimed
        .or(footprint.map(|f| f.0))
        .unwrap_or(Vec2::splat(4.0))
}

/// Draws a health bar below each unit of the selected regiments and the one
/// under the cursor, and in front of the selected building and the one
/// under the cursor, whoever's they are: the width a bar has there but
/// square to the screen and over everything.
#[allow(clippy::too_many_arguments)]
fn show_health(
    mut commands: Commands,
    (selected, chosen, focus): (Res<Selected>, Res<SelectedBuilding>, Res<Focus>),
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    images: Res<OverlayImages>,
    fixed: Res<Time<Fixed>>,
    ui: Res<UiScale>,
    cameras: Query<(&Camera, &Transform), With<RtsCamera>>,
    units: Query<(&UnitView, Option<&Footprint>)>,
    buildings: Query<&BuildingView>,
    mut bars: Query<(&mut Node, &mut ImageNode, &mut Visibility), With<HealthBar>>,
) {
    let mut wanted = Vec::new();
    if let Ok((camera, transform)) = cameras.single() {
        // This frame's camera, which moves after transforms propagate.
        let global = GlobalTransform::from(*transform);
        let toward = (transform.back().as_vec3() * Vec3::new(1.0, 0.0, 1.0)).normalize_or_zero();
        let ground = &loaded.map.ground;
        let t = fixed.overstep_fraction();
        // Where on the ground each bar's middle goes, its size there, and
        // the frame it shows.
        let mut shown = Vec::new();
        let regiment_shown = |r: usize| selected.0.contains(&r) || *focus == Focus::Regiment(r);
        let building_shown = |b: usize| chosen.0 == Some(b) || *focus == Focus::Building(b);
        for (view, footprint) in &units {
            let unit = &sim.0.units[view.unit];
            if !regiment_shown(unit.regiment) || !unit.alive() {
                continue;
            }
            let Some(fighter) = &unit.fighter else {
                continue;
            };
            let [x, y] = unit.drawn_at(t);
            // At the edge of its footprint nearest the camera, so the bar
            // clears the model.
            let reach = footprint.map_or(0.0, |f| f.0.length() / 2.0);
            let at = world(x, y, ground.stand_at(view.footing, [x, y])) + toward * reach;
            let image = images.health_bar(unit.health / fighter.health);
            shown.push((at, HEALTH_BAR, image, 0.0));
        }
        for view in &buildings {
            let b = view.building;
            let building = &sim.0.buildings[b];
            // A disabled one shows its empty bar.
            if !building_shown(b) || building.gone() {
                continue;
            }
            // On the ground at its middle, which from above reads as in
            // front of it.
            let [x, y] = building.position;
            let at = world(x, y, view.placed.y);
            let image = images.building_health_bar(building.health / building.fighter.health);
            shown.push((at, BUILDING_HEALTH_BAR, image, 0.0));
            // A relay's supply stockpile, below its health.
            if let Some(supply) = sim
                .0
                .supply_of(b)
                .filter(|s| s.relay && s.stockpile_max > 0.0)
            {
                let share = building.supply.stockpile / supply.stockpile_max;
                let image = images.stockpile_bar(share);
                shown.push((at, BUILDING_HEALTH_BAR, image, 1.0));
            }
        }
        let half = transform.right().as_vec3() / 2.0;
        for (at, size, image, below) in shown {
            let Some(image) = image else {
                continue;
            };
            let (Ok(a), Ok(b)) = (
                camera.world_to_viewport(&global, at - half * size.x),
                camera.world_to_viewport(&global, at + half * size.x),
            ) else {
                continue;
            };
            let width = a.distance(b);
            let height = width * size.y / size.x;
            let centre = (a + b) / 2.0 + Vec2::new(0.0, height * (1.0 + 1.5 * below));
            wanted.push((
                centre - Vec2::new(width, height) / 2.0,
                Vec2::new(width, height),
                image.clone(),
            ));
        }
    }
    let place = |node: &mut Node, corner: Vec2, size: Vec2| place_on_map(node, corner, size, &ui);
    let mut wanted = wanted.into_iter();
    for (mut node, mut image, mut visibility) in &mut bars {
        match wanted.next() {
            Some((corner, size, frame)) => {
                place(&mut node, corner, size);
                if image.image != frame {
                    image.image = frame;
                }
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
    for (corner, size, frame) in wanted {
        let mut node = Node {
            position_type: PositionType::Absolute,
            ..default()
        };
        place(&mut node, corner, size);
        commands.spawn((
            HealthBar,
            node,
            // Over the banners.
            GlobalZIndex(1),
            ImageNode::new(frame).with_mode(NodeImageMode::Stretch),
            Pickable::IGNORE,
            DespawnOnExit(Screen::Match),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_follows_the_pause_and_speed_the_match_is_given() {
        let pathing = aaa_map::Pathing::uniform(4, aaa_map::Domain::Land);
        let sim = World::from_regiments(Default::default(), pathing, &[]);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Sim(sim))
            .init_resource::<Orders>()
            .add_systems(Update, (carry_out, keep_time).chain());
        let clock = |app: &App| {
            let time = app.world().resource::<Time<Virtual>>();
            (time.is_paused(), time.relative_speed())
        };
        app.update();
        assert_eq!(clock(&app), (false, 1.0));
        let mut orders = app.world_mut().resource_mut::<Orders>();
        orders.act(aaa_sim::Action::Pause(true));
        orders.act(aaa_sim::Action::Speed(2.0));
        app.update();
        assert_eq!(clock(&app), (true, 2.0));
        assert_eq!(app.world().resource::<Orders>().record().given.len(), 2);
        // While the loading screen is up the clock is its to hold.
        let loading = app.world_mut().spawn(LoadingScreen).id();
        (app.world_mut().resource_mut::<Orders>()).act(aaa_sim::Action::Pause(false));
        app.update();
        assert!(clock(&app).0);
        app.world_mut().despawn(loading);
        app.update();
        assert!(!clock(&app).0);
    }

    #[test]
    fn the_sim_waits_for_the_match_to_be_shown() {
        #[derive(Resource, Default)]
        struct Stepped(u32);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .add_plugins(HoldFirstTick)
            .insert_state(Screen::Loading)
            .init_resource::<Stepped>()
            .insert_resource(Time::<Fixed>::from_seconds(TICK.into()))
            // Every frame long enough for a tick.
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f32(TICK * 1.5),
            ))
            .add_systems(
                FixedUpdate,
                (|mut s: ResMut<Stepped>| s.0 += 1)
                    .in_set(Ticks)
                    .run_if(in_state(Screen::Match)),
            );
        app.update();
        app.update();
        app.world_mut()
            .resource_mut::<NextState<Screen>>()
            .set(Screen::Match);
        app.update();
        assert_eq!(app.world().resource::<Stepped>().0, 0, "first frame");
        app.update();
        assert!(app.world().resource::<Stepped>().0 > 0, "after it");
    }

    /// A side a person may play, on no team.
    pub(crate) fn player(id: u32) -> aaa_map::Player {
        aaa_map::Player {
            id,
            name: format!("kingdom_{id}"),
            color: "blue".into(),
            rgb: [1.0; 3],
            team: None,
            independent: false,
            playable: true,
            default_defeat: true,
            outcome: Default::default(),
            won_by_cities: false,
            nation: None,
            general: None,
            relations: Vec::new(),
            team_relations: Vec::new(),
            stockpile: 0.0,
            technologies: Vec::new(),
            handicap: None,
            barred: Vec::new(),
        }
    }

    #[test]
    fn things_name_the_persons_side_the_player_and_the_computers_its_general() {
        let general = |id: &str, name: &str| aaa_data::General {
            id: id.into(),
            name: name.into(),
            short_name: id.into(),
            operations: Vec::new(),
            experience_max: None,
            public: true,
            portrait: None,
        };
        let leaders = Leaders(HashMap::from([
            (1, general("patton", "General Patton")),
            (2, general("rommel", "General Rommel")),
        ]));
        let map: aaa_data::Strings = [("kingdom_4".into(), "Civilian (Aggressive)".into())]
            .into_iter()
            .collect();
        assert_eq!(leaders.owner(&player(1), Some(1), &map), "Player");
        assert_eq!(leaders.owner(&player(2), Some(1), &map), "General Rommel");
        // Watching, every side is the computer's.
        assert_eq!(leaders.owner(&player(1), None, &map), "General Patton");
        // A side with no general goes by the map's name for it, if any.
        assert_eq!(leaders.owner(&player(3), Some(1), &map), "kingdom_3");
        assert_eq!(
            leaders.owner(&player(4), Some(1), &map),
            "Civilian (Aggressive)"
        );
    }

    #[test]
    fn the_nearest_unit_within_reach_is_pointed_at() {
        let units = || [(0, [0.0, 0.0]), (1, [3.0, 0.0]), (2, [1.5, 0.0])].into_iter();
        assert_eq!(nearest(units(), [2.0, 0.0], 1.0), Some(2));
        assert_eq!(nearest(units(), [2.9, 0.0], 1.0), Some(1));
        assert_eq!(nearest(units(), [0.0, 5.0], 1.0), None);
    }
}
