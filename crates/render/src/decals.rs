//! Pictures laid on the ground, following its slopes: scorch marks and the
//! ground worn or charred under things.

use std::collections::HashMap;

use aaa_data::{Decal, GameData};
use aaa_map::Ground;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::terrain::world;
use crate::{MapAssets, overlay, textures};

/// How much higher each layer of decals lies than the one below, so that
/// it's drawn over it.
const LAYER_LIFT: f32 = 0.01;

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
    materials: HashMap<String, Handle<StandardMaterial>>,
}

/// What a decal's picture is multiplied by, shown as much as `opacity`.
pub fn decal_colour(decal: &Decal, opacity: f32) -> Color {
    let [r, g, b] = decal.colour;
    Color::srgba(r, g, b, opacity)
}

fn material(decal: &Decal, image: Handle<Image>, opacity: f32) -> StandardMaterial {
    StandardMaterial {
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
        // Over the ground, each layer over the one below.
        depth_bias: 10.0 * f32::from(decal.layer + 1),
        ..default()
    }
}

impl MapAssets<'_> {
    /// What draws `decal` laid at `position` with its picture's top toward
    /// `facing`, shown as much as `opacity`; `None` where its picture
    /// can't be read. It's placed where it lies, so that it's drawn in
    /// its turn among what's blended.
    #[allow(clippy::too_many_arguments)]
    pub fn decal(
        &mut self,
        game: &mut GameData,
        cache: &mut DecalCache,
        ground: &Ground,
        decal: &Decal,
        position: [f32; 2],
        facing: [f32; 2],
        opacity: f32,
    ) -> Option<impl Bundle + use<>> {
        let image = match cache.pictures.get(&decal.texture) {
            Some(known) => known.clone(),
            None => {
                let bytes = game.read(&decal.texture);
                let read = bytes
                    .and_then(|b| textures::decode(&b, &decal.texture, self.formats.0, true))
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
                    .or_insert_with(|| self.materials.add(material(decal, image, 1.0)))
                    .clone()
            }
            false => self.materials.add(material(decal, image, opacity)),
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
