//! Models that blend by adding to what's behind them, such as ripples and
//! glows, drawn the way the game adds them.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub type GlowMaterial = ExtendedMaterial<StandardMaterial, Glow>;

/// What scales the colour a model adds, as its blend flags name it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    One,
    /// The colour itself, so it adds its square.
    Color,
    Alpha,
}

impl Source {
    /// From an alpha property's source factor, if it's one of these.
    pub fn of(factor: u16) -> Option<Self> {
        match factor {
            0 => Some(Source::One),
            2 => Some(Source::Color),
            6 => Some(Source::Alpha),
            _ => None,
        }
    }
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct Glow {
    /// How much more the material reflects ambient light than its base
    /// colour does, as a factor of it (rgb), and the [`Source`] (w).
    #[uniform(100)]
    pub params: Vec4,
}

impl Glow {
    pub fn new(ambient: [f32; 3], diffuse: [f32; 3], source: Source) -> Self {
        let gain = |i: usize| match diffuse[i] {
            d if d > 0.0 => ambient[i] / d - 1.0,
            _ => 0.0,
        };
        let source = match source {
            Source::One => 0.0,
            Source::Color => 1.0,
            Source::Alpha => 2.0,
        };
        Glow {
            params: Vec4::new(gain(0), gain(1), gain(2), source),
        }
    }
}

impl MaterialExtension for Glow {
    fn fragment_shader() -> ShaderRef {
        "embedded://aaa_render/glow.wesl".into()
    }
}
