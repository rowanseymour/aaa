//! Lighting the world from a map's sun.

use bevy::camera::Exposure;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use tgck::map::Sun;

use crate::space::in_world;
use crate::stored;

/// Lights a map of `size` across from its sun, whose shadows take away
/// `shade` of its light: two lights, that share casting shadows and the
/// rest not, either left out where its share is nothing.
pub(crate) fn spawn(
    commands: &mut Commands,
    sun: &Sun,
    size: f32,
    shade: f32,
) -> [Option<Entity>; 2] {
    let mut ambient = GlobalAmbientLight::default();
    let facing = Transform::default().looking_to(in_world(sun.direction), Vec3::Y);
    let lights = [(shade, true), (1.0 - shade, false)].map(|(share, shadows)| {
        if share <= 0.0 {
            return None;
        }
        let mut directional = DirectionalLight {
            illuminance: std::f32::consts::PI * light_unit() * share,
            shadow_maps_enabled: shadows,
            ..default()
        };
        color_lights(&mut directional, &mut ambient, sun.diffuse, sun.ambient);
        let cascades = CascadeShadowConfigBuilder {
            maximum_distance: size * 1.5,
            ..default()
        };
        Some(commands.spawn((directional, facing, cascades.build())).id())
    });
    commands.insert_resource(ambient);
    lights
}

/// Light of strength 1 to the camera: the inverse of its exposure.
pub(crate) fn light_unit() -> f32 {
    1.0 / Exposure::default().exposure()
}

/// Gives the sun and ambient lights a sun's `diffuse` and `ambient` colours,
/// keeping each sun light's share of the light.
///
/// The game lights like Direct3D's fixed pipeline: texture * (ambient +
/// diffuse * N.L), clamped. Undoing the exposure and Lambert's 1/pi in Bevy's
/// PBR gets the same result, as long as the camera skips tonemapping.
pub fn color_lights(
    directional: &mut DirectionalLight,
    ambient: &mut GlobalAmbientLight,
    diffuse: [f32; 3],
    ambient_color: [f32; 3],
) {
    directional.color = stored::rgb(diffuse);
    ambient.color = stored::rgb(ambient_color);
    ambient.brightness = light_unit();
}
