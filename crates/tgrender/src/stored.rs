//! Drawing in colour values as the game stores and shows them.
//!
//! The game lights, blends and adds colours as they're stored for the
//! screen, never converting them to linear light, so a glow added over a
//! dark wall is brighter than the same sum in linear light, and soft edges
//! blend differently. Cameras with [`StoredColours`] do the same: the world's
//! textures and colours reach their shaders unconverted, every sum and blend
//! happens on those values, and one last step turns the finished picture into
//! linear light for the screen, before the interface is drawn over it as
//! usual.

use bevy::camera::Hdr;
use bevy::core_pipeline::fullscreen_material::FullscreenMaterial;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::prelude::*;
use bevy::render::{RenderApp, extract_component::ExtractComponent, render_resource::ShaderType};
use bevy::shader::ShaderRef;

/// Marks a camera that draws the world in stored colour values.
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default)]
#[extract_app(RenderApp)]
#[require(Hdr)]
pub struct StoredColours {
    // A uniform needs a field; WebGL2 needs 16 bytes of them.
    _unused: Vec4,
}

impl FullscreenMaterial for StoredColours {
    fn fragment_shader() -> ShaderRef {
        "embedded://tgrender/stored.wesl".into()
    }
}

/// A 3D camera that draws the world in stored colour values, and without
/// tonemapping, which [`crate::light::color_lights`] needs.
pub fn world_camera() -> impl Bundle {
    (
        Camera3d::default(),
        Tonemapping::None,
        // Dithering only runs as part of tonemapping.
        DebandDither::Disabled,
        StoredColours::default(),
    )
}

/// `colour` as a world shader takes it: its stored value, unconverted.
pub fn stored(colour: Color) -> Color {
    let [r, g, b, a] = colour.to_srgba().to_f32_array();
    Color::linear_rgba(r, g, b, a)
}

/// A stored colour as a world shader takes it, from its channels.
pub fn rgb([r, g, b]: [f32; 3]) -> Color {
    Color::linear_rgb(r, g, b)
}
