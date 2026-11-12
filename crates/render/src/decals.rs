//! Pictures laid on the ground, following its slopes: scorch marks and the
//! ground worn or charred under things.

use std::collections::HashMap;

use aaa_data::{Decal, GameData};
use aaa_map::Ground;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::{MapAssets, overlay, textures};

/// One of the pictures laid on the ground.
#[derive(Component)]
#[require(Mesh3d, NotShadowCaster, Pickable = Pickable::IGNORE)]
pub struct Laid;

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
    /// can't be read. `pictures` keeps those read, by archive path.
    #[allow(clippy::too_many_arguments)]
    pub fn decal(
        &mut self,
        game: &mut GameData,
        pictures: &mut HashMap<String, Option<Handle<Image>>>,
        ground: &Ground,
        decal: &Decal,
        position: [f32; 2],
        facing: [f32; 2],
        opacity: f32,
    ) -> Option<impl Bundle + use<>> {
        let image = match pictures.get(&decal.texture) {
            Some(known) => known.clone(),
            None => {
                let bytes = game.read(&decal.texture);
                let read = bytes
                    .and_then(|b| textures::decode(&b, &decal.texture, self.formats.0, true))
                    .map(|i| self.images.add(i));
                pictures.insert(decal.texture.clone(), read.clone());
                read
            }
        }?;
        let mesh = overlay::draped_square(ground, position, facing, Vec2::splat(decal.size));
        Some((
            Laid,
            Mesh3d(self.meshes.add(mesh)),
            MeshMaterial3d(self.materials.add(material(decal, image, opacity))),
        ))
    }
}
