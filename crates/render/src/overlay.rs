//! What the game draws over objects to show what the cursor is on and what
//! is selected: rings and squares on the ground, white and tinted as they're
//! drawn, health bars that face the screen, and regiments' banners.

use std::collections::HashMap;

use aaa_data::{GameData, Id};
use aaa_map::{Division, Ground, Shape, from_local};
use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
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
    /// The frame for a template of no nation: a nation's, which all share
    /// a shape, with its colour taken out.
    plain_frame: Option<Handle<Image>>,
    /// The frames and division badges of each nation, and each
    /// organization template's nation.
    nations: aaa_data::NationBanners,
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
        let nations = game.nation_banners();
        let plain_frame = nations.frames.values().min().and_then(|path| {
            let image = textures::decode(&game.read(path)?, path, formats, true)?;
            Some(images.add(whitened(image)))
        });
        BannerImages {
            plate,
            engaged,
            focus,
            out_of_supply,
            shortage,
            icon_paths: game.banner_icons(),
            plain_frame,
            nations,
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

    /// The id of the nation an organization of `template` is of.
    fn nation_of(&self, template: &str) -> Option<String> {
        let nation = self.nations.of_template.get(&Id::from(template))?;
        Some(nation.to_string())
    }

    /// The frame round the banner of an organization of `template`, its
    /// nation's; loaded the first time it's asked for.
    pub fn frame(
        &mut self,
        game: &mut GameData,
        images: &mut Assets<Image>,
        template: &str,
    ) -> Option<Handle<Image>> {
        match self.nation_of(template) {
            Some(nation) => self.nation_frame(game, images, &nation),
            None => self.plain_frame.clone(),
        }
    }

    /// The frame round the banners of `nation`, by its id, for what flies
    /// one that isn't its template's; loaded the first time it's asked for.
    pub fn nation_frame(
        &mut self,
        game: &mut GameData,
        images: &mut Assets<Image>,
        nation: &str,
    ) -> Option<Handle<Image>> {
        let Some(path) = self.nations.frames.get(&Id::from(nation)).cloned() else {
            return self.plain_frame.clone();
        };
        self.image(game, images, path)
    }

    /// How many badges the nation of an organization of `template` has.
    pub fn badges(&self, template: &str) -> usize {
        let nation = self.nation_of(template);
        nation.map_or(0, |n| self.nation_badges(&n))
    }

    /// How many badges `nation` has, by its id.
    pub fn nation_badges(&self, nation: &str) -> usize {
        let badges = self.nations.badges.get(&Id::from(nation));
        badges.map_or(0, Vec::len)
    }

    /// The `badge`th of `nation`'s badges, by its id; loaded the first time
    /// it's asked for.
    pub fn nation_badge(
        &mut self,
        game: &mut GameData,
        images: &mut Assets<Image>,
        nation: &str,
        badge: usize,
    ) -> Option<Handle<Image>> {
        let badges = self.nations.badges.get(&Id::from(nation))?;
        let path = badges.get(badge)?.clone();
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
        let nation = self.nation_of(&division.template)?;
        let count = self.nation_badges(&nation);
        let badge = division.badge.checked_rem(count)?;
        self.nation_badge(game, images, &nation, badge)
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

/// An image with every coloured texel turned white, black and see-through
/// ones left be.
fn whitened(mut image: Image) -> Image {
    if let Some(data) = image.data.as_mut().filter(|_| {
        matches!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm
        )
    }) {
        for texel in data.as_chunks_mut::<4>().0 {
            if texel[3] > 0 && texel[..3] != [0, 0, 0] {
                texel[..3].fill(255);
            }
        }
    }
    image
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
    draped_square_about(ground, position, facing, size, Vec3::ZERO)
}

/// [`draped_square`] with its points given from `origin`, for something
/// placed there.
pub fn draped_square_about(
    ground: &Ground,
    position: [f32; 2],
    facing: [f32; 2],
    size: Vec2,
    origin: Vec3,
) -> Mesh {
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
            let point = world(x, y, ground.height_at(x, y) + LIFT) - origin;
            positions.push(point.to_array());
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

/// How many of an outline's sides one guide point stands for, at most.
const RUN: usize = 4;

/// The guide points a border's curve is drawn by: the corners of a closed
/// outline taken in runs from its start, each of up to [`RUN`] sides and
/// ending before a side that turns a right angle or more from its first,
/// with a guide point at the average of each run's corners. A run starts
/// on the corner the one before ended on. Straight edges give a guide
/// point every few tiles and a staircase one a side.
fn guides(corners: &[Vec2]) -> Vec<Vec2> {
    let n = corners.len();
    let at = |i: usize| corners[i % n];
    let mut guides = Vec::new();
    let mut from = 0;
    while from < n {
        let heading = at(from + 1) - at(from);
        let ahead = |to: usize| (at(to + 1) - at(to)).dot(heading) > 0.0;
        let mut to = from + 1;
        while to < n && to - from < RUN && ahead(to) {
            to += 1;
        }
        let run = (from..=to).map(at);
        guides.push(run.sum::<Vec2>() / (to - from + 1) as f32);
        from = to;
    }
    guides
}

/// The closed uniform cubic B-spline with `guides` as its control points,
/// as the straight pieces it's drawn in: for each guide point, the
/// curve's joint by it and the point halfway along to the next joint.
fn spline(guides: &[Vec2]) -> Vec<Vec2> {
    let n = guides.len();
    let g = |i: usize| guides[i % n];
    (0..n)
        .flat_map(|i| {
            let (before, at, next, after) = (g(i + n - 1), g(i), g(i + 1), g(i + 2));
            let joint = (before + 4.0 * at + next) / 6.0;
            let halfway = (before + 23.0 * at + 23.0 * next + after) / 48.0;
            [joint, halfway]
        })
        .collect()
}

/// The line a border along the closed outline `corners` is drawn as.
fn curve(corners: &[[f32; 2]]) -> Vec<Vec2> {
    let corners: Vec<Vec2> = corners.iter().map(|&p| Vec2::from(p)).collect();
    spline(&guides(&corners))
}

/// Points along a closed path, about `step` apart all the way round, for
/// what's drawn along it to bend with the ground between its own points.
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

/// The paths a border along `loops` follows, in map units: the straight
/// pieces of each loop's curve, as [`border`] draws it.
pub fn border_paths(loops: &[Vec<[f32; 2]>]) -> Vec<Vec<[f32; 2]>> {
    let long = loops.iter().filter(|l| l.len() > 2);
    long.map(|path| curve(path).iter().map(Vec2::to_array).collect())
        .collect()
}

/// The image a border is drawn with, sampled over and over along it.
pub fn border_image(
    game: &mut GameData,
    path: &str,
    formats: CompressedImageFormats,
) -> Option<Image> {
    let mut image = textures::decode(&game.read(path)?, path, formats, true)?;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    Some(image)
}

/// A border along `loops`, closed outlines in map units, a tile's side to
/// a step, with what they bound on their left: a strip `width` across laid
/// over the ground in world space, each outline's curve down its middle. Its image runs along it, top to
/// bottom once for every `width` of the way, its left edge outermost.
pub fn border(ground: &Ground, loops: &[Vec<[f32; 2]>], width: f32) -> Mesh {
    // How far apart the strip's points are along it, near enough for it
    // to bend with the ground, and how many it has across.
    const STEP: f32 = 0.2;
    const ACROSS: u32 = 3;
    // How near the map's edge a path counts as running along it.
    const EDGE: f32 = CELL_SIZE / 2.0;
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for path in loops.iter().filter(|l| l.len() > 2) {
        let path = paced(&curve(path), STEP);
        let n = path.len();
        let inward = |i: usize| {
            let along = path[(i + 1) % n] - path[(i + n - 1) % n];
            along.perp().normalize_or_zero()
        };
        // Where a zone runs off the map there's no edge of it to mark.
        let on_map = |i: usize| {
            let inside = EDGE..=ground.size() - EDGE;
            inside.contains(&path[i].x) && inside.contains(&path[i].y)
        };
        // A whole number of times round, so the image meets itself.
        let repeats = (n as f32 * STEP / width.max(STEP)).round().max(1.0);
        let first = positions.len() as u32;
        // The first point again at the end, with the image's last row.
        for i in 0..=n {
            for a in 0..ACROSS {
                let u = a as f32 / (ACROSS - 1) as f32;
                let Vec2 { x, y } = path[i % n] + inward(i % n) * (u - 0.5) * width;
                positions.push(world(x, y, ground.height_at(x, y) + LIFT).to_array());
                uvs.push([u, i as f32 / n as f32 * repeats]);
            }
        }
        for i in (0..n).filter(|&i| on_map(i) && on_map((i + 1) % n)) {
            for a in 0..ACROSS - 1 {
                let near = first + i as u32 * ACROSS + a;
                let far = near + ACROSS;
                indices.extend([near, near + 1, far + 1, near, far + 1, far]);
            }
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

    /// The corners, a tile apart, round a square of tiles from `from` to
    /// `to` each way, what it bounds on the left.
    fn square(from: i32, to: i32) -> Vec<[f32; 2]> {
        let along = || (from..to).step_by(CELL_SIZE as usize);
        let back = || (from + 2..=to).rev().step_by(CELL_SIZE as usize);
        let (from, to) = (from as f32, to as f32);
        let near = along().map(|x| [x as f32, from]);
        let right = along().map(|y| [to, y as f32]);
        let far = back().map(|x| [x as f32, to]);
        let left = back().map(|y| [from, y as f32]);
        near.chain(right).chain(far).chain(left).collect()
    }

    #[test]
    fn guide_points_thin_an_outline_by_runs() {
        let points = |corners: &[[f32; 2]]| -> Vec<[f32; 2]> {
            let corners: Vec<Vec2> = corners.iter().map(|&c| Vec2::from(c)).collect();
            guides(&corners).iter().map(Vec2::to_array).collect()
        };
        // Four tiles by one: a run of four sides gives a point at its
        // middle corner, and a turn ends a run on the corner it turns at,
        // where the next begins.
        let slab = [
            [0.0, 0.0],
            [2.0, 0.0],
            [4.0, 0.0],
            [6.0, 0.0],
            [8.0, 0.0],
            [8.0, 2.0],
            [6.0, 2.0],
            [4.0, 2.0],
            [2.0, 2.0],
            [0.0, 2.0],
        ];
        assert_eq!(
            points(&slab),
            [[4.0, 0.0], [8.0, 1.0], [4.0, 2.0], [0.0, 1.0]]
        );
        // A staircase gives one a side, at its middle, evenly along the
        // slant.
        let stairs = [
            [0.0, 0.0],
            [2.0, 0.0],
            [2.0, 2.0],
            [4.0, 2.0],
            [4.0, 4.0],
            [2.0, 4.0],
            [0.0, 4.0],
            [0.0, 2.0],
        ];
        assert_eq!(
            points(&stairs),
            [
                [1.0, 0.0],
                [2.0, 1.0],
                [3.0, 2.0],
                [4.0, 3.0],
                [2.0, 4.0],
                [0.0, 2.0]
            ]
        );
    }

    #[test]
    fn border_curves_lie_on_straight_edges_and_cut_inside_corners() {
        // Two pieces to a guide point.
        let line = curve(&square(8, 56));
        let corners: Vec<Vec2> = square(8, 56).into_iter().map(Vec2::from).collect();
        assert_eq!(line.len(), 2 * guides(&corners).len());
        // Along the middle of a side, where the guide points are evenly
        // spaced, it's on the edge.
        let near: Vec<&Vec2> = (line.iter())
            .filter(|p| (20.0..44.0).contains(&p.x) && p.y < 20.0)
            .collect();
        assert!(near.len() >= 4);
        assert!(near.iter().all(|p| (p.y - 8.0).abs() < 1e-4));
        // Nowhere outside the square, and inside it at the corners.
        assert!(
            line.iter()
                .all(|p| p.min_element() >= 8.0 && p.max_element() <= 56.0)
        );
        let corner = Vec2::splat(8.0);
        let nearest = line
            .iter()
            .map(|p| p.distance(corner))
            .fold(f32::MAX, f32::min);
        assert!(nearest > 0.5, "{nearest}");
        // A guide point at a right-angled corner with its neighbours 8
        // away is cut by 8 cos(45) / 3.
        let guides = [[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 8.0]].map(Vec2::from);
        let cut = spline(&guides)[2].distance(Vec2::new(8.0, 0.0));
        assert!(
            (cut - 8.0 * 45f32.to_radians().cos() / 3.0).abs() < 1e-4,
            "{cut}"
        );
    }

    #[test]
    fn borders_run_along_their_paths_on_the_ground() {
        let side = 33;
        let heights = (0..side * side).map(|i| (i % 5) as f32).collect();
        let ground = Ground::from_heights(heights);
        let mesh = border(&ground, &[square(8, 56)], 1.0);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uvs)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("no uvs");
        };
        assert!(!positions.is_empty());
        // Half its width either side of the path, the image's left edge
        // outside what the path bounds.
        for (p, [u, _]) in positions.iter().zip(uvs) {
            let [x, y] = crate::terrain::map_point(Vec3::from_array(*p));
            assert!((p[1] - ground.height_at(x, y) - LIFT).abs() < 1e-4);
            assert!((7.49..56.51).contains(&x) && (7.49..56.51).contains(&y));
            // Along the middle of the near side, clear of the corners.
            if (24.0..40.0).contains(&x) && y < 12.0 {
                assert!(*u != 0.0 || y < 8.0, "{x}, {y}");
                assert!(*u != 1.0 || y > 8.0, "{x}, {y}");
            }
        }
        // The image goes round a whole number of times.
        let last = uvs.last().unwrap()[1];
        assert!(last >= 1.0 && last.fract() == 0.0, "{last}");
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
