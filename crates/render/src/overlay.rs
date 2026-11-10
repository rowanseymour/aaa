//! What the game draws over objects to show what the cursor is on and what
//! is selected: rings and squares on the ground, white and tinted as they're
//! drawn, health bars that face the screen, and regiments' banners.

use std::collections::HashMap;

use aaa_data::{GameData, Id};
use aaa_map::{Division, Ground, Shape, from_local};
use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use nif::{Nif, Object};
use tgck::map::CELL_SIZE;

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
    /// A building's, the same but longer.
    pub building_health: Vec<Handle<Image>>,
    /// A relay's supply stockpile bar, likewise.
    pub stockpile: Vec<Handle<Image>>,
}

/// The share of the square image's size between each edge and its outline.
pub const SQUARE_INSET: f32 = 34.0 / 256.0;

/// A unit's health bar's width and height in world units; they're drawn
/// that size at the unit, but square to the screen.
pub const HEALTH_BAR: Vec2 = Vec2::new(1.12, 0.16);

/// A building's health bar's, likewise.
pub const BUILDING_HEALTH_BAR: Vec2 = Vec2::new(3.35, 0.16);

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
        let mut bar = |name: &str| -> Vec<Handle<Image>> {
            game.read(name)
                .and_then(|b| Nif::parse(&b).map_err(|e| warn!("{name}: {e}")).ok())
                .map(|nif| frames(&nif, formats))
                .unwrap_or_default()
                .into_iter()
                .map(|i| images.add(i))
                .collect()
        };
        let health = bar("Effects/HealthBar/HealthBar.nif");
        let building_health = bar("Effects/HealthBar/HealthBarMedium.nif");
        let stockpile = bar("Effects/SupplyStockpileBar/SupplyStockpileBar.nif");
        OverlayImages {
            ring,
            square,
            health,
            building_health,
            stockpile,
        }
    }

    /// The health bar for a unit with `share` of its health left.
    pub fn health_bar(&self, share: f32) -> Option<&Handle<Image>> {
        bar_frame(&self.health, share)
    }

    /// The health bar for a building with `share` of its health left.
    pub fn building_health_bar(&self, share: f32) -> Option<&Handle<Image>> {
        bar_frame(&self.building_health, share)
    }

    /// The bar for a relay with `share` of its supply stockpile left.
    pub fn stockpile_bar(&self, share: f32) -> Option<&Handle<Image>> {
        bar_frame(&self.stockpile, share)
    }
}

/// Of a health bar's frames, the one for `share` of health left.
fn bar_frame(frames: &[Handle<Image>], share: f32) -> Option<&Handle<Image>> {
    let last = frames.len().checked_sub(1)?;
    let empty = ((1.0 - share.clamp(0.0, 1.0)) * last as f32).round() as usize;
    // Anything left shows as a sliver, not as nothing.
    let frame = if share > 0.0 {
        empty.min(last.saturating_sub(1))
    } else {
        last
    };
    frames.get(frame)
}

/// The layers of the banners that float over regiments, each a square
/// image drawn the same size, one over another.
#[derive(Resource)]
pub struct BannerImages {
    /// The plate, grey to be tinted in the owner's colour.
    pub plate: Option<Handle<Image>>,
    /// Crossed rifles behind it while the regiment is engaged.
    pub engaged: Option<Handle<Image>>,
    /// Corner brackets around it while the cursor is on the regiment, white
    /// to be tinted.
    pub focus: Option<Handle<Image>>,
    /// The border it wears while the regiment is out of supply.
    pub out_of_supply: Option<Handle<Image>>,
    /// The border that flashes on it while its side can't pay its upkeep.
    pub shortage: Option<Handle<Image>>,
    /// The archive path of each organization template's icon.
    icon_paths: HashMap<Id, String>,
    /// The archive path of the frame round each organization template's
    /// nation's banners.
    frame_paths: HashMap<Id, String>,
    /// The archive paths of the division badges of each organization
    /// template's nation.
    badge_paths: HashMap<Id, Vec<String>>,
    /// Images loaded so far, by path.
    loaded: HashMap<String, Option<Handle<Image>>>,
    formats: CompressedImageFormats,
}

impl BannerImages {
    pub fn load(
        game: &mut GameData,
        images: &mut Assets<Image>,
        formats: CompressedImageFormats,
    ) -> Self {
        let mut add = |name: &str| {
            let image = textures::decode(&game.read(name)?, name, formats, true)?;
            Some(images.add(image))
        };
        let plate = add("Organizations/Banners/Shared/BannerPlayerColor.png");
        let engaged = add("Organizations/Banners/Shared/BannerEngaged1.png");
        let focus = add("Effects/Selection/BannerFocusBase.tga");
        let out_of_supply = add("Organizations/Banners/Shared/BannerOutOfSupply.png");
        let shortage = add("Organizations/Banners/Shared/BannerBleeding.png");
        BannerImages {
            plate,
            engaged,
            focus,
            out_of_supply,
            shortage,
            icon_paths: game.banner_icons(),
            frame_paths: game.banner_frames(),
            badge_paths: game.division_badges(),
            loaded: HashMap::new(),
            formats,
        }
    }

    /// The image on the banner of an organization of `template`, loaded the
    /// first time it's asked for.
    pub fn icon(
        &mut self,
        game: &mut GameData,
        images: &mut Assets<Image>,
        template: &str,
    ) -> Option<Handle<Image>> {
        let path = self.icon_paths.get(&Id::from(template))?.clone();
        self.image(game, images, path)
    }

    /// The frame round the banner of an organization of `template`, its
    /// nation's; loaded the first time it's asked for.
    pub fn frame(
        &mut self,
        game: &mut GameData,
        images: &mut Assets<Image>,
        template: &str,
    ) -> Option<Handle<Image>> {
        let path = self.frame_paths.get(&Id::from(template))?.clone();
        self.image(game, images, path)
    }

    /// The badge an organization in `division` wears below its banner's
    /// plate, drawn the banner's size; loaded the first time it's asked for.
    /// A nation with more divisions than badges goes round them again.
    pub fn badge(
        &mut self,
        game: &mut GameData,
        images: &mut Assets<Image>,
        division: &Division,
    ) -> Option<Handle<Image>> {
        let badges = self
            .badge_paths
            .get(&Id::from(division.template.as_str()))?;
        let path = badges
            .get(division.badge.checked_rem(badges.len())?)?
            .clone();
        self.image(game, images, path)
    }

    fn image(
        &mut self,
        game: &mut GameData,
        images: &mut Assets<Image>,
        path: String,
    ) -> Option<Handle<Image>> {
        let formats = self.formats;
        crate::models::cached(&mut self.loaded, &path, || {
            textures::decode(&game.read(&path)?, &path, formats, true).map(|i| images.add(i))
        })
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

/// A mark like [`mark_transform`]'s but laid over the ground point by point,
/// in world space: across a building's footprint the ground bends too much
/// for one plane to lie on it without floating somewhere.
pub fn draped_square(ground: &Ground, position: [f32; 2], facing: [f32; 2], size: Vec2) -> Mesh {
    let facing = Vec2::from(facing)
        .try_normalize()
        .unwrap_or(Vec2::NEG_Y)
        .to_array();
    // Finer than the ground's cells, so it bends where they do.
    let steps = |len: f32| ((2.0 * len / CELL_SIZE).ceil() as u32).max(1);
    let (nx, ny) = (steps(size.x), steps(size.y));
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    for j in 0..=ny {
        for i in 0..=nx {
            let (u, v) = (i as f32 / nx as f32, j as f32 / ny as f32);
            // The image's top is ahead.
            let local = [(u - 0.5) * size.x, (0.5 - v) * size.y];
            let [x, y] = from_local(position, facing, local);
            positions.push(world(x, y, ground.height_at(x, y) + LIFT).to_array());
            uvs.push([u, v]);
        }
    }
    let at = |i: u32, j: u32| j * (nx + 1) + i;
    let mut indices = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            indices.extend([a, c, b, a, d, c]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// A closed path evened out, each point drawn toward its neighbours over
/// and over, so the steps a few units long that tiles leave in it go and
/// its broad sweep stays.
fn smoothed(path: &[Vec2]) -> Vec<Vec2> {
    let mut path = paced(path, 1.0);
    let n = path.len();
    for _ in 0..12 {
        path = (0..n)
            .map(|i| (path[(i + n - 1) % n] + 2.0 * path[i] + path[(i + 1) % n]) / 4.0)
            .collect();
    }
    path
}

/// Points along a closed path, about `step` apart all the way round.
fn paced(path: &[Vec2], step: f32) -> Vec<Vec2> {
    let next = |i: usize| path[(i + 1) % path.len()];
    let round: f32 = (0..path.len()).map(|i| path[i].distance(next(i))).sum();
    let count = (round / step).round().max(3.0) as usize;
    let step = round / count as f32;
    let mut points = Vec::with_capacity(count);
    // How far round the path the side being walked starts.
    let (mut side, mut before) = (0, 0.0);
    for n in 0..count {
        let along = n as f32 * step;
        while side + 1 < path.len() && before + path[side].distance(next(side)) < along {
            before += path[side].distance(next(side));
            side += 1;
        }
        let length = path[side].distance(next(side)).max(f32::EPSILON);
        points.push(path[side].lerp(next(side), (along - before) / length));
    }
    points
}

/// The lines of a border along `loops`, closed paths in map units with
/// what they bound on their left, laid over the ground in world space: a
/// solid line on each path and a thinner broken one just inside it.
pub fn border(ground: &Ground, loops: &[Vec<[f32; 2]>]) -> Mesh {
    // How far apart the lines' points are, near enough for them to bend
    // with the ground.
    const STEP: f32 = 0.2;
    // Each line's near and far edge in from the path, and how many steps
    // it's drawn for and then left out for.
    const LINES: [(f32, f32, usize, usize); 2] = [(0.0, 0.15, 1, 0), (0.23, 0.32, 3, 1)];
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for path in loops.iter().filter(|l| l.len() > 2) {
        let path: Vec<Vec2> = path.iter().map(|&p| Vec2::from(p)).collect();
        let path = paced(&smoothed(&path), STEP);
        let n = path.len();
        let inward = |i: usize| {
            let along = path[(i + 1) % n] - path[(i + n - 1) % n];
            along.perp().normalize_or_zero()
        };
        let at = |i: usize, inset: f32| {
            let Vec2 { x, y } = path[i] + inward(i) * inset;
            world(x, y, ground.height_at(x, y) + LIFT).to_array()
        };
        for (near, far, on, off) in LINES {
            for i in (0..n).filter(|i| i % (on + off) < on) {
                let j = (i + 1) % n;
                let first = positions.len() as u32;
                positions.extend([at(i, near), at(i, far), at(j, far), at(j, near)]);
                indices.extend([0, 1, 2, 0, 2, 3].map(|k| first + k));
            }
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let uvs = vec![[0.0, 0.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draped_squares_follow_the_ground() {
        // A ridge running across the middle of a 16x16 cell map.
        let side = 17;
        let heights = (0..side * side)
            .map(|i| if i % side == 8 { 5.0 } else { 0.0 })
            .collect();
        let ground = Ground::from_heights(heights);
        let mesh = draped_square(&ground, [16.0, 16.0], [0.0, 1.0], Vec2::splat(12.0));
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        for p in positions {
            let [x, y] = crate::terrain::map_point(Vec3::from_array(*p));
            assert!((p[1] - ground.height_at(x, y) - LIFT).abs() < 1e-4);
        }
        // Its corners are at the size asked for, around the position.
        let xs = positions.iter().map(|p| p[0]);
        assert_eq!(xs.clone().fold(f32::MAX, f32::min), 10.0);
        assert_eq!(xs.fold(f32::MIN, f32::max), 22.0);
    }

    #[test]
    fn borders_run_just_inside_their_paths_on_the_ground() {
        let side = 17;
        let heights = (0..side * side).map(|i| (i % 5) as f32).collect();
        let ground = Ground::from_heights(heights);
        // Round a square, what it bounds on the left.
        let path = vec![[8.0, 8.0], [24.0, 8.0], [24.0, 24.0], [8.0, 24.0]];
        let mesh = border(&ground, &[path]);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        assert!(!positions.is_empty());
        for p in positions {
            let [x, y] = crate::terrain::map_point(Vec3::from_array(*p));
            assert!((p[1] - ground.height_at(x, y) - LIFT).abs() < 1e-4);
            assert!((7.99..24.01).contains(&x) && (7.99..24.01).contains(&y));
        }
        assert!(
            mesh.indices()
                .is_some_and(|i| i.len() == positions.len() / 4 * 6)
        );
    }

    #[test]
    fn health_bars_empty_only_with_no_health_left() {
        let frames = OverlayImages {
            ring: None,
            square: None,
            health: (0..33)
                .map(|i| Handle::Uuid(bevy::asset::uuid::Uuid::from_u128(i), default()))
                .collect(),
            building_health: Vec::new(),
            stockpile: Vec::new(),
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
