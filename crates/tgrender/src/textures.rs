//! Decoding the game's image files, and building the images drawn with
//! them.

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageType,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use tgdata::GameData;

/// Reads and decodes the image file at archive path `path`, warning if it
/// can't be decoded: `srgb` as for [`decode`].
pub fn read(
    game: &GameData,
    path: &str,
    formats: CompressedImageFormats,
    srgb: bool,
) -> Option<Image> {
    decode(&game.read(path)?, path, formats, srgb)
}

/// Decodes an image file by its name's extension, warning if it can't:
/// `srgb` for one the interface draws, not the world (see [`crate::stored`]).
/// An SVG is drawn at the size it gives itself.
pub fn decode(
    bytes: &[u8],
    name: &str,
    formats: CompressedImageFormats,
    srgb: bool,
) -> Option<Image> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    if ext == "svg" {
        return svg(bytes, srgb).map_err(|e| warn!("{name}: {e}")).ok();
    }
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

/// The most texels across or down an SVG is drawn at.
const SVG_MAX: u32 = 4096;

/// An SVG drawn at the size it gives itself.
fn svg(bytes: &[u8], srgb: bool) -> Result<Image, String> {
    use resvg::{tiny_skia, usvg};
    let tree =
        usvg::Tree::from_data(bytes, &usvg::Options::default()).map_err(|e| e.to_string())?;
    let size = tree.size().to_int_size();
    if size.width().max(size.height()) > SVG_MAX {
        let (w, h) = (size.width(), size.height());
        return Err(format!("{w} by {h} is bigger than {SVG_MAX} a side"));
    }
    let mut pixmap =
        tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("no size to draw at")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    // Drawn with its colours multiplied by their alpha, which images aren't.
    let data = (pixmap.pixels().iter())
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    let size = UVec2::new(size.width(), size.height());
    Ok(image_2d(size, 1, data, rgba8_format(srgb)))
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

/// A sampler that stops at the texture's edges.
pub fn clamped() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..ImageSamplerDescriptor::linear()
    })
}

/// RGBA8 for an image the interface draws if `srgb`, else for the world's
/// stored colour values (see [`crate::stored`]).
pub fn rgba8_format(srgb: bool) -> TextureFormat {
    match srgb {
        true => TextureFormat::Rgba8UnormSrgb,
        false => TextureFormat::Rgba8Unorm,
    }
}

/// A 2D image `size` across, or an array of `layers` of them, from its
/// top level's data.
pub fn image_2d(size: UVec2, layers: u32, data: Vec<u8>, format: TextureFormat) -> Image {
    Image::new(
        extent(size, layers),
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// [`image_2d`] from the data of all its `mips` levels, a layer at a time.
pub fn mipmapped(
    size: UVec2,
    layers: u32,
    mips: u32,
    data: Vec<u8>,
    format: TextureFormat,
) -> Image {
    let mut image = Image::new_uninit(
        extent(size, layers),
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = mips;
    image
}

fn extent(size: UVec2, layers: u32) -> Extent3d {
    Extent3d {
        width: size.x,
        height: size.y,
        depth_or_array_layers: layers,
    }
}

/// Stacks equally sized images, each with the same mip chain, into one
/// array texture with a repeating sampler.
pub fn texture_array(layers: Vec<Image>) -> Image {
    let first = &layers[0];
    let size = first.size();
    let mips = first.texture_descriptor.mip_level_count;
    let format = first.texture_descriptor.format;
    let count = layers.len() as u32;
    let data = layers.into_iter().flat_map(|l| l.data.unwrap()).collect();
    array_image(data, size, mips, format, count)
}

/// An array texture with a repeating sampler, of `count` layers of `size`,
/// each with `mips` levels, from their data one after another.
pub fn array_image(
    data: Vec<u8>,
    size: UVec2,
    mips: u32,
    format: TextureFormat,
    count: u32,
) -> Image {
    let mut image = mipmapped(size, count, mips, data, format);
    // Otherwise a single layer would be viewed as a plain 2D texture.
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    image.sampler = repeating();
    image
}

/// A square, mipmapped array layer of side `n` from its RGBA8 pixels, in
/// stored colour values (see [`crate::stored`]).
pub fn layer_image(data: Vec<u8>, n: usize) -> Image {
    let mut image = image_2d(UVec2::splat(n as u32), 1, data, rgba8_format(false));
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

/// The top level of `image` as RGBA8, its bytes as stored whatever its
/// format says they mean, for textures resampled or stacked on the CPU.
pub fn rgba8(image: Image) -> Option<Image> {
    let mut image = decompress(image)?;
    // Bevy converts only sRGB-labelled RGBA8; the bytes are the same.
    if image.texture_descriptor.format == TextureFormat::Rgba8Unorm {
        image.texture_descriptor.format = TextureFormat::Rgba8UnormSrgb;
    }
    image.convert(TextureFormat::Rgba8UnormSrgb)
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
    let size = UVec2::new(w as u32, h as u32);
    Some(image_2d(size, 1, out, rgba8_format(srgb)))
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
    fn svgs_are_drawn_at_their_own_size_unmultiplied() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
            <rect width="4" height="2" fill="#ff8000" fill-opacity="0.5"/>
        </svg>"##;
        let image = decode(svg, "a.SVG", CompressedImageFormats::NONE, true).unwrap();
        assert_eq!(image.size(), UVec2::new(4, 2));
        assert_eq!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb
        );
        let texel = &image.data.as_ref().unwrap()[..4];
        assert_eq!(texel[3], 128);
        assert!(
            texel[0] >= 254 && (127..=129).contains(&texel[1]),
            "{texel:?}"
        );
        assert!(decode(b"<svg", "b.svg", CompressedImageFormats::NONE, true).is_none());
        let huge = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20000" height="1"/>"#;
        assert!(decode(huge, "c.svg", CompressedImageFormats::NONE, true).is_none());
    }

    /// Every SVG in this project's own data, by path.
    fn own_svgs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                own_svgs(&path, out);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
            {
                out.push(path);
            }
        }
    }

    #[test]
    fn this_projects_own_svgs_are_drawn() {
        let mut svgs = Vec::new();
        own_svgs(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"),
            &mut svgs,
        );
        assert!(!svgs.is_empty());
        for path in svgs {
            let bytes = std::fs::read(&path).unwrap();
            let name = path.display().to_string();
            let image = decode(&bytes, &name, CompressedImageFormats::NONE, true);
            assert!(image.is_some_and(|i| i.size().min_element() > 0), "{name}");
        }
    }

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
