//! Typed forms of the game's `.tgi` blocks, named after the block kinds they
//! read. Each holds only the keys something uses so far.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Terrain {
    pub ids: Option<String>,
    pub terrain_color: Option<[f32; 3]>,
    #[serde(default)]
    pub base_priority: i32,
    #[serde(default)]
    pub variation: Vec<Variation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Variation {
    pub ids: Option<String>,
    pub texture: Option<String>,
    pub texture_width: Option<f32>,
    pub texture_height: Option<f32>,
    pub terrain_color: Option<[f32; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Biome {
    #[serde(default)]
    pub water_info: Vec<WaterInfo>,
}

/// How a biome's water terrain is coloured.
#[derive(Debug, Clone, Deserialize)]
pub struct WaterInfo {
    pub terrain_ids: Option<String>,
    #[serde(default)]
    pub depth_info: Vec<DepthInfo>,
}

/// One stop of a water's colour by depth.
#[derive(Debug, Clone, Deserialize)]
pub struct DepthInfo {
    pub depth: f32,
    pub color: [f32; 3],
    pub alpha: f32,
}

/// An object template: units, buildings, doodads, sound emitters, ...
#[derive(Debug, Clone, Deserialize)]
pub struct Thing {
    pub ids: Option<String>,
    #[serde(default)]
    pub flags: Vec<Flags>,
    pub view: Option<View>,
    #[serde(default)]
    pub size: Vec<Size>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Flags {
    #[serde(default)]
    pub invisible: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Size {
    pub scale_min: Option<f32>,
    pub scale_max: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct View {
    /// Substituted for `%s` in model paths.
    pub resource_prefix: Option<String>,
    #[serde(default)]
    pub aspect: Vec<Aspect>,
    #[serde(default)]
    pub aspect_normal: Vec<Aspect>,
    #[serde(default)]
    pub aspects_as_variations: bool,
    #[serde(default)]
    pub texture_search_path: Vec<TextureSearchPath>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Aspect {
    pub model: Option<String>,
    #[serde(default)]
    pub texture_rename: Vec<TextureRename>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextureSearchPath {
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextureRename {
    pub substring_from: Option<String>,
    pub substring_to: Option<String>,
}

/// A renamed id, which maps made before the rename still use.
#[derive(Debug, Clone, Deserialize)]
pub struct Translate {
    /// The kind of block renamed, e.g. `Thing`.
    pub node: Option<String>,
    pub ids_old: Option<String>,
    pub ids_new: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionSet {
    /// Start of the mask file names, relative to the transition set's file.
    pub path_prefix: Option<String>,
}
