//! Reading each object type. Each layout lists the fields present in
//! version 10.1.0.0; `[old: ...]` marks how 10.0.1.0 and 4.2.2.0 differ,
//! `[4.2.2.0: ...]` how 4.2.2.0 differs further.
//!
//! ```text
//! named      string name, refs extra data [4.2.2.0: ref to the first of a
//!            list], ref controller
//! scene      named, u16 flags, f32 translation[3], f32 rotation[9] (rows),
//!            f32 scale, [4.2.2.0: f32 velocity[3]], refs properties,
//!            ref collision [4.2.2.0: bool + bounding volume instead]
//! bounding volume  u32 kind, then 0: f32 centre[3], radius; 1: f32
//!            centre[3], axes[3][3], extents[3]; 2: f32 centre[3],
//!            axis[3], extent, radius; 4: u32 n + bounding volumes;
//!            5: f32 plane[4], centre[3]
//! NiNode     scene, refs children, refs effects
//!   NiBillboardNode adds u16 mode [old: nothing]
//! NiTriShape, NiTriStrips
//!            scene, ref data, ref skin, bool has shader (+ string, i32)
//!            [4.2.2.0: no shader]
//! geometry   u16 vertices, u8 keep, u8 compress [old: neither],
//!            bool + f32 positions[n][3],
//!            u16 flags (low 6 bits: UV sets; 0x1000: tangents)
//!            [4.2.2.0: none],
//!            bool + f32 normals[n][3] (+ tangents, bitangents [old: never]),
//!            f32 bound centre[3], radius, bool + f32 colours[n][4],
//!            [4.2.2.0: u16 UV sets], f32 uv[sets][n][2],
//!            u16 consistency [4.2.2.0: none]
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
//!                  per shader map bool + slot + u32 id [4.2.2.0: no shader
//!                  maps]
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
//! extra data       string name [4.2.2.0: ref next], then String: string
//!                  [4.2.2.0: u32 size first]; Integer: u32; Boolean: u8;
//!                  Binary: u32 n + bytes; Strings: u32 n + strings
//! NiCamera         scene, u16 flags, f32 frustum[6], bool orthographic
//!                  [old: neither the flags nor the bool],
//!                  f32 viewport[4], f32 lod, ref scene, u32 2 unused
//! NiTextKeyExtraData string name [4.2.2.0: ref next, u32 unused], u32 n,
//!                  per key f32 time + string
//! NiControllerSequence  string name, string accumulation root, ref text
//!                  keys, u32 n, per block string target + ref controller
//! controller       ref next, u16 flags [old: only bits 0-3 its own],
//!                  f32 frequency, phase, start, stop, ref target
//! NiKeyframeController, NiAlphaController, NiVisController
//!                  controller, ref data
//! NiLookAtController, NiMaterialColorController, NiLightColorController
//!                  controller, u16 flags [old: none], ref node or data
//! NiPathController controller, u16 flags [old: none; the controller's
//!                  flags above bit 3 instead], i32 bank direction,
//!                  f32 max bank, smoothing, i16 follow axis, ref
//!                  positions (NiPosData), ref fractions along them
//!                  (NiFloatData)
//! NiTextureTransformController
//!                  controller, bool shader map, u32 slot, u32 operation,
//!                  ref data
//! NiFlipController controller, u32 slot, f32 time, f32 delta, refs textures
//! NiKeyframeData   u32 n + rotation keys (u32 kind; kind 4: f32 order and
//!                  three float key groups; else f32 time + quaternion[4]
//!                  (+ three TCB floats for kind 3)), vector key group,
//!                  float key group
//! NiFloatData, NiPosData, NiColorData
//!                  key group of f32, f32[3], f32[4]
//! NiVisData        bool keys: u32 n, per key f32 time + u8
//! key group        u32 n, u32 kind if n > 0, keys of f32 time + value
//!                  (+ two tangents for kind 2, + three TCB floats for kind 3)
//! NiSkinInstance   ref skin data, ref skeleton root, refs bones
//! bone transform   f32 rotation[9], translation[3], scale
//! NiSkinData       bone transform (skeleton parent to skin), u32 bones,
//!                  ref skin partition, u8 has weights, per bone:
//!                  bone transform, f32 bound[4], u16 n, if weights
//!                  n × (u16 vertex, f32 weight)
//! NiSkinPartition  u32 n, per partition u16 vertices, triangles, bones,
//!                  strips, weights per vertex, u16 bones[], bool + u16
//!                  vertex map[v], bool + f32 weights[v][w], u16 strip
//!                  lengths[strips], bool + u16 strip points or, without
//!                  strips, triangles[t][3] [old: none of these three
//!                  bools], bool + u8 bone indices[v][w]
//! NiDitherProperty named, u16 flags
//! NiAmbientLight, NiPointLight
//!                  scene, refs affected nodes [old: none], f32 dimmer,
//!                  ambient[3], diffuse[3], specular[3] (+ attenuation[3]
//!                  for point lights)
//! NiParticles, NiRotatingParticles, NiParticleMeshes, NiParticleSystem
//!                  as NiTriShape; NiParticleSystem adds bool world space,
//!                  refs modifiers
//! particle info    40 bytes: f32 velocity[3], axis[3], age, life span,
//!                  last update, u16 generation, u16 vertex
//! NiParticlesData, NiRotatingParticlesData, NiParticleMeshesData, NiPSysData
//!                  geometry, bool + f32 radii[n] [old: f32 radius],
//!                  u16 active, bool + f32 sizes[n], bool + f32
//!                  rotations[n][4] [4.2.2.0: none, but NiRotatingParticlesData
//!                  ends with them]; NiParticleMeshesData adds ref node,
//!                  NiPSysData particle info[n], u16 added, u16 added base
//! NiParticleSystemController
//!                  controller, f32 speed[2], declination[2], planar
//!                  angle[2], normal[3], colour[4], size, emit start, emit
//!                  stop, u8 reset, f32 birth rate, life span[2], u8 use
//!                  birth rate, u8 spawn on death, f32 emitter size[3], ref
//!                  emitter, u16 spawn generations, f32 spawn percentage,
//!                  u16 multiplier, f32 speed chaos, direction chaos, u16 n,
//!                  u16 valid, particle info[n], ref emitter modifier, ref
//!                  modifier, ref collider, u8 static bound
//! NiGravity, NiParticleColorModifier, NiParticleGrowFade,
//! NiParticleRotation, NiParticleMeshModifier
//!                  ref next, ref controller, then f32 decay, strength, u32
//!                  force, f32 position[3], direction[3]; ref colour data;
//!                  f32 grow, fade; u8 random axis, f32 axis[3], speed; refs
//!                  meshes
//! ```
//!
//! The `NiPSys` types occur only in 10.1.0.0:
//!
//! ```text
//! modifier         string name, u32 order, ref system, bool active
//! NiPSysAgeDeathModifier     modifier, bool spawn, ref spawn modifier
//! NiPSysBoundUpdateModifier  modifier, u16 skip
//! NiPSysColliderManager, NiPSysColorModifier  modifier, ref
//! NiPSysDragModifier         modifier, ref object, f32 axis[3],
//!                            percentage, range, falloff
//! NiPSysGravityModifier      modifier, ref object, f32 axis[3], decay,
//!                            strength, u32 force, f32 turbulence[2]
//! NiPSysGrowFadeModifier     modifier, f32 grow, u16 generation, f32 fade,
//!                            u16 generation
//! NiPSysPositionModifier     modifier
//! NiPSysRotationModifier     modifier, f32 speed, bool random, f32 axis[3]
//! NiPSysSpawnModifier        modifier, u16 generations, f32 spawned, u16
//!                            min, max, f32 speed, direction, life span[2]
//! emitter          modifier, f32 speed[2], declination[2], planar
//!                  angle[2], colour[4], radius, life span[2]
//! NiPSysBoxEmitter, NiPSysCylinderEmitter, NiPSysSphereEmitter
//!                  emitter, ref object, f32 size[3], [2] or [1]
//! NiPSysMeshEmitter emitter, refs meshes, u32 velocity, u32 emission,
//!                  f32 axis[3]
//! NiPSysPlanarCollider, NiPSysSphericalCollider
//!                  f32 bounce, bool spawn, bool die, ref spawn modifier,
//!                  ref manager, ref next, ref object, then f32 width,
//!                  height, x axis[3], y axis[3]; f32 radius
//! NiPSysEmitterCtlr, NiPSysModifierActiveCtlr, NiPSysEmitterSpeedCtlr,
//! NiPSysEmitterInitialRadiusCtlr
//!                  controller, string modifier, ref data
//! NiPSysUpdateCtlr controller
//! NiPSysEmitterCtlrData  float key group, bool keys
//! ```

use crate::animation::Value;
use crate::objects::*;
use crate::{Error, Reader, Ref, Result, Transform};

impl Object {
    /// Reads an object of the named type, or `None` if the type is unknown.
    pub(crate) fn read(kind: &str, r: &mut Reader) -> Result<Option<Object>> {
        Ok(Some(match kind {
            "NiNode" | "NiBillboardNode" => Object::Node(node(kind, r)?),
            "NiTriShape" | "NiTriStrips" => Object::Shape(shape(r)?),
            "NiTriShapeData" => Object::Geometry(tri_shape_data(r)?),
            "NiTriStripsData" => Object::Geometry(tri_strips_data(r)?),
            "NiTexturingProperty" => Object::Texturing(texturing(r)?),
            "NiSourceTexture" => Object::SourceTexture(source_texture(r)?),
            "NiPixelData" => Object::Pixels(pixels(r)?),
            "NiMaterialProperty" => Object::Material(material(r)?),
            "NiAlphaProperty" => Object::Alpha(alpha(r)?),
            "NiStencilProperty" => Object::Stencil(stencil(r)?),
            "NiZBufferProperty" => Object::ZBuffer(z_buffer(r)?),
            "NiVertexColorProperty" => Object::VertexColor(vertex_color(r)?),
            "NiSpecularProperty" | "NiDitherProperty" => flags_property(kind, r)?,
            "NiStringExtraData" | "NiIntegerExtraData" | "NiBooleanExtraData"
            | "NiBinaryExtraData" | "NiStringsExtraData" => Object::ExtraData(extra_data(kind, r)?),
            "NiCamera" => Object::Camera(camera(r)?),
            "NiTextKeyExtraData" => Object::TextKeys(text_keys(r)?),
            "NiControllerSequence" => Object::Sequence(sequence(r)?),
            "NiKeyframeController" | "NiAlphaController" => {
                Object::Controller(data_controller(kind, r)?)
            }
            "NiKeyframeData" => Object::Keyframes(keyframes(r)?),
            "NiFloatData" => Object::Floats(key_group(r)?),
            "NiSkinInstance" => Object::SkinInstance(skin_instance(r)?),
            "NiSkinData" => Object::SkinData(skin_data(r)?),
            "NiSkinPartition" => skin_partition(kind, r)?,
            "NiAmbientLight" | "NiPointLight" => light(kind, r)?,
            "NiVisController" => Object::Controller(vis_controller(r)?),
            "NiPSysUpdateCtlr" => Object::Controller(particle_update_controller(r)?),
            "NiLookAtController" | "NiMaterialColorController" | "NiLightColorController" => {
                Object::Controller(flagged_controller(r)?)
            }
            "NiPathController" => Object::Controller(path_controller(r)?),
            "NiTextureTransformController" => Object::Controller(texture_controller(r)?),
            "NiFlipController" => Object::Controller(flip_controller(r)?),
            "NiPosData" => Object::Positions(key_group(r)?),
            "NiColorData" => Object::Colors(key_group(r)?),
            "NiVisData" => Object::Visibility(bool_keys(r)?),
            "NiParticles" | "NiRotatingParticles" | "NiParticleMeshes" => {
                Object::OldParticles(OldParticles { shape: shape(r)? })
            }
            "NiParticleSystem" => Object::ParticleSystem(particle_system(r)?),
            "NiParticlesData" | "NiRotatingParticlesData" | "NiParticleMeshesData" => {
                Object::OldParticleData(old_particle_data(kind, r)?)
            }
            "NiPSysData" => Object::ParticleData(particle_data(r)?),
            "NiParticleSystemController" => {
                Object::OldParticleController(old_particle_controller(r)?)
            }
            "NiGravity"
            | "NiParticleColorModifier"
            | "NiParticleGrowFade"
            | "NiParticleRotation"
            | "NiParticleMeshModifier" => Object::OldModifier(old_modifier(kind, r)?),
            "NiPSysAgeDeathModifier"
            | "NiPSysBoundUpdateModifier"
            | "NiPSysColliderManager"
            | "NiPSysColorModifier"
            | "NiPSysDragModifier"
            | "NiPSysGravityModifier"
            | "NiPSysGrowFadeModifier"
            | "NiPSysPositionModifier"
            | "NiPSysRotationModifier"
            | "NiPSysSpawnModifier"
            | "NiPSysBoxEmitter"
            | "NiPSysCylinderEmitter"
            | "NiPSysSphereEmitter"
            | "NiPSysMeshEmitter" => Object::Modifier(modifier(kind, r)?),
            "NiPSysPlanarCollider" | "NiPSysSphericalCollider" => {
                Object::Collider(collider(kind, r)?)
            }
            "NiPSysEmitterCtlr"
            | "NiPSysModifierActiveCtlr"
            | "NiPSysEmitterInitialRadiusCtlr"
            | "NiPSysEmitterSpeedCtlr" => Object::Controller(particle_controller(kind, r)?),
            "NiPSysEmitterCtlrData" => Object::EmitterKeys(emitter_keys(r)?),
            _ => return Ok(None),
        }))
    }
}

fn node(kind: &str, r: &mut Reader) -> Result<Node> {
    let av = scene(r)?;
    let children = r.references()?;
    r.references()?; // effects
    let billboard = match kind {
        "NiBillboardNode" if !r.old() => Some(r.u16()?),
        "NiBillboardNode" => Some(0),
        _ => None,
    };
    Ok(Node {
        av,
        children,
        billboard,
    })
}

fn tri_shape_data(r: &mut Reader) -> Result<Geometry> {
    let (_, mut g) = geometry(r)?;
    let count = r.u16()? as usize;
    r.u32()?;
    if r.old() || r.bool()? {
        g.triangles = r.u16_array(count)?;
    }
    for _ in 0..r.u16()? {
        let n = r.u16()? as usize;
        r.skip(n * 2)?;
    }
    Ok(g)
}

fn tri_strips_data(r: &mut Reader) -> Result<Geometry> {
    let (_, mut g) = geometry(r)?;
    r.u16()?;
    let strips = r.u16()? as usize;
    let lengths = r.u16s(strips)?;
    if r.old() || r.bool()? {
        for len in lengths {
            let points = r.u16s(len as usize)?;
            g.triangles.extend(strip_triangles(&points));
        }
    }
    Ok(g)
}

fn texturing(r: &mut Reader) -> Result<Texturing> {
    let (_, _, controller) = named(r)?;
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
    let shader_maps = if r.netimmerse() { 0 } else { r.u32()? };
    for _ in 0..shader_maps {
        if r.bool()? {
            texture_slot(r)?;
            r.u32()?;
        }
    }
    Ok(Texturing {
        controller,
        apply_mode,
        base,
        dark,
        detail,
        gloss,
        glow,
    })
}

fn source_texture(r: &mut Reader) -> Result<SourceTexture> {
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
    Ok(SourceTexture {
        file: external.then_some(file),
        pixels,
    })
}

fn pixels(r: &mut Reader) -> Result<Pixels> {
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
    Ok(Pixels {
        format,
        mipmaps,
        data,
    })
}

fn material(r: &mut Reader) -> Result<Material> {
    let (_, _, controller) = named(r)?;
    if r.old() {
        r.u16()?;
    }
    Ok(Material {
        controller,
        ambient: r.floats()?,
        diffuse: r.floats()?,
        specular: r.floats()?,
        emissive: r.floats()?,
        glossiness: r.f32()?,
        alpha: r.f32()?,
    })
}

fn alpha(r: &mut Reader) -> Result<Alpha> {
    named(r)?;
    Ok(Alpha {
        flags: r.u16()?,
        threshold: r.u8()?,
    })
}

fn stencil(r: &mut Reader) -> Result<Stencil> {
    named(r)?;
    if r.old() {
        r.u16()?;
    }
    r.skip(1 + 6 * 4)?;
    Ok(Stencil {
        draw_mode: r.u32()?,
    })
}

fn z_buffer(r: &mut Reader) -> Result<ZBuffer> {
    named(r)?;
    Ok(ZBuffer {
        flags: r.u16()?,
        function: r.u32()?,
    })
}

fn vertex_color(r: &mut Reader) -> Result<VertexColor> {
    named(r)?;
    r.skip(2)?;
    Ok(VertexColor {
        vertex_mode: r.u32()?,
        lighting: r.u32()?,
    })
}

/// Steps over a property that holds only flags.
fn flags_property(kind: &str, r: &mut Reader) -> Result<Object> {
    named(r)?;
    r.u16()?;
    Ok(Object::Other(kind.to_string()))
}

fn extra_data(kind: &str, r: &mut Reader) -> Result<ExtraData> {
    let name = extra_data_name(r)?;
    let value = match kind {
        "NiStringExtraData" => {
            if r.netimmerse() {
                r.u32()?;
            }
            ExtraValue::String(r.string()?)
        }
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
    Ok(ExtraData { name, value })
}

fn camera(r: &mut Reader) -> Result<Camera> {
    let av = scene(r)?;
    if !r.old() {
        r.skip(2)?;
    }
    let [left, right, top, bottom, near, far] = r.floats()?;
    let orthographic = !r.old() && r.bool()?;
    r.skip(5 * 4 + 4 + 4 + 4)?;
    Ok(Camera {
        av,
        left,
        right,
        top,
        bottom,
        near,
        far,
        orthographic,
    })
}

fn text_keys(r: &mut Reader) -> Result<TextKeys> {
    let name = extra_data_name(r)?;
    if r.netimmerse() {
        r.u32()?;
    }
    let n = r.u32()? as usize;
    // Each key takes at least eight bytes.
    r.ensure(n.saturating_mul(8))?;
    let keys = (0..n)
        .map(|_| {
            Ok(TextKey {
                time: r.f32()?,
                text: r.string()?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(TextKeys { name, keys })
}

fn sequence(r: &mut Reader) -> Result<Sequence> {
    let name = r.string()?;
    let accumulation_root = r.string()?;
    let text_keys = r.reference()?;
    let n = r.u32()? as usize;
    r.ensure(n.saturating_mul(8))?;
    let blocks = (0..n)
        .map(|_| {
            Ok(ControlledBlock {
                target: r.string()?,
                controller: r.reference()?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(Sequence {
        name,
        accumulation_root,
        text_keys,
        blocks,
    })
}

/// A keyframe or alpha controller, which add only their data.
fn data_controller(kind: &str, r: &mut Reader) -> Result<Controller> {
    let c = time_controller(r)?;
    Ok(Controller {
        kind: match kind {
            "NiKeyframeController" => ControllerKind::Keyframe,
            _ => ControllerKind::Alpha,
        },
        data: r.reference()?,
        ..c
    })
}

fn keyframes(r: &mut Reader) -> Result<Keyframes> {
    let n = r.u32()? as usize;
    let rotation = match n {
        0 => Rotation::default(),
        _ => match r.u32()? {
            4 => {
                r.f32()?;
                Rotation::Euler([key_group(r)?, key_group(r)?, key_group(r)?])
            }
            // Rotations have no tangents, only TCB values.
            kind => Rotation::Quaternion(keys(r, n, interpolation(kind)?, false)?),
        },
    };
    Ok(Keyframes {
        rotation,
        translation: key_group(r)?,
        scale: key_group(r)?,
    })
}

fn skin_instance(r: &mut Reader) -> Result<SkinInstance> {
    Ok(SkinInstance {
        data: r.reference()?,
        skeleton_root: r.reference()?,
        bones: r.references()?,
    })
}

fn skin_data(r: &mut Reader) -> Result<SkinData> {
    let transform = bone_transform(r)?;
    let n = r.u32()? as usize;
    let partition = r.reference()?;
    let has_weights = r.bool()?;
    // Each bone takes at least 70 bytes.
    r.ensure(n.saturating_mul(70))?;
    let bones = (0..n)
        .map(|_| {
            let transform = bone_transform(r)?;
            let bound = r.floats()?;
            let n = r.u16()? as usize;
            let weights = match has_weights {
                true => (0..n)
                    .map(|_| Ok((r.u16()?, r.f32()?)))
                    .collect::<Result<_>>()?,
                false => Vec::new(),
            };
            Ok(Bone {
                transform,
                bound,
                weights,
            })
        })
        .collect::<Result<_>>()?;
    Ok(SkinData {
        transform,
        partition,
        bones,
    })
}

fn skin_partition(kind: &str, r: &mut Reader) -> Result<Object> {
    for _ in 0..r.u32()? {
        let [v, t, bones, strips, w] = r.u16_array::<5>(1)?[0].map(usize::from);
        r.skip(bones * 2)?;
        if r.old() || r.bool()? {
            r.skip(v * 2)?;
        }
        if r.old() || r.bool()? {
            r.skip(v * w * 4)?;
        }
        let lengths = r.u16s(strips)?;
        if r.old() || r.bool()? {
            let points = match strips {
                0 => t * 3,
                _ => lengths.iter().map(|&n| n as usize).sum(),
            };
            r.skip(points * 2)?;
        }
        if r.bool()? {
            r.skip(v * w)?;
        }
    }
    Ok(Object::Other(kind.to_string()))
}

fn light(kind: &str, r: &mut Reader) -> Result<Object> {
    scene(r)?;
    if !r.old() {
        r.references()?; // affected nodes
    }
    r.floats::<10>()?;
    if kind == "NiPointLight" {
        r.floats::<3>()?;
    }
    Ok(Object::Other(kind.to_string()))
}

fn vis_controller(r: &mut Reader) -> Result<Controller> {
    let c = time_controller(r)?;
    r.reference()?;
    Ok(c)
}

fn particle_update_controller(r: &mut Reader) -> Result<Controller> {
    Ok(Controller {
        kind: ControllerKind::ParticleUpdate,
        ..time_controller(r)?
    })
}

/// A look-at, material colour or light colour controller.
fn flagged_controller(r: &mut Reader) -> Result<Controller> {
    let c = time_controller(r)?;
    if !r.old() {
        r.u16()?;
    }
    r.reference()?;
    Ok(c)
}

fn path_controller(r: &mut Reader) -> Result<Controller> {
    let (c, stored) = stored_time_controller(r)?;
    let flags = if r.old() { stored >> 4 } else { r.u16()? };
    let bank_direction = r.u32()? as i32;
    let [max_bank, smoothing] = r.floats()?;
    let follow_axis = r.u16()? as i16;
    let positions = r.reference()?;
    Ok(Controller {
        kind: ControllerKind::Path(PathSettings {
            flags,
            bank_direction,
            max_bank,
            smoothing,
            follow_axis,
            positions,
        }),
        data: r.reference()?,
        ..c
    })
}

fn texture_controller(r: &mut Reader) -> Result<Controller> {
    let c = time_controller(r)?;
    let shader_map = r.bool()?;
    let slot = r.u32()?;
    let change = match r.u32()? {
        0 => Some(TextureChange::OffsetU),
        1 => Some(TextureChange::OffsetV),
        2 => Some(TextureChange::Turn),
        3 => Some(TextureChange::ScaleU),
        4 => Some(TextureChange::ScaleV),
        _ => None,
    };
    let data = r.reference()?;
    Ok(match change.filter(|_| !shader_map) {
        Some(change) => Controller {
            kind: ControllerKind::Texture { slot, change },
            data,
            ..c
        },
        None => c,
    })
}

fn flip_controller(r: &mut Reader) -> Result<Controller> {
    let c = time_controller(r)?;
    r.skip(4 + 4 + 4)?;
    r.references()?;
    Ok(c)
}

fn particle_system(r: &mut Reader) -> Result<ParticleSystem> {
    Ok(ParticleSystem {
        shape: shape(r)?,
        world_space: r.bool()?,
        modifiers: r.references()?,
    })
}

fn old_particle_data(kind: &str, r: &mut Reader) -> Result<OldParticleData> {
    let (n, mut data) = particles_data(r)?;
    if kind == "NiRotatingParticlesData" && r.netimmerse() && r.bool()? {
        data.rotations = r.float_array(n)?;
    }
    let kind = match kind {
        "NiParticlesData" => OldParticleKind::Points,
        "NiRotatingParticlesData" => OldParticleKind::Rotating,
        _ => OldParticleKind::Meshes {
            node: r.reference()?,
        },
    };
    Ok(OldParticleData { kind, ..data })
}

fn particle_data(r: &mut Reader) -> Result<ParticleData> {
    let (n, _) = particles_data(r)?;
    r.skip(n * PARTICLE_INFO_SIZE)?;
    r.skip(2 + 2)?;
    Ok(ParticleData { capacity: n })
}

fn old_modifier(kind: &str, r: &mut Reader) -> Result<OldModifier> {
    Ok(OldModifier {
        next: r.reference()?,
        controller: r.reference()?,
        kind: match kind {
            "NiGravity" => OldModifierKind::Gravity {
                decay: r.f32()?,
                strength: r.f32()?,
                force: r.u32()?,
                position: r.floats()?,
                direction: r.floats()?,
            },
            "NiParticleColorModifier" => OldModifierKind::Color {
                data: r.reference()?,
            },
            "NiParticleGrowFade" => OldModifierKind::GrowFade {
                grow: r.f32()?,
                fade: r.f32()?,
            },
            "NiParticleRotation" => OldModifierKind::Rotation {
                random_axis: r.bool()?,
                axis: r.floats()?,
                speed: r.f32()?,
            },
            _ => OldModifierKind::Meshes {
                meshes: r.references()?,
            },
        },
    })
}

fn collider(kind: &str, r: &mut Reader) -> Result<Collider> {
    Ok(Collider {
        bounce: r.f32()?,
        spawn_on_collide: r.bool()?,
        die_on_collide: r.bool()?,
        spawn: r.reference()?,
        manager: r.reference()?,
        next: r.reference()?,
        object: r.reference()?,
        shape: if kind == "NiPSysPlanarCollider" {
            ColliderShape::Plane {
                width: r.f32()?,
                height: r.f32()?,
                x_axis: r.floats()?,
                y_axis: r.floats()?,
            }
        } else {
            ColliderShape::Sphere { radius: r.f32()? }
        },
    })
}

/// A controller of a particle system's modifier.
fn particle_controller(kind: &str, r: &mut Reader) -> Result<Controller> {
    let kind = match kind {
        "NiPSysEmitterCtlr" => ControllerKind::Emitter,
        "NiPSysModifierActiveCtlr" => ControllerKind::ModifierActive,
        "NiPSysEmitterInitialRadiusCtlr" => ControllerKind::EmitterInitialRadius,
        _ => ControllerKind::EmitterSpeed,
    };
    let c = time_controller(r)?;
    Ok(Controller {
        kind,
        modifier: r.string()?,
        data: r.reference()?,
        ..c
    })
}

fn emitter_keys(r: &mut Reader) -> Result<EmitterKeys> {
    Ok(EmitterKeys {
        birth_rate: key_group(r)?,
        active: bool_keys(r)?,
    })
}

/// The size of a particle info record, as laid out in the module docs.
const PARTICLE_INFO_SIZE: usize = 40;

/// The fields every controller starts with, as a controller of a kind not
/// read further.
fn time_controller(r: &mut Reader) -> Result<Controller> {
    Ok(stored_time_controller(r)?.0)
}

/// [`time_controller`] and its flags as stored, which in older files also
/// hold a path controller's own flags above the low four bits.
fn stored_time_controller(r: &mut Reader) -> Result<(Controller, u16)> {
    let next = r.reference()?;
    let stored = r.u16()?;
    // Older files keep only the timing bits, and their controllers all
    // have the manager-update flag (0x20) set.
    let flags = match r.old() {
        true => stored & 0xf | 0x20,
        false => stored,
    };
    let [frequency, phase, start, stop] = r.floats::<4>()?;
    let c = Controller {
        kind: ControllerKind::Unread,
        modifier: String::new(),
        next,
        flags,
        frequency,
        phase,
        start,
        stop,
        target: r.reference()?,
        data: None,
    };
    Ok((c, stored))
}

fn old_particle_controller(r: &mut Reader) -> Result<OldParticleController> {
    let timing = Controller {
        kind: ControllerKind::OldParticles,
        ..time_controller(r)?
    };
    let [
        speed,
        speed_variation,
        declination,
        declination_variation,
        planar_angle,
        planar_angle_variation,
        nx,
        ny,
        nz,
        red,
        green,
        blue,
        alpha,
        size,
        emit_start,
        emit_stop,
    ] = r.floats()?;
    let reset = r.bool()?;
    let [birth_rate, life_span, life_span_variation] = r.floats()?;
    let use_birth_rate = r.bool()?;
    let spawn_on_death = r.bool()?;
    let emitter_size = r.floats()?;
    let emitter = r.reference()?;
    let spawn_generations = r.u16()?;
    let spawn_percentage = r.f32()?;
    let spawn_multiplier = r.u16()?;
    let [spawn_speed_chaos, spawn_direction_chaos] = r.floats()?;
    let n = r.u16()? as usize;
    let valid = r.u16()?;
    r.ensure(n * PARTICLE_INFO_SIZE)?;
    let particles = (0..n)
        .map(|_| {
            Ok(SavedParticle {
                velocity: r.floats()?,
                axis: r.floats()?,
                age: r.f32()?,
                life_span: r.f32()?,
                last_update: r.f32()?,
                generation: r.u16()?,
                vertex: r.u16()?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(OldParticleController {
        timing,
        speed,
        speed_variation,
        declination,
        declination_variation,
        planar_angle,
        planar_angle_variation,
        normal: [nx, ny, nz],
        color: [red, green, blue, alpha],
        size,
        emit_start,
        emit_stop,
        reset,
        birth_rate,
        life_span,
        life_span_variation,
        use_birth_rate,
        spawn_on_death,
        emitter_size,
        emitter,
        spawn_generations,
        spawn_percentage,
        spawn_multiplier,
        spawn_speed_chaos,
        spawn_direction_chaos,
        particles,
        valid,
        emitter_modifier: r.reference()?,
        modifier: r.reference()?,
        collider: r.reference()?,
        static_bound: r.bool()?,
    })
}

fn bool_keys(r: &mut Reader) -> Result<BoolKeys> {
    let n = r.u32()? as usize;
    r.ensure(n.saturating_mul(5))?;
    (0..n).map(|_| Ok((r.f32()?, r.u8()? != 0))).collect()
}

/// Particle data and the particle count.
fn particles_data(r: &mut Reader) -> Result<(usize, OldParticleData)> {
    let (n, geometry) = geometry(r)?;
    let radii = match r.old() {
        true => vec![r.f32()?],
        false if r.bool()? => r.f32s(n)?,
        false => Vec::new(),
    };
    let active = r.u16()?;
    let sizes = if r.bool()? { r.f32s(n)? } else { Vec::new() };
    let rotations = if !r.netimmerse() && r.bool()? {
        r.float_array(n)?
    } else {
        Vec::new()
    };
    let data = OldParticleData {
        kind: OldParticleKind::Points,
        geometry,
        radii,
        active,
        sizes,
        rotations,
    };
    Ok((n, data))
}

fn modifier(kind: &str, r: &mut Reader) -> Result<Modifier> {
    let name = r.string()?;
    let order = r.u32()?;
    let system = r.reference()?;
    let active = r.bool()?;
    let kind = match kind {
        "NiPSysAgeDeathModifier" => ModifierKind::AgeDeath {
            spawn_on_death: r.bool()?,
            spawn: r.reference()?,
        },
        "NiPSysBoundUpdateModifier" => {
            r.u16()?; // particles skipped when bounding
            ModifierKind::BoundUpdate
        }
        "NiPSysColliderManager" => ModifierKind::ColliderManager {
            collider: r.reference()?,
        },
        "NiPSysColorModifier" => ModifierKind::Color {
            data: r.reference()?,
        },
        "NiPSysDragModifier" => ModifierKind::Drag(Drag {
            object: r.reference()?,
            axis: r.floats()?,
            percentage: r.f32()?,
            range: r.f32()?,
            falloff: r.f32()?,
        }),
        "NiPSysGravityModifier" => ModifierKind::Gravity(Gravity {
            object: r.reference()?,
            axis: r.floats()?,
            decay: r.f32()?,
            strength: r.f32()?,
            force: r.u32()?,
            turbulence: r.f32()?,
            turbulence_scale: r.f32()?,
        }),
        "NiPSysGrowFadeModifier" => ModifierKind::GrowFade(GrowFade {
            grow: r.f32()?,
            grow_generation: r.u16()?,
            fade: r.f32()?,
            fade_generation: r.u16()?,
        }),
        "NiPSysPositionModifier" => ModifierKind::Position,
        "NiPSysRotationModifier" => ModifierKind::Rotation(ParticleRotation {
            speed: r.f32()?,
            random_axis: r.bool()?,
            axis: r.floats()?,
        }),
        "NiPSysSpawnModifier" => ModifierKind::Spawn(Spawn {
            generations: r.u16()?,
            percentage: r.f32()?,
            min: r.u16()?,
            max: r.u16()?,
            speed_chaos: r.f32()?,
            direction_chaos: r.f32()?,
            life_span: r.f32()?,
            life_span_variation: r.f32()?,
        }),
        _ => ModifierKind::Emitter(emitter(kind, r)?),
    };
    Ok(Modifier {
        name,
        order,
        system,
        active,
        kind,
    })
}

fn emitter(kind: &str, r: &mut Reader) -> Result<Emitter> {
    let [
        speed,
        speed_variation,
        declination,
        declination_variation,
        planar_angle,
        planar_angle_variation,
        red,
        green,
        blue,
        alpha,
        radius,
        life_span,
        life_span_variation,
    ] = r.floats()?;
    let shape = match kind {
        "NiPSysMeshEmitter" => EmitterShape::Mesh {
            meshes: r.references()?,
            velocity: r.u32()?,
            emission: r.u32()?,
            axis: r.floats()?,
        },
        "NiPSysBoxEmitter" => EmitterShape::Box {
            object: r.reference()?,
            size: r.floats()?,
        },
        "NiPSysCylinderEmitter" => EmitterShape::Cylinder {
            object: r.reference()?,
            radius: r.f32()?,
            height: r.f32()?,
        },
        _ => EmitterShape::Sphere {
            object: r.reference()?,
            radius: r.f32()?,
        },
    };
    Ok(Emitter {
        speed,
        speed_variation,
        declination,
        declination_variation,
        planar_angle,
        planar_angle_variation,
        color: [red, green, blue, alpha],
        radius,
        life_span,
        life_span_variation,
        shape,
    })
}

fn named(r: &mut Reader) -> Result<(String, Vec<Ref>, Ref)> {
    let name = r.string()?;
    let extra_data = if r.netimmerse() {
        r.reference()?.into_iter().map(Some).collect()
    } else {
        r.references()?
    };
    Ok((name, extra_data, r.reference()?))
}

/// The name of an extra data object; empty in 4.2.2.0.
fn extra_data_name(r: &mut Reader) -> Result<String> {
    if r.netimmerse() {
        r.reference()?;
        Ok(String::new())
    } else {
        r.string()
    }
}

fn scene(r: &mut Reader) -> Result<SceneObject> {
    let (name, extra_data, controller) = named(r)?;
    let flags = r.u16()?;
    let translation = r.floats()?;
    let rotation = [r.floats()?, r.floats()?, r.floats()?];
    let scale = r.f32()?;
    if r.netimmerse() {
        r.floats::<3>()?; // velocity
    }
    let properties = r.references()?;
    if !r.netimmerse() {
        r.reference()?; // collision
    } else if r.bool()? {
        bounding_volume(r, 0)?;
    }
    Ok(SceneObject {
        name,
        extra_data,
        controller,
        flags,
        transform: Transform {
            translation,
            rotation,
            scale,
        },
        properties,
    })
}

fn bone_transform(r: &mut Reader) -> Result<Transform> {
    let rotation = [r.floats()?, r.floats()?, r.floats()?];
    Ok(Transform {
        rotation,
        translation: r.floats()?,
        scale: r.f32()?,
    })
}

fn shape(r: &mut Reader) -> Result<Shape> {
    let av = scene(r)?;
    let data = r.reference()?;
    let skin = r.reference()?;
    if !r.netimmerse() && r.bool()? {
        r.string()?; // shader
        r.u32()?;
    }
    Ok(Shape { av, data, skin })
}

/// How deep bounding volumes may nest in a union, so a corrupt file can't
/// overflow the stack; none of the game's files nest them at all.
const MAX_BOUNDING_DEPTH: usize = 8;

fn bounding_volume(r: &mut Reader, depth: usize) -> Result<()> {
    match r.u32()? {
        0 => r.skip(4 * 4),
        1 => r.skip(15 * 4),
        2 => r.skip(8 * 4),
        4 if depth < MAX_BOUNDING_DEPTH => {
            for _ in 0..r.u32()? {
                bounding_volume(r, depth + 1)?;
            }
            Ok(())
        }
        4 => Err(Error::Corrupt(format!(
            "bounding volumes nested over {MAX_BOUNDING_DEPTH} deep"
        ))),
        5 => r.skip(7 * 4),
        kind => Err(Error::Corrupt(format!("unknown bounding volume {kind}"))),
    }
}

/// The vertex count and geometry.
fn geometry(r: &mut Reader) -> Result<(usize, Geometry)> {
    let n = r.u16()? as usize;
    if !r.old() {
        r.skip(2)?;
    }
    let positions = if r.bool()? {
        r.float_array(n)?
    } else {
        Vec::new()
    };
    let flags = if r.netimmerse() { 0 } else { r.u16()? };
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
    let sets = if r.netimmerse() {
        r.u16()?
    } else {
        flags & 0x3f
    };
    let uv_sets = (0..sets).map(|_| r.float_array(n)).collect::<Result<_>>()?;
    if !r.netimmerse() {
        r.u16()?;
    }
    Ok((
        n,
        Geometry {
            positions,
            normals,
            colors,
            uv_sets,
            triangles: Vec::new(),
        },
    ))
}

fn texture_slot(r: &mut Reader) -> Result<TextureSlot> {
    let slot = TextureSlot {
        source: r.reference()?,
        clamp: r.u32()?,
        filter: r.u32()?,
        uv_set: r.u32()?,
        placement: None,
    };
    r.skip(4)?;
    if !r.old() && r.bool()? {
        let [ou, ov, su, sv, turn] = r.floats::<5>()?;
        let order = r.u32()?;
        let centre = r.floats::<2>()?;
        return Ok(TextureSlot {
            placement: Some(TexturePlacement {
                offset: [ou, ov],
                scale: [su, sv],
                turn,
                order,
                centre,
            }),
            ..slot
        });
    }
    Ok(slot)
}

fn interpolation(kind: u32) -> Result<Interpolation> {
    match kind {
        1 => Ok(Interpolation::Linear),
        2 => Ok(Interpolation::Quadratic),
        3 => Ok(Interpolation::Tcb),
        _ => Err(Error::Corrupt(format!("unknown key kind {kind}"))),
    }
}

trait ReadValue: Value {
    fn read(r: &mut Reader) -> Result<Self>;
}

impl ReadValue for f32 {
    fn read(r: &mut Reader) -> Result<Self> {
        r.f32()
    }
}

impl<const N: usize> ReadValue for [f32; N] {
    fn read(r: &mut Reader) -> Result<Self> {
        r.floats()
    }
}

fn key_group<T: ReadValue>(r: &mut Reader) -> Result<Keys<T>> {
    let n = r.u32()? as usize;
    if n == 0 {
        return Ok(Keys::default());
    }
    let interpolation = interpolation(r.u32()?)?;
    keys(r, n, interpolation, true)
}

/// `n` keys; quadratic ones carry tangents only if `tangents` is set.
fn keys<T: ReadValue>(
    r: &mut Reader,
    n: usize,
    interpolation: Interpolation,
    tangents: bool,
) -> Result<Keys<T>> {
    // Each key takes at least eight bytes.
    r.ensure(n.saturating_mul(8))?;
    let keys = (0..n)
        .map(|_| {
            let time = r.f32()?;
            let value = T::read(r)?;
            let tangents = match interpolation {
                Interpolation::Quadratic if tangents => Some([T::read(r)?, T::read(r)?]),
                Interpolation::Quadratic => None,
                Interpolation::Tcb => {
                    r.floats::<3>()?;
                    None
                }
                Interpolation::Linear => None,
            };
            Ok(Key {
                time,
                value,
                tangents,
            })
        })
        .collect::<Result<_>>()?;
    Ok(Keys {
        interpolation,
        keys,
    })
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
