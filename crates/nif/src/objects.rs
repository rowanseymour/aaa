//! The object types the reader understands and their layouts. Each layout
//! lists the fields present in version 10.1.0.0; `[old: ...]` marks how
//! 10.0.1.0 and 4.2.2.0 differ, `[4.2.2.0: ...]` how 4.2.2.0 differs
//! further. Types read only so they can be stepped over become
//! [`Object::Other`].
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
//! controller       ref next, u16 flags, f32 frequency, phase, start,
//!                  stop, ref target
//! NiKeyframeController, NiAlphaController, NiVisController
//!                  controller, ref data
//! NiLookAtController, NiMaterialColorController, NiLightColorController
//!                  controller, u16 flags [old: none], ref node or data
//! NiPathController controller, u16 flags [old: none], i32 bank direction,
//!                  f32 max bank, smoothing, i16 follow axis, ref path
//!                  data, ref percent data
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
//!                  last update, u16 generation, u16 code
//! NiParticlesData, NiRotatingParticlesData, NiParticleMeshesData, NiPSysData
//!                  geometry, bool + f32 radii[n] [old: f32 radius],
//!                  u16 active, bool + f32 sizes[n], bool + f32
//!                  rotations[n][4] [4.2.2.0: none, but NiRotatingParticlesData
//!                  ends with them]; NiParticleMeshesData adds ref node,
//!                  NiPSysData particle info[n], u16 added, u16 added base
//! NiParticleSystemController
//!                  controller, f32 emission[16], u8 reset, f32 birth rate,
//!                  life span[2], u8 use birth rate, u8 spawn on death,
//!                  f32 emitter size[3], ref emitter, u16 generations,
//!                  f32 spawned, u16 multiplier, f32 speed chaos, direction
//!                  chaos, u16 n, u16 valid, particle info[n], ref emitter
//!                  modifier, ref modifier, ref collider, u8 static bound
//! NiGravity, NiParticleColorModifier, NiParticleGrowFade,
//! NiParticleRotation, NiParticleMeshModifier
//!                  ref next, ref controller, then f32 decay, force, u32
//!                  kind, f32 position[3], direction[3]; ref colour data;
//!                  f32 grow, fade; u8 random, f32 axis[3], speed; refs
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
    VertexColor(VertexColor),
    ExtraData(ExtraData),
    Controller(Controller),
    Keyframes(Keyframes),
    Floats(Keys<f32>),
    Sequence(Sequence),
    TextKeys(TextKeys),
    SkinInstance(SkinInstance),
    SkinData(SkinData),
    ParticleSystem(ParticleSystem),
    ParticleData(ParticleData),
    Modifier(Modifier),
    Collider(Collider),
    EmitterKeys(EmitterKeys),
    Colors(Keys<[f32; 4]>),
    Visibility(BoolKeys),
    /// Understood well enough to step over; the type name.
    Other(String),
}

impl Object {
    /// The scene fields of a node or shape.
    pub fn scene(&self) -> Option<&SceneObject> {
        match self {
            Object::Node(n) => Some(&n.av),
            Object::Shape(s) => Some(&s.av),
            Object::ParticleSystem(p) => Some(&p.shape.av),
            _ => None,
        }
    }
}

/// The types an [`Object`] can hold, for picking one out by type.
pub trait ObjectType {
    fn of(object: &Object) -> Option<&Self>;
}

macro_rules! object_types {
    ($($variant:ident($ty:ty)),* $(,)?) => {$(
        impl ObjectType for $ty {
            fn of(object: &Object) -> Option<&Self> {
                match object {
                    Object::$variant(v) => Some(v),
                    _ => None,
                }
            }
        }
    )*};
}

object_types!(
    Node(Node),
    Shape(Shape),
    Geometry(Geometry),
    Texturing(Texturing),
    SourceTexture(SourceTexture),
    Pixels(Pixels),
    Material(Material),
    Alpha(Alpha),
    Stencil(Stencil),
    VertexColor(VertexColor),
    ExtraData(ExtraData),
    Controller(Controller),
    Keyframes(Keyframes),
    Floats(Keys<f32>),
    Sequence(Sequence),
    TextKeys(TextKeys),
    SkinInstance(SkinInstance),
    SkinData(SkinData),
    ParticleSystem(ParticleSystem),
    ParticleData(ParticleData),
    Modifier(Modifier),
    Collider(Collider),
    EmitterKeys(EmitterKeys),
    Colors(Keys<[f32; 4]>),
    Visibility(BoolKeys),
);

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
    pub controller: Ref,
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
    /// Where the texture lies on the mesh, if the slot moves it.
    pub placement: Option<TexturePlacement>,
}

/// How a texture slot moves, turns and stretches its texture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TexturePlacement {
    pub offset: [f32; 2],
    pub scale: [f32; 2],
    /// Radians.
    pub turn: f32,
    /// Which order the changes are made in.
    pub order: u32,
    /// The point it turns and stretches about.
    pub centre: [f32; 2],
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
    pub controller: Ref,
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
    /// Whether blending adds to what's behind: the destination factor is
    /// one, as for glows and ripples.
    pub fn adds(&self) -> bool {
        (self.flags >> 5) & 0xf == 0
    }
    /// What the colour drawn is scaled by: 0 one, 2 the colour itself, 6
    /// its alpha, and so on through the usual blend factors.
    pub fn source(&self) -> u16 {
        (self.flags >> 1) & 0xf
    }
}

#[derive(Debug, Clone)]
pub struct Stencil {
    /// Which faces are drawn; 3 means both sides.
    pub draw_mode: u32,
}

/// How a shape's vertex colours and lights combine.
#[derive(Debug, Clone)]
pub struct VertexColor {
    /// What the vertex colours stand for: 0 nothing, 1 emissive, 2
    /// ambient and diffuse.
    pub vertex_mode: u32,
    /// 0 for emissive light only, 1 for emissive, ambient and diffuse.
    pub lighting: u32,
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

/// An animation in a `.kf` file: controllers for the nodes of a model with
/// the same names. The timing (start, stop, frequency and cycle) is each
/// controller's own.
#[derive(Debug, Clone)]
pub struct Sequence {
    pub name: String,
    /// The node whose animated movement carries the whole model.
    pub accumulation_root: String,
    pub text_keys: Ref,
    pub blocks: Vec<ControlledBlock>,
}

#[derive(Debug, Clone)]
pub struct ControlledBlock {
    /// Name of the node the controller animates.
    pub target: String,
    pub controller: Ref,
}

/// Binds a mesh to the bones that bend it.
#[derive(Debug, Clone)]
pub struct SkinInstance {
    pub data: Ref,
    /// The node the bones hang from.
    pub skeleton_root: Ref,
    /// The bone nodes, in the order of [`SkinData::bones`].
    pub bones: Vec<Ref>,
}

/// How a mesh's vertices follow its bones.
#[derive(Debug, Clone)]
pub struct SkinData {
    /// From the space of the skeleton root's parent to the mesh's, as bound.
    pub transform: Transform,
    pub partition: Ref,
    pub bones: Vec<Bone>,
}

/// One bone's hold on a mesh. A vertex follows the bone as it moves from
/// where it was bound: the bone's current transform, below the skeleton
/// root's parent, after this one places the vertex.
#[derive(Debug, Clone)]
pub struct Bone {
    /// From the mesh's space to the bone's, as bound.
    pub transform: Transform,
    /// Centre and radius of a sphere around the bone's vertices, in the
    /// bone's space.
    pub bound: [f32; 4],
    /// Vertex indices and how strongly the bone moves each.
    pub weights: Vec<(u16, f32)>,
}

/// Named moments of an animation, such as `start` and `end`.
#[derive(Debug, Clone)]
pub struct TextKeys {
    pub name: String,
    pub keys: Vec<TextKey>,
}

#[derive(Debug, Clone)]
pub struct TextKey {
    pub time: f32,
    pub text: String,
}

/// A shape whose vertices are particles.
#[derive(Debug, Clone)]
pub struct ParticleSystem {
    /// The data is a [`ParticleData`].
    pub shape: Shape,
    pub world_space: bool,
    /// [`Modifier`]s, emitters among them.
    pub modifiers: Vec<Ref>,
}

#[derive(Debug, Clone)]
pub struct ParticleData {
    /// The most particles alive at once.
    pub capacity: usize,
}

/// One of a particle system's modifiers.
#[derive(Debug, Clone)]
pub struct Modifier {
    /// What particle controllers name it by.
    pub name: String,
    pub order: u32,
    pub system: Ref,
    pub active: bool,
    pub kind: ModifierKind,
}

#[derive(Debug, Clone)]
pub enum ModifierKind {
    /// `spawn` is a [`ModifierKind::Spawn`].
    AgeDeath {
        spawn_on_death: bool,
        spawn: Ref,
    },
    BoundUpdate,
    /// `collider` is the first of a chain of [`Collider`]s.
    ColliderManager {
        collider: Ref,
    },
    /// `data` is an [`Object::Colors`].
    Color {
        data: Ref,
    },
    Drag(Drag),
    Gravity(Gravity),
    GrowFade(GrowFade),
    Position,
    Rotation(ParticleRotation),
    Spawn(Spawn),
    Emitter(Emitter),
}

#[derive(Debug, Clone)]
pub struct Drag {
    pub object: Ref,
    pub axis: [f32; 3],
    pub percentage: f32,
    pub range: f32,
    pub falloff: f32,
}

#[derive(Debug, Clone)]
pub struct Gravity {
    pub object: Ref,
    pub axis: [f32; 3],
    pub decay: f32,
    pub strength: f32,
    /// 0 planar, 1 spherical.
    pub force: u32,
    pub turbulence: f32,
    pub turbulence_scale: f32,
}

#[derive(Debug, Clone)]
pub struct GrowFade {
    pub grow: f32,
    pub grow_generation: u16,
    pub fade: f32,
    pub fade_generation: u16,
}

#[derive(Debug, Clone)]
pub struct ParticleRotation {
    pub speed: f32,
    pub random_axis: bool,
    pub axis: [f32; 3],
}

/// Named by the `spawn` of a [`ModifierKind::AgeDeath`] or [`Collider`].
#[derive(Debug, Clone)]
pub struct Spawn {
    pub generations: u16,
    pub percentage: f32,
    pub min: u16,
    pub max: u16,
    pub speed_chaos: f32,
    pub direction_chaos: f32,
    pub life_span: f32,
    pub life_span_variation: f32,
}

/// Its birth rate comes from its [`ControllerKind::Emitter`] controller.
/// Each `_variation` goes with the value before it.
#[derive(Debug, Clone)]
pub struct Emitter {
    pub speed: f32,
    pub speed_variation: f32,
    pub declination: f32,
    pub declination_variation: f32,
    pub planar_angle: f32,
    pub planar_angle_variation: f32,
    pub color: [f32; 4],
    pub radius: f32,
    pub life_span: f32,
    pub life_span_variation: f32,
    pub shape: EmitterShape,
}

/// A volume placed by a node, or meshes.
#[derive(Debug, Clone)]
pub enum EmitterShape {
    Box {
        object: Ref,
        size: [f32; 3],
    },
    Cylinder {
        object: Ref,
        radius: f32,
        height: f32,
    },
    Sphere {
        object: Ref,
        radius: f32,
    },
    Mesh {
        meshes: Vec<Ref>,
        /// 0 along the normals, 1 random, 2 along `axis`.
        velocity: u32,
        /// 0 at vertices, 1 face centres, 2 edge centres, 3 face surfaces,
        /// 4 edge surfaces.
        emission: u32,
        axis: [f32; 3],
    },
}

/// Linked to the next of a chain by `next`.
#[derive(Debug, Clone)]
pub struct Collider {
    pub bounce: f32,
    pub spawn_on_collide: bool,
    pub die_on_collide: bool,
    pub spawn: Ref,
    pub manager: Ref,
    pub next: Ref,
    /// The node it's placed by.
    pub object: Ref,
    pub shape: ColliderShape,
}

#[derive(Debug, Clone)]
pub enum ColliderShape {
    Plane {
        width: f32,
        height: f32,
        x_axis: [f32; 3],
        y_axis: [f32; 3],
    },
    Sphere {
        radius: f32,
    },
}

/// What a [`ControllerKind::Emitter`] plays.
#[derive(Debug, Clone)]
pub struct EmitterKeys {
    pub birth_rate: Keys<f32>,
    /// Whether it emits.
    pub active: BoolKeys,
}

/// Times and an on/off value, each holding until the next key's time.
pub type BoolKeys = Vec<(f32, bool)>;

/// Plays its data on its target: a [`Keyframes`] on a scene object for
/// [`ControllerKind::Keyframe`], a [`Keys`] on a material's alpha for
/// [`ControllerKind::Alpha`], on a texture's placement for
/// [`ControllerKind::Texture`], and for the particle kinds, on the
/// [`Modifier`] named `modifier` of the [`ParticleSystem`] it targets.
#[derive(Debug, Clone)]
pub struct Controller {
    pub kind: ControllerKind,
    /// Empty for kinds that don't drive a modifier.
    pub modifier: String,
    /// The next controller on the same target.
    pub next: Ref,
    pub flags: u16,
    pub frequency: f32,
    pub phase: f32,
    pub start: f32,
    pub stop: f32,
    pub target: Ref,
    pub data: Ref,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerKind {
    Keyframe,
    Alpha,
    /// An emitter's birth rate and when it's on, from an [`EmitterKeys`].
    Emitter,
    /// When a modifier runs, from [`Object::Visibility`] keys.
    ModifierActive,
    /// An emitter's speed, from [`Object::Floats`].
    EmitterSpeed,
    /// An emitter's initial radius, from [`Object::Floats`].
    EmitterInitialRadius,
    /// Steps its particle system; it has no data.
    ParticleUpdate,
    /// One whose own fields are stepped over; kept so chains of
    /// controllers can be followed past it.
    Unread,
    /// Moves, turns or stretches the texture in one of a texturing
    /// property's slots.
    Texture {
        slot: u32,
        change: TextureChange,
    },
}

/// What a texture controller's keys set of where the texture lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureChange {
    OffsetU,
    OffsetV,
    /// Radians.
    Turn,
    ScaleU,
    ScaleV,
}

impl Controller {
    /// The time in the data's keys at `t` seconds of playing.
    pub fn key_time(&self, t: f32) -> f32 {
        let span = self.stop - self.start;
        let t = t * self.frequency + self.phase;
        if span <= 0.0 {
            return self.start;
        }
        // Flag bits 1-2: 0 loops, 1 plays back and forth, 2 holds the end.
        match (self.flags >> 1) & 3 {
            1 => {
                let u = (t - self.start).rem_euclid(2.0 * span);
                self.start + if u > span { 2.0 * span - u } else { u }
            }
            2 => t.clamp(self.start, self.stop),
            _ => self.start + (t - self.start).rem_euclid(span),
        }
    }
}

/// Animated translation, rotation and scale of a scene object; parts
/// without keys keep the object's own.
#[derive(Debug, Clone, Default)]
pub struct Keyframes {
    pub rotation: Rotation,
    pub translation: Keys<[f32; 3]>,
    pub scale: Keys<f32>,
}

#[derive(Debug, Clone)]
pub enum Rotation {
    /// w, x, y, z.
    Quaternion(Keys<[f32; 4]>),
    /// Angles in radians about x, y and z, applied in that order.
    Euler([Keys<f32>; 3]),
}

impl Default for Rotation {
    fn default() -> Self {
        Rotation::Quaternion(Keys::default())
    }
}

impl Keyframes {
    /// `base` with its animated parts taken from the keys at `time`.
    pub fn apply(&self, base: &Transform, time: f32) -> Transform {
        let mut out = *base;
        if let Some(t) = self.translation.sample(time) {
            out.translation = t;
        }
        if let Some(s) = self.scale.sample(time) {
            out.scale = s;
        }
        match &self.rotation {
            Rotation::Quaternion(keys) => {
                if let Some(q) = keys.sample_rotation(time) {
                    out.rotation = quaternion_matrix(q);
                }
            }
            Rotation::Euler(axes) if axes.iter().any(|k| !k.keys.is_empty()) => {
                let [x, y, z] = std::array::from_fn(|i| axes[i].sample(time).unwrap_or(0.0));
                out.rotation = euler_matrix(x, y, z);
            }
            Rotation::Euler(_) => {}
        }
        out
    }
}

/// A key group: values at times, eased between by their interpolation.
#[derive(Debug, Clone)]
pub struct Keys<T> {
    pub interpolation: Interpolation,
    pub keys: Vec<Key<T>>,
}

impl<T> Default for Keys<T> {
    fn default() -> Self {
        Keys {
            interpolation: Interpolation::Linear,
            keys: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Interpolation {
    #[default]
    Linear,
    /// Hermite curves through each key's tangents.
    Quadratic,
    /// Kochanek-Bartels splines; their tension, continuity and bias are
    /// skipped and the keys eased linearly.
    Tcb,
}

#[derive(Debug, Clone, Copy)]
pub struct Key<T> {
    pub time: f32,
    pub value: T,
    /// Incoming and outgoing tangents of quadratic keys.
    pub tangents: Option<[T; 2]>,
}

impl<T: Value> Keys<T> {
    /// The value at `time`, held before the first key and after the last.
    pub fn sample(&self, time: f32) -> Option<T> {
        let (a, b, u) = self.around(time)?;
        Some(match (a.tangents, b.tangents) {
            (Some([_, out]), Some([inc, _])) if self.interpolation == Interpolation::Quadratic => {
                let (u2, u3) = (u * u, u * u * u);
                a.value
                    .scaled(2.0 * u3 - 3.0 * u2 + 1.0)
                    .plus(out.scaled(u3 - 2.0 * u2 + u))
                    .plus(b.value.scaled(-2.0 * u3 + 3.0 * u2))
                    .plus(inc.scaled(u3 - u2))
            }
            _ => a.value.scaled(1.0 - u).plus(b.value.scaled(u)),
        })
    }

    /// The keys either side of `time` and how far it is between them.
    fn around(&self, time: f32) -> Option<(&Key<T>, &Key<T>, f32)> {
        let first = self.keys.first()?;
        let next = self.keys.iter().position(|k| k.time > time);
        Some(match next {
            Some(0) => (first, first, 0.0),
            None => {
                let last = self.keys.last()?;
                (last, last, 0.0)
            }
            Some(i) => {
                let (a, b) = (&self.keys[i - 1], &self.keys[i]);
                (a, b, (time - a.time) / (b.time - a.time))
            }
        })
    }
}

impl Keys<[f32; 4]> {
    /// The rotation at `time`, eased along the shorter arc.
    pub fn sample_rotation(&self, time: f32) -> Option<[f32; 4]> {
        let (a, b, u) = self.around(time)?;
        let dot: f32 = (0..4).map(|i| a.value[i] * b.value[i]).sum();
        let b = if dot < 0.0 {
            b.value.scaled(-1.0)
        } else {
            b.value
        };
        let q = a.value.scaled(1.0 - u).plus(b.scaled(u));
        let len = q.iter().map(|v| v * v).sum::<f32>().sqrt();
        (len > 0.0).then(|| q.map(|v| v / len))
    }
}

/// Values keys can hold.
pub trait Value: Copy {
    fn scaled(self, f: f32) -> Self;
    fn plus(self, other: Self) -> Self;
}

impl Value for f32 {
    fn scaled(self, f: f32) -> Self {
        self * f
    }
    fn plus(self, other: Self) -> Self {
        self + other
    }
}

impl<const N: usize> Value for [f32; N] {
    fn scaled(self, f: f32) -> Self {
        self.map(|v| v * f)
    }
    fn plus(self, other: Self) -> Self {
        std::array::from_fn(|i| self[i] + other[i])
    }
}

/// Rows of the matrix that rotates column vectors by `q` (w, x, y, z).
fn quaternion_matrix([w, x, y, z]: [f32; 4]) -> [[f32; 3]; 3] {
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

fn euler_matrix(x: f32, y: f32, z: f32) -> [[f32; 3]; 3] {
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    // Rz * Ry * Rx
    [
        [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
        [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
        [-sy, sx * cy, cx * cy],
    ]
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
            "NiTriShape" | "NiTriStrips" => Object::Shape(shape(r)?),
            "NiTriShapeData" => {
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
                Object::Geometry(g)
            }
            "NiTriStripsData" => {
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
                Object::Geometry(g)
            }
            "NiTexturingProperty" => {
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
                Object::Texturing(Texturing {
                    controller,
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
                let (_, _, controller) = named(r)?;
                if r.old() {
                    r.u16()?;
                }
                Object::Material(Material {
                    controller,
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
                r.skip(2)?;
                Object::VertexColor(VertexColor {
                    vertex_mode: r.u32()?,
                    lighting: r.u32()?,
                })
            }
            "NiSpecularProperty" => {
                named(r)?;
                r.skip(2)?;
                other()
            }
            "NiStringExtraData" | "NiIntegerExtraData" | "NiBooleanExtraData"
            | "NiBinaryExtraData" | "NiStringsExtraData" => {
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
                Object::ExtraData(ExtraData { name, value })
            }
            "NiCamera" => {
                scene(r)?;
                let extra = if r.old() { 0 } else { 2 + 1 };
                r.skip(extra + 6 * 4 + 5 * 4 + 4 + 4 + 4)?;
                other()
            }
            "NiTextKeyExtraData" => {
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
                Object::TextKeys(TextKeys { name, keys })
            }
            "NiControllerSequence" => {
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
                Object::Sequence(Sequence {
                    name,
                    accumulation_root,
                    text_keys,
                    blocks,
                })
            }
            "NiKeyframeController" | "NiAlphaController" => {
                let c = time_controller(r)?;
                Object::Controller(Controller {
                    kind: match kind {
                        "NiKeyframeController" => ControllerKind::Keyframe,
                        _ => ControllerKind::Alpha,
                    },
                    data: r.reference()?,
                    ..c
                })
            }
            "NiKeyframeData" => {
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
                Object::Keyframes(Keyframes {
                    rotation,
                    translation: key_group(r)?,
                    scale: key_group(r)?,
                })
            }
            "NiFloatData" => Object::Floats(key_group(r)?),
            "NiSkinInstance" => Object::SkinInstance(SkinInstance {
                data: r.reference()?,
                skeleton_root: r.reference()?,
                bones: r.references()?,
            }),
            "NiSkinData" => {
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
                Object::SkinData(SkinData {
                    transform,
                    partition,
                    bones,
                })
            }
            "NiSkinPartition" => {
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
                other()
            }
            "NiDitherProperty" => {
                named(r)?;
                r.u16()?;
                other()
            }
            "NiAmbientLight" | "NiPointLight" => {
                scene(r)?;
                if !r.old() {
                    r.references()?; // affected nodes
                }
                r.floats::<10>()?;
                if kind == "NiPointLight" {
                    r.floats::<3>()?;
                }
                other()
            }
            "NiVisController" => {
                let c = time_controller(r)?;
                r.reference()?;
                Object::Controller(c)
            }
            "NiPSysUpdateCtlr" => Object::Controller(Controller {
                kind: ControllerKind::ParticleUpdate,
                ..time_controller(r)?
            }),
            "NiLookAtController" | "NiMaterialColorController" | "NiLightColorController" => {
                let c = time_controller(r)?;
                if !r.old() {
                    r.u16()?;
                }
                r.reference()?;
                Object::Controller(c)
            }
            "NiPathController" => {
                let c = time_controller(r)?;
                if !r.old() {
                    r.u16()?;
                }
                r.skip(4 + 4 + 4 + 2)?;
                r.reference()?;
                r.reference()?;
                Object::Controller(c)
            }
            "NiTextureTransformController" => {
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
                match change.filter(|_| !shader_map) {
                    Some(change) => Object::Controller(Controller {
                        kind: ControllerKind::Texture { slot, change },
                        data,
                        ..c
                    }),
                    None => Object::Controller(c),
                }
            }
            "NiFlipController" => {
                let c = time_controller(r)?;
                r.skip(4 + 4 + 4)?;
                r.references()?;
                Object::Controller(c)
            }
            "NiPosData" => {
                key_group::<[f32; 3]>(r)?;
                other()
            }
            "NiColorData" => Object::Colors(key_group(r)?),
            "NiVisData" => Object::Visibility(bool_keys(r)?),
            "NiParticles" | "NiRotatingParticles" | "NiParticleMeshes" => {
                shape(r)?;
                other()
            }
            "NiParticleSystem" => Object::ParticleSystem(ParticleSystem {
                shape: shape(r)?,
                world_space: r.bool()?,
                modifiers: r.references()?,
            }),
            "NiParticlesData" | "NiRotatingParticlesData" | "NiParticleMeshesData" => {
                let n = particles_data(r)?;
                match kind {
                    "NiRotatingParticlesData" if r.netimmerse() => {
                        if r.bool()? {
                            r.float_array::<4>(n)?;
                        }
                    }
                    "NiParticleMeshesData" => {
                        r.reference()?;
                    }
                    _ => {}
                }
                other()
            }
            "NiPSysData" => {
                let n = particles_data(r)?;
                r.skip(n * 40)?; // particle info
                r.skip(2 + 2)?;
                Object::ParticleData(ParticleData { capacity: n })
            }
            "NiParticleSystemController" => {
                let c = time_controller(r)?;
                r.floats::<16>()?;
                r.u8()?;
                r.floats::<3>()?;
                r.skip(1 + 1 + 3 * 4)?;
                r.reference()?;
                r.skip(2 + 4 + 2 + 4 + 4)?;
                let n = r.u16()? as usize;
                r.u16()?;
                r.skip(n * 40)?;
                r.skip(3 * 4 + 1)?;
                Object::Controller(c)
            }
            "NiGravity"
            | "NiParticleColorModifier"
            | "NiParticleGrowFade"
            | "NiParticleRotation"
            | "NiParticleMeshModifier" => {
                r.reference()?; // next
                r.reference()?; // controller
                match kind {
                    "NiGravity" => r.skip(2 * 4 + 4 + 6 * 4)?,
                    "NiParticleColorModifier" => {
                        r.reference()?;
                    }
                    "NiParticleGrowFade" => r.skip(2 * 4)?,
                    "NiParticleRotation" => r.skip(1 + 4 * 4)?,
                    _ => {
                        r.references()?;
                    }
                }
                other()
            }
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
            "NiPSysPlanarCollider" | "NiPSysSphericalCollider" => Object::Collider(Collider {
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
            }),
            "NiPSysEmitterCtlr"
            | "NiPSysModifierActiveCtlr"
            | "NiPSysEmitterInitialRadiusCtlr"
            | "NiPSysEmitterSpeedCtlr" => {
                let kind = match kind {
                    "NiPSysEmitterCtlr" => ControllerKind::Emitter,
                    "NiPSysModifierActiveCtlr" => ControllerKind::ModifierActive,
                    "NiPSysEmitterInitialRadiusCtlr" => ControllerKind::EmitterInitialRadius,
                    _ => ControllerKind::EmitterSpeed,
                };
                let c = time_controller(r)?;
                Object::Controller(Controller {
                    kind,
                    modifier: r.string()?,
                    data: r.reference()?,
                    ..c
                })
            }
            "NiPSysEmitterCtlrData" => Object::EmitterKeys(EmitterKeys {
                birth_rate: key_group(r)?,
                active: bool_keys(r)?,
            }),
            _ => return Ok(None),
        }))
    }
}

/// The fields every controller starts with, as a controller of a kind not
/// read further.
fn time_controller(r: &mut Reader) -> Result<Controller> {
    let next = r.reference()?;
    let flags = r.u16()?;
    let [frequency, phase, start, stop] = r.floats::<4>()?;
    Ok(Controller {
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
    })
}

/// Linear keys of f32 time and a u8 on/off value.
fn bool_keys(r: &mut Reader) -> Result<BoolKeys> {
    let n = r.u32()? as usize;
    r.ensure(n.saturating_mul(5))?;
    (0..n).map(|_| Ok((r.f32()?, r.u8()? != 0))).collect()
}

/// Geometry plus per-particle sizes and rotations; returns the particle
/// count.
fn particles_data(r: &mut Reader) -> Result<usize> {
    let (n, _) = geometry(r)?;
    if !r.old() {
        if r.bool()? {
            r.skip(n * 4)?; // radii
        }
    } else {
        r.f32()?; // radius
    }
    r.u16()?; // active
    if r.bool()? {
        r.skip(n * 4)?; // sizes
    }
    if !r.netimmerse() && r.bool()? {
        r.float_array::<4>(n)?; // rotations
    }
    Ok(n)
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

/// The name of an extra data object; 4.2.2.0 has none, but links to the next.
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

/// A skin's transform: rotation rows, then translation and scale.
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

fn bounding_volume(r: &mut Reader, depth: usize) -> Result<()> {
    match r.u32()? {
        0 => r.skip(4 * 4),
        1 => r.skip(15 * 4),
        2 => r.skip(8 * 4),
        4 if depth < 8 => {
            for _ in 0..r.u32()? {
                bounding_volume(r, depth + 1)?;
            }
            Ok(())
        }
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
