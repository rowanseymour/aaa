//! Plays Axis & Allies (2004) matches, on a map picked from a menu or given
//! on the command line.
//!
//! You play one of the map's players. Click one of your units to select its
//! regiment and right-click the ground to send it there, stopping to fight
//! enemies on the way, or Alt+right-click to have it run there, ignoring
//! them. The number keys pick its formation, in the order its layout lists
//! them. Models wear their owner's colour. Rings mark the units of the
//! regiment under the cursor, and a square the building under it, green for
//! your own; health bars show under the selected regiment's units.

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::ExitCode;

use aaa_data::GameData;
use aaa_map::{Ground, Map, Shape};
use aaa_render::MapObject;
use aaa_render::camera::{self, RtsCamera};
use aaa_render::models::{Footprint, pose};
use aaa_render::overlay::{self, HEALTH_BAR, Mark, OverlayImages, SQUARE_INSET};
use aaa_render::shot::{self, ShotPlugin};
use aaa_render::terrain::world;
use aaa_render::{Edge, MapAssets, MapRenderPlugin, Playing};
use aaa_sim::{Relation, State, TICK, World};
use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, ScrollArea};
use bevy::window::PrimaryWindow;

const USAGE: &str = "\
usage:
  aaa-game <game dir> [options]          pick a map from a menu
  aaa-game <game dir> <map> [options]    play a map, e.g. \"allies10_berlin\"

options:
  --side <n>                play the map's nth player (\"Side n\" in its lobby)
                            instead of the first one a person may play
  --screenshot <file.png>   save a frame to a file and exit, without a window";

#[derive(States, Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Screen {
    Menu,
    Loading,
    Match,
}

/// The game's data while the menu waits for a map.
#[derive(Resource)]
struct Data(GameData);

/// A menu entry, holding the map's archive path.
#[derive(Component)]
struct MapChoice(String);

/// The map picked from the menu, loaded once the loading screen has had a
/// chance to show, since loading holds up the frame.
#[derive(Resource)]
struct Pending {
    name: String,
    frames: u32,
}

#[derive(Resource)]
struct Loaded {
    game: GameData,
    map: Map,
}

#[derive(Resource)]
struct Sim(World);

/// Which of the map's major players to play, counting from 1.
#[derive(Resource, Clone, Copy)]
struct Side(Option<usize>);

/// The player orders come from; none where the map has nobody to play.
#[derive(Resource)]
struct Local(Option<u32>);

/// The regiment orders go to.
#[derive(Resource, Default)]
struct Selected(Option<usize>);

/// What the cursor is over.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    #[default]
    Nothing,
    Regiment(usize),
    Building(usize),
}

/// The flat square marks are drawn on and their materials, by how the
/// marked object's owner stands toward the player: theirs, friendly, or
/// anyone else's.
#[derive(Resource)]
struct Marks {
    square: Handle<Mesh>,
    rings: [Handle<StandardMaterial>; 3],
    squares: [Handle<StandardMaterial>; 3],
}

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
    /// The outline its model tilts on to follow the ground, if it does.
    conform: Option<Shape>,
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
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let side = match args.iter().position(|a| a == "--side") {
        Some(i) => match args.get(i + 1).and_then(|n| n.parse().ok()) {
            Some(n @ 1..) => {
                args.drain(i..i + 2);
                Some(n)
            }
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        },
        None => None,
    };
    let screenshot = match args.iter().position(|a| a == "--screenshot") {
        Some(i) if i + 1 < args.len() => {
            let path = args.remove(i + 1);
            args.remove(i);
            Some(ShotPlugin { path: path.into() })
        }
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        None => None,
    };
    let (dir, map) = match args.as_slice() {
        [dir] => (dir, None),
        [dir, map] => (dir, Some(map)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let opened = GameData::open(&PathBuf::from(dir)).and_then(|mut game| {
        let Some(map) = map else {
            return Ok((game, None));
        };
        let name = game
            .find_map(map)
            .ok_or_else(|| format!("no map named {map:?}; run without one to pick from a menu"))?;
        Map::load(&mut game, &name).map(|map| (game, Some(map)))
    });
    let (game, map) = match opened {
        Ok(o) => o,
        Err(e) => {
            eprintln!("aaa-game: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut app = App::new();
    let window = match screenshot {
        Some(_) => shot::no_window(),
        None => WindowPlugin {
            primary_window: Some(Window {
                title: "aaa-game".into(),
                ..default()
            }),
            ..default()
        },
    };
    app.add_plugins(DefaultPlugins.set(window));
    if let Some(s) = screenshot {
        app.add_plugins(s);
    }
    app.add_plugins(MapRenderPlugin { edge: Edge::Fade })
        .insert_resource(Time::<Fixed>::from_seconds(TICK.into()))
        .init_resource::<Selected>()
        .init_resource::<Focus>()
        .insert_resource(Side(side))
        .add_systems(OnEnter(Screen::Menu), menu)
        .add_systems(Update, highlight.run_if(in_state(Screen::Menu)))
        .add_systems(OnEnter(Screen::Loading), loading)
        .add_systems(Update, load.run_if(in_state(Screen::Loading)))
        .add_systems(OnEnter(Screen::Match), (setup, setup_overlays))
        .add_systems(FixedUpdate, step.run_if(in_state(Screen::Match)))
        .add_systems(
            Update,
            (
                (focus, command).chain(),
                choose_formation,
                show_units,
                show_buildings,
                show_focus,
                show_health.after(camera::Controls),
            )
                .run_if(in_state(Screen::Match)),
        );
    match map {
        Some(map) => {
            begin(app.world_mut(), Loaded { game, map });
            app.insert_state(Screen::Match);
        }
        None => {
            app.insert_resource(Data(game)).insert_state(Screen::Menu);
        }
    }
    match app.run() {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

/// Readies a match on a loaded map, for [`setup`] to start.
fn begin(world: &mut bevy::ecs::world::World, loaded: Loaded) {
    let sim = World::new(&loaded.map);
    let local = match world.resource::<Side>().0 {
        Some(n) => sim
            .sides
            .players
            .iter()
            .filter(|p| !p.independent)
            .nth(n - 1),
        None => sim.sides.first_playable(),
    };
    match local {
        Some(p) => info!("playing {} ({})", p.name, p.color),
        None => warn!("{}: no such player to play", loaded.map.title),
    }
    world.insert_resource(Local(local.map(|p| p.id)));
    world.insert_resource(Sim(sim));
    world.insert_resource(loaded);
}

const IDLE: Color = Color::srgb(0.15, 0.15, 0.15);
const HOVER: Color = Color::srgb(0.3, 0.3, 0.3);

/// Lists the maps in a column per folder.
fn menu(mut commands: Commands, data: Res<Data>) {
    let mut folders: Vec<(&str, Vec<&str>)> = Vec::new();
    for name in data.0.map_names() {
        let folder = name.rsplit_once('/').map_or("", |(f, _)| f);
        let folder = folder.rsplit('/').next().unwrap_or(folder);
        match folders.iter_mut().find(|(f, _)| *f == folder) {
            Some((_, maps)) => maps.push(name),
            None => folders.push((folder, vec![name])),
        }
    }
    commands.spawn((Camera2d, DespawnOnExit(Screen::Menu)));
    commands
        .spawn((
            DespawnOnExit(Screen::Menu),
            Node {
                width: percent(100),
                height: percent(100),
                // Otherwise it grows to fit the longest list instead of
                // letting it scroll.
                min_height: px(0),
                padding: UiRect::all(px(16)),
                column_gap: px(16),
                ..default()
            },
        ))
        .with_children(|root| {
            for (folder, maps) in folders {
                root.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    ..default()
                })
                .with_children(|column| {
                    column.spawn((
                        Text::new(folder),
                        TextFont {
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                    ));
                    column
                        .spawn((
                            ScrollArea,
                            Node {
                                flex_direction: FlexDirection::Column,
                                overflow: Overflow::scroll_y(),
                                min_height: px(0),
                                row_gap: px(2),
                                ..default()
                            },
                        ))
                        .with_children(|list| {
                            for name in maps {
                                list.spawn((
                                    Button,
                                    Hovered::default(),
                                    MapChoice(name.to_string()),
                                    Node {
                                        padding: UiRect::axes(px(8), px(4)),
                                        ..default()
                                    },
                                    BackgroundColor(IDLE),
                                    children![(
                                        Text::new(aaa_data::map_title(name)),
                                        TextFont {
                                            font_size: FontSize::Px(14.0),
                                            ..default()
                                        },
                                    )],
                                ))
                                .observe(choose_map);
                            }
                        });
                });
            }
        });
}

fn highlight(mut buttons: Query<(&Hovered, &mut BackgroundColor), Changed<Hovered>>) {
    for (hovered, mut background) in &mut buttons {
        background.0 = if hovered.get() { HOVER } else { IDLE };
    }
}

/// Shows the loading screen for the clicked map.
fn choose_map(
    activate: On<Activate>,
    choices: Query<&MapChoice>,
    mut commands: Commands,
    mut next: ResMut<NextState<Screen>>,
) {
    let Ok(MapChoice(name)) = choices.get(activate.entity) else {
        return;
    };
    commands.insert_resource(Pending {
        name: name.clone(),
        frames: 0,
    });
    next.set(Screen::Loading);
}

fn loading(mut commands: Commands, pending: Res<Pending>) {
    commands.spawn((Camera2d, DespawnOnExit(Screen::Loading)));
    commands.spawn((
        DespawnOnExit(Screen::Loading),
        Node {
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        children![(
            Text::new(format!("Loading {}...", aaa_data::map_title(&pending.name))),
            TextFont {
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextColor(Color::srgb(0.6, 0.6, 0.6)),
        )],
    ));
}

/// Loads the pending map and starts a match on it, or goes back to the menu
/// if it can't.
fn load(world: &mut bevy::ecs::world::World) {
    // One frame to lay the screen out and one to draw it.
    let mut pending = world.resource_mut::<Pending>();
    pending.frames += 1;
    if pending.frames < 3 {
        return;
    }
    let Some(Pending { name, .. }) = world.remove_resource::<Pending>() else {
        return;
    };
    let Some(Data(mut game)) = world.remove_resource::<Data>() else {
        return;
    };
    match Map::load(&mut game, &name) {
        Ok(map) => {
            begin(world, Loaded { game, map });
            world.resource_mut::<NextState<Screen>>().set(Screen::Match);
        }
        Err(e) => {
            error!("{e}");
            world.insert_resource(Data(game));
            world.resource_mut::<NextState<Screen>>().set(Screen::Menu);
        }
    }
}

/// Loads what [`show_focus`] and [`show_health`] draw with.
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
        [OWN, FRIEND, OTHER].map(|c| materials.add(overlay::mark_material(image.clone(), c)))
    };
    commands.insert_resource(Marks {
        square: meshes.add(overlay::flat_square()),
        rings: tinted(&images.ring),
        squares: tinted(&images.square),
    });
    commands.insert_resource(images);
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
    let leave_out: HashSet<u32> = ids.chain(map.buildings.iter().map(|b| b.id)).collect();
    let mut models = aaa_render::spawn_map(&mut commands, &mut assets, game, map, false, |t| {
        leave_out.contains(&t.id)
    });
    let members = map.regiments.iter().flat_map(|r| &r.members);
    let mut missing = 0;
    for (i, m) in members.enumerate() {
        debug_assert_eq!(sim.0.units[i].id, m.thing.id);
        let owner = sim.0.regiments[sim.0.units[i].regiment].owner;
        let mut model_assets = assets.models();
        match models.spawn_unit(
            &mut commands,
            game,
            &mut model_assets,
            &m.thing,
            owner,
            m.speed,
        ) {
            Some(e) => {
                commands.entity(e).insert(UnitView {
                    unit: i,
                    shot: None,
                    dead: false,
                    conform: models.conform(&m.thing.template),
                });
            }
            None => missing += 1,
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
                });
            }
            None => missing += 1,
        }
    }
    info!(
        "{} buildings, {missing} without a model",
        sim.0.buildings.len()
    );
    let own = (0..sim.0.regiments.len()).find(|&r| local.0.is_some_and(|p| sim.0.commands(p, r)));
    let cam = match own.or((!sim.0.regiments.is_empty()).then_some(0)) {
        Some(r) => {
            let [x, y] = sim.0.centre(r);
            RtsCamera::new(world(x, y, map.ground.height_at(x, y)))
        }
        None => RtsCamera::centred(&map.ground),
    };
    camera::spawn(&mut commands, cam);
}

fn step(mut sim: ResMut<Sim>, mut selected: ResMut<Selected>) {
    sim.0.tick();
    if selected.0.is_some_and(|r| sim.0.destroyed(r)) {
        selected.0 = None;
    }
}

/// Places units between where the last two ticks left them, playing their
/// moving animation while they move, their attack as they fire and their
/// death as they die; the dead without one are hidden.
fn show_units(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    fixed: Res<Time<Fixed>>,
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
        if !unit.alive() && !view.dead {
            view.dead = true;
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
            if let (Some(p), Some(ids)) = (
                playing.as_mut(),
                weapon.and_then(|w| w.animation.as_deref()),
            ) {
                p.use_ability(ids);
            }
        }
        let [px, py] = unit.previous;
        let [x, y] = unit.position;
        let (x, y) = (px + (x - px) * t, py + (y - py) * t);
        (transform.translation, transform.rotation) =
            pose(&loaded.map.ground, [x, y], unit.facing, view.conform);
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

/// Finds what the cursor is over: the regiment of the nearest unit within
/// reach of the ground it points at, else the building standing there.
fn focus(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &RtsCamera)>,
    loaded: Res<Loaded>,
    sim: Res<Sim>,
    mut focus: ResMut<Focus>,
) {
    const REACH: f32 = 2.0;
    let Some(point) = pointed(&windows, &cameras, &loaded.map.ground) else {
        focus.set_if_neq(Focus::Nothing);
        return;
    };
    let near = |p: [f32; 2]| (p[0] - point[0]).hypot(p[1] - point[1]);
    let unit = sim
        .0
        .units
        .iter()
        .filter(|u| u.alive())
        .map(|u| (u.regiment, near(u.position)))
        .filter(|(_, d)| *d < REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let building = || {
        sim.0.buildings.iter().position(|b| {
            !matches!(b.state, State::Destroyed(_))
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

/// Left click selects the regiment of yours under the cursor, right click
/// sends the selected one where it points, running with Alt.
#[allow(clippy::too_many_arguments)]
fn command(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &RtsCamera)>,
    loaded: Res<Loaded>,
    local: Res<Local>,
    focus: Res<Focus>,
    mut sim: ResMut<Sim>,
    mut selected: ResMut<Selected>,
) {
    let Some(player) = local.0 else {
        return;
    };
    if buttons.just_pressed(MouseButton::Left) {
        selected.0 = match *focus {
            Focus::Regiment(r) if sim.0.commands(player, r) => Some(r),
            _ => None,
        };
        return;
    }
    // The free camera turns with the right button.
    let free = cameras.single().is_ok_and(|(_, _, rts)| rts.free);
    if !buttons.just_pressed(MouseButton::Right) || free {
        return;
    }
    let Some(point) = pointed(&windows, &cameras, &loaded.map.ground) else {
        return;
    };
    if let Some(r) = selected.0 {
        // Selection only takes the player's own.
        let _ = match keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) {
            true => sim.0.order_run(player, r, point),
            false => sim.0.order_move(player, r, point),
        };
    }
}

fn choose_formation(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Res<Selected>,
    local: Res<Local>,
    mut sim: ResMut<Sim>,
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
    let (Some(r), Some(player)) = (selected.0, local.0) else {
        return;
    };
    if let Some(f) = KEYS.iter().position(|&k| keys.just_pressed(k))
        && sim.0.set_formation(player, r, f).is_ok()
    {
        let regiment = &sim.0.regiments[r];
        info!(
            "{} in formation {}",
            regiment.template, regiment.formations[regiment.formation].id
        );
    }
}

/// Where a ray from the camera first meets the ground, in world units.
fn pick_ground(ground: &Ground, ray: Ray3d) -> Option<[f32; 2]> {
    let above = |p: Vec3| p.y - ground.height_at(p.x, -p.z);
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
            let (x, y) = (b.x, -b.z);
            return ((0.0..=size).contains(&x) && (0.0..=size).contains(&y)).then_some([x, y]);
        }
        was = p;
    }
    None
}

const OWN: Color = Color::srgb(0.1, 0.9, 0.1);
const FRIEND: Color = Color::srgb(0.2, 0.55, 1.0);
const OTHER: Color = Color::srgb(0.95, 0.15, 0.1);

/// Which of [`Marks`]' tints suits what `owner` owns.
fn tint(sim: &World, local: Option<u32>, owner: u32) -> usize {
    match local.map(|p| sim.sides.relation(p, owner)) {
        Some(Relation::Itself) => 0,
        Some(Relation::Ally) => 1,
        _ => 2,
    }
}

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
    mut shown: Query<
        (
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
            &mut Visibility,
        ),
        With<Mark>,
    >,
) {
    // The ring fills its image but for a thin margin.
    const RING_FILL: f32 = 238.0 / 256.0;
    // Infantry are about this wide.
    const SMALLEST: Vec2 = Vec2::splat(0.4);
    let ground = &loaded.map.ground;
    let t = fixed.overstep_fraction();
    let mut wanted = Vec::new();
    match *focus {
        Focus::Nothing => {}
        Focus::Regiment(r) => {
            let material = &marks.rings[tint(&sim.0, local.0, sim.0.regiments[r].owner)];
            for (view, footprint) in &units {
                let unit = &sim.0.units[view.unit];
                if unit.regiment != r || !unit.alive() {
                    continue;
                }
                let [px, py] = unit.previous;
                let [x, y] = unit.position;
                let at = [px + (x - px) * t, py + (y - py) * t];
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
                let size = footprint.map_or(Vec2::splat(4.0), |f| f.0) / (1.0 - 2.0 * SQUARE_INSET);
                let transform = overlay::mark_transform(ground, building.position, [fx, fy], size);
                wanted.push((transform, material.clone()));
            }
        }
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

/// Draws a health bar below each of the selected regiment's units, the
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
    if let (Some(r), Ok((camera, transform))) = (selected.0, cameras.single()) {
        // This frame's camera, which moves after transforms propagate.
        let global = GlobalTransform::from(*transform);
        let half = transform.right() * HEALTH_BAR.x / 2.0;
        let toward = (transform.back().as_vec3() * Vec3::new(1.0, 0.0, 1.0)).normalize_or_zero();
        let ground = &loaded.map.ground;
        let t = fixed.overstep_fraction();
        for (view, footprint) in &units {
            let unit = &sim.0.units[view.unit];
            if unit.regiment != r || !unit.alive() {
                continue;
            }
            let Some(fighter) = &unit.fighter else {
                continue;
            };
            let Some(image) = images.health_bar(unit.health / fighter.health) else {
                continue;
            };
            let [px, py] = unit.previous;
            let [x, y] = unit.position;
            let (x, y) = (px + (x - px) * t, py + (y - py) * t);
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
            ImageNode::new(frame),
            Pickable::IGNORE,
            DespawnOnExit(Screen::Match),
        ));
    }
}
