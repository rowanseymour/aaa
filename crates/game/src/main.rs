//! Plays Axis & Allies (2004) matches, starting one straight away on the
//! map it's given.
//!
//! Click a unit to select its regiment and right-click the ground to send
//! it there.

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
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

const USAGE: &str = "\
usage:
  aaa-game <game dir> <map>    play a map, e.g. \"allies10_berlin\"; aaa-editor lists them";

#[derive(Resource)]
struct Loaded {
    game: GameData,
    map: Map,
}

#[derive(Resource)]
struct Sim(World);

/// The regiment orders go to.
#[derive(Resource, Default)]
struct Selected(Option<usize>);

/// Draws one of the simulation's units.
#[derive(Component)]
struct UnitView(usize);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, map] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let loaded = GameData::open(&PathBuf::from(dir)).and_then(|mut game| {
        let name = game
            .find_map(map)
            .ok_or_else(|| format!("no map named {map:?}"))?;
        Map::load(&mut game, &name).map(|map| Loaded { game, map })
    });
    let loaded = match loaded {
        Ok(l) => l,
        Err(e) => {
            eprintln!("aaa-game: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: format!("{} - aaa-game", loaded.map.title),
            ..default()
        }),
        ..default()
    }));
    app.add_plugins(MapRenderPlugin { edge: Edge::Fade })
        .insert_resource(Sim(World::new(&loaded.map)))
        .insert_resource(loaded)
        .insert_resource(Time::<Fixed>::from_seconds(TICK.into()))
        .init_resource::<Selected>()
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, step)
        .add_systems(Update, (command, show_units, show_selection));
    match app.run() {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

fn setup(mut commands: Commands, mut loaded: ResMut<Loaded>, sim: Res<Sim>, mut assets: MapAssets) {
    let Loaded { game, map } = &mut *loaded;
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
    let cam = match sim.0.regiments.first() {
        Some(_) => {
            let [x, y] = sim.0.centre(0);
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

/// Left click selects the regiment of the unit under the cursor, right
/// click sends the selected one where it points.
fn command(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &RtsCamera)>,
    loaded: Res<Loaded>,
    mut sim: ResMut<Sim>,
    mut selected: ResMut<Selected>,
) {
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
        sim.0.order_move(r, point);
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

/// Rings the selected regiment's units.
fn show_selection(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    selected: Res<Selected>,
    fixed: Res<Time<Fixed>>,
    mut gizmos: Gizmos,
) {
    let Some(r) = selected.0 else {
        return;
    };
    let t = fixed.overstep_fraction();
    for &u in &sim.0.regiments[r].units {
        let unit = &sim.0.units[u];
        let [px, py] = unit.previous;
        let [x, y] = unit.position;
        let (x, y) = (px + (x - px) * t, py + (y - py) * t);
        let at = world(x, y, loaded.map.ground.height_at(x, y) + 0.05);
        gizmos.circle(
            Isometry3d::new(at, Quat::from_rotation_x(FRAC_PI_2)),
            0.6,
            Color::srgb(0.3, 1.0, 0.3),
        );
    }
}
