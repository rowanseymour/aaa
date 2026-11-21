//! What the main menu stands over: a stretch of one of the game's maps,
//! as its [`View`] says, with its buildings and scenery, in the game's own
//! view, loaded and built off the main thread, then faded in. It has no
//! units, as there's no sim behind it.
//!
//! It's kept while the screens the menu opens are up, its camera off, so
//! coming back shows it at once; it goes when a match starts loading.

use std::collections::HashSet;
use std::thread::JoinHandle;

use bevy::prelude::*;
use tgck::map::Thing;
use tgmap::{Map, Recruits, Role};
use tgrender::camera::{self, RtsCamera};
use tgrender::terrain::{self, Prepared};
use tgrender::{Edge, MapAssets};

use crate::{Data, Program, Screen};

/// Which stretch of which map the backdrop shows.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// The map, as [`tgdata::GameData::find_map`] takes its name.
    pub map: &'static str,
    /// Where on it the camera looks, in map units, a little to the right of
    /// what's to be seen, for the menu's column.
    pub focus: [f32; 2],
}

/// How much further back than the game's camera it stands, at the game's
/// angle, to take in a little more.
const ZOOM: f32 = 1.25;
/// How far from the focus objects are drawn: past the edge of the view,
/// even in the widest windows.
const REACH: f32 = 90.0;
/// Seconds the land stays covered once it's spawned, for its materials to
/// reach the GPU before it shows.
const HOLD: f32 = 0.3;
/// Seconds the cover then takes to thin away.
const FADE_IN: f32 = 1.0;

pub struct BackdropPlugin;

impl Plugin for BackdropPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::MainMenu), show)
            .add_systems(OnExit(Screen::MainMenu), hide)
            .add_systems(OnEnter(Screen::Loading), clear)
            .add_systems(Update, (arrive, fade_in).run_if(in_state(Screen::MainMenu)));
    }
}

/// On everything the backdrop spawns.
#[derive(Component, Clone)]
pub(crate) struct Backdrop;

/// The map being loaded and its land built, until it's spawned.
#[derive(Resource)]
pub struct Building(Option<JoinHandle<Result<(Map, Prepared), String>>>);

/// A cover over the land that thins away, seconds after it's spawned.
#[derive(Component)]
pub struct Fading(f32);

/// Whether the backdrop is all there to see, for screenshots: built,
/// spawned and faded in, or not coming, or not on this screen.
pub fn settled(
    screen: Res<State<Screen>>,
    building: Option<Res<Building>>,
    covers: Query<(), With<Fading>>,
) -> bool {
    *screen.get() != Screen::MainMenu || (building.is_none() && covers.is_empty())
}

/// Turns the camera back on, or the first time starts building the land,
/// with a camera that shows the menu until it's there.
fn show(
    mut commands: Commands,
    data: Res<Data>,
    edge: Res<Edge>,
    program: Res<Program>,
    mut cameras: Query<&mut Camera, With<Backdrop>>,
) {
    if !cameras.is_empty() {
        cameras.iter_mut().for_each(|mut c| c.is_active = true);
        return;
    }
    let mut cam = RtsCamera::new(Vec3::ZERO);
    cam.distance *= ZOOM;
    let camera = camera::spawn(&mut commands, cam);
    // Below the cameras of the screens the menu opens, which would otherwise
    // lose the interface to it: it goes to the top camera, on or off.
    let below = Camera {
        order: -1,
        ..default()
    };
    commands.entity(camera).insert((Backdrop, below));
    let game = data.0.clone();
    let fade = edge.fade();
    let map = program.backdrop.map;
    let thread = std::thread::spawn(move || {
        let name = game
            .find_map(map)
            .ok_or_else(|| format!("{map}: no such map"))?;
        let map = Map::load(&game, &Recruits::new(&game), &name)?;
        let prepared = terrain::prepare(&game, &map.ground, &map.terrain, fade);
        Ok((map, prepared))
    });
    commands.insert_resource(Building(Some(thread)));
}

/// Leaves the backdrop for the menu's other screens, which have cameras of
/// their own.
fn hide(mut cameras: Query<&mut Camera, With<Backdrop>>) {
    cameras.iter_mut().for_each(|mut c| c.is_active = false);
}

/// Takes the backdrop away for a match, and the map's ambient light with
/// it, which isn't on any entity to go with the rest.
fn clear(mut commands: Commands, backdrop: Query<Entity, With<Backdrop>>) {
    commands.remove_resource::<Building>();
    commands.insert_resource(GlobalAmbientLight::default());
    backdrop.iter().for_each(|e| commands.entity(e).despawn());
}

/// Once the land's built, spawns it and the defences near the camera's
/// focus, and points the camera at them.
fn arrive(
    mut commands: Commands,
    mut building: Option<ResMut<Building>>,
    data: Res<Data>,
    program: Res<Program>,
    mut assets: MapAssets,
    mut cameras: Query<(&mut RtsCamera, &mut Transform), With<Backdrop>>,
) {
    let Some(thread) = building
        .as_mut()
        .and_then(|b| b.0.take_if(|t| t.is_finished()))
    else {
        return;
    };
    commands.remove_resource::<Building>();
    let (map, prepared) = match thread.join() {
        Ok(Ok(built)) => built,
        Ok(Err(e)) => return warn!("menu backdrop: {e}"),
        Err(_) => return warn!("menu backdrop: building it panicked"),
    };
    let at = program.backdrop.focus;
    let size = map.ground.size();
    let sun = &map.sun;
    tgrender::spawn_land(&mut commands, &mut assets, prepared, sun, size, Backdrop);
    // Buildings and scenery, but no units: there's no sim to move them.
    let still: HashSet<u32> = (map.roles())
        .filter(|(_, role)| matches!(role, Role::Building | Role::Other))
        .map(|(t, _)| t.id)
        .collect();
    let near = |t: &Thing| {
        let [x, y, _] = t.position;
        Vec2::new(x, y).distance(Vec2::from(at)) < REACH
    };
    let which = |t: &Thing| still.contains(&t.id) && near(t);
    let (assets, game) = (&mut assets, &data.0);
    tgrender::spawn_objects(&mut commands, assets, game, &map, false, which, Backdrop);
    let [x, y] = at;
    let focus = tgrender::space::world(x, y, map.ground.height_at(at));
    for (mut cam, mut transform) in &mut cameras {
        cam.focus = focus;
        *transform = cam.transform();
    }
    commands.spawn((
        Fading(0.0),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundColor(Color::BLACK),
        // Under the menu, over the land.
        GlobalZIndex(crate::ui::layer::BACKDROP),
        Pickable::IGNORE,
        // Gone with the menu, or it would cover the screens it opens.
        DespawnOnExit(Screen::MainMenu),
    ));
}

fn fade_in(
    mut commands: Commands,
    time: Res<Time>,
    mut covers: Query<(Entity, &mut Fading, &mut BackgroundColor)>,
) {
    for (cover, mut fading, mut color) in &mut covers {
        fading.0 += time.delta_secs();
        let left = 1.0 - (fading.0 - HOLD).max(0.0) / FADE_IN;
        match left > 0.0 {
            true => color.0.set_alpha(left),
            false => commands.entity(cover).despawn(),
        }
    }
}
