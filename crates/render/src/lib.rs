//! Draws Axis & Allies (2004) maps with Bevy: the ground and water, placed
//! objects with the game's models, and an RTS camera to look at them with.

pub mod blend;
pub mod camera;
pub mod models;
pub mod objects;
mod rig;
pub mod terrain;
mod textures;

pub use rig::Playing;

use aaa_data::GameData;
use aaa_map::Map;
use bevy::asset::embedded_asset;
use bevy::camera::Exposure;
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use tgck::map::Sun;

use blend::GroundMaterial;
use models::{ModelAssets, Models};
use tgck::map::Thing;

/// What drawing a map needs: the ground material, model animation, the sky and
/// the camera's controls.
pub struct MapRenderPlugin;

impl Plugin for MapRenderPlugin {
    fn build(&self, app: &mut App) {
        // The shader has to be registered before the material that loads it.
        embedded_asset!(app, "blend.wesl");
        app.add_plugins(MaterialPlugin::<GroundMaterial>::default())
            .insert_resource(ClearColor(Color::srgb(0.55, 0.7, 0.85)))
            .add_systems(Update, (models::animate, rig::play, camera::update));
    }
}

/// The assets [`spawn_map`] adds to.
#[derive(SystemParam)]
pub struct MapAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    ground_materials: ResMut<'w, Assets<GroundMaterial>>,
    images: ResMut<'w, Assets<Image>>,
    skins: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
    formats: Res<'w, CompressedImageFormatSupport>,
}

impl MapAssets<'_> {
    /// The stores [`Models`] add to.
    pub fn models(&mut self) -> ModelAssets<'_> {
        ModelAssets {
            meshes: &mut self.meshes,
            materials: &mut self.materials,
            images: &mut self.images,
            skins: &mut self.skins,
        }
    }
}

/// Spawns a map's ground, water, objects and lights, but no camera, with
/// placeholder markers for objects that can't be drawn shown if `markers`.
/// Objects `leave_out` picks are left for the caller to spawn, with the
/// [`Models`] returned.
pub fn spawn_map(
    commands: &mut Commands,
    assets: &mut MapAssets,
    game: &mut GameData,
    map: &Map,
    markers: bool,
    leave_out: impl Fn(&Thing) -> bool,
) -> Models {
    let MapAssets {
        meshes,
        materials,
        ground_materials,
        images,
        skins,
        formats,
    } = assets;
    let built = terrain::build(game, &map.ground, &map.terrain, images);
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
    let mut model_assets = ModelAssets {
        meshes,
        materials,
        images,
        skins,
    };
    let rest = models.spawn(
        commands,
        game,
        &mut model_assets,
        map.things.iter().filter(|t| !leave_out(t)),
        map.world_version,
    );
    objects::spawn(commands, meshes, materials, &map.ground, &rest, markers);
    light(commands, &map.sun, map.ground.size());
    models
}

/// Lights a map of `size` across from its sun.
///
/// The game lights like Direct3D's fixed pipeline: texture * (ambient +
/// diffuse * N.L), clamped. Undoing the exposure and Lambert's 1/pi in Bevy's
/// PBR gets the same result, as long as the camera skips tonemapping.
fn light(commands: &mut Commands, sun: &Sun, size: f32) {
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
}
