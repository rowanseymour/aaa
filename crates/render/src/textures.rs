//! Decoding the game's image files.

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageType,
};
use bevy::prelude::*;

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
