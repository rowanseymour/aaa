//! Plays Axis & Allies (2004) matches, set up in a custom game's lobby or on
//! a map given on the command line.
//!
//! You play one of the map's sides, or watch, and the computer plays the rest. Click one of your units or its
//! banner to select its regiment, Shift+click to add a regiment to the
//! selection or take it out, or drag a box over several, with Shift to add
//! them, up to six at once. Click one of your buildings to select it
//! instead; the [`pane`] along the bottom shows what's selected. The selection
//! passes from a truck to the building it unpacks into, and back as the
//! building packs up. Right-click the ground to send the selection there, keeping its
//! regiments' places relative to each other and stopping to fight enemies on
//! the way. The number keys pick the selection's formation, in the order each regiment's
//! layout lists them, and the buttons in the corner give the selection the
//! orders in [`actions`]. Clicking the minimap moves the camera there, and the
//! key left of 1 opens a console for script commands. Units and buildings
//! make their sounds as they're selected, ordered, move, fire and die.

mod actions;
mod banners;
mod console;
mod ghost;
mod lobby;
mod menu;
mod minimap;
mod pane;
mod presets;
mod remote;
mod sound;
mod waypoints;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::ExitCode;

use aaa_data::GameData;
use aaa_map::{Ground, Map, Member, Recruits, Shape};
use aaa_render::MapObject;
use aaa_render::camera::{self, RtsCamera};
use aaa_render::models::{Footprint, ModelAssets, Models, pose};
use aaa_render::overlay::{self, HEALTH_BAR, Mark, OverlayImages, SQUARE_INSET};
use aaa_render::shot::{self, ShotPlugin};
use aaa_render::terrain::{map_point, world};
use aaa_render::{Edge, MapAssets, MapRenderPlugin, Playing};
use aaa_sim::{Relation, State, TICK, Target, World};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::window::PrimaryWindow;
use tgck::map::{Detail, Thing};

const USAGE: &str = "\
usage:
  aaa-game <game dir> [options]          set up a game from the menus
  aaa-game <game dir> <map> [options]    play a map, e.g. \"allies10_berlin\"

options:
  --side <n>                play the map's nth player (\"Side n\" in its lobby)
                            instead of the first one a person may play
  --exec <script.txt>       run a file of console commands once the match starts;
                            may be given more than once
  --console <port>          take console lines on a port of this machine, each
                            answered with what the console says, then an empty line
  --screenshot <file.png>   save a frame to a file and exit, without a window";

#[derive(States, Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Screen {
    MainMenu,
    NewGame,
    Lobby,
    Loading,
    Match,
}

/// The game's data while the menus wait for a map.
#[derive(Resource)]
struct Data(GameData);

/// The map to play, loaded once the loading screen has had a chance to show,
/// since loading holds up the frame.
#[derive(Resource)]
struct Pending {
    name: String,
    /// What the loading screen calls it.
    title: String,
    /// What the loading screen says of it.
    description: String,
    /// The teams the lobby put the sides in, by player id, for
    /// [`Map::arrange`]; none to play it as made.
    teams: Option<Vec<Vec<u32>>>,
    /// The player the person plays, or none to watch, when launched from
    /// the lobby.
    person: Option<u32>,
    /// The nation each side plays as, by player id, when launched from the
    /// lobby.
    nations: HashMap<u32, aaa_data::Nation>,
    frames: u32,
    /// Whether it was launched from the lobby, which failing to load it goes
    /// back to and which waits for the person to continue once it's loaded,
    /// rather than given on the command line, which it exits or starts.
    from_menu: bool,
}

/// The loading screen, which stays over the match while it's set up.
#[derive(Component)]
struct LoadingScreen;

/// What the loading screen says it's doing.
#[derive(Component)]
struct LoadingStatus;

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
    game: GameData,
    map: Map,
}

#[derive(Resource)]
struct Sim(World);

/// Commands from every player, in the order given, waiting to be carried
/// out between ticks. They're the only way players change a match, so that
/// every copy of it can play the same commands out on the same tick.
#[derive(Resource, Default)]
struct Orders(Vec<(u32, aaa_sim::Command)>);

impl Orders {
    /// Queues `command` from `player`, unless the sim would refuse it now.
    fn give(
        &mut self,
        sim: &World,
        player: u32,
        command: aaa_sim::Command,
    ) -> Result<(), aaa_sim::Refused> {
        sim.check(player, &command)?;
        self.0.push((player, command));
        Ok(())
    }

    /// Carries out every command waiting. Checked when given, they're only
    /// refused where one before them changed things, such as a building
    /// told to pack and then to commission.
    fn carry_out(&mut self, sim: &mut World) {
        for (player, command) in self.0.drain(..) {
            if let Err(e) = sim.apply(player, command) {
                info!("player {player}'s order refused: {e:?}");
            }
        }
    }
}

/// Carries out orders as soon as they're given, ticking or not, so that
/// what shows them sees them taken up.
fn carry_out(mut sim: ResMut<Sim>, mut orders: ResMut<Orders>) {
    orders.carry_out(&mut sim.0);
}

/// The models objects are drawn with, for drawing more mid-match.
#[derive(Resource)]
struct Drawn(Models);

/// Which of the map's sides to play, counting from 1, as asked on the command
/// line.
#[derive(Resource, Clone, Copy)]
struct Side(Option<usize>);

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
/// mid-match get drawn too.
#[derive(Resource, Default)]
struct Viewed {
    units: usize,
    buildings: usize,
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
}

/// Draws the trailer one of the sim's units tows.
#[derive(Component)]
struct TrailerView {
    /// Index into the sim's units.
    unit: usize,
    conform: Option<Shape>,
}

/// Spawns the model of the sim's `unit`, placed from `member`, and of any
/// `trailer` it tows; false if the unit has none.
#[allow(clippy::too_many_arguments)]
fn spawn_unit_view(
    commands: &mut Commands,
    models: &mut Models,
    game: &mut GameData,
    assets: &mut ModelAssets,
    ground: &Ground,
    member: &Member,
    trailer: Option<&aaa_sim::Trailer>,
    owner: u32,
    unit: usize,
) -> bool {
    let thing = &member.thing;
    if let Some(t) = trailer {
        let towed = thing_at(ground, thing.id, &t.template, t.position, t.facing);
        if let Some(e) = models.spawn_building(commands, game, assets, &towed, owner) {
            let conform = models.conform(&t.template);
            commands.entity(e).insert(TrailerView { unit, conform });
        }
    }
    let Some(e) = models.spawn_unit(commands, game, assets, thing, owner, member.speed) else {
        return false;
    };
    commands.entity(e).insert(UnitView {
        unit,
        shot: None,
        dead: false,
        moving: false,
        conform: models.conform(&thing.template),
    });
    true
}

/// Places trailers between where the last two ticks left them, hiding
/// them along with what towed them.
fn show_trailers(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
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
        (transform.translation, transform.rotation) = pose(
            &loaded.map.ground,
            trailer.drawn_at(t),
            trailer.facing,
            view.conform,
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
    Some(Options {
        side,
        scripts,
        console,
        screenshot,
    })
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let Some(Options {
        side,
        scripts,
        console,
        screenshot,
    }) = options(&mut args)
    else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let remote = match console.map(remote::Remote::listen).transpose() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("aaa-game: {e}");
            return ExitCode::FAILURE;
        }
    };
    let screenshot = screenshot.map(|path| ShotPlugin { path: path.into() });
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
    let (mut game, map) = match opened {
        Ok(o) => o,
        Err(e) => {
            eprintln!("aaa-game: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut app = App::new();
    let window = shot::window("aaa-game".into(), screenshot.is_some());
    app.add_plugins(DefaultPlugins.set(window));
    if let Some(s) = screenshot {
        app.add_plugins(s);
    }
    let mut console = console::Console::default();
    for s in &scripts {
        console.exec_on_start(s);
    }
    app.add_plugins(MapRenderPlugin { edge: Edge::Fade })
        .add_plugins(console::ConsolePlugin)
        .add_plugins(sound::SoundPlugin)
        .insert_resource(console)
        .configure_sets(Update, camera::Controls.run_if(console::closed))
        .insert_resource(Time::<Fixed>::from_seconds(TICK.into()))
        .add_message::<waypoints::Sent>()
        .init_resource::<Selected>()
        .init_resource::<SelectedBuilding>()
        .init_resource::<Focus>()
        .init_resource::<ghost::Ghost>()
        .init_resource::<actions::Page>()
        .insert_resource(Side(side))
        .add_plugins((menu::MenuPlugin, lobby::LobbyPlugin))
        .add_systems(OnEnter(Screen::Loading), loading)
        .add_systems(Update, load.run_if(in_state(Screen::Loading)))
        .add_systems(
            OnEnter(Screen::Match),
            (
                setup,
                setup_overlays,
                banners::setup_banners,
                minimap::spawn,
                actions::spawn,
                pane::spawn,
                waypoints::load,
                console::start_scripts,
                ready,
            )
                .chain(),
        )
        .add_systems(FixedUpdate, step.run_if(in_state(Screen::Match)))
        .add_systems(
            Update,
            (
                (
                    spawn_views,
                    (show_units, show_trailers, show_buildings, show_packing),
                )
                    .chain(),
                drop_lost_building,
                selection_sounds,
                show_focus,
                show_health.after(camera::Controls),
                (banners::add_banners, banners::show_banners)
                    .chain()
                    .after(camera::Controls),
            )
                .run_if(in_state(Screen::Match)),
        )
        .add_systems(
            Update,
            (
                minimap::steer.before(camera::Controls),
                minimap::update.after(camera::Controls),
                (actions::back_out, actions::show).chain(),
                (pane::lay_out, pane::refresh).chain(),
                (ghost::show, ghost::redden).chain(),
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
                    actions::aim
                        .run_if(minimap::pointer_away)
                        .run_if(actions::pointer_away),
                )
                    .chain(),
                choose_formation,
            )
                .run_if(in_state(Screen::Match))
                .run_if(console::closed)
                .before(carry_out),
        )
        .add_systems(
            Update,
            carry_out
                .after(console::run)
                .run_if(in_state(Screen::Match)),
        );
    if let Some(remote) = remote {
        app.insert_resource(remote);
    }
    app.insert_resource(lobby::Nations(game.nations()))
        .insert_resource(Data(game));
    match map {
        Some(name) => {
            app.insert_resource(Pending {
                title: aaa_data::map_title(&name).to_string(),
                name,
                description: String::new(),
                teams: None,
                person: None,
                nations: HashMap::new(),
                frames: 0,
                from_menu: false,
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

/// Readies a match on a loaded map, with a person playing the `person`
/// player's side, if any, for [`setup`] to start.
fn begin(world: &mut bevy::ecs::world::World, loaded: Loaded, person: Option<u32>) {
    let sim = World::new(&loaded.map);
    let lineup = lobby::Lineup::new(&loaded.map.players, person);
    for &(id, control) in &lineup.0 {
        if let Some(p) = sim.sides.player(id) {
            let team = p
                .team
                .map_or("no team".into(), |t| format!("team {}", t + 1));
            let nation = p.nation.as_deref().unwrap_or("no nation");
            info!("{} ({}, {team}, {nation}): {control:?}", p.name, p.color);
        }
    }
    world.insert_resource(Local(lineup.person()));
    world.insert_resource(lineup);
    world.insert_resource(Sim(sim));
    world.insert_resource(Orders::default());
    let mut loaded = loaded;
    let raising = Recruits::new(&mut loaded.game);
    world.resource_mut::<Sim>().0.packing = raising.packings();
    world.insert_resource(Raising(raising));
    world.insert_resource(loaded);
}

/// Shows the map being loaded, as the game does: its name over what it's
/// about, and how loading goes.
fn loading(mut commands: Commands, pending: Res<Pending>) {
    commands.spawn((Camera2d, DespawnOnExit(Screen::Loading)));
    let font = |size: f32| TextFont {
        font_size: FontSize::Px(size),
        ..default()
    };
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
            BackgroundColor(Color::BLACK),
            GlobalZIndex(10),
        ))
        .id();
    commands.spawn((ChildOf(root), Text::new(pending.title.clone()), font(22.0)));
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
            BorderColor::all(Color::srgb(0.75, 0.6, 0.3)),
            children![(Text::new(pending.description.clone()), font(16.0))],
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
                Text::new("Loading..."),
                font(16.0),
                TextColor(Color::srgb(0.6, 0.6, 0.6)),
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

/// Loads the pending map and starts setting a match up on it behind the
/// loading screen, or goes back to the lobby or exits if it can't.
fn load(world: &mut bevy::ecs::world::World) {
    // One frame to lay the screen out and one to draw it.
    let Some(mut pending) = world.get_resource_mut::<Pending>() else {
        return;
    };
    pending.frames += 1;
    if pending.frames < 3 {
        return;
    }
    let Some(Pending {
        name,
        teams,
        person,
        nations,
        from_menu,
        ..
    }) = world.remove_resource::<Pending>()
    else {
        return;
    };
    let Some(Data(mut game)) = world.remove_resource::<Data>() else {
        return;
    };
    // Given on the command line, each side plays as the nation its map
    // fixes or one at random.
    let nations = match from_menu {
        true => nations,
        false => aaa_map::Brief::load(&mut game, &name).map_or_else(
            |_| HashMap::new(),
            |b| {
                lobby::nations(
                    &b.players,
                    &HashMap::new(),
                    &world.resource::<lobby::Nations>().0,
                )
            },
        ),
    };
    match Map::load_as(&mut game, &name, &nations) {
        Ok(mut map) => {
            if let Some(teams) = &teams {
                map.arrange(teams);
            }
            let person = match from_menu {
                true => person,
                false => lobby::default_person(&map.players, *world.resource::<Side>()),
            };
            if !from_menu && world.resource::<Side>().0.is_some() && person.is_none() {
                warn!("{}: no such side to play", map.title);
            }
            begin(world, Loaded { game, map }, person);
            world.insert_resource(Readying { wait: from_menu });
            world.resource_mut::<NextState<Screen>>().set(Screen::Match);
        }
        Err(e) if from_menu => {
            error!("{e}");
            let mut screens = world.query_filtered::<Entity, With<LoadingScreen>>();
            for screen in screens.iter(world).collect::<Vec<_>>() {
                world.despawn(screen);
            }
            world.insert_resource(Data(game));
            world.resource_mut::<NextState<Screen>>().set(Screen::Lobby);
        }
        Err(e) => {
            error!("{e}");
            world.write_message(AppExit::error());
        }
    }
}

/// Once the match is set up, says so and holds it until the person
/// continues, or starts it straight away.
fn ready(
    mut commands: Commands,
    readying: Option<Res<Readying>>,
    mut time: ResMut<Time<Virtual>>,
    mut status: Query<&mut Text, With<LoadingStatus>>,
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
        text.0 = "Finished Loading".into();
    }
    for slot in &slot {
        commands
            .spawn((ChildOf(slot), menu::button("Continue", 20.0)))
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
    mut loaded: ResMut<Loaded>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let images = OverlayImages::load(&mut loaded.game, &mut images, formats.0);
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
    mut loaded: ResMut<Loaded>,
    sim: Res<Sim>,
    local: Res<Local>,
    mut assets: MapAssets,
    mut window: Option<Single<&mut Window, With<PrimaryWindow>>>,
) {
    let Loaded { game, map } = &mut *loaded;
    if let Some(window) = &mut window {
        window.title = format!("{} - aaa-game", map.title);
    }
    let ids = map.regiments.iter().map(|r| r.id);
    // The sim's units draw the trailers they tow.
    let members = map.regiments.iter().flat_map(|r| &r.members);
    let trailers = members.filter_map(|m| Some(m.trailer.as_ref()?.placed?.0));
    let leave_out: HashSet<u32> = ids
        .chain(trailers)
        .chain(map.buildings.iter().map(|b| b.id))
        .collect();
    let mut models = aaa_render::spawn_map(&mut commands, &mut assets, game, map, false, |t| {
        leave_out.contains(&t.id)
    });
    let members = map.regiments.iter().flat_map(|r| &r.members);
    let mut missing = 0;
    for (i, m) in members.enumerate() {
        debug_assert_eq!(sim.0.units[i].id, m.thing.id);
        let owner = sim.0.regiments[sim.0.units[i].regiment].owner;
        let mut model_assets = assets.models();
        if !spawn_unit_view(
            &mut commands,
            &mut models,
            game,
            &mut model_assets,
            &map.ground,
            m,
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
    let things = map.things.iter().filter(|t| leave_out.contains(&t.id));
    let things: std::collections::HashMap<u32, _> = things.map(|t| (t.id, t)).collect();
    let mut missing = 0;
    for (i, b) in map.buildings.iter().enumerate() {
        debug_assert_eq!(sim.0.buildings[i].id, b.id);
        let thing = things[&b.id];
        match models.spawn_object(&mut commands, game, &mut assets.models(), thing) {
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
        Some([x, y]) => RtsCamera::new(world(x, y, map.ground.height_at(x, y))),
        None => RtsCamera::centred(&map.ground),
    };
    camera::spawn(&mut commands, cam);
    commands.insert_resource(Viewed {
        units: sim.0.units.len(),
        buildings: sim.0.buildings.len(),
    });
    commands.insert_resource(Drawn(models));
    commands.insert_resource(sound::Sounds::new(game));
}

/// Plays the callout of a regiment newly added to the selection.
fn selection_sounds(
    selected: Res<Selected>,
    sim: Res<Sim>,
    time: Res<Time>,
    mut sounds: ResMut<sound::Sounds>,
    mut before: bevy::prelude::Local<Vec<usize>>,
) {
    if !selected.is_changed() {
        return;
    }
    if let Some(&r) = selected.0.iter().find(|r| !before.contains(r)) {
        let regiment = &sim.0.regiments[r];
        let at = sim.0.centre(r);
        sounds.cue(
            &time,
            &regiment.template,
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
    orders.carry_out(sim);
    sim.tick();
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
        position: [x, y, ground.height_at(x, y)],
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
    mut loaded: ResMut<Loaded>,
    mut drawn: ResMut<Drawn>,
    mut viewed: ResMut<Viewed>,
    mut assets: MapAssets,
) {
    let Loaded { game, map } = &mut *loaded;
    for u in viewed.units..sim.0.units.len() {
        let unit = &sim.0.units[u];
        let regiment = &sim.0.regiments[unit.regiment];
        let member = Member {
            thing: thing_at(
                &map.ground,
                unit.id,
                &unit.template,
                unit.position,
                unit.facing,
            ),
            speed: regiment.speed,
            shape: unit.shape,
            acceleration: 0.0,
            turn_rate: 0.0,
            trailer: None,
            fighter: unit.fighter.clone(),
        };
        let mut models = assets.models();
        spawn_unit_view(
            &mut commands,
            &mut drawn.0,
            game,
            &mut models,
            &map.ground,
            &member,
            unit.trailer.as_ref(),
            regiment.owner,
            u,
        );
        if regiment.units.first() == Some(&u) {
            info!("{} {} raised", regiment.template, regiment.id);
        }
    }
    viewed.units = sim.0.units.len();
    for b in viewed.buildings..sim.0.buildings.len() {
        let building = &sim.0.buildings[b];
        let thing = thing_at(
            &map.ground,
            building.id,
            &building.template,
            building.position,
            building.facing,
        );
        let mut models = assets.models();
        let Some(e) =
            drawn
                .0
                .spawn_building(&mut commands, game, &mut models, &thing, building.owner)
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

/// Plays a building's packing or unpacking model in its place while it
/// packs or unpacks, and hides it once it's packed up.
fn show_packing(
    mut commands: Commands,
    sim: Res<Sim>,
    mut loaded: ResMut<Loaded>,
    mut drawn: ResMut<Drawn>,
    mut assets: MapAssets,
    mut views: Query<(&mut BuildingView, &MapObject, &mut Visibility)>,
) {
    for (mut view, object, mut visibility) in &mut views {
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
        let gone = matches!(building.state, State::Packed(..));
        if let (Some((start, unpacking)), None) = (wanted, view.packing) {
            let into = (sim.0.ticks - start) as f32 * TICK;
            let mut models = assets.models();
            let spawned = drawn.0.spawn_packing(
                &mut commands,
                &mut loaded.game,
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

/// Lets go of the selected building once it's no longer the player's to
/// give orders to.
fn drop_lost_building(sim: Res<Sim>, local: Res<Local>, mut chosen: ResMut<SelectedBuilding>) {
    if let Some(b) = chosen.0 {
        let building = &sim.0.buildings[b];
        if Some(building.owner) != local.0 || !building.standing() {
            chosen.0 = None;
        }
    }
}

/// Places units between where the last two ticks left them, playing their
/// moving animation while they move, their attack as they fire and their
/// death as they die; the dead without one are hidden.
fn show_units(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
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
        if unit.unpacked.is_some() {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        if !unit.alive() && !view.dead {
            view.dead = true;
            sounds.cue(
                &time,
                &unit.template,
                |a| &a.destroyed,
                unit.position,
                owner,
            );
            let dies = playing.as_mut().is_some_and(|p| p.die(unit.id as usize));
            if !dies {
                *visibility = Visibility::Hidden;
            }
        }
        if unit.shot != view.shot {
            view.shot = unit.shot;
            let weapon = unit
                .shot
                .and_then(|(_, w)| unit.fighter.as_ref()?.weapons.get(w));
            if let (Some(w), Some(target)) = (weapon, unit.target) {
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
        );
        if unit.alive() && unit.moving() != view.moving {
            view.moving = unit.moving();
            let event: fn(&aaa_data::ObjectAudio) -> &Vec<_> = match view.moving {
                true => |a| &a.start_moving,
                false => |a| &a.stop_moving,
            };
            sounds.cue(&time, &unit.template, event, unit.position, owner);
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
        let building = &sim.0.buildings[view.building];
        if let (State::Destroyed(_), false) = (building.state, view.destroyed) {
            view.destroyed = true;
            let (at, owner) = (building.position, building.owner);
            sounds.cue(&time, &building.template, |a| &a.destroyed, at, owner);
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

/// Finds what the cursor is over: the regiment whose banner it's on, else
/// that of the nearest unit within reach of the ground it points at, else
/// the building standing there.
fn focus(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &RtsCamera)>,
    banners: Query<(
        &banners::Banner,
        &RelativeCursorPosition,
        &ZIndex,
        &Visibility,
    )>,
    loaded: Res<Loaded>,
    sim: Res<Sim>,
    scenery: Res<banners::Scenery>,
    mut focus: ResMut<Focus>,
) {
    const REACH: f32 = 2.0;
    if let Some(r) = banners::under_cursor(&banners) {
        focus.set_if_neq(Focus::Regiment(r));
        return;
    }
    let Some(point) = pointed(&windows, &cameras, &loaded.map.ground) else {
        focus.set_if_neq(Focus::Nothing);
        return;
    };
    let near = |p: [f32; 2]| (p[0] - point[0]).hypot(p[1] - point[1]);
    let unit = sim
        .0
        .units
        .iter()
        .filter(|u| u.alive() && !scenery.has(&sim.0, u.regiment))
        .map(|u| (u.regiment, near(u.position)))
        .filter(|(_, d)| *d < REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let building = || {
        sim.0.buildings.iter().position(|b| {
            !b.gone()
                && b.obstacle
                    .as_ref()
                    .is_some_and(|o| o.distance(point) == 0.0)
        })
    };
    let found = match unit {
        Some((r, _)) => Focus::Regiment(r),
        None => building().map_or(Focus::Nothing, Focus::Building),
    };
    focus.set_if_neq(found);
}

/// Left click selects the regiment of yours under the cursor and dragging
/// selects those with a unit in the box, either adding to the selection with
/// Shift, which also takes a clicked regiment already in it back out.
#[allow(clippy::too_many_arguments)]
fn select(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &RtsCamera)>,
    minimap: Query<&RelativeCursorPosition, With<minimap::MinimapView>>,
    grid: Query<(&RelativeCursorPosition, &InheritedVisibility), With<actions::Blocks>>,
    loaded: Res<Loaded>,
    local: Res<Local>,
    focus: Res<Focus>,
    sim: Res<Sim>,
    scenery: Res<banners::Scenery>,
    mut selected: ResMut<Selected>,
    mut chosen: ResMut<SelectedBuilding>,
    mut start: bevy::prelude::Local<Option<Vec2>>,
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
            node.left = px(r.min.x);
            node.top = px(r.min.y);
            node.width = px(r.width());
            node.height = px(r.height());
            visibility.set_if_neq(Visibility::Inherited);
        }
        None => {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
    if !released || start.take().is_none() {
        return;
    }
    let Some(player) = local.0 else {
        return;
    };
    let adding = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let Some(rect) = dragged else {
        match *focus {
            Focus::Regiment(r) if sim.0.commands(player, r) => {
                chosen.0 = None;
                match selected.0.iter().position(|&s| s == r) {
                    Some(i) if adding => {
                        selected.0.remove(i);
                    }
                    None if adding && selected.0.len() < pane::MAX_SELECTED => selected.0.push(r),
                    None if adding => {}
                    _ => selected.0 = vec![r],
                }
            }
            Focus::Building(b)
                if !adding
                    && sim.0.buildings[b].owner == player
                    && sim.0.buildings[b].standing() =>
            {
                selected.0.clear();
                chosen.0 = Some(b);
            }
            _ if adding => {}
            _ => {
                selected.0.clear();
                chosen.0 = None;
            }
        }
        return;
    };
    let Ok((camera, transform, _)) = cameras.single() else {
        return;
    };
    let ground = &loaded.map.ground;
    let inside = |u: usize| {
        let [x, y] = sim.0.units[u].position;
        camera
            .world_to_viewport(transform, world(x, y, ground.height_at(x, y)))
            .is_ok_and(|p| rect.contains(p))
    };
    if !adding {
        selected.0.clear();
        chosen.0 = None;
    }
    for r in 0..sim.0.regiments.len() {
        if selected.0.len() >= pane::MAX_SELECTED {
            break;
        }
        if sim.0.commands(player, r)
            && !scenery.has(&sim.0, r)
            && !selected.0.contains(&r)
            && sim.0.survivors(r).any(inside)
        {
            selected.0.push(r);
            chosen.0 = None;
        }
    }
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
    // The free camera turns with the right button.
    let free = cameras.single().is_ok_and(|(_, _, rts)| rts.free);
    if !buttons.just_pressed(MouseButton::Right) || free {
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
        sounds.cue(time, template, |a| &a.commanded, sim.centre(first), player);
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
            actions::Aim::Unpack => aaa_sim::Command::Unpack { regiment, at: to },
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
            info!("{} to formation {}", regiment.template, formation.id);
        }
    }
}

/// Where a ray from the camera first meets the ground, in world units.
fn pick_ground(ground: &Ground, ray: Ray3d) -> Option<[f32; 2]> {
    let above = |p: Vec3| {
        let [x, y] = map_point(p);
        p.y - ground.height_at(x, y)
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
                // Around the ground its template claims, which takes in the
                // pad and yard; the model's own box is about its walls.
                let claimed = match building.obstacle.as_ref().map(|o| o.shape) {
                    Some(Shape::Rect { width, height }) => Some(Vec2::new(width, height)),
                    Some(Shape::Circle { radius }) => Some(Vec2::splat(2.0 * radius)),
                    _ => None,
                };
                let size = claimed
                    .or(footprint.map(|f| f.0))
                    .unwrap_or(Vec2::splat(4.0))
                    / (1.0 - 2.0 * SQUARE_INSET);
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

/// Draws a health bar below each of the selected regiments' units, the
/// width a bar has there but square to the screen and over everything.
#[allow(clippy::too_many_arguments)]
fn show_health(
    mut commands: Commands,
    selected: Res<Selected>,
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    images: Res<OverlayImages>,
    fixed: Res<Time<Fixed>>,
    cameras: Query<(&Camera, &Transform), With<RtsCamera>>,
    units: Query<(&UnitView, Option<&Footprint>)>,
    mut bars: Query<(&mut Node, &mut ImageNode, &mut Visibility), With<HealthBar>>,
) {
    let mut wanted = Vec::new();
    if let (false, Ok((camera, transform))) = (selected.0.is_empty(), cameras.single()) {
        // This frame's camera, which moves after transforms propagate.
        let global = GlobalTransform::from(*transform);
        let half = transform.right() * HEALTH_BAR.x / 2.0;
        let toward = (transform.back().as_vec3() * Vec3::new(1.0, 0.0, 1.0)).normalize_or_zero();
        let ground = &loaded.map.ground;
        let t = fixed.overstep_fraction();
        for (view, footprint) in &units {
            let unit = &sim.0.units[view.unit];
            if !selected.0.contains(&unit.regiment) || !unit.alive() {
                continue;
            }
            let Some(fighter) = &unit.fighter else {
                continue;
            };
            let Some(image) = images.health_bar(unit.health / fighter.health) else {
                continue;
            };
            let [x, y] = unit.drawn_at(t);
            // At the edge of its footprint nearest the camera, so the bar
            // clears the model.
            let reach = footprint.map_or(0.0, |f| f.0.length() / 2.0);
            let at = world(x, y, ground.height_at(x, y)) + toward * reach;
            let (Ok(a), Ok(b)) = (
                camera.world_to_viewport(&global, at - half),
                camera.world_to_viewport(&global, at + half),
            ) else {
                continue;
            };
            let width = a.distance(b);
            let height = width * HEALTH_BAR.y / HEALTH_BAR.x;
            let centre = (a + b) / 2.0 + Vec2::new(0.0, height);
            wanted.push((
                centre - Vec2::new(width, height) / 2.0,
                Vec2::new(width, height),
                image.clone(),
            ));
        }
    }
    let place = |node: &mut Node, corner: Vec2, size: Vec2| {
        node.left = px(corner.x);
        node.top = px(corner.y);
        node.width = px(size.x);
        node.height = px(size.y);
    };
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
            ImageNode::new(frame),
            Pickable::IGNORE,
            DespawnOnExit(Screen::Match),
        ));
    }
}
