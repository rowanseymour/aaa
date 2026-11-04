//! Decoding the game's image files.

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageType,
};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Decodes an image file by its name's extension, warning if it can't.
pub fn decode(
    bytes: &[u8],
    name: &str,
    formats: CompressedImageFormats,
    srgb: bool,
) -> Option<Image> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    Image::from_buffer(
        bytes,
        ImageType::Extension(&ext),
        formats,
        srgb,
        ImageSampler::Default,
        RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|e| warn!("{name}: {e}"))
    .ok()
}

/// A sampler that tiles the texture.
pub fn repeating() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        anisotropy_clamp: 16,
        ..ImageSamplerDescriptor::linear()
    })
}

/// Decompresses the top level of a BC1-3 (DXT1-5) image to RGBA8, for
/// textures that are resampled or stacked on the CPU. Other images pass
/// through unchanged.
pub fn decompress(image: Image) -> Option<Image> {
    let (block, srgb) = match image.texture_descriptor.format {
        TextureFormat::Bc1RgbaUnorm => (Bc::One, false),
        TextureFormat::Bc1RgbaUnormSrgb => (Bc::One, true),
        TextureFormat::Bc2RgbaUnorm => (Bc::Two, false),
        TextureFormat::Bc2RgbaUnormSrgb => (Bc::Two, true),
        TextureFormat::Bc3RgbaUnorm => (Bc::Three, false),
        TextureFormat::Bc3RgbaUnormSrgb => (Bc::Three, true),
        _ => return Some(image),
    };
    let (w, h) = (image.width() as usize, image.height() as usize);
    let data = image.data.as_ref()?;
    let size = if let Bc::One = block { 8 } else { 16 };
    let across = w.div_ceil(4);
    let mut out = vec![0u8; w * h * 4];
    for (i, bytes) in data
        .chunks_exact(size)
        .take(across * h.div_ceil(4))
        .enumerate()
    {
        let pixels = block.decode(bytes);
        for (j, px) in pixels.iter().enumerate() {
            let (x, y) = (i % across * 4 + j % 4, i / across * 4 + j / 4);
            if x < w && y < h {
                out[(y * w + x) * 4..][..4].copy_from_slice(px);
            }
        }
    }
    let format = match srgb {
        true => TextureFormat::Rgba8UnormSrgb,
        false => TextureFormat::Rgba8Unorm,
    };
    Some(Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        out,
        format,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

#[derive(Clone, Copy)]
enum Bc {
    One,
    Two,
    Three,
}

impl Bc {
    /// The 16 pixels of a block, row by row.
    fn decode(self, b: &[u8]) -> [[u8; 4]; 16] {
        let (alpha, color) = match self {
            Bc::One => (None, b),
            Bc::Two => (Some(explicit_alpha(&b[..8])), &b[8..]),
            Bc::Three => (Some(interpolated_alpha(&b[..8])), &b[8..]),
        };
        let mut pixels = color_block(color, alpha.is_none());
        if let Some(alpha) = alpha {
            for (p, a) in pixels.iter_mut().zip(alpha) {
                p[3] = a;
            }
        }
        pixels
    }
}

/// A BC1 colour block; only BC1 itself has the three-colour mode with
/// transparent black.
fn color_block(b: &[u8], bc1: bool) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([b[0], b[1]]);
    let c1 = u16::from_le_bytes([b[2], b[3]]);
    let rgb = |c: u16| {
        let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
        [
            (r * 255 + 15) / 31,
            (g * 255 + 31) / 63,
            (b * 255 + 15) / 31,
        ]
        .map(|v| v as u32)
    };
    let (p0, p1) = (rgb(c0), rgb(c1));
    let mix = |a: u32, b: u32, d: u32| [0, 1, 2].map(|i| (p0[i] * a + p1[i] * b) / d);
    let opaque = |c: [u32; 3]| [c[0] as u8, c[1] as u8, c[2] as u8, 255];
    let palette = if !bc1 || c0 > c1 {
        [
            opaque(p0),
            opaque(p1),
            opaque(mix(2, 1, 3)),
            opaque(mix(1, 2, 3)),
        ]
    } else {
        [opaque(p0), opaque(p1), opaque(mix(1, 1, 2)), [0; 4]]
    };
    let indices = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    std::array::from_fn(|i| palette[(indices >> (2 * i)) as usize & 3])
}

/// BC2's four bits of alpha per pixel.
fn explicit_alpha(b: &[u8]) -> [u8; 16] {
    std::array::from_fn(|i| ((b[i / 2] >> (4 * (i % 2))) & 15) * 17)
}

/// BC3's two end alphas and three-bit indices between them.
fn interpolated_alpha(b: &[u8]) -> [u8; 16] {
    let (a0, a1) = (b[0] as u32, b[1] as u32);
    let palette: [u32; 8] = if a0 > a1 {
        std::array::from_fn(|i| match i {
            0 => a0,
            1 => a1,
            i => (a0 * (8 - i as u32) + a1 * (i as u32 - 1)) / 7,
        })
    } else {
        std::array::from_fn(|i| match i {
            0 => a0,
            1 => a1,
            6 => 0,
            7 => 255,
            i => (a0 * (6 - i as u32) + a1 * (i as u32 - 1)) / 5,
        })
    };
    let bits = b[2..8]
        .iter()
        .rev()
        .fold(0u64, |acc, &x| (acc << 8) | x as u64);
    std::array::from_fn(|i| palette[(bits >> (3 * i)) as usize & 7] as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc1_blocks() {
        // Pure red and pure blue ends; rows pick 0, 1, 2 and 3.
        let block = [0x00, 0xf8, 0x1f, 0x00, 0x00, 0x55, 0xaa, 0xff];
        let p = Bc::One.decode(&block);
        assert_eq!(p[0], [255, 0, 0, 255]);
        assert_eq!(p[4], [0, 0, 255, 255]);
        assert_eq!(p[8], [170, 0, 85, 255]);
        assert_eq!(p[12], [85, 0, 170, 255]);
        // Ends in the other order: three colours and transparent black.
        let block = [0x1f, 0x00, 0x00, 0xf8, 0x00, 0x00, 0xaa, 0xff];
        let p = Bc::One.decode(&block);
        assert_eq!(p[8], [127, 0, 127, 255]);
        assert_eq!(p[12], [0; 4]);
    }

    #[test]
    fn bc2_and_bc3_alpha() {
        let mut block = [0u8; 16];
        block[0] = 0xf0;
        let p = Bc::Two.decode(&block);
        assert_eq!((p[0][3], p[1][3]), (0, 255));

        // Ends 255 and 0, the first pixel index 2: 6/7 of the way to 255.
        let mut block = [0u8; 16];
        block[..3].copy_from_slice(&[255, 0, 2]);
        let p = Bc::Three.decode(&block);
        assert_eq!((p[0][3], p[1][3]), (218, 255));
    }
}
