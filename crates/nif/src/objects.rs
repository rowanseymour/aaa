//! The object types the reader understands and their layouts. Each layout
//! lists the fields present in version 10.1.0.0; `[old: ...]` marks how
//! 10.0.1.0 differs. Types read only so they can be stepped over become
//! [`Object::Other`].
//!
//! ```text
//! named      string name, refs extra data, ref controller
//! scene      named, u16 flags, f32 translation[3], f32 rotation[9] (rows),
//!            f32 scale, refs properties, ref collision
//! NiNode     scene, refs children, refs effects
//!   NiBillboardNode adds u16 mode [old: nothing]
//! NiTriShape, NiTriStrips
//!            scene, ref data, ref skin, bool has shader (+ string, i32)
//! geometry   u16 vertices, u8 keep, u8 compress [old: neither],
//!            bool + f32 positions[n][3],
//!            u16 flags (low 6 bits: UV sets; 0x1000: tangents),
//!            bool + f32 normals[n][3] (+ tangents, bitangents [old: never]),
//!            f32 bound centre[3], radius, bool + f32 colours[n][4],
//!            f32 uv[sets][n][2], u16 consistency
//! NiTriShapeData   geometry, u16 triangles, u32 points, bool + u16 triangles[t][3]
//!                  [old: no bool],
//!                  u16 groups, per group u16 n + u16[n]
//! NiTriStripsData  geometry, u16 triangles, u16 strips, u16 lengths[strips],
//!                  bool [old: no bool] + u16 points per strip
//! texture slot     ref source, u32 clamp, u32 filter, u32 uv set, i16 2 unused,
//!                  bool + transform (f32 translation[2], scale[2], rotation,
//!                  u32 method, f32 centre[2]) [old: neither]
//! NiTexturingProperty  named, [old: u16 flags], u32 apply mode, u32 slot count, then per slot
//!                  bool + slot: base, dark, detail, gloss, glow, bump (+ f32 6),
//!                  decals 0-3 (only while the count allows), then u32 n +
//!                  per shader map bool + slot + u32 id
//! NiSourceTexture  named, u8 external, string file, ref pixel data,
//!                  u32 layout, u32 mipmaps, u32 alpha, u8 static
//!                  [old: external: string file; else u8 internal + ref pixel
//!                  data if internal]
//! NiPixelData      u32 format, u32 masks[4], u32 bits per pixel, u8 unused[8],
//!                  u32 tiling [old: none], ref palette, u32 mipmaps,
//!                  u32 bytes per pixel, per mipmap u32 width, height, offset,
//!                  u32 n + bytes[n]
//! NiMaterialProperty  named, [old: u16 flags], f32 ambient[3], diffuse[3], specular[3],
//!                  emissive[3], glossiness, alpha
//! NiAlphaProperty  named, u16 flags, u8 threshold
//! NiZBufferProperty named, u16 flags, u32 function
//! NiVertexColorProperty named, u16 flags, u32 vertex mode, u32 lighting
//! NiStencilProperty named, [old: u16 flags], u8 enabled, u32 function, reference, mask,
//!                  fail, z fail, pass, draw mode
//! NiSpecularProperty named, u16 flags
//! extra data       string name, then String: string; Integer: u32;
//!                  Boolean: u8; Binary: u32 n + bytes; Strings: u32 n + strings
//! NiCamera         scene, u16 flags, f32 frustum[6], bool orthographic
//!                  [old: neither the flags nor the bool],
//!                  f32 viewport[4], f32 lod, ref scene, u32 2 unused
//! NiTextKeyExtraData string name, u32 n, per key f32 time + string
//! NiKeyframeController ref next, u16 flags, f32 frequency, phase, start,
//!                  stop, ref target, ref data
//! NiKeyframeData   u32 n + rotation keys (u32 kind; kind 4: f32 order and
//!                  three float key groups; else f32 time + quaternion[4]
//!                  (+ three TCB floats for kind 3)), vector key group,
//!                  float key group
//! key group        u32 n, u32 kind if n > 0, keys of f32 time + value
//!                  (+ two tangents for kind 2, + three TCB floats for kind 3)
//! ```

use crate::{Error, Reader, Ref, Result, Transform};

#[derive(Debug, Clone)]
pub enum Object {
    Node(Node),
    Shape(Shape),
    Geometry(Geometry),
    Texturing(Texturing),
    SourceTexture(SourceTexture),
    Pixels(Pixels),
    Material(Material),
    Alpha(Alpha),
    Stencil(Stencil),
    ExtraData(ExtraData),
    /// Understood well enough to step over; the type name.
    Other(String),
}

/// Fields shared by everything placed in the scene.
#[derive(Debug, Clone)]
pub struct SceneObject {
    pub name: String,
    pub extra_data: Vec<Ref>,
    pub controller: Ref,
    pub flags: u16,
    pub transform: Transform,
    pub properties: Vec<Ref>,
}

impl SceneObject {
    /// Gamebryo's "app culled" flag: hidden unless shown by code.
    pub fn hidden(&self) -> bool {
        self.flags & 1 != 0
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    pub av: SceneObject,
    pub children: Vec<Ref>,
    /// Set for nodes that turn to face the camera.
    pub billboard: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct Shape {
    pub av: SceneObject,
    pub data: Ref,
    pub skin: Ref,
}

#[derive(Debug, Clone, Default)]
pub struct Geometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub uv_sets: Vec<Vec<[f32; 2]>>,
    /// Strips are converted to triangle lists.
    pub triangles: Vec<[u16; 3]>,
}

#[derive(Debug, Clone)]
pub struct Texturing {
    pub apply_mode: u32,
    pub base: Option<TextureSlot>,
    pub dark: Option<TextureSlot>,
    pub detail: Option<TextureSlot>,
    pub gloss: Option<TextureSlot>,
    pub glow: Option<TextureSlot>,
}

#[derive(Debug, Clone)]
pub struct TextureSlot {
    pub source: Ref,
    pub clamp: u32,
    pub filter: u32,
    pub uv_set: u32,
}

#[derive(Debug, Clone)]
pub struct SourceTexture {
    /// File name for external textures, as stored (often without a folder).
    pub file: Option<String>,
    /// Pixel data object for embedded textures.
    pub pixels: Ref,
}

/// A texture stored inside the model.
#[derive(Debug, Clone)]
pub struct Pixels {
    /// 0: RGB, 1: RGBA, 2-3: palette, 4-6: DXT1, DXT3, DXT5.
    pub format: u32,
    /// Width, height and byte offset of each level, largest first.
    pub mipmaps: Vec<[u32; 3]>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Material {
    pub ambient: [f32; 3],
    pub diffuse: [f32; 3],
    pub specular: [f32; 3],
    pub emissive: [f32; 3],
    pub glossiness: f32,
    pub alpha: f32,
}

#[derive(Debug, Clone)]
pub struct Alpha {
    pub flags: u16,
    pub threshold: u8,
}

impl Alpha {
    pub fn blend(&self) -> bool {
        self.flags & 1 != 0
    }
    pub fn test(&self) -> bool {
        self.flags & 0x200 != 0
    }
}

#[derive(Debug, Clone)]
pub struct Stencil {
    /// Which faces are drawn; 3 means both sides.
    pub draw_mode: u32,
}

impl Stencil {
    pub fn double_sided(&self) -> bool {
        self.draw_mode == 3
    }
}

#[derive(Debug, Clone)]
pub struct ExtraData {
    pub name: String,
    pub value: ExtraValue,
}

#[derive(Debug, Clone)]
pub enum ExtraValue {
    String(String),
    Integer(u32),
    Boolean(bool),
    Binary(Vec<u8>),
    Strings(Vec<String>),
}

impl Object {
    /// Reads an object of the named type, or `None` if the type is unknown.
    pub(crate) fn read(kind: &str, r: &mut Reader) -> Result<Option<Object>> {
        let other = || Object::Other(kind.to_string());
        Ok(Some(match kind {
            "NiNode" | "NiBillboardNode" => {
                let av = scene(r)?;
                let children = r.references()?;
                r.references()?; // effects
                let billboard = match kind {
                    "NiBillboardNode" if !r.old() => Some(r.u16()?),
                    "NiBillboardNode" => Some(0),
                    _ => None,
                };
                Object::Node(Node {
                    av,
                    children,
                    billboard,
                })
            }
            "NiTriShape" | "NiTriStrips" => {
                let av = scene(r)?;
                let data = r.reference()?;
                let skin = r.reference()?;
                if r.bool()? {
                    r.string()?;
                    r.u32()?;
                }
                Object::Shape(Shape { av, data, skin })
            }
            "NiTriShapeData" => {
                let mut g = geometry(r)?;
                let count = r.u16()? as usize;
                r.u32()?;
                if r.old() || r.bool()? {
                    let t = r.bytes(count * 6)?;
                    g.triangles = t
                        .as_chunks::<6>()
                        .0
                        .iter()
                        .map(|c| {
                            std::array::from_fn(|i| u16::from_le_bytes([c[2 * i], c[2 * i + 1]]))
                        })
                        .collect();
                }
                for _ in 0..r.u16()? {
                    let n = r.u16()? as usize;
                    r.skip(n * 2)?;
                }
                Object::Geometry(g)
            }
            "NiTriStripsData" => {
                let mut g = geometry(r)?;
                r.u16()?;
                let strips = r.u16()?;
                let lengths = (0..strips).map(|_| r.u16()).collect::<Result<Vec<_>>>()?;
                if r.old() || r.bool()? {
                    for len in lengths {
                        let points = (0..len).map(|_| r.u16()).collect::<Result<Vec<_>>>()?;
                        g.triangles.extend(strip_triangles(&points));
                    }
                }
                Object::Geometry(g)
            }
            "NiTexturingProperty" => {
                named(r)?;
                if r.old() {
                    r.u16()?;
                }
                let apply_mode = r.u32()?;
                let count = r.u32()?;
                let mut slot = || r.bool()?.then(|| texture_slot(r)).transpose();
                let base = slot()?;
                let dark = slot()?;
                let detail = slot()?;
                let gloss = slot()?;
                let glow = slot()?;
                if count > 5 && r.bool()? {
                    texture_slot(r)?;
                    r.floats::<6>()?;
                }
                for decal in 0..4 {
                    if count > 6 + decal && r.bool()? {
                        texture_slot(r)?;
                    }
                }
                for _ in 0..r.u32()? {
                    if r.bool()? {
                        texture_slot(r)?;
                        r.u32()?;
                    }
                }
                Object::Texturing(Texturing {
                    apply_mode,
                    base,
                    dark,
                    detail,
                    gloss,
                    glow,
                })
            }
            "NiSourceTexture" => {
                named(r)?;
                let external = r.u8()? == 1;
                let (file, pixels) = if !r.old() {
                    (r.string()?, r.reference()?)
                } else if external {
                    (r.string()?, None)
                } else if r.u8()? != 0 {
                    (String::new(), r.reference()?)
                } else {
                    (String::new(), None)
                };
                r.skip(13)?;
                Object::SourceTexture(SourceTexture {
                    file: external.then_some(file),
                    pixels,
                })
            }
            "NiPixelData" => {
                let format = r.u32()?;
                r.skip(5 * 4 + 8)?;
                if !r.old() {
                    r.u32()?;
                }
                r.reference()?;
                let count = r.u32()?;
                r.u32()?;
                let mipmaps = (0..count)
                    .map(|_| Ok([r.u32()?, r.u32()?, r.u32()?]))
                    .collect::<Result<_>>()?;
                let n = r.u32()? as usize;
                let data = r.bytes(n)?.to_vec();
                Object::Pixels(Pixels {
                    format,
                    mipmaps,
                    data,
                })
            }
            "NiMaterialProperty" => {
                named(r)?;
                if r.old() {
                    r.u16()?;
                }
                Object::Material(Material {
                    ambient: r.floats()?,
                    diffuse: r.floats()?,
                    specular: r.floats()?,
                    emissive: r.floats()?,
                    glossiness: r.f32()?,
                    alpha: r.f32()?,
                })
            }
            "NiAlphaProperty" => {
                named(r)?;
                Object::Alpha(Alpha {
                    flags: r.u16()?,
                    threshold: r.u8()?,
                })
            }
            "NiStencilProperty" => {
                named(r)?;
                if r.old() {
                    r.u16()?;
                }
                r.skip(1 + 6 * 4)?;
                Object::Stencil(Stencil {
                    draw_mode: r.u32()?,
                })
            }
            "NiZBufferProperty" => {
                named(r)?;
                r.skip(2 + 4)?;
                other()
            }
            "NiVertexColorProperty" => {
                named(r)?;
                r.skip(2 + 4 + 4)?;
                other()
            }
            "NiSpecularProperty" => {
                named(r)?;
                r.skip(2)?;
                other()
            }
            "NiStringExtraData" | "NiIntegerExtraData" | "NiBooleanExtraData"
            | "NiBinaryExtraData" | "NiStringsExtraData" => {
                let name = r.string()?;
                let value = match kind {
                    "NiStringExtraData" => ExtraValue::String(r.string()?),
                    "NiIntegerExtraData" => ExtraValue::Integer(r.u32()?),
                    "NiBooleanExtraData" => ExtraValue::Boolean(r.bool()?),
                    "NiBinaryExtraData" => {
                        let n = r.u32()? as usize;
                        ExtraValue::Binary(r.bytes(n)?.to_vec())
                    }
                    _ => {
                        let n = r.u32()?;
                        ExtraValue::Strings((0..n).map(|_| r.string()).collect::<Result<_>>()?)
                    }
                };
                Object::ExtraData(ExtraData { name, value })
            }
            "NiCamera" => {
                scene(r)?;
                let extra = if r.old() { 0 } else { 2 + 1 };
                r.skip(extra + 6 * 4 + 5 * 4 + 4 + 4 + 4)?;
                other()
            }
            "NiTextKeyExtraData" => {
                r.string()?;
                for _ in 0..r.u32()? {
                    r.f32()?;
                    r.string()?;
                }
                other()
            }
            "NiKeyframeController" => {
                r.skip(4 + 2 + 16 + 4 + 4)?;
                other()
            }
            "NiKeyframeData" => {
                let n = r.u32()? as usize;
                if n > 0 {
                    match r.u32()? {
                        4 => {
                            r.f32()?;
                            for _ in 0..3 {
                                key_group(r, 1)?;
                            }
                        }
                        // Rotations have no tangents, only TCB values.
                        kind => {
                            let size = if kind == 3 { 7 } else { 4 };
                            key_size(kind, 4)?;
                            r.skip(n * 4 * (1 + size))?
                        }
                    }
                }
                key_group(r, 3)?;
                key_group(r, 1)?;
                other()
            }
            _ => return Ok(None),
        }))
    }
}

fn named(r: &mut Reader) -> Result<(String, Vec<Ref>, Ref)> {
    Ok((r.string()?, r.references()?, r.reference()?))
}

fn scene(r: &mut Reader) -> Result<SceneObject> {
    let (name, extra_data, controller) = named(r)?;
    let flags = r.u16()?;
    let translation = r.floats()?;
    let m = r.floats::<9>()?;
    let scale = r.f32()?;
    let properties = r.references()?;
    r.reference()?; // collision
    Ok(SceneObject {
        name,
        extra_data,
        controller,
        flags,
        transform: Transform {
            translation,
            rotation: [[m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]],
            scale,
        },
        properties,
    })
}

fn geometry(r: &mut Reader) -> Result<Geometry> {
    let n = r.u16()? as usize;
    if !r.old() {
        r.skip(2)?;
    }
    let positions = if r.bool()? {
        r.float_array(n)?
    } else {
        Vec::new()
    };
    let flags = r.u16()?;
    let normals = if r.bool()? {
        let normals = r.float_array(n)?;
        if flags & 0x1000 != 0 && !r.old() {
            r.float_array::<6>(n)?;
        }
        normals
    } else {
        Vec::new()
    };
    r.floats::<4>()?;
    let colors = if r.bool()? {
        r.float_array(n)?
    } else {
        Vec::new()
    };
    let uv_sets = (0..flags & 0x3f)
        .map(|_| r.float_array(n))
        .collect::<Result<_>>()?;
    r.u16()?;
    Ok(Geometry {
        positions,
        normals,
        colors,
        uv_sets,
        triangles: Vec::new(),
    })
}

fn texture_slot(r: &mut Reader) -> Result<TextureSlot> {
    let slot = TextureSlot {
        source: r.reference()?,
        clamp: r.u32()?,
        filter: r.u32()?,
        uv_set: r.u32()?,
    };
    r.skip(4)?;
    if !r.old() && r.bool()? {
        r.skip(5 * 4 + 4 + 2 * 4)?;
    }
    Ok(slot)
}

/// Floats per key after the time, for an interpolation kind.
fn key_size(kind: u32, width: usize) -> Result<usize> {
    match kind {
        1 => Ok(width),
        2 => Ok(3 * width),
        3 => Ok(width + 3),
        _ => Err(Error::Corrupt(format!("unknown key kind {kind}"))),
    }
}

fn key_group(r: &mut Reader, width: usize) -> Result<()> {
    let n = r.u32()? as usize;
    if n > 0 {
        let size = key_size(r.u32()?, width)?;
        r.skip(n * 4 * (1 + size))?;
    }
    Ok(())
}

/// Triangles of a strip, with alternate ones flipped to keep the winding
/// and degenerate ones dropped.
pub(crate) fn strip_triangles(points: &[u16]) -> impl Iterator<Item = [u16; 3]> + '_ {
    points.windows(3).enumerate().filter_map(|(i, w)| {
        let t = if i % 2 == 0 {
            [w[0], w[1], w[2]]
        } else {
            [w[0], w[2], w[1]]
        };
        (t[0] != t[1] && t[1] != t[2] && t[0] != t[2]).then_some(t)
    })
}
