//! Plays Axis & Allies (2004) matches, on a map picked from a menu or given
//! on the command line.
//!
//! You play one of the map's players. Click one of your units to select its
//! regiment and right-click the ground to send it there, stopping to fight
//! enemies on the way, or Alt+right-click to have it run there, ignoring
//! them. The number keys pick its formation, in the order its layout lists
//! them. Rings in their owner's colour mark every regiment's units, outlines
//! every building and a wider ring each city's reach; grey outlines mark
//! disabled buildings. The key left of 1 opens a console for script
//! commands, such as moving the camera or raising regiments.

mod console;
mod remote;

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::ExitCode;

use aaa_data::GameData;
use aaa_map::{Ground, Map, Obstacle, Shape};
use aaa_render::camera::{self, RtsCamera};
use aaa_render::models::{Models, pose};
use aaa_render::shot::{self, ShotPlugin};
use aaa_render::terrain::world;
use aaa_render::{Edge, MapAssets, MapRenderPlugin, Playing};
use aaa_sim::{State, TICK, World};
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
  --exec <script.txt>       run a file of console commands once the match starts;
                            may be given more than once
  --console <port>          take console lines on a port of this machine, each
                            answered with what the console says, then an empty line
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

/// The models objects are drawn with, for drawing more mid-match.
#[derive(Resource)]
struct Drawn(Models);

/// Which of the map's major players to play, counting from 1.
#[derive(Resource, Clone, Copy)]
struct Side(Option<usize>);

/// The player orders come from; none where the map has nobody to play.
#[derive(Resource)]
struct Local(Option<u32>);

/// The regiment orders go to.
#[derive(Resource, Default)]
struct Selected(Option<usize>);

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
    let mut scripts = Vec::new();
    while let Some(i) = args.iter().position(|a| a == "--exec") {
        if i + 1 >= args.len() {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        scripts.push(args.remove(i + 1));
        args.remove(i);
    }
    let remote = match args.iter().position(|a| a == "--console") {
        Some(i) => match args.get(i + 1).and_then(|p| p.parse().ok()) {
            Some(port) => {
                args.drain(i..i + 2);
                match remote::Remote::listen(port) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        eprintln!("aaa-game: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            None => {
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
    let mut console = console::Console::default();
    for s in &scripts {
        console.exec_on_start(s);
    }
    app.add_plugins(MapRenderPlugin { edge: Edge::Fade })
        .add_plugins(console::ConsolePlugin)
        .insert_resource(console)
        .configure_sets(Update, camera::Controls.run_if(console::closed))
        .insert_resource(Time::<Fixed>::from_seconds(TICK.into()))
        .init_resource::<Selected>()
        .insert_resource(Side(side))
        .add_systems(OnEnter(Screen::Menu), menu)
        .add_systems(Update, highlight.run_if(in_state(Screen::Menu)))
        .add_systems(OnEnter(Screen::Loading), loading)
        .add_systems(Update, load.run_if(in_state(Screen::Loading)))
        .add_systems(
            OnEnter(Screen::Match),
            (setup, console::start_scripts).chain(),
        )
        .add_systems(FixedUpdate, step.run_if(in_state(Screen::Match)))
        .add_systems(
            Update,
            (show_units, show_buildings, show_selection).run_if(in_state(Screen::Match)),
        )
        .add_systems(
            Update,
            (command, choose_formation)
                .run_if(in_state(Screen::Match))
                .run_if(console::closed),
        );
    if let Some(remote) = remote {
        app.insert_resource(remote);
    }
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
        match models.spawn_unit(&mut commands, game, &mut assets.models(), &m.thing, m.speed) {
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
    commands.insert_resource(Drawn(models));
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

/// Left click selects the regiment of the unit of yours under the cursor,
/// right click sends the selected one where it points, running with Alt.
#[allow(clippy::too_many_arguments)]
fn command(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &RtsCamera)>,
    loaded: Res<Loaded>,
    local: Res<Local>,
    mut sim: ResMut<Sim>,
    mut selected: ResMut<Selected>,
) {
    let Some(player) = local.0 else {
        return;
    };
    let (left, right) = (
        buttons.just_pressed(MouseButton::Left),
        buttons.just_pressed(MouseButton::Right),
    );
    if !left && !right {
        return;
    }
    let (Ok(window), Ok((camera, transform, rts))) = (windows.single(), cameras.single()) else {
        return;
    };
    // The free camera turns with the right button.
    if right && rts.free {
        return;
    }
    let Some(ray) = window
        .cursor_position()
        .and_then(|c| camera.viewport_to_world(transform, c).ok())
    else {
        return;
    };
    let Some(point) = pick_ground(&loaded.map.ground, ray) else {
        return;
    };
    if left {
        const REACH: f32 = 2.0;
        selected.0 = sim
            .0
            .units
            .iter()
            .filter(|u| u.alive() && sim.0.commands(player, u.regiment))
            .map(|u| {
                (
                    u,
                    (u.position[0] - point[0]).hypot(u.position[1] - point[1]),
                )
            })
            .filter(|(_, d)| *d < REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(u, _)| u.regiment);
    } else if let Some(r) = selected.0 {
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

/// Rings every regiment's units in their owner's colour, and the selected
/// regiment's again, wider; outlines buildings in theirs, and rings cities'
/// reach.
fn show_selection(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    selected: Res<Selected>,
    fixed: Res<Time<Fixed>>,
    mut gizmos: Gizmos,
) {
    const RADIUS: f32 = 0.6;
    let t = fixed.overstep_fraction();
    let ground = &loaded.map.ground;
    for unit in sim.0.units.iter().filter(|u| u.alive()) {
        let [px, py] = unit.previous;
        let [x, y] = unit.position;
        let (x, y) = (px + (x - px) * t, py + (y - py) * t);
        let owner = sim.0.regiments[unit.regiment].owner;
        let [r, g, b] = sim.0.sides.player(owner).map_or([1.0; 3], |p| p.rgb);
        gizmos.linestrip(ring(ground, x, y, RADIUS), Color::srgb(r, g, b));
        if selected.0 == Some(unit.regiment) {
            gizmos.linestrip(ring(ground, x, y, RADIUS * 1.4), Color::WHITE);
        }
    }
    for building in &sim.0.buildings {
        let colour = match building.state {
            State::Standing => {
                let rgb = sim.0.sides.player(building.owner).map(|p| p.rgb);
                let [r, g, b] = rgb.unwrap_or([1.0; 3]);
                Color::srgb(r, g, b)
            }
            State::Disabled => Color::srgb(0.4, 0.4, 0.4),
            State::Destroyed(_) => continue,
        };
        let [x, y] = building.position;
        match &building.obstacle {
            Some(o) => gizmos.linestrip(outline(ground, o), colour),
            None => gizmos.linestrip(ring(ground, x, y, 1.0), colour),
        }
        if let Some(zone) = building.zone {
            gizmos.linestrip(ring(ground, x, y, zone), colour.with_alpha(0.5));
        }
    }
}

/// The edge of the ground an obstacle covers, following the ground's rise
/// and fall.
fn outline(ground: &Ground, o: &Obstacle) -> Vec<Vec3> {
    let [x, y] = o.position;
    let (w, h) = match o.shape {
        Shape::Rect { width, height } => (width, height),
        Shape::Circle { radius } => return ring(ground, x, y, radius).collect(),
        Shape::Point => return ring(ground, x, y, 1.0).collect(),
    };
    // Its y axis runs along the facing.
    let [fx, fy] = o.facing;
    let corner = |right: f32, ahead: f32| {
        let (rx, ry) = (right * w / 2.0, ahead * h / 2.0);
        [x + rx * fy + ry * fx, y - rx * fx + ry * fy]
    };
    let corners = [
        corner(-1.0, -1.0),
        corner(1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, 1.0),
        corner(-1.0, -1.0),
    ];
    // Several points a side, so it follows the ground between corners.
    const STEPS: usize = 8;
    corners
        .windows(2)
        .flat_map(|side| {
            let ([ax, ay], [bx, by]) = (side[0], side[1]);
            (0..STEPS).map(move |i| {
                let t = i as f32 / STEPS as f32;
                (ax + (bx - ax) * t, ay + (by - ay) * t)
            })
        })
        .chain(std::iter::once((corners[4][0], corners[4][1])))
        .map(|(px, py)| world(px, py, ground.height_at(px, py) + 0.05))
        .collect()
}

/// A ring around a point that follows the ground's rise and fall, so it
/// doesn't sink into slopes the way a flat circle would.
fn ring(ground: &Ground, x: f32, y: f32, radius: f32) -> impl Iterator<Item = Vec3> {
    const SEGMENTS: usize = 24;
    (0..=SEGMENTS).map(move |i| {
        let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let (px, py) = (x + radius * a.cos(), y + radius * a.sin());
        world(px, py, ground.height_at(px, py) + 0.05)
    })
}
