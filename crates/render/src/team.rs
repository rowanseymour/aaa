//! Models painted in their owner's colour where the model marks a mesh for
//! it: the texture's see-through texels show the colour instead.

use std::collections::HashMap;

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::models::Animated;

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

/// Draws the meshes of the model under the entity this is on in their own
/// colours multiplied by this one, for as long as it's there.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct Tint(pub [f32; 3]);

/// On a mesh drawn tinted: the tint, the material it's drawn in without
/// it, and the copy it's drawn in with it.
#[derive(Component)]
pub(crate) struct Tinted<M: Material>(Tint, Handle<M>, Handle<M>);

/// The tinted copies made of each material, by its id and the tint's bits,
/// kept while anything is tinted.
#[derive(Resource)]
pub(crate) struct TintedCopies<M: Material>(HashMap<(AssetId<M>, [u32; 3]), Handle<M>>);

impl<M: Material> Default for TintedCopies<M> {
    fn default() -> Self {
        TintedCopies(HashMap::new())
    }
}

/// A material whose colour a [`Tint`] can multiply.
pub(crate) trait Tintable: Material + Clone {
    fn colour(&mut self) -> &mut Color;
}

impl Tintable for StandardMaterial {
    fn colour(&mut self) -> &mut Color {
        &mut self.base_color
    }
}

impl Tintable for TeamMaterial {
    fn colour(&mut self) -> &mut Color {
        &mut self.base.base_color
    }
}

/// Swaps the materials of meshes under a [`Tint`] for tinted copies, and
/// back once it's gone. Meshes whose materials are animated keep them, as
/// a copy would stand still.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn tint<M: Tintable>(
    mut commands: Commands,
    roots: Query<(Entity, &Tint)>,
    children: Query<&Children>,
    plain: Query<&MeshMaterial3d<M>, (Without<Tinted<M>>, Without<Animated>)>,
    drawn: Query<(Entity, &Tinted<M>, &MeshMaterial3d<M>)>,
    parents: Query<&ChildOf>,
    mut materials: ResMut<Assets<M>>,
    mut copies: ResMut<TintedCopies<M>>,
) {
    if roots.is_empty() && drawn.is_empty() {
        copies.0.clear();
        return;
    }
    for (root, &tint) in &roots {
        for e in children.iter_descendants(root) {
            let Ok(material) = plain.get(e) else {
                continue;
            };
            let key = (material.0.id(), tint.0.map(f32::to_bits));
            let copy = copies.0.get(&key).cloned().or_else(|| {
                let mut copy = materials.get(&material.0)?.clone();
                let [r, g, b] = tint.0;
                let c = copy.colour().to_linear();
                *copy.colour() =
                    LinearRgba::new(c.red * r, c.green * g, c.blue * b, c.alpha).into();
                let copy = materials.add(copy);
                copies.0.insert(key, copy.clone());
                Some(copy)
            });
            if let Some(copy) = copy {
                let tinted = Tinted(tint, material.0.clone(), copy.clone());
                commands.entity(e).insert((MeshMaterial3d(copy), tinted));
            }
        }
    }
    for (e, Tinted(tint, plain, copy), now) in &drawn {
        // Given another material since, it keeps that one, to be tinted
        // afresh if it's still to be.
        if now.0 != *copy {
            commands.entity(e).remove::<Tinted<M>>();
            continue;
        }
        let still = parents
            .iter_ancestors(e)
            .any(|a| roots.get(a).is_ok_and(|(_, t)| t == tint));
        if !still {
            let plain = MeshMaterial3d(plain.clone());
            commands.entity(e).insert(plain).remove::<Tinted<M>>();
        }
    }
}
