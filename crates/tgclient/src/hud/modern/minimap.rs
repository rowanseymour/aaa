//! Our minimap: the shared [view](crate::minimap::Minimap::view) on a disc
//! in the window's lower left corner, with round buttons on its rim.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::minimap::Minimap;

/// Width of the disc on screen, in logical pixels, which the diamond's
/// corners touch.
pub const DISC: f32 = 236.0;
/// Width of the round buttons on the disc's rim.
pub const RIM_BUTTON: f32 = 34.0;
/// How far the buttons' centres are from the disc's, across and up: on the
/// rim, midway between the diamond's corners.
pub const BOLT: f32 = DISC / 2.0 * std::f32::consts::FRAC_1_SQRT_2;
/// Texels across the backdrop the disc and its buttons share.
const BACKDROP_TEXELS: u32 = 512;

/// Spawns our interface's disc, in the window's lower left corner.
pub fn spawn(mut commands: Commands, minimap: Res<Minimap>, mut images: ResMut<Assets<Image>>) {
    let backdrop = images.add(silhouette(BACKDROP_TEXELS, backdrop_shape));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(crate::MARGIN),
            bottom: px(crate::MARGIN),
            width: px(DISC),
            height: px(DISC),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        // Over the banners, which stack by where they are on screen.
        GlobalZIndex(1),
        children![
            (
                Backdrop,
                ImageNode::new(backdrop),
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

/// Whether a point of the disc's square is on the disc or one of the
/// buttons on its rim.
fn backdrop_shape(x: f32, y: f32) -> bool {
    let (bolt, radius) = (BOLT / (DISC / 2.0), RIM_BUTTON / DISC);
    let (bx, by) = (x.abs() - bolt, y.abs() - bolt);
    x * x + y * y <= 1.0 || bx * bx + by * by <= radius * radius
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
