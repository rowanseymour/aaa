//! Builds the ground and water meshes from a map's height map and tiles.

use std::collections::{HashMap, VecDeque};

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use tgck::map::{CELL_SIZE, HeightMap, Terrain};

use crate::blend::{self, Blend, GroundMaterial};
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

/// The ground, blended across terrain types, and the water surface.
pub struct Built {
    pub ground: Option<(Mesh, GroundMaterial)>,
    pub water: Option<(Mesh, StandardMaterial)>,
}

/// Side of each terrain texture in the array.
const TEXTURE_SIZE: usize = 256;

/// A terrain type used by the map, as one layer of the texture array.
struct Kind {
    image: Image,
    repeat: [f32; 2],
    /// Blend order: base priority, then position in the map's table.
    order: (i32, u16),
}

pub fn build(
    game: &mut Game,
    ground: &Ground,
    terrain: &Terrain,
    images: &mut Assets<Image>,
) -> Built {
    let variations = game.terrain_variations();
    let side = terrain.side;

    // Terrain types in use, numbered as texture array layers.
    let mut layer_of: HashMap<u16, u32> = HashMap::new();
    let mut kinds = Vec::new();
    let mut water_color = None;
    for tile in &terrain.tiles {
        if layer_of.contains_key(&tile.terrain) {
            continue;
        }
        if kinds.len() == blend::MAX_TYPES {
            warn!("more than {} terrain types", blend::MAX_TYPES);
            break;
        }
        let name = &terrain.names[tile.terrain as usize];
        let variation = variations.get(&name.to_ascii_lowercase());
        let image = match variation {
            Some(v) => {
                water_color = water_color.or(v.water_color);
                game.read(&v.texture)
                    .and_then(|b| load_texture(&b, &v.texture))
                    .unwrap_or_else(|| {
                        warn!("{name}: texture {} missing", v.texture);
                        solid(v.color)
                    })
            }
            None => {
                warn!("{name}: no terrain definition");
                solid([1.0, 0.0, 1.0])
            }
        };
        layer_of.insert(tile.terrain, kinds.len() as u32);
        kinds.push(Kind {
            image,
            repeat: variation.map_or([8.0, 8.0], |v| v.repeat),
            order: (variation.map_or(0, |v| v.priority), tile.terrain),
        });
    }
    // Tiles of types past the limit borrow the first type.
    let layer = |t: u16| layer_of.get(&t).copied().unwrap_or(0);

    let water: Vec<(usize, usize)> = (0..side)
        .flat_map(|y| (0..side).map(move |x| (x, y)))
        .filter(|&(x, y)| terrain.at(x, y).water_body != 0)
        .collect();
    let water = (!water.is_empty()).then(|| {
        let [r, g, b] = water_color.unwrap_or([0.12, 0.26, 0.33]);
        (
            water_mesh(ground, &water),
            StandardMaterial {
                base_color: Color::srgba(r, g, b, 0.8),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.15,
                ..default()
            },
        )
    });
    if kinds.is_empty() {
        return Built {
            ground: None,
            water,
        };
    }

    let masks = blend::mask_array(&game.transition_masks());
    let order: Vec<(i32, u16)> = terrain
        .tiles
        .iter()
        .map(|t| kinds[layer(t.terrain) as usize].order)
        .collect();
    let mesh = ground_mesh(ground, side, |x, y| match masks {
        Some(_) => Layers::of(side, x, y, &order, |&(_, t)| layer(t)),
        None => Layers::single(layer(terrain.at(x, y).terrain)),
    });
    let mut repeats = [Vec4::ONE; blend::MAX_TYPES];
    for (r, k) in repeats.iter_mut().zip(&kinds) {
        *r = Vec4::new(k.repeat[0], k.repeat[1], 0.0, 0.0);
    }
    let textures = blend::texture_array(kinds.into_iter().map(|k| k.image).collect());
    let masks = masks.unwrap_or_else(blend::empty_masks);
    let material = GroundMaterial {
        base: StandardMaterial {
            perceptual_roughness: 1.0,
            reflectance: 0.1,
            ..default()
        },
        extension: Blend {
            layers: blend::Layers { repeats },
            textures: images.add(textures),
            masks: images.add(masks),
        },
    };
    Built {
        ground: Some((mesh, material)),
        water,
    }
}

/// The terrain layers of one tile and their masks, as the blend shader
/// reads them from vertex attributes.
struct Layers {
    kinds: [u32; blend::MAX_LAYERS],
    masks: [u32; blend::MAX_LAYERS],
}

impl Layers {
    fn single(kind: u32) -> Self {
        Layers {
            kinds: [kind; blend::MAX_LAYERS],
            masks: [blend::EMPTY_MASK; blend::MAX_LAYERS],
        }
    }

    /// Every terrain in the 3x3 neighbourhood that does not outrank the
    /// tile's own, lowest first. Each layer's mask covers the cells holding
    /// that terrain or a higher one, so neighbouring tiles agree along their
    /// shared edge. Neighbours beyond the map edge repeat the edge.
    fn of<K: Ord + Copy>(
        side: usize,
        x: usize,
        y: usize,
        order: &[K],
        layer: impl Fn(&K) -> u32,
    ) -> Self {
        let at = |dx: i32, dy: i32| {
            let cx = (x as i32 + dx).clamp(0, side as i32 - 1) as usize;
            let cy = (y as i32 + dy).clamp(0, side as i32 - 1) as usize;
            order[cy * side + cx]
        };
        let own = at(0, 0);
        let mut present: Vec<K> = (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| at(dx, dy)))
            .filter(|k| *k <= own)
            .collect();
        present.sort();
        present.dedup();
        let present = &present[present.len().saturating_sub(blend::MAX_LAYERS)..];

        let mut layers = Layers::single(layer(&present[0]));
        for (i, k) in present.iter().enumerate().skip(1) {
            let mut pattern = 0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if (dx, dy) != (0, 0) && at(dx, dy) >= *k {
                        pattern |= blend::bit(dx, dy);
                    }
                }
            }
            layers.kinds[i] = layer(k);
            layers.masks[i] = pattern as u32;
        }
        layers
    }
}

/// Quads for every tile, four unshared vertices each so the layer
/// attributes stay constant across a tile.
fn ground_mesh(ground: &Ground, side: usize, layers: impl Fn(usize, usize) -> Layers) -> Mesh {
    let count = side * side * 4;
    let mut positions = Vec::with_capacity(count);
    let mut normals = Vec::with_capacity(count);
    let mut uvs = Vec::with_capacity(count);
    let mut kinds = Vec::with_capacity(count);
    let mut masks = Vec::with_capacity(count);
    for y in 0..side {
        for x in 0..side {
            let l = layers(x, y);
            let k = l.kinds.map(|k| k as f32);
            let m = [(l.masks[1] + 512 * l.masks[2]) as f32, l.masks[3] as f32];
            for (dx, dy) in CORNERS {
                let (cx, cy) = (x + dx, y + dy);
                let (wx, wy) = (cx as f32 * CELL_SIZE, cy as f32 * CELL_SIZE);
                positions.push(world(wx, wy, ground.at(cx, cy)).to_array());
                normals.push(ground.normal(cx, cy));
                uvs.push([dx as f32, 1.0 - dy as f32]);
                kinds.push(k);
                masks.push(m);
            }
        }
    }
    quads(positions, normals, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, kinds)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, masks)
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

/// Decodes a texture into a square, mipmapped array layer.
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
    .ok()?
    .convert(TextureFormat::Rgba8UnormSrgb)?;
    let (w, h) = (image.width() as usize, image.height() as usize);
    let src = image.data?;
    // Nearest-neighbour resampling: the textures are 128 or 256 square.
    let n = TEXTURE_SIZE;
    let data = (0..n * n)
        .flat_map(|i| {
            let (x, y) = (i % n * w / n, i / n * h / n);
            let p = (y * w + x) * 4;
            [src[p], src[p + 1], src[p + 2], src[p + 3]]
        })
        .collect();
    Some(layer_image(data))
}

/// A single-colour array layer, for terrain whose texture is missing.
fn solid(color: [f32; 3]) -> Image {
    let [r, g, b, a] = Color::srgb(color[0], color[1], color[2])
        .to_srgba()
        .to_u8_array();
    let n = TEXTURE_SIZE;
    layer_image([r, g, b, a].repeat(n * n))
}

fn layer_image(data: Vec<u8>) -> Image {
    let n = TEXTURE_SIZE as u32;
    let mut image = Image::new(
        Extent3d {
            width: n,
            height: n,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    generate_mipmaps(&mut image);
    image
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
