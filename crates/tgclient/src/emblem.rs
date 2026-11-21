//! The player's nation's emblem, which our interface draws faintly behind
//! its object display.

use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use tgrender::textures;

/// `emblem` centred on a clear image `size` texels across and down, each
/// texel's alpha scaled by `alpha`; `None` unless `emblem` is sRGB RGBA8.
pub fn faint(emblem: &Image, size: UVec2, alpha: f32) -> Option<Image> {
    let from = emblem.size();
    let texels = (emblem.data.as_ref())
        .filter(|d| d.len() == (from.x * from.y * 4) as usize)
        .filter(|_| emblem.texture_descriptor.format == TextureFormat::Rgba8UnormSrgb)?;
    let mut data = vec![0; (size.x * size.y * 4) as usize];
    let at = (size.as_ivec2() - from.as_ivec2()) / 2;
    for y in 0..from.y {
        for x in 0..from.x {
            let to = at + UVec2::new(x, y).as_ivec2();
            if to.cmplt(IVec2::ZERO).any() || to.cmpge(size.as_ivec2()).any() {
                continue;
            }
            let s = ((y * from.x + x) * 4) as usize;
            let d = ((to.y as u32 * size.x + to.x as u32) * 4) as usize;
            data[d..d + 3].copy_from_slice(&texels[s..s + 3]);
            data[d + 3] = (texels[s + 3] as f32 * alpha).round() as u8;
        }
    }
    Some(textures::image_2d(
        size,
        1,
        data,
        TextureFormat::Rgba8UnormSrgb,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_emblem_is_faint_in_the_middle_and_clear_elsewhere() {
        let red = [255, 0, 0, 255].repeat(4);
        let emblem = textures::image_2d(UVec2::new(2, 2), 1, red, TextureFormat::Rgba8UnormSrgb);
        let image = faint(&emblem, UVec2::new(6, 4), 0.5).unwrap();
        let data = image.data.unwrap();
        let texel = |x: u32, y: u32| &data[((y * 6 + x) * 4) as usize..][..4];
        assert_eq!(texel(2, 1), [255, 0, 0, 128]);
        assert_eq!(texel(3, 2), [255, 0, 0, 128]);
        assert_eq!(texel(1, 1), [0; 4]);
        assert_eq!(texel(4, 2), [0; 4]);
        // Bigger than what it's drawn on, it's cut to fit.
        assert_eq!(
            faint(&emblem, UVec2::new(1, 1), 1.0).unwrap().data.unwrap(),
            [255, 0, 0, 255]
        );
        // Only what's stored a texel at a time can be drawn so.
        let packed = textures::image_2d(
            UVec2::new(4, 4),
            1,
            vec![0; 8],
            TextureFormat::Bc1RgbaUnormSrgb,
        );
        assert!(faint(&packed, UVec2::new(6, 4), 1.0).is_none());
    }
}
