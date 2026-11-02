//! 3D viewer for Axis & Allies (2004) maps, reading straight from the game's
//! installed data files.

mod blend;
mod camera;
mod game;
mod models;
mod objects;
mod terrain;
mod textures;

use std::path::PathBuf;
use std::process::ExitCode;

use bevy::asset::embedded_asset;
use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::CompressedImageFormatSupport;
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use tgck::map::{CELL_SIZE, Sun, Terrain, Thing};

use blend::GroundMaterial;
use camera::RtsCamera;
use game::Game;
use models::{ModelAssets, Models};
use terrain::Ground;

const USAGE: &str = "\
usage:
  aaa-viewer <game dir>          list the maps
  aaa-viewer <game dir> <map>    view a map, e.g. \"(2v2)Berlin\"

options:
  --camera <x,y,dist,yaw,pitch>  start the free camera looking at x,y (fractions
                                 of the map's width) from dist away, with yaw
                                 and pitch in radians
  --modelless                    start with markers shown for objects without
                                 models, such as sound emitters (M toggles them)
  --screenshot <file.png>        save a frame to a file and exit, without the
                                 help text or placeholder markers other than
                                 those --modelless asks for";

#[derive(Resource)]
struct Loaded {
    game: Game,
    title: String,
    ground: Ground,
    terrain: Terrain,
    things: Vec<Thing>,
    sun: Sun,
}

/// Set by `--modelless`.
#[derive(Resource)]
struct ShowModelless;

/// Where the camera starts, from `--camera`.
#[derive(Resource, Clone, Copy)]
struct StartView([f32; 5]);

/// Saves a screenshot once the scene has settled, then exits.
#[derive(Resource)]
struct ScreenshotTo {
    path: PathBuf,
    frame: u32,
    attempts: u32,
}

/// Frames to let the scene settle before asking for a capture.
const SETTLE_FRAMES: u32 = 60;
/// Frames to wait for a capture before asking again, since one can be dropped
/// (e.g. while the window can't be presented) and never arrive.
const CAPTURE_PATIENCE: u32 = 120;
/// Captures to try before giving up rather than running forever.
const MAX_CAPTURES: u32 = 5;

#[derive(Debug, PartialEq)]
enum ShotStep {
    Wait,
    Capture,
    GiveUp,
}

impl ScreenshotTo {
    /// Advances a frame, saying whether to ask for a capture now.
    fn tick(&mut self) -> ShotStep {
        self.frame += 1;
        if self.frame == SETTLE_FRAMES {
            self.attempts += 1;
            ShotStep::Capture
        } else if self.frame >= SETTLE_FRAMES + CAPTURE_PATIENCE {
            self.retry()
        } else {
            ShotStep::Wait
        }
    }

    /// Starts over after a capture that was unusable or never came.
    fn retry(&mut self) -> ShotStep {
        if self.attempts >= MAX_CAPTURES {
            return ShotStep::GiveUp;
        }
        self.frame = 0;
        ShotStep::Wait
    }
}

/// Removes `flag` and the value after it from `args`.
fn take_option(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, ()> {
    match args.iter().position(|a| a == flag) {
        Some(i) if i + 1 < args.len() => {
            let value = args.remove(i + 1);
            args.remove(i);
            Ok(Some(value))
        }
        Some(_) => Err(()),
        None => Ok(None),
    }
}

/// Removes `flag` from `args`, returning whether it was there.
fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != flag);
    args.len() != before
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let modelless = take_flag(&mut args, "--modelless");
    let (Ok(screenshot), Ok(camera)) = (
        take_option(&mut args, "--screenshot"),
        take_option(&mut args, "--camera"),
    ) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let screenshot = screenshot.map(|path| ScreenshotTo {
        path: path.into(),
        frame: 0,
        attempts: 0,
    });
    let camera = match camera.map(|c| game::floats(&c)) {
        None => None,
        Some(Some(view)) => Some(StartView(view)),
        Some(None) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let (dir, map) = match args.as_slice() {
        [dir] => (PathBuf::from(dir), None),
        [dir, map] => (PathBuf::from(dir), Some(map.as_str())),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let loaded = Game::open(&dir).and_then(|game| {
        let Some(map) = map else {
            for name in game.map_names() {
                println!("{name}");
            }
            return Ok(None);
        };
        let name = game
            .find_map(map)
            .ok_or_else(|| format!("no map named {map:?}; run without a map name to list them"))?;
        load(game, &name).map(Some)
    });
    let loaded = match loaded {
        Ok(Some(l)) => l,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("aaa-viewer: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: format!("{} - aaa-viewer", loaded.title),
            ..default()
        }),
        ..default()
    }));
    // The shader has to be registered before the material that loads it.
    embedded_asset!(app, "blend.wesl");
    app.add_plugins(MaterialPlugin::<GroundMaterial>::default())
        .insert_resource(ClearColor(Color::srgb(0.55, 0.7, 0.85)))
        .insert_resource(loaded)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (camera::update, models::animate, objects::toggle_modelless),
        );
    if let Some(c) = camera {
        app.insert_resource(c);
    }
    if modelless {
        app.insert_resource(ShowModelless);
    }
    if let Some(s) = screenshot {
        app.insert_resource(s).add_systems(Update, take_screenshot);
    }
    match app.run() {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

fn give_up(shot: &ScreenshotTo, exit: &mut MessageWriter<AppExit>) {
    error!(
        "no usable frame for {} after {} captures",
        shot.path.display(),
        shot.attempts
    );
    exit.write(AppExit::error());
}

fn take_screenshot(
    mut commands: Commands,
    mut shot: ResMut<ScreenshotTo>,
    mut exit: MessageWriter<AppExit>,
) {
    match shot.tick() {
        ShotStep::Wait => return,
        ShotStep::GiveUp => return give_up(&shot, &mut exit),
        ShotStep::Capture => {}
    }
    commands.spawn(Screenshot::primary_window()).observe(
        |captured: On<ScreenshotCaptured>,
         mut shot: ResMut<ScreenshotTo>,
         mut exit: MessageWriter<AppExit>| {
            // A busy machine can hand back a frame before the window has drawn
            // anything, all zeros, so that one is retried.
            let blank = captured
                .image
                .data
                .as_ref()
                .is_none_or(|d| d.chunks(4).all(|px| px[..3].iter().all(|&c| c == 0)));
            if blank {
                if shot.retry() == ShotStep::GiveUp {
                    give_up(&shot, &mut exit);
                }
                return;
            }
            save_to_disk(shot.path.clone())(captured);
            exit.write(AppExit::Success);
        },
    );
}

fn load(mut game: Game, name: &str) -> Result<Loaded, String> {
    let mut map = game.open_map(name)?;
    let err = |e: tgck::Error| format!("{name}: {e}");
    let hm = map.height_map().map_err(err)?;
    let mountain = map.mountain_height_map().map_err(err)?;
    let terrain = map.terrain().map_err(err)?;
    let things = map.things().map_err(err)?;
    let sun = map.sun().map_err(err)?;
    Ok(Loaded {
        title: game::map_title(name).to_string(),
        ground: Ground::new(&hm, &mountain),
        terrain,
        things,
        sun,
        game,
    })
}

#[allow(clippy::too_many_arguments)] // Bevy systems take their resources as arguments
fn setup(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut ground_materials: ResMut<Assets<GroundMaterial>>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    start: Option<Res<StartView>>,
    screenshot: Option<Res<ScreenshotTo>>,
    modelless: Option<Res<ShowModelless>>,
) {
    let Loaded {
        game,
        ground,
        terrain,
        things,
        sun,
        ..
    } = &mut *loaded;
    let built = terrain::build(game, ground, terrain, &mut images);
    if let Some((mesh, material)) = built.ground {
        commands.spawn((
            Name::new("ground"),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(ground_materials.add(material)),
        ));
    }
    if let Some((mesh, material)) = built.water {
        commands.spawn((
            Name::new("water"),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(material)),
            NotShadowCaster,
        ));
    }
    let mut models = Models::new(game, formats.0);
    let mut assets = ModelAssets {
        meshes: &mut meshes,
        materials: &mut materials,
        images: &mut images,
    };
    let rest = models.spawn(&mut commands, game, &mut assets, things);
    objects::spawn(
        &mut commands,
        &mut meshes,
        &mut materials,
        ground,
        &rest,
        screenshot.is_none(),
        modelless.is_some(),
    );

    let size = (ground.side - 1) as f32 * CELL_SIZE;
    let centre = terrain::world(
        size / 2.0,
        size / 2.0,
        ground.height_at(size / 2.0, size / 2.0),
    );
    let mut cam = RtsCamera::new(centre);
    if let Some(StartView([x, y, distance, yaw, pitch])) = start.as_deref().copied() {
        let (x, y) = (x * size, y * size);
        cam = RtsCamera {
            focus: terrain::world(x, y, ground.height_at(x, y)),
            distance,
            yaw,
            pitch,
            free: true,
        };
    }
    // The game lights like Direct3D's fixed pipeline: texture * (ambient +
    // diffuse * N.L), clamped. Undoing the exposure and Lambert's 1/pi in Bevy's
    // PBR, and skipping tonemapping, gets the same result.
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        cam.projection(),
        cam.transform(),
        cam,
    ));
    let unit = 1.0 / Exposure::default().exposure();
    let rgb = |[r, g, b]: [f32; 3]| Color::srgb(r, g, b);
    let [dx, dy, dz] = sun.direction;
    commands.spawn((
        DirectionalLight {
            color: rgb(sun.diffuse),
            illuminance: std::f32::consts::PI * unit,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default().looking_to(terrain::world(dx, dy, dz), Vec3::Y),
        CascadeShadowConfigBuilder {
            maximum_distance: size * 1.5,
            ..default()
        }
        .build(),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: rgb(sun.ambient),
        brightness: unit,
        ..default()
    });

    if screenshot.is_some() {
        return;
    }
    commands.spawn((
        Text::new(camera::HELP),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            bottom: px(8),
            left: px(8),
            ..default()
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot() -> ScreenshotTo {
        ScreenshotTo {
            path: "out.png".into(),
            frame: 0,
            attempts: 0,
        }
    }

    #[test]
    fn screenshot_captures_once_settled() {
        let mut shot = shot();
        let steps: Vec<_> = (0..SETTLE_FRAMES).map(|_| shot.tick()).collect();
        assert!(
            steps[..steps.len() - 1]
                .iter()
                .all(|s| *s == ShotStep::Wait)
        );
        assert_eq!(steps.last(), Some(&ShotStep::Capture));
    }

    #[test]
    fn screenshot_gives_up_when_captures_never_arrive() {
        let mut shot = shot();
        let mut captures = 0;
        for _ in 0..MAX_CAPTURES * (SETTLE_FRAMES + CAPTURE_PATIENCE) + 1 {
            match shot.tick() {
                ShotStep::Wait => {}
                ShotStep::Capture => captures += 1,
                ShotStep::GiveUp => {
                    assert_eq!(captures, MAX_CAPTURES);
                    return;
                }
            }
        }
        panic!("never gave up");
    }

    #[test]
    fn screenshot_gives_up_on_blank_frames() {
        let mut shot = shot();
        for _ in 1..MAX_CAPTURES {
            while shot.tick() != ShotStep::Capture {}
            assert_eq!(shot.retry(), ShotStep::Wait);
        }
        while shot.tick() != ShotStep::Capture {}
        assert_eq!(shot.retry(), ShotStep::GiveUp);
    }
}
