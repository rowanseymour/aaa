//! What the game draws over objects to show what the cursor is on and what
//! is selected: rings and squares on the ground, white and tinted as they're
//! drawn, and health bars that face the screen.

use aaa_data::GameData;
use aaa_map::{Ground, Shape};
use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use nif::{Nif, Object};

use crate::models::embedded_image;
use crate::terrain::world;
use crate::textures;

/// How far over the ground a mark floats, so the ground doesn't cut it.
const LIFT: f32 = 0.05;

/// The images the overlays are drawn with.
#[derive(Resource, Clone)]
pub struct OverlayImages {
    /// A thick ring that fills its square, edge to edge.
    pub ring: Option<Handle<Image>>,
    /// A square outline, broken mid-side, inset from its edges by
    /// [`SQUARE_INSET`] of its size.
    pub square: Option<Handle<Image>>,
    /// A unit's health bar, full first and empty last.
    pub health: Vec<Handle<Image>>,
}

/// The share of the square image's size between each edge and its outline.
pub const SQUARE_INSET: f32 = 34.0 / 256.0;

/// A unit's health bar's width and height in world units; they're drawn
/// that size at the unit, but square to the screen.
pub const HEALTH_BAR: Vec2 = Vec2::new(1.12, 0.16);

impl OverlayImages {
    pub fn load(
        game: &mut GameData,
        images: &mut Assets<Image>,
        formats: CompressedImageFormats,
    ) -> Self {
        let mut texture = |name: &str| {
            let image = textures::decode(&game.read(name)?, name, formats, true)?;
            Some(images.add(image))
        };
        let ring = texture("Effects/Selection/FocusedCircle.dds");
        let square = texture("Effects/Selection/FocusedSquare.dds");
        const BAR: &str = "Effects/HealthBar/HealthBar.nif";
        let health = game
            .read(BAR)
            .and_then(|b| Nif::parse(&b).map_err(|e| warn!("{BAR}: {e}")).ok())
            .map(|nif| frames(&nif, formats))
            .unwrap_or_default()
            .into_iter()
            .map(|i| images.add(i))
            .collect();
        OverlayImages {
            ring,
            square,
            health,
        }
    }

    /// The health bar for a unit with `share` of its health left.
    pub fn health_bar(&self, share: f32) -> Option<&Handle<Image>> {
        let last = self.health.len().checked_sub(1)?;
        let empty = ((1.0 - share.clamp(0.0, 1.0)) * last as f32).round() as usize;
        // Anything left shows as a sliver, not as nothing.
        let frame = if share > 0.0 {
            empty.min(last.saturating_sub(1))
        } else {
            last
        };
        self.health.get(frame)
    }
}

/// A flip book's frames, in the order the model holds them.
fn frames(nif: &Nif, formats: CompressedImageFormats) -> Vec<Image> {
    nif.objects
        .iter()
        .filter_map(|o| match o {
            Object::Pixels(p) => embedded_image(p, formats),
            _ => None,
        })
        .map(|mut i| {
            i.sampler = ImageSampler::linear();
            i
        })
        .collect()
}

/// A square of side 1 lying flat, facing up, its image's top ahead (-z).
pub fn flat_square() -> Mesh {
    let positions = vec![
        [-0.5, 0.0, -0.5],
        [0.5, 0.0, -0.5],
        [0.5, 0.0, 0.5],
        [-0.5, 0.0, 0.5],
    ];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    )
    .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]))
}

/// An unlit, see-through material showing `image` in `colour`.
pub fn mark_material(image: Option<Handle<Image>>, colour: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: colour,
        base_color_texture: image,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        depth_bias: 100.0,
        ..default()
    }
}

/// A mark on the ground: one of the overlay's flat squares.
#[derive(Component)]
#[require(Mesh3d, NotShadowCaster, Pickable = Pickable::IGNORE)]
pub struct Mark;

/// Where a mark `size` across (width, length) at `position` facing
/// `facing` lies: on the ground under it, tilted with it.
pub fn mark_transform(
    ground: &Ground,
    position: [f32; 2],
    facing: [f32; 2],
    size: Vec2,
) -> Transform {
    let facing = Vec2::from(facing).try_normalize().unwrap_or(Vec2::NEG_Y);
    let footprint = Shape::Rect {
        width: size.x,
        height: size.y,
    };
    let tilt = ground.tilt(position, facing.to_array(), footprint);
    let axis = |[x, y, z]: [f32; 3]| world(x, y, z);
    let up = axis(tilt.up).normalize();
    // The square's -z, its image's top, points ahead.
    let ahead = axis(tilt.forward);
    let right = ahead.cross(up).normalize();
    let back = right.cross(up);
    let [x, y] = position;
    Transform {
        translation: world(x, y, tilt.height) + up * LIFT,
        rotation: Quat::from_mat3(&Mat3::from_cols(right, up, back)),
        scale: Vec3::new(size.x, 1.0, size.y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_bars_empty_only_with_no_health_left() {
        let frames = OverlayImages {
            ring: None,
            square: None,
            health: (0..33)
                .map(|i| Handle::Uuid(bevy::asset::uuid::Uuid::from_u128(i), default()))
                .collect(),
        };
        let frame = |share| {
            frames
                .health
                .iter()
                .position(|h| Some(h) == frames.health_bar(share))
        };
        assert_eq!(frame(1.0), Some(0));
        assert_eq!(frame(0.5), Some(16));
        assert_eq!(frame(0.001), Some(31));
        assert_eq!(frame(0.0), Some(32));
    }
}
