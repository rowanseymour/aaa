//! The water surface's material: its depth colours over an animated texture.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, Water>;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct Water {
    /// Texture repeat in world units (xy), frames per second (z) and the
    /// number of frames (w).
    #[uniform(100)]
    pub animation: Vec4,
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    pub frames: Handle<Image>,
}

impl MaterialExtension for Water {
    fn fragment_shader() -> ShaderRef {
        "embedded://aaa_render/water.wesl".into()
    }
}
