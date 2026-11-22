//! Our minimap: the shared [view](crate::minimap::Minimap::view) on a disc
//! in the window's lower left corner, with round buttons on its rim.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::displays::Rim;
use crate::minimap::Minimap;

/// Width of the disc on screen, in logical pixels, which the diamond's
/// corners touch.
pub const DISC: f32 = 236.0;
/// Width of the round buttons on the disc's rim.
pub const RIM_BUTTON: f32 = 34.0;
/// Texels across the backdrop the disc and its buttons share.
const BACKDROP_TEXELS: u32 = 512;

/// Spawns our interface's disc, in the window's lower left corner.
pub fn spawn(mut commands: Commands, minimap: Res<Minimap>) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(crate::ui_scale::MARGIN),
            bottom: px(crate::ui_scale::MARGIN),
            width: px(DISC),
            height: px(DISC),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        // Over the banners, which stack by where they are on screen.
        GlobalZIndex(crate::ui::layer::HUD),
        DespawnOnExit(super::DRAWN),
        children![
            (
                Backdrop,
                ImageNode::default(),
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                Pickable::IGNORE,
            ),
            (
                minimap.view(),
                Node {
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
            ),
        ],
    ));
}

/// The disc's backdrop, drawn as one with the buttons on its rim so where
/// they overlap is no darker.
#[derive(Component)]
pub struct Backdrop;

/// Places the round buttons on the rim, once they're all spawned, and
/// draws the backdrop under them.
pub fn lay_out_rim(
    mut buttons: Query<(&Rim, &mut Node)>,
    mut backdrops: Query<&mut ImageNode, With<Backdrop>>,
    mut images: ResMut<Assets<Image>>,
) {
    let rims: Vec<Rim> = buttons.iter().map(|(rim, _)| *rim).collect();
    let corner = crate::ui_scale::MARGIN + DISC / 2.0 - RIM_BUTTON / 2.0;
    for (rim, mut node) in &mut buttons {
        let at = corner + rim.centre(&rims);
        node.left = px(at.x);
        node.bottom = px(at.y);
    }
    let centres: Vec<Vec2> = (rims.iter())
        .map(|r| r.centre(&rims) / (DISC / 2.0))
        .collect();
    let backdrop = images.add(silhouette(BACKDROP_TEXELS, |x, y| {
        backdrop_shape(&centres, Vec2::new(x, -y))
    }));
    for mut image in &mut backdrops {
        image.image = backdrop.clone();
    }
}

/// Whether `point` of the disc's square, y up, is on the disc or on one of
/// the buttons centred at `buttons`.
fn backdrop_shape(buttons: &[Vec2], point: Vec2) -> bool {
    let radius = RIM_BUTTON / DISC;
    point.length_squared() <= 1.0 || buttons.iter().any(|b| b.distance(point) <= radius)
}

/// Fills the backdrop with the colour the interface over the map has.
pub fn tint(theme: Res<crate::ui::Theme>, mut backdrops: Query<&mut ImageNode, With<Backdrop>>) {
    let colour = theme.surface(crate::ui::Surface::Hud);
    for mut backdrop in &mut backdrops {
        if backdrop.color != colour {
            backdrop.color = colour;
        }
    }
}

/// A white shape on a clear ground, `texels` across, to be tinted: where
/// `inside` holds of the square from -1 to 1 each way, y down.
fn silhouette(texels: u32, inside: impl Fn(f32, f32) -> bool) -> Image {
    // Each texel's coverage from a grid of samples across it.
    const SAMPLES: u32 = 4;
    let half = texels as f32 / 2.0;
    let mut data = Vec::with_capacity((texels * texels * 4) as usize);
    for v in 0..texels {
        for u in 0..texels {
            let mut hits = 0;
            for j in 0..SAMPLES {
                for i in 0..SAMPLES {
                    let at = |t: u32, s: u32| {
                        (t as f32 + (s as f32 + 0.5) / SAMPLES as f32 - half) / half
                    };
                    hits += inside(at(u, i), at(v, j)) as u32;
                }
            }
            let alpha = (255 * hits / (SAMPLES * SAMPLES)) as u8;
            data.extend_from_slice(&[255, 255, 255, alpha]);
        }
    }
    Image::new(
        Extent3d {
            width: texels,
            height: texels,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}
