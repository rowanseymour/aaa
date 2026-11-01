//! 3D viewer for Axis & Allies (2004) maps, reading straight from the game's
//! installed data files.

mod blend;
mod camera;
mod game;
mod models;
mod objects;
mod terrain;

use std::path::PathBuf;
use std::process::ExitCode;

use bevy::asset::embedded_asset;
use bevy::image::CompressedImageFormatSupport;
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use tgck::map::{CELL_SIZE, Terrain, Thing};

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
  --screenshot <file.png>        save the first frames to a file and exit";

#[derive(Resource)]
struct Loaded {
    game: Game,
    title: String,
    ground: Ground,
    terrain: Terrain,
    things: Vec<Thing>,
}

/// Saves a screenshot once the scene has settled, then exits.
#[derive(Resource)]
struct ScreenshotTo {
    path: PathBuf,
    frame: u32,
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let screenshot = match args.iter().position(|a| a == "--screenshot") {
        Some(i) if i + 1 < args.len() => {
            let path = args.remove(i + 1);
            args.remove(i);
            Some(ScreenshotTo {
                path: path.into(),
                frame: 0,
            })
        }
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        None => None,
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
        .add_systems(Update, camera::update);
    if let Some(s) = screenshot {
        app.insert_resource(s).add_systems(Update, take_screenshot);
    }
    app.run();
    ExitCode::SUCCESS
}

fn take_screenshot(
    mut commands: Commands,
    mut shot: ResMut<ScreenshotTo>,
    mut exit: MessageWriter<AppExit>,
) {
    shot.frame += 1;
    match shot.frame {
        60 => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(shot.path.clone()));
        }
        90 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

fn load(mut game: Game, name: &str) -> Result<Loaded, String> {
    let mut map = game.open_map(name)?;
    let err = |e: tgck::Error| format!("{name}: {e}");
    let hm = map.height_map().map_err(err)?;
    let terrain = map.terrain().map_err(err)?;
    let things = map.things().map_err(err)?;
    let title = name.rsplit('/').next().unwrap_or(name);
    Ok(Loaded {
        title: title[..title.len() - 4].to_string(),
        ground: Ground::new(&hm),
        terrain,
        things,
        game,
    })
}

fn setup(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut ground_materials: ResMut<Assets<GroundMaterial>>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let Loaded {
        game,
        ground,
        terrain,
        things,
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
    let rest = models.spawn(&mut commands, game, &mut assets, ground, things);
    objects::spawn(&mut commands, &mut meshes, &mut materials, ground, &rest);

    let size = (ground.side - 1) as f32 * CELL_SIZE;
    let centre = terrain::world(
        size / 2.0,
        size / 2.0,
        ground.height_at(size / 2.0, size / 2.0),
    );
    let cam = RtsCamera::new(centre);
    commands.spawn((Camera3d::default(), cam.projection(), cam.transform(), cam));

    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(1.0, 2.0, 0.6).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder {
            maximum_distance: size * 1.5,
            ..default()
        }
        .build(),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 400.0,
        ..default()
    });

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
