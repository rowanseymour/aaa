//! Soft edges between terrain types. Each tile draws, lowest priority first,
//! every terrain in its 3x3 neighbourhood up to its own, each through an
//! alpha mask chosen by which of the nine cells hold that terrain or a higher
//! one. Lower terrain therefore shows through where higher terrain thins out.

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageType,
};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;

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
}

#[derive(ShaderType, Debug, Clone)]
pub struct Layers {
    /// Texture repeat of each terrain type, in tiles (xy).
    pub repeats: [Vec4; MAX_TYPES],
}

impl MaterialExtension for Blend {
    fn fragment_shader() -> ShaderRef {
        "embedded://aaa_viewer/blend.wesl".into()
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
/// one, from the set's files. Patterns without a file of their own use a
/// rotated or mirrored one, failing that the union of the largest ones they
/// contain.
pub fn mask_array(files: &[(String, Vec<u8>)]) -> Option<Image> {
    let mut known: [Option<Vec<u8>>; 256] = std::array::from_fn(|_| None);
    for (digits, bytes) in files {
        let Some(pattern) = pattern(digits) else {
            continue;
        };
        let Some(alpha) = decode_alpha(bytes, digits) else {
            continue;
        };
        known[pattern as usize] = Some(alpha);
    }
    let symmetric: Vec<Option<Vec<u8>>> = (0..=255u8).map(|p| by_symmetry(&known, p)).collect();
    if symmetric[0].is_none() {
        warn!("no terrain transition masks");
        return None;
    }

    let mut data = Vec::with_capacity(MASK_SIZE * MASK_SIZE * (EMPTY_MASK as usize + 1));
    for p in 0..=255u8 {
        let mask = symmetric[p as usize].clone().unwrap_or_else(|| {
            // Union of the largest available sub-patterns.
            let subs: Vec<u8> = (0..=255u8)
                .filter(|&q| q & !p == 0 && symmetric[q as usize].is_some())
                .collect();
            let maximal = subs
                .iter()
                .filter(|&&q| !subs.iter().any(|&r| r != q && r & q == q));
            let mut out = vec![0u8; MASK_SIZE * MASK_SIZE];
            for &q in maximal {
                for (o, &a) in out.iter_mut().zip(symmetric[q as usize].as_ref().unwrap()) {
                    *o = (*o).max(a);
                }
            }
            out
        });
        data.extend_from_slice(&mask);
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
    let image = Image::from_buffer(
        bytes,
        ImageType::Extension("tga"),
        CompressedImageFormats::NONE,
        false,
        ImageSampler::Default,
        RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|e| warn!("transition {name}: {e}"))
    .ok()?;
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

fn mirror(p: u8) -> u8 {
    from_cells(|c, r| (2 - c, r), p)
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

/// The mask for `p`, from a file for `p` turned or mirrored.
fn by_symmetry(known: &[Option<Vec<u8>>; 256], p: u8) -> Option<Vec<u8>> {
    for mirrored in [false, true] {
        let mut q = if mirrored { mirror(p) } else { p };
        for turns in 0..4 {
            if let Some(m) = &known[q as usize] {
                let mut m = m.clone();
                for _ in 0..(4 - turns) % 4 {
                    m = rotate_pixels(&m);
                }
                if mirrored {
                    m = mirror_pixels(&m);
                }
                return Some(m);
            }
            q = rotate(q);
        }
    }
    None
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
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        anisotropy_clamp: 16,
        ..ImageSamplerDescriptor::linear()
    });
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
        }
    }
}
