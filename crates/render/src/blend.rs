//! Soft edges between terrain types. Each of a tile's edges and corners
//! takes the lowest terrain among the tiles that meet there, and the tile
//! itself is the centre. The lowest terrain in those nine cells is drawn
//! whole, then each higher one through an alpha mask chosen by which edges
//! and corners hold exactly that terrain.

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat, TextureViewDescriptor,
    TextureViewDimension,
};
use bevy::shader::ShaderRef;

use crate::textures;

pub type GroundMaterial = ExtendedMaterial<StandardMaterial, Blend>;

/// Most terrain layers one tile blends; tiles touching more drop the lowest.
pub const MAX_LAYERS: usize = 4;
/// Most terrain types one map can use.
pub const MAX_TYPES: usize = 256;

/// Side of a mask, in pixels.
const MASK_SIZE: usize = 32;
/// Mask array layer that is fully transparent, for unused layers.
pub const EMPTY_MASK: u32 = 256;

/// Keypad digits of the eight neighbours, in bit order. The masks are named
/// after the keypad: 7 8 9 is the north row, 5 the tile itself.
const DIGITS: [u8; 8] = [1, 2, 3, 4, 6, 7, 8, 9];

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct Blend {
    #[uniform(100)]
    pub layers: Layers,
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    pub textures: Handle<Image>,
    #[texture(103, dimension = "2d_array")]
    #[sampler(104)]
    pub masks: Handle<Image>,
    /// How brightly each tile shows through the fog of war, one texel a
    /// tile, row-major from the south-west corner.
    #[texture(105)]
    #[sampler(106)]
    pub fog: Handle<Image>,
}

#[derive(ShaderType, Debug, Clone)]
pub struct Layers {
    /// Texture repeat of each terrain type, in world units (xy).
    pub repeats: [Vec4; MAX_TYPES],
    /// The map's size (x) and how far in from its edge it fades to black (y),
    /// in world units; no fade if y is 0.
    pub edge: Vec4,
}

impl MaterialExtension for Blend {
    fn fragment_shader() -> ShaderRef {
        "embedded://aaa_render/blend.wesl".into()
    }
}

/// Bit in a neighbour pattern for the neighbour at offset (dx, dy), y north.
pub fn bit(dx: i32, dy: i32) -> u8 {
    let digit = (5 + dx + 3 * dy) as u8;
    1 << DIGITS
        .iter()
        .position(|&d| d == digit)
        .expect("not the centre")
}

/// Builds the array of 257 masks, one per neighbour pattern plus an empty
/// one, from the set's files. Patterns without a file of their own turn or
/// mirror the first file, in pattern order, that one reaches: turns first,
/// then a north-south flip and its turns, then an east-west flip and its
/// turns. The rest, which [`fill_sides`] never produces, use the empty
/// pattern's mask.
pub fn mask_array(files: &[(String, Vec<u8>)]) -> Option<Image> {
    let mut bases: Vec<(u8, Vec<u8>)> = files
        .iter()
        .filter_map(|(digits, bytes)| {
            let pattern = pattern(digits)?;
            Some((pattern, decode_alpha(bytes, &format!("{digits}.tga"))?))
        })
        .collect();
    bases.sort_by_key(|(p, _)| *p);
    let masks = derive(&bases);
    let Some(fallback) = masks[0].clone() else {
        warn!("no terrain transition masks");
        return None;
    };

    let mut data = Vec::with_capacity(MASK_SIZE * MASK_SIZE * (EMPTY_MASK as usize + 1));
    for mask in masks {
        data.extend_from_slice(mask.as_ref().unwrap_or(&fallback));
    }
    data.extend(std::iter::repeat_n(0, MASK_SIZE * MASK_SIZE));

    let mut image = Image::new(
        Extent3d {
            width: MASK_SIZE as u32,
            height: MASK_SIZE as u32,
            depth_or_array_layers: EMPTY_MASK + 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..ImageSamplerDescriptor::linear()
    });
    Some(image)
}

/// Completes a side that has both its corners but not its middle, the
/// only shapes the mask set doesn't cover.
pub fn fill_sides(p: u8) -> u8 {
    let mut p = p;
    for (a, mid, b) in [
        ((-1, -1), (0, -1), (1, -1)),
        ((-1, -1), (-1, 0), (-1, 1)),
        ((1, -1), (1, 0), (1, 1)),
        ((-1, 1), (0, 1), (1, 1)),
    ] {
        let has = |p: u8, (dx, dy): (i32, i32)| p & bit(dx, dy) != 0;
        if has(p, a) && has(p, b) {
            p |= bit(mid.0, mid.1);
        }
    }
    p
}

/// A mask array with every mask empty, for when the masks are missing.
pub fn empty_masks() -> Image {
    Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: EMPTY_MASK + 1,
        },
        TextureDimension::D2,
        &[0],
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// A fog of `side` texels a side, blended between tiles, from how brightly
/// each tile shows, row-major from the south-west corner: 255 for as it
/// is.
pub fn fog_image(side: usize, light: Vec<u8>) -> Image {
    let side = side.max(1) as u32;
    let mut image = Image::new(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        light,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// Neighbour pattern of a mask name like `12345`.
fn pattern(digits: &str) -> Option<u8> {
    let mut p = 0;
    for c in digits.chars() {
        let d = c.to_digit(10)? as u8;
        if d != 5 {
            p |= 1 << DIGITS.iter().position(|&x| x == d)?;
        }
    }
    Some(p)
}

fn decode_alpha(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
    let image = textures::decode(bytes, name, CompressedImageFormats::NONE, false)?;
    let image = match image.texture_descriptor.format {
        TextureFormat::Rgba8Unorm => image,
        _ => image.convert(TextureFormat::Rgba8Unorm)?,
    };
    if image.width() as usize != MASK_SIZE || image.height() as usize != MASK_SIZE {
        warn!("transition {name}: not {MASK_SIZE}x{MASK_SIZE}");
        return None;
    }
    Some(image.data?.chunks(4).map(|px| px[3]).collect())
}

/// Cell (column, row) of each pattern bit, row 0 north.
fn cell(bit: usize) -> (usize, usize) {
    let d = DIGITS[bit] as usize - 1;
    (d % 3, 2 - d / 3)
}

fn from_cells(f: impl Fn(usize, usize) -> (usize, usize), p: u8) -> u8 {
    (0..8)
        .filter(|&b| p & (1 << b) != 0)
        .map(|b| {
            let (c, r) = cell(b);
            let (c, r) = f(c, r);
            1 << (0..8).find(|&b| cell(b) == (c, r)).unwrap()
        })
        .fold(0, |a, b| a | b)
}

/// A quarter turn clockwise, as seen from above.
fn rotate(p: u8) -> u8 {
    from_cells(|c, r| (2 - r, c), p)
}

/// East-west mirror image.
fn mirror(p: u8) -> u8 {
    from_cells(|c, r| (2 - c, r), p)
}

/// North-south mirror image.
fn flip(p: u8) -> u8 {
    from_cells(|c, r| (c, 2 - r), p)
}

fn rotate_pixels(m: &[u8]) -> Vec<u8> {
    let n = MASK_SIZE;
    (0..n * n)
        .map(|i| {
            let (y, x) = (i / n, i % n);
            m[(n - 1 - x) * n + y]
        })
        .collect()
}

fn mirror_pixels(m: &[u8]) -> Vec<u8> {
    let n = MASK_SIZE;
    (0..n * n).map(|i| m[i / n * n + n - 1 - i % n]).collect()
}

fn flip_pixels(m: &[u8]) -> Vec<u8> {
    let n = MASK_SIZE;
    (0..n * n).map(|i| m[(n - 1 - i / n) * n + i % n]).collect()
}

/// The mask of every pattern that `bases` reach by turning or mirroring.
fn derive(bases: &[(u8, Vec<u8>)]) -> Vec<Option<Vec<u8>>> {
    type Masks = Vec<Option<Vec<u8>>>;
    /// Sets `p`'s mask unless it has one, saying whether it was set.
    fn add(out: &mut Masks, p: u8, m: Vec<u8>) -> bool {
        let new = out[p as usize].is_none();
        if new {
            out[p as usize] = Some(m);
        }
        new
    }
    fn turns(out: &mut Masks, mut p: u8, m: &[u8]) {
        let mut m = m.to_vec();
        for _ in 0..3 {
            (p, m) = (rotate(p), rotate_pixels(&m));
            add(out, p, m.clone());
        }
    }

    let mut out: Masks = vec![None; 256];
    for (p, m) in bases {
        out[*p as usize] = Some(m.clone());
    }
    for (p, m) in bases {
        turns(&mut out, *p, m);
        for (q, n) in [(flip(*p), flip_pixels(m)), (mirror(*p), mirror_pixels(m))] {
            if add(&mut out, q, n.clone()) {
                turns(&mut out, q, &n);
            }
        }
    }
    out
}

/// Stacks equally sized RGBA8 images, each with the same mip chain, into one
/// array texture with a repeating sampler.
pub fn texture_array(layers: Vec<Image>) -> Image {
    let first = &layers[0];
    let (width, height) = (first.width(), first.height());
    let mips = first.texture_descriptor.mip_level_count;
    let format = first.texture_descriptor.format;
    let count = layers.len() as u32;
    let data = layers.into_iter().flat_map(|l| l.data.unwrap()).collect();
    let mut image = Image::new_uninit(
        Extent3d {
            width,
            height,
            depth_or_array_layers: count,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = mips;
    // Otherwise a single layer would be viewed as a plain 2D texture.
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    image.sampler = textures::repeating();
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetries() {
        // South neighbour turns to west, then north; mirroring swaps east and west.
        assert_eq!(rotate(bit(0, -1)), bit(-1, 0));
        assert_eq!(rotate(rotate(bit(0, -1))), bit(0, 1));
        assert_eq!(mirror(bit(1, 1)), bit(-1, 1));
        assert_eq!(pattern("25"), Some(bit(0, -1)));
        assert_eq!(pattern("123456789"), Some(0xff));
        for p in 0..=255u8 {
            assert_eq!(rotate(rotate(rotate(rotate(p)))), p);
            assert_eq!(mirror(mirror(p)), p);
            assert_eq!(flip(flip(p)), p);
        }
    }

    /// The digits of the shipped transition set's files.
    const SHIPPED: &str = "5 15 25 125 245 345 456 1235 1245 1456 2345 2357 2456 2567 \
        12345 12456 14567 23457 23567 24567 24568 34567 123456 123457 124567 \
        124568 234567 1234567 1234568 2345678 12345678 123456789 357";

    #[test]
    fn shipped_masks_cover_every_filled_pattern() {
        let bases: Vec<(u8, Vec<u8>)> = SHIPPED
            .split_whitespace()
            .map(|d| (pattern(d).unwrap(), vec![0; MASK_SIZE * MASK_SIZE]))
            .collect();
        let masks = derive(&bases);
        for p in 0..=255u8 {
            assert!(masks[fill_sides(p) as usize].is_some(), "{p:#04x}");
        }
    }

    #[test]
    fn derived_masks_turn_with_their_pattern() {
        // A south edge mask, opaque at the middle of its bottom row only.
        let n = MASK_SIZE;
        let mut south = vec![0; n * n];
        south[(n - 1) * n + n / 2] = 255;
        let masks = derive(&[(bit(0, -1), south)]);
        let west = masks[bit(-1, 0) as usize].as_ref().unwrap();
        assert_eq!(west[n / 2 * n], 255);
        let north = masks[bit(0, 1) as usize].as_ref().unwrap();
        assert_eq!(north[n / 2 - 1], 255);
    }

    #[test]
    fn sides_with_both_corners_fill_in() {
        let sw_se = bit(-1, -1) | bit(1, -1);
        assert_eq!(fill_sides(sw_se), sw_se | bit(0, -1));
        assert_eq!(fill_sides(bit(-1, -1)), bit(-1, -1));
    }
}
