//! Plays the matches of a game on TimeGate's engine, set up in a custom
//! game's lobby or on a map given on the command line, or hosted elsewhere
//! and joined. Everything comes from the game's data but what a [`Program`]
//! gives: each game's own program is a [`run`] with its own.
//!
//! You play one of the map's sides, or watch, and the computer plays the rest. Click one of your units or its
//! banner to select its organization, Shift+click to add an organization to the
//! selection or take it out, or drag a box over several, with Shift to add
//! them, up to six at once. Click one of your buildings to select it
//! instead; the [`object_display`] along the bottom shows what's selected. The selection
//! passes from a truck to the building it unpacks into, and back as the
//! building packs up. Right-click the ground to send the selection there, keeping its
//! organizations' places relative to each other and stopping to fight enemies on
//! the way. The number keys pick the selection's formation, in the order each organization's
//! layout lists them, as the [`formations`] buttons beside the display do, and the buttons in the corner give the selection the
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
mod campaigns;
mod chat;
mod cli;
mod clock;
mod commanding;
mod confirm;
mod connection;
mod console;
mod control_groups;
mod deliveries;
mod displays;
mod drive;
mod effects;
mod emblem;
mod fields;
mod fog;
mod formations;
mod ghost;
mod heard;
mod hud;
mod idle;
mod intent;
mod keys;
mod lan;
mod leaving;
mod loading;
mod lobby;
mod marks;
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
mod paused;
mod placing;
mod pointing;
mod politics;
mod random;
mod ticking;
mod ui_scale;
mod views;
use tgserver::presets;
mod resource_bar;
mod saved;
mod scenario;
mod selection;
mod sound;
mod special_ops;
mod staging;
mod sunlight;
mod supply;
mod technologies;
mod times;
mod ui;
mod waypoints;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use cli::{Options, usage};
use clock::keep_time;
use commanding::{choose_formation, command, keep_selection, select, selection_sounds};
use loading::{
    LoadingScreen, Pending, StageWords, join, load, open_screen, own_game, ready, stop_awaiting,
};
use marks::{TINTS, setup_overlays, show_focus, show_health};
use orders::Orders;
use pointing::focus;
use tgdata::GameData;
use tgmap::{Map, Recruits};
use tgrender::camera;
use tgrender::models::Models;
use tgrender::shot::{self, ShotPlugin};
use tgrender::{Edge, MapRenderPlugin};
use tgsim::{TICK, World};
use ticking::{HoldFirstTick, InTouch, Telling, Ticks, carry_out, step};
use ui_scale::{UiScaleSetting, fit_ui, target_map};
use views::{
    clear_scenery, setup, show_buildings, show_packing, show_trailers, show_units, spawn_views,
};

/// What a game's program says of it that its data doesn't.
#[derive(Resource, Clone)]
pub struct Program {
    /// The program's name, for its usage, errors and the folder its
    /// options are kept in.
    pub name: &'static str,
    /// What the game's data calls the game, as [`GameData::is_of`] takes
    /// it: the program plays only data of its own game.
    pub tag: &'static str,
    /// A map of the game's to name in the usage.
    pub example_map: &'static str,
    /// The program's version, shown on the main menu.
    pub version: &'static str,
}

/// How our interface looks for the game, from this project's own data
/// ([`GameData::modern_interface`]) and the [`Program`].
#[derive(Resource, Clone)]
pub(crate) struct Look {
    /// What the game is called on its window, and on the main menu
    /// without a logo: the program's name where the data doesn't say.
    pub(crate) title: String,
    /// The path in the data of the image crowning the main menu.
    pub(crate) logo: Option<String>,
    /// What the main menu stands over, if anything.
    pub(crate) backdrop: Option<backdrop::View>,
    pub(crate) version: &'static str,
}

impl Look {
    fn new(program: &Program, look: &tgdata::defs::ModernInterface) -> Look {
        let backdrop = look.backdrop_map.clone().and_then(|map| {
            let Some(focus) = look.backdrop_focus else {
                warn!("no backdrop_focus for the backdrop {map}");
                return None;
            };
            Some(backdrop::View {
                map,
                focus,
                troops: look.backdrop_troops.clone(),
                plane: look.backdrop_plane.clone(),
            })
        });
        Look {
            title: (look.title.clone()).unwrap_or_else(|| program.name.to_string()),
            logo: look.logo.clone(),
            backdrop,
            version: program.version,
        }
    }
}

#[derive(States, Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Screen {
    MainMenu,
    Options,
    Lan,
    Lobby,
    Loading,
    Match,
    Debriefing,
    Campaign,
}

/// The game's data, which a match shares as it's loaded and played.
#[derive(Resource)]
struct Data(Arc<GameData>);

#[derive(Resource)]
struct Loaded {
    game: Arc<GameData>,
    map: Map,
}

#[derive(Resource)]
struct Sim(World);

/// The models objects are drawn with, for drawing more mid-match.
#[derive(Resource)]
struct Drawn(Models);

/// Which of the map's sides to play, counting from 1, as asked on the command
/// line.
#[derive(Resource, Clone, Copy)]
struct Side(Option<usize>);

/// What the person playing goes by.
#[derive(Resource, Clone, PartialEq)]
pub(crate) struct Nickname(pub(crate) String);

/// Whether the console's verbs for developing the game may run, as asked
/// on the command line.
#[derive(Resource, Clone, Copy, Default)]
pub(crate) struct Dev(pub(crate) bool);

/// What matches' chances are drawn from, as asked on the command line.
#[derive(Resource, Clone, Copy)]
struct Seed(Option<u64>);

impl Seed {
    /// The one asked for, else one no match has had.
    fn get(self) -> u64 {
        self.0.unwrap_or_else(tgserver::fresh_seed)
    }
}

/// The faction that leads each side, by player id, none for sides without a
/// nation; and the sides the person here plays, with what they go by.
#[derive(Resource, Default)]
struct Leaders(HashMap<u32, tgdata::Faction>, Option<(Vec<u32>, String)>);

impl Leaders {
    /// The banner, by id, the faction leading `player`'s side flies.
    fn banner(&self, player: u32) -> Option<&tgdata::Id> {
        self.0.get(&player)?.banner.as_ref()
    }

    /// What a side is called, as its things' owner and in lines: a
    /// person's side, or one a computer player took over from a person,
    /// by the name `sides` has for it, or the person here's by theirs
    /// until the match gives it, the computer's after the computer player
    /// of its faction, such as `Kesselring AI`, and the rest by the name
    /// the map gives them.
    fn owner(&self, player: &tgmap::Player, sides: &tgsim::Sides, map: &tgdata::Strings) -> String {
        let named = sides.names.get(&player.id);
        if let Some(name) = named.or_else(|| sides.computers.get(&player.id)) {
            return name.clone();
        }
        if let Some((_, name)) = (self.1.as_ref()).filter(|(own, _)| own.contains(&player.id)) {
            return name.clone();
        }
        let faction = self.0.get(&player.id);
        if let Some(name) = faction.and_then(|g| g.computer_name.clone()) {
            return name;
        }
        map.get(&player.name)
            .cloned()
            .unwrap_or_else(|| player.name.clone())
    }
}

/// The player orders come from; none where the map has nobody to play.
#[derive(Resource)]
struct Local(Option<u32>);

/// The organizations orders go to, in the order they were picked.
#[derive(Resource, Default)]
struct Selected(Vec<usize>);

/// The player's building that's selected, whose orders the buttons give
/// while no organizations are.
#[derive(Resource, Default)]
struct SelectedBuilding(Option<usize>);

/// What raising organizations and commissioning things takes, read from the
/// game's data as the match starts.
#[derive(Resource)]
struct Raising(Recruits);

/// What the cursor is over.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    #[default]
    Nothing,
    Organization(usize),
    Building(usize),
}

impl Focus {
    /// The organization or building focused, if it's either.
    fn piece(self) -> Option<tgsim::Piece> {
        match self {
            Focus::Nothing => None,
            Focus::Organization(r) => Some(tgsim::Piece::Organization(r)),
            Focus::Building(b) => Some(tgsim::Piece::Building(b)),
        }
    }
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

/// The folders of this project's own data for the game tagged `tag`, in the
/// order they're looked in after the game's: `modern_<tag>` where there's
/// one, then `modern`, both in the first folder named `data` at or above
/// the program's that has `modern`, as an install of the program or the
/// checkout it was built in does.
fn own_data(tag: &str) -> Result<Vec<PathBuf>, String> {
    let program = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("can't find where the program is: {e}"))?;
    let root = (program.ancestors().skip(1))
        .map(|dir| dir.join("data"))
        .find(|data| data.join("modern").is_dir())
        .ok_or_else(|| format!("no data/modern beside {} or above it", program.display()))?;
    let own = root.join(format!("modern_{tag}"));
    let own = own.is_dir().then_some(own);
    Ok(own.into_iter().chain([root.join("modern")]).collect())
}

/// Plays the game `program` is of, as its command line asks.
pub fn run(program: Program) -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let Some(Options {
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
        join: joining,
        name,
    }) = cli::parse(&mut args)
    else {
        eprintln!("{}", usage(&program));
        return ExitCode::from(2);
    };
    let log = match tgrender::log::plugin(log.as_deref()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}: {e}", program.name);
            return ExitCode::from(2);
        }
    };
    let remote = match console.map(console::Remote::listen).transpose() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}: {e}", program.name);
            return ExitCode::FAILURE;
        }
    };
    let once = screenshot.is_some();
    let windowless = once || headless;
    let (dir, map) = match args.as_slice() {
        [dir] => (dir, None),
        [dir, map] if joining.is_none() => (dir, Some(map)),
        _ => {
            eprintln!("{}", usage(&program));
            return ExitCode::from(2);
        }
    };
    let opened = GameData::open(&PathBuf::from(dir)).and_then(|mut game| {
        own_game(&game, program.tag).map_err(|e| format!("{dir}: {e}"))?;
        for depot in own_data(program.tag)? {
            game.add_depot(&depot)?;
        }
        let faces = ui::Faces::read(&game)?;
        let Some(map) = map else {
            return Ok((game, None, faces));
        };
        let name = game
            .find_map(map)
            .ok_or_else(|| format!("no map named {map:?}; run without one to pick from a menu"))?;
        Ok((game, Some(name), faces))
    });
    let (game, map, faces) = match opened {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{}: {e}", program.name);
            return ExitCode::FAILURE;
        }
    };
    // What's kept is the player's own, which a run for testing, without a
    // window or driven by the console, neither takes nor changes unless
    // given somewhere else to keep it.
    let testing = windowless || console.is_some() || !scripts.is_empty();
    let saved = match (saved, testing) {
        (Some(path), _) => saved::Saved::read(path.into()),
        (None, true) => saved::Saved::none(),
        (None, false) => match saved::default_path(program.name) {
            Some(path) => saved::Saved::read(path),
            None => saved::Saved::none(),
        },
    };
    let ui_scale = saved.ui_scale().unwrap_or(1.0);
    // Without a window, the resolution chosen sizes the picture instead.
    let screenshot = windowless.then(|| ShotPlugin {
        path: screenshot.map(Into::into),
        size: saved.resolution().map_or(shot::SIZE, UVec2::from),
    });
    // Before there's a window, so that a host that isn't there or won't
    // let the game in is said where the game was asked for.
    let name = name
        .or_else(|| saved.nickname())
        .unwrap_or_else(|| tgserver::default_name(&game.strings()));
    let joining = joining.map(|(at, players)| join(at, players, name.clone()));
    let joined = match joining.transpose() {
        Ok(j) => j,
        Err(e) => {
            eprintln!("{}: {e}", program.name);
            return ExitCode::FAILURE;
        }
    };

    let modern = game.modern_interface();
    let look = Look::new(&program, &modern);
    let mut app = App::new();
    let mut window = shot::window(look.title.clone(), screenshot.is_some());
    // Its size, and whether it fills the screen, are chosen in the options.
    if let Some(window) = &mut window.primary_window {
        window.resizable = false;
        window.enabled_buttons.maximize = false;
        saved.shape(window);
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
        // A match hosted elsewhere is played at its host's pace, which
        // only frames that take the time they stand for keep to.
        match once && joined.is_none() {
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
        if once && (map.is_some() || joined.is_some()) {
            app.insert_resource(ShotAfter(shot_after))
                .add_systems(OnEnter(Screen::Match), note_start)
                .configure_sets(
                    Update,
                    shot::Capture
                        .run_if(in_state(Screen::Match))
                        .run_if(displays::loaded)
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
        .add_plugins(hud::HudPlugin)
        .add_plugins(music::MusicPlugin)
        .add_plugins(effects::EffectsPlugin)
        .insert_resource(console)
        .insert_resource(UiScaleSetting(ui_scale))
        .insert_resource(ui::Theme::of(&modern))
        .insert_resource(look)
        .init_resource::<hud::Hud>()
        // The menu's backdrop has a camera too, which stays put.
        .configure_sets(
            Update,
            camera::Controls
                .run_if(in_state(Screen::Match))
                .run_if(takes_input),
        )
        .insert_resource(Time::<Fixed>::from_seconds(TICK.into()))
        .add_message::<intent::Intent>()
        .add_observer(intent::send)
        .add_observer(formations::on_press)
        .add_observer(formations::on_activate)
        .add_observer(actions::hold_card_button)
        .add_observer(actions::press_card_button)
        .add_observer(control_groups::press)
        .add_message::<waypoints::Sent>()
        .add_message::<messages::Refused>()
        .add_message::<messages::Failed>()
        .init_resource::<Selected>()
        .init_resource::<SelectedBuilding>()
        .init_resource::<control_groups::ControlGroups>()
        .init_resource::<control_groups::Strip>()
        .add_systems(OnEnter(Screen::Match), control_groups::open_strip)
        .add_systems(
            Update,
            (control_groups::picture, control_groups::tips)
                .chain()
                .run_if(in_state(Screen::Match)),
        )
        .init_resource::<Focus>()
        .init_resource::<pointing::Placed>()
        .init_resource::<ghost::Ghost>()
        .init_resource::<placing::Anchor>()
        .init_resource::<ghost::Landing>()
        .init_resource::<deliveries::Flights>()
        .init_resource::<supply::Border>()
        .init_resource::<idle::Shown>()
        .init_resource::<clock::PauseState>()
        .init_resource::<supply::Outline>()
        .init_resource::<actions::Page>()
        .init_resource::<actions::AimedFor>()
        .init_resource::<formations::Formations>()
        .insert_resource(Side(side))
        .insert_resource(Nickname(name))
        .insert_resource(Seed(seed))
        .insert_resource(Dev(dev))
        .add_plugins((
            ui::UiPlugin,
            confirm::ConfirmPlugin,
            menu::MenuPlugin,
            backdrop::BackdropPlugin,
            options::OptionsPlugin,
            lobby::LobbyPlugin,
            scenario::ScenarioPlugin,
            pause::PausePlugin,
            paused::PausedPlugin,
            displays::DisplaysPlugin,
            technologies::TechnologyPlugin,
            objectives::ObjectivesPlugin,
            outcome::OutcomePlugin,
            connection::ConnectionPlugin,
            chat::ChatPlugin,
        ))
        .add_plugins((
            fields::FieldsPlugin,
            lan::LanPlugin,
            staging::StagingPlugin,
            leaving::LeavingPlugin,
            times::TimesPlugin,
            politics::PoliticsPlugin,
            campaigns::CampaignsPlugin,
            heard::HeardPlugin,
        ))
        .add_systems(OnEnter(Screen::MainMenu), console::start_scripts)
        .add_systems(
            OnEnter(Screen::Loading),
            (leaving::note_kept, (open_screen, messages::start)).chain(),
        )
        .add_systems(Update, load.run_if(in_state(Screen::Loading)))
        .add_systems(
            OnEnter(Screen::Match),
            (
                setup,
                title_window,
                deliveries::reset,
                fog::setup,
                setup_overlays,
                keys::load,
                banners::setup_banners,
                (
                    minimap::setup,
                    actions::setup,
                    object_display::setup,
                    resource_bar::setup,
                    supply::setup,
                ),
                messages::spawn,
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
                keys::mark.run_if(resource_exists_and_changed::<keys::Bindings>),
            )
                .chain()
                .run_if(in_state(Screen::Match)),
        )
        .add_plugins((HoldFirstTick, InTouch))
        .configure_sets(Update, Telling.after(carry_out).before(messages::show))
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
                        .before(tgrender::Animate),
                    deliveries::show,
                    fog::hide_buildings,
                )
                    .chain(),
                keep_selection,
                clear_scenery,
                sunlight::follow,
                selection_sounds,
                show_focus,
                (supply::show_border, supply::tint_buildings),
                idle::refresh,
                (
                    clock::follow.run_if(resource_exists::<Orders>),
                    clock::disable_buttons,
                )
                    .chain(),
                (
                    show_health,
                    banners::add_banners,
                    banners::badge_banners,
                    banners::add_hq_banners,
                    (banners::show_banners, banners::show_hq_banners),
                    target_map,
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
                (
                    actions::act.after(keys::send),
                    actions::back_out,
                    actions::fill_slots,
                    actions::sound_slots,
                    actions::hover,
                    actions::tell,
                )
                    .chain(),
                (actions::let_go_card_buttons, actions::show_card_buttons)
                    .after(object_display::refresh),
                (
                    object_display::choose,
                    (
                        object_display::refresh,
                        object_display::portray,
                        object_display::hide_remembered,
                    ),
                )
                    .chain(),
                (
                    formations::let_go,
                    formations::light,
                    formations::press.after(keys::send),
                    formations::show,
                )
                    .chain(),
                resource_bar::show.after(actions::hover),
                (messages::tell, messages::refuse, messages::show).chain(),
                messages::list.after(messages::tell).after(displays::Keys),
                chat::keys
                    .after(keys::send)
                    .run_if(takes_input)
                    .run_if(connection::closed)
                    .run_if(resource_exists::<Orders>)
                    .before(carry_out),
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
                            .run_if(ui::pointer_away),
                    )
                        .run_if(not(actions::aiming))
                        .run_if(not(special_ops::armed)),
                    actions::aim.run_if(ui::pointer_away),
                    placing::place.run_if(ui::pointer_away),
                    special_ops::give,
                )
                    .chain(),
                control_groups::recall,
                control_groups::toggle_strip,
                choose_formation.run_if(keys_free),
            )
                .run_if(in_state(Screen::Match))
                .run_if(takes_input)
                .before(carry_out),
        )
        .add_systems(
            Update,
            (
                keys::send.run_if(keys_free).run_if(keys_heard),
                (
                    special_ops::act,
                    idle::cycle,
                    messages::cycle,
                    supply::toggle_border,
                    fog::cheat,
                    clock::keys,
                )
                    .after(keys::send),
            )
                .run_if(in_state(Screen::Match))
                .run_if(resource_exists::<Orders>)
                // What they work on is all there once the match has loaded.
                .run_if(displays::loaded)
                .before(carry_out),
        )
        .add_systems(
            Update,
            (carry_out, stop_awaiting, keep_time)
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
        .insert_resource(lobby::Sais(game.sais()))
        .insert_resource(lobby::Leaving::of(&strings))
        .insert_resource(confirm::Words::of(&strings))
        .insert_resource(outcome::Words::of(&strings))
        .insert_resource(connection::Words::of(&strings))
        .insert_resource(pause::Words::of(&strings))
        .insert_resource(chat::Words::of(&strings))
        .insert_resource(lan::Words::of(&strings))
        .insert_resource(staging::Words::of(&strings))
        .insert_resource(StageWords::of(&strings))
        .insert_resource(options::Options::new(&game))
        .insert_resource(times::Elapsed(game.shows_elapsed_game_time()))
        .insert_resource(sound::Sounds::new(&game, &ui::CLICKS))
        .insert_resource(music::Music::new(&game))
        .insert_resource(volumes)
        .insert_resource(campaigns::Campaigns::of(&game))
        .insert_resource(Data(Arc::new(game)))
        .insert_resource(faces)
        .init_resource::<ui::Gem>()
        .add_systems(
            Startup,
            (ui::load_gem, connection::load, ui::ImageCache::setup),
        )
        .add_systems(OnExit(Screen::Match), ui::ImageCache::forget);
    saved.apply(app.world_mut());
    app.insert_resource(campaigns::Progress::beside(saved.path()))
        .insert_resource(saved)
        .add_plugins(saved::SavedPlugin);
    let setup = match &joined {
        Some((_, welcome, _)) => Some(welcome.setup.clone()),
        None => map.map(|name| tgserver::Setup {
            map: name,
            seed: Seed(seed).get(),
            rules: (fog.map(|f| (tgdata::Id::from(tgdata::FOG_OF_WAR), f.id().to_string())))
                .into_iter()
                .collect(),
            ..Default::default()
        }),
    };
    match setup {
        Some(setup) => {
            app.insert_resource(Pending {
                title: tgdata::map_title(&setup.map).to_string(),
                description: String::new(),
                person: None,
                from_menu: false,
                back: None,
                setup,
                joined,
            })
            .insert_state(Screen::Loading);
        }
        None => {
            app.insert_state(Screen::MainMenu);
        }
    }
    hud::add_states(&mut app);
    // Before what's placed over the map in Update, once the screen and the
    // interface have changed, and again once a menu has opened or closed,
    // so the scale changes with them rather than a frame late.
    app.add_systems(
        bevy::state::state::StateTransition,
        fit_ui.after(bevy::state::state::StateTransitionSystems::EnterSchedules),
    )
    .add_systems(
        PostUpdate,
        fit_ui
            .before(hud::original::fit_view)
            .before(bevy::ui::UiSystems::Prepare),
    );
    app.add_systems(Last, orders::leave_on_exit)
        .insert_resource(program);
    match app.run() {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

/// Whether none of the console, the pause menu and the loading screen is
/// up, so the match takes the keyboard and mouse. The chat bar takes only
/// the keyboard, so what reads keys checks it as well.
fn takes_input(
    console: Res<console::Console>,
    menu: Res<pause::PauseMenu>,
    displays: Res<displays::Displays>,
    loading: Query<(), With<LoadingScreen>>,
) -> bool {
    keys_heard(console, menu) && displays::closed(Some(displays)) && loading.is_empty()
}

/// Whether keys reach the match's hotkeys: neither the console nor the
/// pause menu is up. Which of them are heard is [`keys::send`]'s to say.
pub(crate) fn keys_heard(console: Res<console::Console>, menu: Res<pause::PauseMenu>) -> bool {
    console::closed(console) && pause::closed(menu)
}

/// Whether the match has the keyboard: neither the chat bar nor the
/// connection dialog, which both take what's typed, has it.
pub fn keys_free(chat: Option<Res<chat::Chat>>, dialog: Option<Res<connection::Dialog>>) -> bool {
    chat::closed(chat) && dialog.is_none_or(connection::closed)
}

/// Names the match's map in the window's title.
fn title_window(
    loaded: Res<Loaded>,
    look: Res<Look>,
    mut window: Option<Single<&mut Window, With<PrimaryWindow>>>,
) {
    if let Some(window) = &mut window {
        window.title = format!("{} - {}", loaded.map.title, look.title);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A side a person may play, on no team.
    pub(crate) fn player(id: u32) -> tgmap::Player {
        tgmap::Player {
            id,
            name: format!("kingdom_{id}"),
            color: "blue".into(),
            rgb: [1.0; 3],
            team: None,
            independent: false,
            playable: true,
            required: false,
            ai_profile: None,
            default_defeat: true,
            outcome: Default::default(),
            won_by_cities: false,
            resigned: false,
            nation: None,
            faction: None,
            relations: Vec::new(),
            team_relations: Vec::new(),
            stockpile: 0.0,
            default_stockpile: false,
            stockpile_bonus: 0.0,
            technologies: Vec::new(),
            handicap: None,
            barred: Vec::new(),
        }
    }

    #[test]
    fn sides_are_called_by_their_person_their_computer_player_or_the_map() {
        let faction = |id: &str, computer: Option<&str>| tgdata::Faction {
            id: id.into(),
            name: format!("General {id}"),
            short_name: id.into(),
            operations: Vec::new(),
            experience_max: None,
            public: true,
            portrait: None,
            computer_name: computer.map(Into::into),
            banner: None,
        };
        let mut leaders = Leaders(
            HashMap::from([
                (1, faction("patton", Some("Patton AI"))),
                (2, faction("rommel", Some("Rommel AI"))),
                // No computer player plays as this one.
                (3, faction("patton_tutorial", None)),
            ]),
            None,
        );
        let map: tgdata::Strings = [("kingdom_4".into(), "Civilian (Aggressive)".into())]
            .into_iter()
            .collect();
        let sides = tgsim::Sides {
            names: [(1, "Bob".to_string())].into(),
            ..Default::default()
        };
        // A person's side goes by their name.
        assert_eq!(leaders.owner(&player(1), &sides, &map), "Bob");
        // Never the faction's own name.
        assert_eq!(leaders.owner(&player(2), &sides, &map), "Rommel AI");
        // Without a computer player's name, a side goes by the map's name
        // for it, if any.
        assert_eq!(leaders.owner(&player(3), &sides, &map), "kingdom_3");
        assert_eq!(
            leaders.owner(&player(4), &sides, &map),
            "Civilian (Aggressive)"
        );
        // The person here's sides go by their name until the match names
        // them, and then by the match's.
        leaders.1 = Some((vec![1, 2], "Ann".into()));
        assert_eq!(leaders.owner(&player(2), &sides, &map), "Ann");
        assert_eq!(leaders.owner(&player(1), &sides, &map), "Bob");
    }
}
