//! Models that blend by adding to what's behind them, such as ripples and
//! glows, drawn the way the game adds them.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use nif::BlendFactor;

pub type GlowMaterial = ExtendedMaterial<StandardMaterial, Glow>;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct Glow {
    /// How much more the material reflects ambient light than its base
    /// colour does, as a factor of it (rgb), and what scales the colour it
    /// adds (w), as the shader numbers it.
    #[uniform(100)]
    pub params: Vec4,
}

impl Glow {
    /// `None` for a source factor the shader doesn't draw.
    pub fn new(ambient: [f32; 3], diffuse: [f32; 3], source: BlendFactor) -> Option<Self> {
        let gain = |i: usize| match diffuse[i] {
            d if d > 0.0 => ambient[i] / d - 1.0,
            _ => 0.0,
        };
        let source = match source {
            BlendFactor::One => 0.0,
            BlendFactor::Color => 1.0,
            BlendFactor::Alpha => 2.0,
            BlendFactor::Other(_) => return None,
        };
        Some(Glow {
            params: Vec4::new(gain(0), gain(1), gain(2), source),
        })
    }
}

impl MaterialExtension for Glow {
    fn fragment_shader() -> ShaderRef {
        "embedded://aaa_render/glow.wesl".into()
    }
}
