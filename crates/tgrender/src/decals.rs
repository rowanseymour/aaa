//! Pictures laid on the ground, following its slopes: scorch marks and the
//! ground worn or charred under things.

use std::collections::HashMap;

use bevy::light::NotShadowCaster;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use tgdata::{Decal, GameData};
use tgmap::Ground;

use crate::terrain::{map_point, world};
use crate::{MapAssets, overlay, textures};

/// How much higher each layer of decals lies than the one below, so that
/// it's drawn over it.
const LAYER_LIFT: f32 = 0.01;

/// How far behind everything else that blends decals are sorted, so that
/// they're drawn first, as part of the ground: farther than any distance
/// within a map.
const UNDER_ALL: f32 = -100_000.0;

/// A decal's material: drawn like a standard one, but lifted over the
/// ground by its own depth offset rather than the one its sorting takes.
pub type DecalMaterial = ExtendedMaterial<StandardMaterial, Lifted>;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bind_group_data(Lift)]
pub struct Lifted {
    /// The depth offset, in the graphics card's units.
    lift: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Lift(i32);

impl From<&Lifted> for Lift {
    fn from(l: &Lifted) -> Self {
        Lift(l.lift)
    }
}

impl MaterialExtension for Lifted {
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.bias.constant = key.bind_group_data.0;
        }
        Ok(())
    }
}

/// One of the pictures laid on the ground.
#[derive(Component)]
#[require(Mesh3d, NotShadowCaster, Pickable = Pickable::IGNORE)]
pub struct Laid;

/// What's been read and made for the decals laid so far.
#[derive(Default)]
pub struct DecalCache {
    /// Pictures, by archive path; `None` for one that can't be read.
    pictures: HashMap<String, Option<Handle<Image>>>,
    /// The material of each kind of decal that never fades, which those
    /// of its kind share.
    materials: HashMap<String, Handle<DecalMaterial>>,
}

/// What a decal's picture is multiplied by, shown as much as `opacity`.
pub fn decal_colour(decal: &Decal, opacity: f32) -> Color {
    let [r, g, b] = decal.colour;
    Color::linear_rgba(r, g, b, opacity)
}

fn material(decal: &Decal, image: Handle<Image>, opacity: f32) -> DecalMaterial {
    // Over the ground, each layer over the one below.
    let lift = 10 * (i32::from(decal.layer) + 1);
    let base = StandardMaterial {
        base_color: decal_colour(decal, opacity),
        base_color_texture: Some(image),
        alpha_mode: match decal.additive {
            true => AlphaMode::Add,
            false => AlphaMode::Blend,
        },
        unlit: !decal.lit,
        // Lit as bare ground is, without a shine.
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        double_sided: true,
        cull_mode: None,
        // Drawn before any model's blending shapes, as things on the
        // ground are drawn over it, and each layer after the one below.
        depth_bias: UNDER_ALL + lift as f32,
        ..default()
    };
    ExtendedMaterial {
        base,
        extension: Lifted { lift },
    }
}

/// Where on the map a decal `placed` lies, and which way the top of its
/// picture points: where the effect placing it faces.
fn laid_at(placed: &Transform) -> ([f32; 2], [f32; 2]) {
    (map_point(placed.translation), map_point(*placed.forward()))
}

impl MapAssets<'_> {
    /// What draws `decal` laid where `placed` puts it, with its picture's
    /// top toward where it faces, shown as much as `opacity`; `None` where
    /// its picture can't be read.
    pub fn decal(
        &mut self,
        game: &GameData,
        cache: &mut DecalCache,
        ground: &Ground,
        decal: &Decal,
        placed: &Transform,
        opacity: f32,
    ) -> Option<impl Bundle + use<>> {
        let (position, facing) = laid_at(placed);
        let image = match cache.pictures.get(&decal.texture) {
            Some(known) => known.clone(),
            None => {
                let bytes = game.read(&decal.texture);
                let read = bytes
                    .and_then(|b| textures::decode(&b, &decal.texture, self.formats.0, false))
                    .map(|i| self.images.add(i));
                cache.pictures.insert(decal.texture.clone(), read.clone());
                read
            }
        }?;
        // One that fades changes its own material as it does.
        let material = match decal.fade == [0.0; 2] {
            true => {
                let kind = format!("{decal:?}");
                let shared = cache.materials.entry(kind);
                shared
                    .or_insert_with(|| self.decal_materials.add(material(decal, image, 1.0)))
                    .clone()
            }
            false => self.decal_materials.add(material(decal, image, opacity)),
        };
        let [x, y] = position;
        let lift = LAYER_LIFT * f32::from(decal.layer);
        let origin = world(x, y, ground.height_at([x, y]) + lift);
        let size = Vec2::splat(decal.size);
        let mesh =
            overlay::draped_square_about(ground, position, facing, size, origin - lift * Vec3::Y);
        Some((
            Laid,
            Mesh3d(self.meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::from_translation(origin),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::world;

    #[test]
    fn decals_sort_under_all_else_yet_lie_over_the_ground_by_layer() {
        let laid = |layer| {
            let decal = Decal {
                texture: String::new(),
                size: 1.0,
                colour: [1.0; 3],
                lit: false,
                additive: false,
                fade: [0.0; 2],
                layer,
            };
            material(&decal, Handle::default(), 1.0)
        };
        let [low, high] = [laid(0), laid(1)];
        // Behind any blending model shape, lower layers first.
        assert!(low.base.depth_bias < -10_000.0);
        assert!(low.base.depth_bias < high.base.depth_bias);
        // Yet lifted over the ground, higher layers more.
        assert!(0 < low.extension.lift && low.extension.lift < high.extension.lift);
    }

    #[test]
    fn decals_lie_under_their_effect_and_point_the_way_it_faces() {
        let east = world(1.0, 0.0, 0.0);
        let placed = Transform::from_translation(world(3.0, 4.0, 2.0)).looking_to(east, Vec3::Y);
        let (position, facing) = laid_at(&placed);
        assert_eq!(position, [3.0, 4.0]);
        assert!(Vec2::from(facing).abs_diff_eq(Vec2::X, 1e-6), "{facing:?}");
    }
}
