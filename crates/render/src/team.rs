//! Models painted in their owner's colour where the model marks a mesh for
//! it: the texture's see-through texels show the colour instead.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub type TeamMaterial = ExtendedMaterial<StandardMaterial, Team>;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct Team {
    /// Linear rgb; w unused.
    #[uniform(100)]
    pub colour: Vec4,
}

impl Team {
    pub fn new([r, g, b]: [f32; 3]) -> Self {
        let c = Color::srgb(r, g, b).to_linear();
        Team {
            colour: Vec4::new(c.red, c.green, c.blue, 1.0),
        }
    }
}

impl MaterialExtension for Team {
    fn fragment_shader() -> ShaderRef {
        "embedded://aaa_render/team.wesl".into()
    }
}

/// Whether a shape is one the owner's colour shows through.
pub(crate) fn marked(nif: &nif::Nif, shape: &nif::SceneObject) -> bool {
    shape.extra_data.iter().any(|&r| {
        nif.get_as::<nif::ExtraData>(r).is_some_and(|e| {
            e.name == "RepColor" && matches!(e.value, nif::ExtraValue::Integer(n) if n != 0)
        })
    })
}
