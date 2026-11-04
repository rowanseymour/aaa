//! Builds the ground and water meshes from a map's height map and tiles.

use std::collections::HashMap;

use aaa_data::{DepthColor, GameData, Id, TextureAnimation};
use aaa_map::Ground;
use bevy::asset::RenderAssetUsages;
use bevy::image::CompressedImageFormats;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use tgck::map::{CELL_SIZE, Terrain};

use crate::blend::{self, Blend, GroundMaterial};
use crate::textures;
use crate::water::{Water, WaterMaterial};

/// Tile corners, counter-clockwise seen from above.
const CORNERS: [(usize, usize); 4] = [(0, 0), (1, 0), (1, 1), (0, 1)];

/// How far the waterline sits below the height map's water level: maps
/// store every floating object (ships, lily pads, waves) exactly this far
/// down.
const SHORE_DROP: f32 = 0.5;

/// Whether to split a tile along its (1,0)-(0,1) diagonal rather than
/// (0,0)-(1,1): the flatter one, so a lone corner that differs gets a
/// triangle to itself and edges such as the shoreline cut across it.
fn flip(ground: &Ground, x: usize, y: usize) -> bool {
    let rising = (ground.at(x, y) - ground.at(x + 1, y + 1)).abs();
    let falling = (ground.at(x + 1, y) - ground.at(x, y + 1)).abs();
    falling < rising
}

fn normal(ground: &Ground, x: usize, y: usize) -> [f32; 3] {
    let h = |x: usize, y: usize| ground.at(x.min(ground.side - 1), y.min(ground.side - 1));
    let dx = h(x + 1, y) - h(x.saturating_sub(1), y);
    let dy = h(x, y + 1) - h(x, y.saturating_sub(1));
    Vec3::new(-dx, 2.0 * CELL_SIZE, dy).normalize().to_array()
}

/// Game space (x east, y north, z up) to Bevy's (x east, y up, -z north).
pub fn world(x: f32, y: f32, height: f32) -> Vec3 {
    Vec3::new(x, height, -y)
}

/// The ground, blended across terrain types, and the water surface.
pub struct Built {
    pub ground: Option<(Mesh, GroundMaterial)>,
    pub water: Option<(Mesh, WaterMaterial)>,
}

/// Side of each terrain texture in the array.
const TEXTURE_SIZE: usize = 256;
/// Side of each water animation frame.
const FRAME_SIZE: usize = 128;
/// World units across one repeat of the water's texture: `AVars.tgi`'s
/// `TerrainBlockSize`, whatever the water terrain's `texture_width`.
const WATER_REPEAT: f32 = 8.0;

/// A terrain type used by the map, as one layer of the texture array.
struct Kind {
    image: Image,
    repeat: [f32; 2],
    /// Blend order: priority, then position in the map's table.
    order: (i32, u16),
}

pub fn build(
    game: &mut GameData,
    ground: &Ground,
    terrain: &Terrain,
    images: &mut Assets<Image>,
    fade: f32,
) -> Built {
    let variations = game.terrain_variations();
    let side = terrain.side;

    // Terrain types in use, numbered as texture array layers.
    let mut layer_of: HashMap<u16, u32> = HashMap::new();
    let mut kinds = Vec::new();
    for tile in &terrain.tiles {
        if layer_of.contains_key(&tile.terrain) {
            continue;
        }
        if kinds.len() == blend::MAX_TYPES {
            warn!("more than {} terrain types", blend::MAX_TYPES);
            break;
        }
        let name = &terrain.names[tile.terrain as usize];
        let variation = variations.get(&Id::from(name.as_str()));
        let image = match variation {
            Some(v) => game
                .read(&v.texture)
                .and_then(|b| load_texture(&b, &v.texture, TEXTURE_SIZE))
                .unwrap_or_else(|| {
                    warn!("{name}: texture {} missing", v.texture);
                    solid(v.color, TEXTURE_SIZE)
                }),
            None => {
                warn!("{name}: no terrain definition");
                solid([1.0, 0.0, 1.0], TEXTURE_SIZE)
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
        .filter(|&(x, y)| {
            CORNERS
                .iter()
                .any(|&(dx, dy)| ground.water[(y + dy) * ground.side + x + dx])
        })
        .collect();
    // One look for all the water: that of the terrain most of it lies on,
    // as some terrains beside water have colours of their own.
    let mut on_water: HashMap<u16, usize> = HashMap::new();
    for &(x, y) in &water {
        *on_water.entry(terrain.at(x, y).terrain).or_default() += 1;
    }
    let water_look = on_water
        .into_iter()
        .filter_map(|(t, n)| {
            let name = Id::from(terrain.names[t as usize].as_str());
            let v = variations.get(&name).filter(|v| v.water.is_some())?;
            Some((n, t, v))
        })
        .max_by_key(|&(n, t, _)| (n, t))
        .map(|(_, _, v)| v);
    let water = (!water.is_empty()).then(|| {
        let colors = water_look.and_then(|v| v.water.clone()).unwrap_or_else(|| {
            vec![DepthColor {
                depth: 0.0,
                color: [0.12, 0.26, 0.33],
                alpha: 0.8,
            }]
        });
        let animation = water_look.and_then(|v| v.animation.as_ref());
        let mut frames = animation.map_or_else(Vec::new, |a| load_frames(game, a));
        if frames.is_empty() {
            frames.push(solid([1.0; 3], FRAME_SIZE));
        }
        let fps = animation.map_or(0.0, |a| a.fps);
        let count = frames.len() as f32;
        (
            water_mesh(ground, &water, &colors, fade),
            WaterMaterial {
                // Lit without highlights, as the game draws it.
                base: StandardMaterial {
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: 1.0,
                    reflectance: 0.0,
                    ..default()
                },
                extension: Water {
                    animation: Vec4::new(WATER_REPEAT, WATER_REPEAT, fps, count),
                    frames: images.add(blend::texture_array(frames)),
                },
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
            layers: blend::Layers {
                repeats,
                edge: Vec4::new(ground.size(), fade, 0.0, 0.0),
            },
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

    /// The terrains of a tile's centre, edges and corners, lowest first,
    /// each edge or corner holding the lowest of the tiles that meet there.
    /// Each layer's mask covers the edges and corners holding exactly that
    /// terrain, so neighbouring tiles agree along their shared edge.
    fn of<K: Ord + Copy>(
        side: usize,
        x: usize,
        y: usize,
        order: &[K],
        layer: impl Fn(&K) -> u32,
    ) -> Self {
        let tile = |dx: i32, dy: i32| {
            let (cx, cy) = (x as i32 + dx, y as i32 + dy);
            let inside = (0..side as i32).contains(&cx) && (0..side as i32).contains(&cy);
            inside.then(|| order[cy as usize * side + cx as usize])
        };
        let own = order[y * side + x];
        let cell = |dx: i32, dy: i32| {
            [(dx, 0), (0, dy), (dx, dy)]
                .into_iter()
                .filter(|&(a, b)| (a, b) != (0, 0))
                .filter_map(|(a, b)| tile(a, b))
                .fold(own, Ord::min)
        };
        let mut present: Vec<K> = (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| cell(dx, dy)))
            .collect();
        present.sort();
        present.dedup();
        let present = &present[present.len().saturating_sub(blend::MAX_LAYERS)..];

        let mut layers = Layers::single(layer(&present[0]));
        for (i, k) in present.iter().enumerate().skip(1) {
            let mut pattern = 0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if (dx, dy) != (0, 0) && cell(dx, dy) == *k {
                        pattern |= blend::bit(dx, dy);
                    }
                }
            }
            layers.kinds[i] = layer(k);
            layers.masks[i] = blend::fill_sides(pattern) as u32;
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
                normals.push(normal(ground, cx, cy));
                uvs.push([dx as f32, 1.0 - dy as f32]);
                kinds.push(k);
                masks.push(m);
            }
        }
    }
    let flips: Vec<bool> = (0..side * side)
        .map(|i| flip(ground, i % side, i / side))
        .collect();
    quads(positions, normals, uvs, &flips)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, kinds)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, masks)
}

/// The water surface over every tile that touches water, coloured by the
/// depth of the bed below each vertex as the game does: clear at the shore,
/// so it fades in where the bank cuts it off. Within `fade` of the map's
/// edge it darkens to opaque black, as the ground does.
fn water_mesh(ground: &Ground, tiles: &[(usize, usize)], colors: &[DepthColor], fade: f32) -> Mesh {
    let mut positions = Vec::with_capacity(tiles.len() * 4);
    let mut vertex_colors = Vec::with_capacity(tiles.len() * 4);
    for &(x, y) in tiles {
        for (cx, cy) in CORNERS.map(|(dx, dy)| (x + dx, y + dy)) {
            let (wx, wy) = (cx as f32 * CELL_SIZE, cy as f32 * CELL_SIZE);
            let surface = ground.surface[cy * ground.side + cx] - SHORE_DROP;
            positions.push(world(wx, wy, surface).to_array());
            let ([r, g, b], a) = at_depth(colors, surface - ground.at(cx, cy));
            let [r, g, b, a] = Color::srgba(r, g, b, a).to_linear().to_f32_array();
            let lit = edge_light(ground.size(), fade, wx, wy);
            vertex_colors.push([r * lit, g * lit, b * lit, a + (1.0 - a) * (1.0 - lit)]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let uvs = vec![[0.0, 0.0]; positions.len()];
    let flips: Vec<bool> = tiles.iter().map(|&(x, y)| flip(ground, x, y)).collect();
    quads(positions, normals, uvs, &flips)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vertex_colors)
}

/// How lit the point (x, y) is, from 1 down to 0 at the edge of a map of
/// `size` over the last `fade` of it; matches `edge_light` in the shader.
fn edge_light(size: f32, fade: f32, x: f32, y: f32) -> f32 {
    if fade <= 0.0 {
        return 1.0;
    }
    let d = x.min(y).min(size - x).min(size - y);
    let t = (d / fade).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Interpolates the colour and alpha for a depth, holding the end values
/// beyond the first and last.
fn at_depth(colors: &[DepthColor], depth: f32) -> ([f32; 3], f32) {
    let next = colors.partition_point(|c| c.depth <= depth);
    let (a, b) = match next {
        0 => (colors[0], colors[0]),
        n if n == colors.len() => (colors[n - 1], colors[n - 1]),
        n => (colors[n - 1], colors[n]),
    };
    let t = if b.depth > a.depth {
        (depth - a.depth) / (b.depth - a.depth)
    } else {
        0.0
    };
    let mix = |x: f32, y: f32| x + (y - x) * t;
    (
        [0, 1, 2].map(|i| mix(a.color[i], b.color[i])),
        mix(a.alpha, b.alpha),
    )
}

/// Quads of four `CORNERS` each, split along the diagonal [`flip`]
/// picks for their tile.
fn quads(
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    flips: &[bool],
) -> Mesh {
    let indices = flips
        .iter()
        .zip((0..).step_by(4))
        .flat_map(|(&flip, i)| match flip {
            false => [i, i + 1, i + 2, i, i + 2, i + 3],
            true => [i, i + 1, i + 3, i + 1, i + 2, i + 3],
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

/// Every frame of a water animation, as array layers.
fn load_frames(game: &mut GameData, animation: &TextureAnimation) -> Vec<Image> {
    let mut frames = Vec::new();
    loop {
        let name = animation.frame(frames.len());
        let Some(frame) = game
            .read(&name)
            .and_then(|b| load_texture(&b, &name, FRAME_SIZE))
        else {
            break;
        };
        frames.push(frame);
    }
    if frames.is_empty() {
        warn!("no water animation frames at {}", animation.frames);
    }
    frames
}

/// Decodes a texture into a square, mipmapped array layer of side `n`.
fn load_texture(bytes: &[u8], name: &str, n: usize) -> Option<Image> {
    let image = textures::decode(bytes, name, CompressedImageFormats::BC, true)?;
    let image = textures::decompress(image)?.convert(TextureFormat::Rgba8UnormSrgb)?;
    let (w, h) = (image.width() as usize, image.height() as usize);
    let src = image.data?;
    // Nearest-neighbour resampling: the textures are 128 or 256 square.
    let data = (0..n * n)
        .flat_map(|i| {
            let (x, y) = (i % n * w / n, i / n * h / n);
            let p = (y * w + x) * 4;
            [src[p], src[p + 1], src[p + 2], src[p + 3]]
        })
        .collect();
    Some(layer_image(data, n))
}

/// A single-colour array layer of side `n`, for a missing texture.
fn solid(color: [f32; 3], n: usize) -> Image {
    let [r, g, b, a] = Color::srgb(color[0], color[1], color[2])
        .to_srgba()
        .to_u8_array();
    layer_image([r, g, b, a].repeat(n * n), n)
}

fn layer_image(data: Vec<u8>, n: usize) -> Image {
    let n = n as u32;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_take_the_lowest_tile_meeting_there() {
        // A 3x3 map of terrain 1 with terrain 0 south of the middle tile.
        let mut order = vec![1; 9];
        order[1] = 0;
        let l = Layers::of(3, 1, 1, &order, |&k| k);
        assert_eq!(&l.kinds[..2], &[0, 1]);
        // The south-west and south-east corners touch the lower tile too.
        let north_and_sides = [(-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
            .into_iter()
            .fold(0, |p, (dx, dy)| p | blend::bit(dx, dy));
        assert_eq!(l.masks[1], north_and_sides as u32);
        assert_eq!(l.masks[2], blend::EMPTY_MASK);
    }

    #[test]
    fn lower_tiles_take_nothing_from_higher_neighbours() {
        let mut order = vec![0; 9];
        order[1] = 1;
        let l = Layers::of(3, 1, 1, &order, |&k| k);
        assert_eq!(l.kinds, [0; blend::MAX_LAYERS]);
        assert_eq!(l.masks, [blend::EMPTY_MASK; blend::MAX_LAYERS]);
    }
}
