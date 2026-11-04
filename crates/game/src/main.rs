//! Plays Axis & Allies (2004) matches, on a map picked from a menu or given
//! on the command line.
//!
//! You play one of the map's players. Click one of your units to select its
//! regiment and right-click the ground to send it there. The number keys pick
//! its formation, in the order its layout lists them. Rings in their owner's
//! colour mark every regiment's units.

use std::collections::HashSet;
use std::f32::consts::FRAC_PI_2;
use std::path::PathBuf;
use std::process::ExitCode;

use aaa_data::GameData;
use aaa_map::{Ground, Map};
use aaa_render::camera::{self, RtsCamera};
use aaa_render::terrain::world;
use aaa_render::{Edge, MapAssets, MapRenderPlugin, Playing};
use aaa_sim::{TICK, World};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, ScrollArea};
use bevy::window::PrimaryWindow;

const USAGE: &str = "\
usage:
  aaa-game <game dir> [--side <n>]          pick a map from a menu
  aaa-game <game dir> <map> [--side <n>]    play a map, e.g. \"allies10_berlin\"

  --side <n>    play the map's nth player (\"Side n\" in its lobby) instead of
                the first one a person may play";

#[derive(States, Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Screen {
    Menu,
    Match,
}

/// The game's data while the menu waits for a map.
#[derive(Resource)]
struct Data(GameData);

/// A menu entry, holding the map's archive path.
#[derive(Component)]
struct MapChoice(String);

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

/// Draws one of the simulation's units.
#[derive(Component)]
struct UnitView(usize);

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
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "aaa-game".into(),
            ..default()
        }),
        ..default()
    }));
    app.add_plugins(MapRenderPlugin { edge: Edge::Fade })
        .insert_resource(Time::<Fixed>::from_seconds(TICK.into()))
        .init_resource::<Selected>()
        .insert_resource(Side(side))
        .add_systems(OnEnter(Screen::Menu), menu)
        .add_systems(Update, highlight.run_if(in_state(Screen::Menu)))
        .add_systems(OnEnter(Screen::Match), setup)
        .add_systems(FixedUpdate, step.run_if(in_state(Screen::Match)))
        .add_systems(
            Update,
            (command, choose_formation, show_units, show_selection).run_if(in_state(Screen::Match)),
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

/// Loads the clicked map and starts a match on it.
fn choose_map(activate: On<Activate>, choices: Query<&MapChoice>, mut commands: Commands) {
    let Ok(MapChoice(name)) = choices.get(activate.entity) else {
        return;
    };
    let name = name.clone();
    commands.queue(move |world: &mut bevy::ecs::world::World| {
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
            }
        }
    });
}

fn setup(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    sim: Res<Sim>,
    local: Res<Local>,
    mut assets: MapAssets,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    let Loaded { game, map } = &mut *loaded;
    window.title = format!("{} - aaa-game", map.title);
    let regiments: HashSet<u32> = map.regiments.iter().map(|r| r.id).collect();
    let mut models = aaa_render::spawn_map(&mut commands, &mut assets, game, map, false, |t| {
        regiments.contains(&t.id)
    });
    let members = map.regiments.iter().flat_map(|r| &r.members);
    let mut missing = 0;
    for (i, m) in members.enumerate() {
        debug_assert_eq!(sim.0.units[i].id, m.thing.id);
        match models.spawn_unit(&mut commands, game, &mut assets.models(), &m.thing, m.speed) {
            Some(e) => {
                commands.entity(e).insert(UnitView(i));
            }
            None => missing += 1,
        }
    }
    info!(
        "{} regiments of {} units, {missing} without a model",
        sim.0.regiments.len(),
        sim.0.units.len()
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

fn step(mut sim: ResMut<Sim>) {
    sim.0.tick();
}

/// Places units between where the last two ticks left them, playing their
/// moving animation while they move.
fn show_units(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    fixed: Res<Time<Fixed>>,
    mut views: Query<(&UnitView, &mut Transform, Option<&mut Playing>)>,
) {
    let t = fixed.overstep_fraction();
    for (view, mut transform, playing) in &mut views {
        let unit = &sim.0.units[view.0];
        let [px, py] = unit.previous;
        let [x, y] = unit.position;
        let (x, y) = (px + (x - px) * t, py + (y - py) * t);
        transform.translation = world(x, y, loaded.map.ground.height_at(x, y));
        let [fx, fy] = unit.facing;
        transform.rotation = Quat::from_rotation_y(fy.atan2(fx) + FRAC_PI_2);
        if let Some(mut playing) = playing {
            let speed = sim.0.regiments[unit.regiment].speed;
            playing.set_moving(unit.moving().then_some(speed));
        }
    }
}

/// Left click selects the regiment of the unit of yours under the cursor,
/// right click sends the selected one where it points.
fn command(
    buttons: Res<ButtonInput<MouseButton>>,
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
            .filter(|u| sim.0.commands(player, u.regiment))
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
        let _ = sim.0.order_move(player, r, point);
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
/// regiment's again, wider.
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
    for unit in &sim.0.units {
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
