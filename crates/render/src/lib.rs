//! Draws Axis & Allies (2004) maps with Bevy: the ground and water, placed
//! objects with the game's models, and an RTS camera to look at them with.

pub mod blend;
pub mod camera;
pub mod models;
pub mod objects;
pub mod terrain;
mod textures;

use bevy::asset::embedded_asset;
use bevy::camera::Exposure;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use tgck::map::Sun;

use blend::GroundMaterial;

/// What drawing a map needs: the ground material and model animation.
pub struct MapRenderPlugin;

impl Plugin for MapRenderPlugin {
    fn build(&self, app: &mut App) {
        // The shader has to be registered before the material that loads it.
        embedded_asset!(app, "blend.wesl");
        app.add_plugins(MaterialPlugin::<GroundMaterial>::default())
            .add_systems(Update, models::animate);
    }
}

/// Lights a map of `size` across from its sun.
///
/// The game lights like Direct3D's fixed pipeline: texture * (ambient +
/// diffuse * N.L), clamped. Undoing the exposure and Lambert's 1/pi in Bevy's
/// PBR gets the same result, as long as the camera skips tonemapping.
pub fn light(commands: &mut Commands, sun: &Sun, size: f32) {
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
