//! The objects a model holds. Types read only so they can be stepped over
//! become [`Object::Other`]; how each is laid out is in the `read` module.

use crate::{Ref, Transform};

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
    ZBuffer(ZBuffer),
    VertexColor(VertexColor),
    ExtraData(ExtraData),
    Controller(Controller),
    Keyframes(Keyframes),
    Floats(Keys<f32>),
    Positions(Keys<[f32; 3]>),
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
    OldParticles(OldParticles),
    OldParticleData(OldParticleData),
    OldParticleController(OldParticleController),
    OldModifier(OldModifier),
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
            Object::OldParticles(p) => Some(&p.shape.av),
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
    ZBuffer(ZBuffer),
    VertexColor(VertexColor),
    ExtraData(ExtraData),
    Keyframes(Keyframes),
    Floats(Keys<f32>),
    Positions(Keys<[f32; 3]>),
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
    OldParticles(OldParticles),
    OldParticleData(OldParticleData),
    OldParticleController(OldParticleController),
    OldModifier(OldModifier),
);

/// An [`OldParticleController`] is a controller too, so chains of
/// controllers can be followed past it.
impl ObjectType for Controller {
    fn of(object: &Object) -> Option<&Self> {
        match object {
            Object::Controller(c) => Some(c),
            Object::OldParticleController(c) => Some(&c.timing),
            _ => None,
        }
    }
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
pub struct ZBuffer {
    pub flags: u16,
    /// The comparison a pixel's depth has to pass.
    pub function: u32,
}

impl ZBuffer {
    /// Whether what's nearer hides the shape.
    pub fn test(&self) -> bool {
        self.flags & 1 != 0
    }
    /// Whether the shape hides what's farther and drawn after it, which it
    /// only does if it tests.
    pub fn write(&self) -> bool {
        self.flags & 3 == 3
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

/// A shape of the older format whose vertices are particles. Its data is
/// an [`OldParticleData`]; its controller chain holds an
/// [`OldParticleController`].
#[derive(Debug, Clone)]
pub struct OldParticles {
    pub shape: Shape,
}

#[derive(Debug, Clone)]
pub struct OldParticleData {
    pub kind: OldParticleKind,
    /// Where the particles were when the model was saved.
    pub geometry: Geometry,
    /// One for all the particles in the older versions.
    pub radii: Vec<f32>,
    pub active: u16,
    pub sizes: Vec<f32>,
    /// w, x, y, z, for rotating particles.
    pub rotations: Vec<[f32; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OldParticleKind {
    Points,
    Rotating,
    /// `node` holds a copy of the meshes for each saved particle.
    Meshes {
        node: Ref,
    },
}

/// The emitter and particle list of an [`OldParticles`]. Each `_variation`
/// goes with the value before it; angles are in radians.
#[derive(Debug, Clone)]
pub struct OldParticleController {
    /// Of kind [`ControllerKind::OldParticles`], with no data.
    pub timing: Controller,
    pub speed: f32,
    pub speed_variation: f32,
    pub declination: f32,
    pub declination_variation: f32,
    pub planar_angle: f32,
    pub planar_angle_variation: f32,
    pub normal: [f32; 3],
    pub color: [f32; 4],
    pub size: f32,
    pub emit_start: f32,
    pub emit_stop: f32,
    pub reset: bool,
    pub birth_rate: f32,
    pub life_span: f32,
    pub life_span_variation: f32,
    pub use_birth_rate: bool,
    pub spawn_on_death: bool,
    pub emitter_size: [f32; 3],
    /// A scene object; in the game's models, a node named for the
    /// particles' parent node with ` Emitter` added.
    pub emitter: Ref,
    pub spawn_generations: u16,
    pub spawn_percentage: f32,
    pub spawn_multiplier: u16,
    pub spawn_speed_chaos: f32,
    pub spawn_direction_chaos: f32,
    pub particles: Vec<SavedParticle>,
    pub valid: u16,
    pub emitter_modifier: Ref,
    /// The first of a chain of [`OldModifier`]s.
    pub modifier: Ref,
    pub collider: Ref,
    pub static_bound: bool,
}

/// One particle's state as saved in a model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavedParticle {
    pub velocity: [f32; 3],
    pub axis: [f32; 3],
    pub age: f32,
    pub life_span: f32,
    pub last_update: f32,
    pub generation: u16,
    /// Which vertex of the data it is.
    pub vertex: u16,
}

/// One of an [`OldParticleController`]'s modifiers, linked to the next by
/// `next`.
#[derive(Debug, Clone)]
pub struct OldModifier {
    pub next: Ref,
    /// The [`OldParticleController`] it belongs to.
    pub controller: Ref,
    pub kind: OldModifierKind,
}

#[derive(Debug, Clone)]
pub enum OldModifierKind {
    Gravity {
        decay: f32,
        strength: f32,
        /// 0 planar, 1 spherical.
        force: u32,
        position: [f32; 3],
        direction: [f32; 3],
    },
    /// `data` is an [`Object::Colors`].
    Color {
        data: Ref,
    },
    GrowFade {
        grow: f32,
        fade: f32,
    },
    Rotation {
        random_axis: bool,
        axis: [f32; 3],
        speed: f32,
    },
    /// Scene objects for an [`OldParticleKind::Meshes`]'s particles.
    Meshes {
        meshes: Vec<Ref>,
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

#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// That of an [`OldParticleController`].
    OldParticles,
    /// One whose own fields are stepped over; kept so chains of
    /// controllers can be followed past it.
    Unread,
    /// Moves, turns or stretches the texture in one of a texturing
    /// property's slots.
    Texture {
        slot: u32,
        change: TextureChange,
    },
    /// Moves a scene object along a curve, [`Object::Positions`], by how
    /// far along it its data, [`Object::Floats`], says.
    Path(PathSettings),
}

/// How a path controller moves its object.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathSettings {
    /// 0x10: placed by distance along the curve rather than by the
    /// positions' key times; 0x20: turned to face along it; 0x40: facing
    /// back along it.
    pub flags: u16,
    pub bank_direction: i32,
    pub max_bank: f32,
    pub smoothing: f32,
    /// The axis turned along the curve: 0 x, 1 y, 2 z.
    pub follow_axis: i16,
    pub positions: Ref,
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
