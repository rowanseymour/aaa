//! What the main menu stands over: a stretch of one of the game's maps,
//! as its [`View`] says, with its buildings and scenery, in the game's own
//! view, loaded and built off the main thread, then faded in. There's no
//! sim behind it, so its only units are for show: troops marching across
//! it over and over, and a plane passing over now and then, as its [`View`]
//! names them.
//!
//! It's kept while the screens the menu opens are up, its camera off, so
//! coming back shows it at once; it goes when a match starts loading.

use std::collections::HashSet;
use std::thread::JoinHandle;

use bevy::prelude::*;
use tgck::map::Thing;
use tgdata::Footing;
use tgmap::{Ground, Map, Recruits, Role, Shape};
use tgrender::camera::{self, RtsCamera};
use tgrender::models::{Models, pose};
use tgrender::space::map_point;
use tgrender::terrain::{self, Prepared};
use tgrender::{Edge, MapAssets, Playing};

use crate::{Data, Look, Screen};

/// Which stretch of which map the backdrop shows.
#[derive(Clone, Debug)]
pub struct View {
    /// The map, as [`tgdata::GameData::find_map`] takes its name.
    pub map: String,
    /// Where on it the camera looks, in map units, a little to the right of
    /// what's to be seen, for the menu's column.
    pub focus: [f32; 2],
    /// The template of the troops that march across it.
    pub troops: Option<String>,
    /// The template of the plane that flies over it.
    pub plane: Option<String>,
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
/// How far troops and the plane go from the focus before coming round
/// again: as far as objects are drawn, out of sight.
const PASS: f32 = REACH;
/// How far nearer the camera than the focus the troops march, in map units.
const LANE: f32 = -6.0;
/// How many bodies of troops march, evenly apart, each in ranks and
/// files, and the room each soldier has.
const BODIES: usize = 3;
const RANKS: usize = 4;
const FILES: usize = 3;
const SPACING: f32 = 1.6;
/// Seconds between one pass of the plane and the next, and before its
/// first.
const PLANE_WAIT: f32 = 15.0;
/// How far left of the focus it passes, in map units.
const PLANE_LEFT: f32 = 12.0;

pub struct BackdropPlugin;

impl Plugin for BackdropPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::MainMenu), show)
            .add_systems(OnExit(Screen::MainMenu), hide)
            .add_systems(OnEnter(Screen::Loading), clear)
            .add_systems(
                Update,
                (arrive, fade_in, (march, fly).before(tgrender::Animate))
                    .run_if(in_state(Screen::MainMenu)),
            );
    }
}

/// On everything the backdrop spawns.
#[derive(Component, Clone)]
pub(crate) struct Backdrop;

/// The map being loaded and its land built, until it's spawned.
#[derive(Resource)]
pub struct Building(Option<JoinHandle<Result<(Map, Prepared), String>>>);

/// The ground the backdrop's units cross, and the way they go, from when
/// it arrived.
#[derive(Resource)]
struct Stage {
    ground: Ground,
    /// Seconds into the app it arrived.
    since: f32,
    focus: Vec2,
    /// Which way on the map is right across the screen, and which up it.
    across: Vec2,
    up: Vec2,
    /// The camera's pitch, for how much nearer the camera is drawn
    /// further up the screen.
    pitch: f32,
}

impl Stage {
    /// Where something `ahead` of the far side of its pass, `aside` of its
    /// line, has got to going `along`, coming round each `2 * PASS`.
    fn place(&self, along: Vec2, aside: Vec2, ahead: f32) -> Vec2 {
        let gone = ahead.rem_euclid(2.0 * PASS) - PASS;
        self.focus + along * gone + aside
    }
}

/// A soldier in the troops marching across, where it stands in their ranks.
#[derive(Component)]
struct Marcher {
    rank: f32,
    aside: f32,
    speed: f32,
    conform: Option<Shape>,
}

/// The plane flying over.
#[derive(Component)]
struct Flier {
    speed: f32,
    height: f32,
}

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
    look: Res<Look>,
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
    let Some(view) = &look.backdrop else {
        return;
    };
    let game = data.0.clone();
    let fade = edge.fade();
    let map = view.map.clone();
    let thread = std::thread::spawn(move || {
        let name = game
            .find_map(&map)
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
    commands.remove_resource::<Stage>();
    commands.insert_resource(GlobalAmbientLight::default());
    backdrop.iter().for_each(|e| commands.entity(e).despawn());
}

/// Once the land's built, spawns it and the defences near the camera's
/// focus, and points the camera at them.
fn arrive(
    mut commands: Commands,
    mut building: Option<ResMut<Building>>,
    time: Res<Time>,
    data: Res<Data>,
    look: Res<Look>,
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
    let Some(at) = look.backdrop.as_ref().map(|v| v.focus) else {
        return;
    };
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
    let mut models =
        tgrender::spawn_objects(&mut commands, assets, game, &map, false, which, Backdrop);
    let [x, y] = at;
    let focus = tgrender::space::world(x, y, map.ground.height_at(at));
    let (mut turned, mut pitch) = (Transform::default(), 0.0);
    for (mut cam, mut transform) in &mut cameras {
        cam.focus = focus;
        *transform = cam.transform();
        (turned, pitch) = (*transform, cam.pitch);
    }
    let flat = |v: Vec3| Vec2::from(map_point(v)).normalize_or_zero();
    let stage = Stage {
        ground: map.ground,
        since: time.elapsed_secs(),
        focus: Vec2::from(at),
        across: flat(*turned.right()),
        up: flat(*turned.up()),
        pitch,
    };
    // Painted as whoever plays first.
    let players = &map.players;
    let owner = (players.iter().find(|p| p.playable && !p.independent))
        .or(players.first())
        .map_or(0, |p| p.id);
    let view = look.backdrop.as_ref();
    let speeds = game.speeds();
    let speed = |template: &str| speeds.get(&tgmap::Id::from(template)).copied();
    if let Some(troops) = view.and_then(|v| v.troops.as_deref()) {
        match speed(troops) {
            Some(speed) => spawn_troops(
                &mut commands,
                &mut models,
                assets,
                &stage,
                troops,
                owner,
                speed,
            ),
            None => warn!("menu backdrop: {troops} doesn't move"),
        }
    }
    if let Some(plane) = view.and_then(|v| v.plane.as_deref()) {
        let flies = game
            .aircraft()
            .get(&tgmap::Id::from(plane))
            .map(|a| a.height);
        match speed(plane).zip(flies) {
            Some((speed, height)) => {
                let thing =
                    crate::views::thing_at(&stage.ground, Footing::Air, 0, plane, at, [0.0, 1.0]);
                let mut parts = assets.models();
                if let Some(e) = models.spawn_unit(&mut commands, &mut parts, &thing, owner, speed)
                {
                    commands.entity(e).insert((
                        Backdrop,
                        Flier { speed, height },
                        Visibility::Hidden,
                    ));
                }
            }
            None => warn!("menu backdrop: {plane} doesn't fly"),
        }
    }
    commands.insert_resource(stage);
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

/// Spawns troops of the template `troops` in their ranks, at the start of
/// their march.
fn spawn_troops(
    commands: &mut Commands,
    models: &mut Models,
    assets: &mut MapAssets,
    stage: &Stage,
    troops: &str,
    owner: u32,
    speed: f32,
) {
    let conform = models.conform(troops);
    let each = RANKS * FILES;
    for i in 0..BODIES * each {
        let body = (i / each) as f32 * 2.0 * PASS / BODIES as f32;
        let (rank, file) = ((i % each / FILES) as f32, (i % FILES) as f32);
        // Out of step a little, as soldiers are.
        let jitter =
            |n: u32| (((i as u32).wrapping_mul(2_654_435_761) ^ n) % 1000) as f32 / 1000.0 - 0.5;
        let marcher = Marcher {
            rank: body - rank * SPACING + jitter(1) * 0.6,
            aside: (file - (FILES - 1) as f32 / 2.0) * SPACING + jitter(2) * 0.4,
            speed,
            conform,
        };
        let at = stage.focus.to_array();
        let id = i as u32 + 1;
        let thing =
            crate::views::thing_at(&stage.ground, Footing::Ground, id, troops, at, [1.0, 0.0]);
        let mut parts = assets.models();
        if let Some(e) = models.spawn_unit(commands, &mut parts, &thing, owner, speed) {
            commands.entity(e).insert((Backdrop, marcher));
        }
    }
}

/// Marches the troops across, round and round.
fn march(
    time: Res<Time>,
    stage: Option<Res<Stage>>,
    mut marchers: Query<(&Marcher, &mut Transform, Option<&mut Playing>)>,
) {
    let Some(stage) = stage else {
        return;
    };
    let t = time.elapsed_secs() - stage.since;
    let lane = stage.up * LANE;
    for (m, mut transform, playing) in &mut marchers {
        let aside = lane + stage.up * m.aside;
        let at = stage.place(stage.across, aside, t * m.speed + m.rank + PASS * 0.4);
        let facing = stage.across.to_array();
        (transform.translation, transform.rotation) = pose(
            &stage.ground,
            at.to_array(),
            facing,
            m.conform,
            Footing::Ground,
        );
        if let Some(mut p) = playing {
            p.set_moving(Some(m.speed));
        }
    }
}

/// Flies the plane over from the top right of the screen to the bottom
/// left, waiting out of sight between passes.
fn fly(
    time: Res<Time>,
    stage: Option<Res<Stage>>,
    mut planes: Query<(
        &Flier,
        &mut Transform,
        &mut Visibility,
        Option<&mut Playing>,
    )>,
) {
    let Some(stage) = stage else {
        return;
    };
    let t = time.elapsed_secs() - stage.since;
    // A step up the map goes only so far up the screen.
    let along = -(stage.across + stage.up / stage.pitch.sin()).normalize_or_zero();
    for (f, mut transform, mut visibility, playing) in &mut planes {
        let crossing = 2.0 * PASS / f.speed;
        let into = t.rem_euclid(PLANE_WAIT + crossing) - PLANE_WAIT;
        visibility.set_if_neq(match into >= 0.0 {
            true => Visibility::Inherited,
            false => Visibility::Hidden,
        });
        // High up, it's seen further up the screen than the ground under
        // it: its line runs nearer the camera to be seen over the focus,
        // and left of it, clear of the menu.
        let aside = -stage.up * f.height / stage.pitch.tan() - stage.across * PLANE_LEFT;
        let at = stage.place(along, aside, into.max(0.0) * f.speed);
        let (mut over, turned) = pose(
            &stage.ground,
            at.to_array(),
            along.to_array(),
            None,
            Footing::Air,
        );
        // Level, at its height over where the backdrop looks.
        over.y = stage.ground.height_at(stage.focus.to_array()) + f.height;
        (transform.translation, transform.rotation) = (over, turned);
        if let Some(mut p) = playing {
            p.set_moving(Some(f.speed));
        }
    }
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
