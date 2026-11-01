//! Builds the ground and water meshes from a map's height map and tiles.

use std::collections::{HashMap, VecDeque};

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageType,
};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use tgck::map::{CELL_SIZE, HeightMap, Terrain};

use crate::game::Game;

/// Tile corners, counter-clockwise seen from above.
const CORNERS: [(usize, usize); 4] = [(0, 0), (1, 0), (1, 1), (0, 1)];

/// How far the bed drops per vertex step away from the shore, and at most.
const BED_SLOPE: f32 = 0.6;
const MAX_DEPTH: f32 = 3.0;

/// Ground heights with the water bed carved out. The height map stores the
/// water surface at water vertices; the game derives the bed itself.
pub struct Ground {
    pub side: usize,
    pub heights: Vec<f32>,
    pub surface: Vec<f32>,
}

impl Ground {
    pub fn new(hm: &HeightMap) -> Self {
        let side = hm.side;
        let water: Vec<bool> = hm.layer.iter().map(|&l| l != 0).collect();
        // Breadth-first distance (in vertex steps) from the nearest land vertex.
        let mut dist = vec![u32::MAX; side * side];
        let mut queue = VecDeque::new();
        for (i, &w) in water.iter().enumerate() {
            if !w {
                dist[i] = 0;
                queue.push_back(i);
            }
        }
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i % side, i / side);
            let neighbours = [
                (x > 0).then(|| i - 1),
                (x + 1 < side).then(|| i + 1),
                (y > 0).then(|| i - side),
                (y + 1 < side).then(|| i + side),
            ];
            for j in neighbours.into_iter().flatten() {
                if dist[j] == u32::MAX {
                    dist[j] = dist[i] + 1;
                    queue.push_back(j);
                }
            }
        }
        let heights = hm
            .heights
            .iter()
            .zip(&dist)
            .map(|(&h, &d)| match d {
                0 => h,
                u32::MAX => h - MAX_DEPTH,
                d => h - (d as f32 * BED_SLOPE).min(MAX_DEPTH),
            })
            .collect();
        Ground {
            side,
            heights,
            surface: hm.heights.clone(),
        }
    }

    fn at(&self, x: usize, y: usize) -> f32 {
        self.heights[y * self.side + x]
    }

    /// Ground height at a world position, interpolated.
    pub fn height_at(&self, wx: f32, wy: f32) -> f32 {
        let max = (self.side - 1) as f32;
        let fx = (wx / CELL_SIZE).clamp(0.0, max);
        let fy = (wy / CELL_SIZE).clamp(0.0, max);
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.side - 1), (y0 + 1).min(self.side - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let top = self.at(x0, y0) * (1.0 - tx) + self.at(x1, y0) * tx;
        let bottom = self.at(x0, y1) * (1.0 - tx) + self.at(x1, y1) * tx;
        top * (1.0 - ty) + bottom * ty
    }

    fn normal(&self, x: usize, y: usize) -> [f32; 3] {
        let h = |x: usize, y: usize| self.at(x.min(self.side - 1), y.min(self.side - 1));
        let dx = h(x + 1, y) - h(x.saturating_sub(1), y);
        let dy = h(x, y + 1) - h(x, y.saturating_sub(1));
        Vec3::new(-dx, 2.0 * CELL_SIZE, dy).normalize().to_array()
    }
}

/// Game space (x east, y north, z up) to Bevy's (x east, y up, -z north).
pub fn world(x: f32, y: f32, height: f32) -> Vec3 {
    Vec3::new(x, height, -y)
}

/// A named mesh and its material, ready to spawn.
pub type Part = (String, Mesh, StandardMaterial);

/// Builds one mesh per terrain variation, plus the water surface.
pub fn build(
    game: &mut Game,
    ground: &Ground,
    terrain: &Terrain,
    images: &mut Assets<Image>,
) -> Vec<Part> {
    let variations = game.terrain_variations();
    let mut parts = Vec::new();
    let mut by_type: HashMap<u16, Vec<(usize, usize)>> = HashMap::new();
    for y in 0..terrain.side {
        for x in 0..terrain.side {
            by_type
                .entry(terrain.at(x, y).terrain)
                .or_default()
                .push((x, y));
        }
    }

    let mut water_color = None;
    for (index, tiles) in by_type {
        let name = &terrain.names[index as usize];
        let variation = variations.get(&name.to_ascii_lowercase());
        let material = match variation {
            Some(v) => {
                water_color = water_color.or(v.water_color);
                match game
                    .read(&v.texture)
                    .and_then(|b| load_texture(&b, &v.texture))
                {
                    Some(image) => StandardMaterial {
                        base_color_texture: Some(images.add(image)),
                        perceptual_roughness: 1.0,
                        reflectance: 0.1,
                        ..default()
                    },
                    None => {
                        warn!("{name}: texture {} missing", v.texture);
                        flat(v.color)
                    }
                }
            }
            None => {
                warn!("{name}: no terrain definition");
                flat([1.0, 0.0, 1.0])
            }
        };
        let repeat = variation.map_or([8.0, 8.0], |v| v.repeat);
        parts.push((name.clone(), ground_mesh(ground, &tiles, repeat), material));
    }

    let water: Vec<(usize, usize)> = (0..terrain.side)
        .flat_map(|y| (0..terrain.side).map(move |x| (x, y)))
        .filter(|&(x, y)| terrain.at(x, y).water_body != 0)
        .collect();
    if !water.is_empty() {
        let [r, g, b] = water_color.unwrap_or([0.12, 0.26, 0.33]);
        parts.push((
            "water".into(),
            water_mesh(ground, &water),
            StandardMaterial {
                base_color: Color::srgba(r, g, b, 0.8),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.15,
                ..default()
            },
        ));
    }
    parts
}

fn flat([r, g, b]: [f32; 3]) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(r, g, b),
        perceptual_roughness: 1.0,
        ..default()
    }
}

/// Quads for the given tiles, four unshared vertices each so every
/// variation can be its own mesh.
fn ground_mesh(ground: &Ground, tiles: &[(usize, usize)], repeat: [f32; 2]) -> Mesh {
    let mut positions = Vec::with_capacity(tiles.len() * 4);
    let mut normals = Vec::with_capacity(tiles.len() * 4);
    let mut uvs = Vec::with_capacity(tiles.len() * 4);
    for &(x, y) in tiles {
        for (cx, cy) in CORNERS.map(|(dx, dy)| (x + dx, y + dy)) {
            let (wx, wy) = (cx as f32 * CELL_SIZE, cy as f32 * CELL_SIZE);
            positions.push(world(wx, wy, ground.at(cx, cy)).to_array());
            normals.push(ground.normal(cx, cy));
            uvs.push([cx as f32 / repeat[0], -(cy as f32) / repeat[1]]);
        }
    }
    quads(positions, normals, uvs)
}

fn water_mesh(ground: &Ground, tiles: &[(usize, usize)]) -> Mesh {
    let mut positions = Vec::with_capacity(tiles.len() * 4);
    for &(x, y) in tiles {
        for (cx, cy) in CORNERS.map(|(dx, dy)| (x + dx, y + dy)) {
            let (wx, wy) = (cx as f32 * CELL_SIZE, cy as f32 * CELL_SIZE);
            positions.push(world(wx, wy, ground.surface[cy * ground.side + cx]).to_array());
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let uvs = vec![[0.0, 0.0]; positions.len()];
    quads(positions, normals, uvs)
}

fn quads(positions: Vec<[f32; 3]>, normals: Vec<[f32; 3]>, uvs: Vec<[f32; 2]>) -> Mesh {
    let indices = (0..positions.len() as u32 / 4)
        .flat_map(|q| {
            let i = q * 4;
            [i, i + 1, i + 2, i, i + 2, i + 3]
        })
        .collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// Decodes a texture and gives it a repeating, mipmapped sampler.
fn load_texture(bytes: &[u8], name: &str) -> Option<Image> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    let image = Image::from_buffer(
        bytes,
        ImageType::Extension(&ext),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|e| warn!("{name}: {e}"))
    .ok()?;
    let mut image = image.convert(TextureFormat::Rgba8UnormSrgb)?;
    generate_mipmaps(&mut image);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        anisotropy_clamp: 16,
        ..ImageSamplerDescriptor::linear()
    });
    Some(image)
}

/// Appends box-filtered mip levels to an RGBA8 image.
fn generate_mipmaps(image: &mut Image) {
    let (mut w, mut h) = (image.width() as usize, image.height() as usize);
    let Some(data) = image.data.as_mut() else {
        return;
    };
    let mut level = data.clone();
    let mut count = 1;
    while w > 1 && h > 1 && w % 2 == 0 && h % 2 == 0 {
        let (nw, nh) = (w / 2, h / 2);
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let p = |dx: usize, dy: usize| {
                        level[((2 * y + dy) * w + 2 * x + dx) * 4 + c] as u32
                    };
                    next[(y * nw + x) * 4 + c] =
                        ((p(0, 0) + p(1, 0) + p(0, 1) + p(1, 1)) / 4) as u8;
                }
            }
        }
        data.extend_from_slice(&next);
        (level, w, h) = (next, nw, nh);
        count += 1;
    }
    image.texture_descriptor.mip_level_count = count;
}
