//! Placed objects drawn with the game's models, where their templates name
//! models this crate can read.

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::f32::consts::FRAC_PI_2;
use std::hash::Hash;
use std::sync::Arc;

use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormats;
use bevy::light::NotShadowCaster;
use bevy::math::Affine2;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use nif::{
    Controller, ControllerKind, Keyframes, Keys, Nif, Object, TextureChange, TexturePlacement,
};
use tgck::map::Thing;
use tgdata::{AnimationKind, Footing, GameData, Id, Portrait, Shape, View};
use tgmap::{Ground, Player, owner};

use crate::MapObject;
use crate::glow::GlowMaterial;
use crate::loader::{Loader, Textures};
use crate::measures::Measures;
use crate::particles::{self, Frames, System};
use crate::rig::{Clips, Nodes, Rig};
use crate::solid::SolidMaterial;
use crate::team::{Team, TeamMaterial};
use crate::terrain::world;

/// Where and how turned to draw an object at `position` facing `facing`: at
/// the height its `footing` stands at and turned only about the vertical, or
/// tilted to rest on the ground under `conform`'s outline.
pub fn pose(
    ground: &Ground,
    position: [f32; 2],
    facing: [f32; 2],
    conform: Option<Shape>,
    footing: Footing,
) -> (Vec3, Quat) {
    let [x, y] = position;
    let [fx, fy] = facing;
    let Some(footprint) = conform else {
        let translation = world(x, y, ground.stand_at(footing, [x, y]));
        return (translation, Quat::from_rotation_y(fy.atan2(fx) + FRAC_PI_2));
    };
    // Objects never turned face their model's -y.
    let facing = Vec2::new(fx, fy).try_normalize().unwrap_or(Vec2::NEG_Y);
    let tilt = ground.tilt(position, facing.to_array(), footprint);
    let axis = |[x, y, z]: [f32; 3]| world(x, y, z);
    let forward = axis(tilt.forward);
    // The model's -y points ahead and its z up, kept square to it.
    let right = (-forward).cross(axis(tilt.up)).normalize();
    let up = right.cross(-forward);
    let rotation = Quat::from_mat3(&Mat3::from_cols(right, up, forward));
    (world(x, y, tilt.height), rotation)
}

/// Which of its view's models a thing is drawn with, before wrapping round
/// them: things with no variation set pick one by id, so neighbours differ.
fn variation(thing: &Thing) -> usize {
    usize::try_from(thing.variation).unwrap_or(thing.id as usize)
}

/// Where a placed object's model goes and how it's turned and sized.
fn placed(thing: &Thing, view: &View) -> Transform {
    // Maps store each object's height as the game places it: on the
    // ground, the seabed or the waterline.
    let [x, y, z] = thing.position;
    // Models face their -y axis, which turns to the object's facing:
    // roads run along it and waves roll back against it to shore.
    let [fx, fy, _] = thing.facing;
    Transform::from_translation(world(x, y, z))
        .with_rotation(Quat::from_rotation_y(fy.atan2(fx) + FRAC_PI_2))
        .with_scale(Vec3::splat(view.scale * thing.scale))
}

/// Draws one of a model's meshes in `material` instead of what it was
/// drawn in, as a placement preview's are.
pub fn repaint(mesh: &mut EntityCommands, material: Handle<StandardMaterial>) {
    mesh.remove::<(
        MeshMaterial3d<TeamMaterial>,
        MeshMaterial3d<GlowMaterial>,
        MeshMaterial3d<SolidMaterial>,
    )>()
    .insert(MeshMaterial3d(material));
}

/// One mesh of a model, in Bevy axes, ready to instance.
#[derive(Clone)]
pub(crate) struct Part {
    pub mesh: Handle<Mesh>,
    pub material: PartMaterial,
    pub animation: Option<Arc<Animation>>,
    /// Bevy's shadows ignore alpha blending, so a faded or see-through mesh
    /// would cast a solid one.
    pub blended: bool,
    /// Its owner's colour shows where its texture is see-through.
    pub team: bool,
    /// How much ambient light its material reflects, and how much of the
    /// light that falls on it, where it has a material to say.
    pub reflects: Option<([f32; 3], [f32; 3])>,
}

/// A part's material: most are standard, those that add to what's behind
/// them, or blend with it while writing depth, need their own.
#[derive(Clone)]
pub(crate) enum PartMaterial {
    Standard(Handle<StandardMaterial>),
    Glow(Handle<GlowMaterial>),
    Team(Handle<TeamMaterial>),
    Solid(Handle<SolidMaterial>),
}

impl PartMaterial {
    pub fn insert(&self, entity: &mut EntityCommands) {
        match self {
            PartMaterial::Standard(m) => entity.insert(MeshMaterial3d(m.clone())),
            PartMaterial::Glow(m) => entity.insert(MeshMaterial3d(m.clone())),
            PartMaterial::Team(m) => entity.insert(MeshMaterial3d(m.clone())),
            PartMaterial::Solid(m) => entity.insert(MeshMaterial3d(m.clone())),
        };
    }
}

/// The stores a model part's material can be in, one for each kind.
pub struct PartMaterials<'a> {
    pub standard: &'a mut Assets<StandardMaterial>,
    pub glow: &'a mut Assets<GlowMaterial>,
    pub team: &'a mut Assets<TeamMaterial>,
    pub solid: &'a mut Assets<SolidMaterial>,
}

impl PartMaterials<'_> {
    /// A copy of `material` of its own, to animate.
    pub(crate) fn copy(&mut self, material: &PartMaterial) -> PartMaterial {
        fn copy<M: Asset + Clone>(store: &mut Assets<M>, h: &Handle<M>) -> Handle<M> {
            store.get(h).cloned().map_or(h.clone(), |m| store.add(m))
        }
        match material {
            PartMaterial::Standard(h) => PartMaterial::Standard(copy(self.standard, h)),
            PartMaterial::Glow(h) => PartMaterial::Glow(copy(self.glow, h)),
            PartMaterial::Team(h) => PartMaterial::Team(copy(self.team, h)),
            PartMaterial::Solid(h) => PartMaterial::Solid(copy(self.solid, h)),
        }
    }

    /// The standard material `material` is built on, to change.
    fn base_mut(&mut self, material: &PartMaterial) -> Option<&mut StandardMaterial> {
        match material {
            PartMaterial::Standard(h) => self.standard.get_mut(h).map(|m| m.into_inner()),
            PartMaterial::Glow(h) => self.glow.get_mut(h).map(|m| &mut m.into_inner().base),
            PartMaterial::Team(h) => self.team.get_mut(h).map(|m| &mut m.into_inner().base),
            PartMaterial::Solid(h) => self.solid.get_mut(h).map(|m| &mut m.into_inner().base),
        }
    }
}

/// The stores a model part's material can be in, for a system.
#[derive(SystemParam)]
pub struct MaterialStores<'w> {
    pub(crate) standard: ResMut<'w, Assets<StandardMaterial>>,
    glow: ResMut<'w, Assets<GlowMaterial>>,
    team: ResMut<'w, Assets<TeamMaterial>>,
    solid: ResMut<'w, Assets<SolidMaterial>>,
}

impl MaterialStores<'_> {
    pub fn parts(&mut self) -> PartMaterials<'_> {
        PartMaterials {
            standard: &mut self.standard,
            glow: &mut self.glow,
            team: &mut self.team,
            solid: &mut self.solid,
        }
    }
}

/// A loaded model.
#[derive(Clone)]
pub(crate) struct Model {
    pub(crate) meshes: Meshes,
    particles: Vec<Arc<System>>,
    /// Its selection box's width and length, in model units.
    selection: Option<Vec2>,
}

/// A model's meshes placed in the model, or a rig of its nodes for one that
/// plays a `.kf` animation.
#[derive(Clone)]
pub(crate) enum Meshes {
    Parts(Vec<Part>),
    Rig(Arc<Rig>),
}

/// The ground an object's model marks as its own, in world units: width
/// across its facing and length along it. On the root of objects whose
/// models have one.
#[derive(Component, Clone, Copy, Debug)]
pub struct Footprint(pub Vec2);

/// What a model's controllers do to one of its meshes.
pub struct Animation {
    /// The animated objects above the mesh, or the mesh itself, from the
    /// model's root down; the mesh is built relative to the last.
    motion: Vec<Moved>,
    /// Fades the material's alpha; each instance gets its own material.
    fade: Option<(Controller, Keys<f32>)>,
    /// Moves, turns or stretches the material's texture, as fades do.
    texture: Option<TextureMotion>,
}

/// An object its keyframes move.
pub(crate) struct Moved {
    /// From the animated object above it, or the model's root, down to its
    /// parent.
    above: nif::Transform,
    controller: Controller,
    keys: Keyframes,
    /// Its own transform, which the keys change.
    base: nif::Transform,
}

/// Changes to where a texture lies, from where its slot puts it.
struct TextureMotion {
    rest: TexturePlacement,
    changes: Vec<(Controller, TextureChange, Keys<f32>)>,
}

impl TextureMotion {
    /// Where the texture lies at `t`.
    fn at(&self, t: f32) -> TexturePlacement {
        let mut p = self.rest;
        for (c, change, keys) in &self.changes {
            let Some(v) = keys.sample(c.key_time(t)) else {
                continue;
            };
            match change {
                TextureChange::OffsetU => p.offset[0] = v,
                TextureChange::OffsetV => p.offset[1] = v,
                TextureChange::Turn => p.turn = v,
                TextureChange::ScaleU => p.scale[0] = v,
                TextureChange::ScaleV => p.scale[1] = v,
            }
        }
        p
    }
}

/// Where a texture lies when its slot doesn't say: unmoved, about the
/// centre every slot in the game's models that does say turns about.
const UNPLACED: TexturePlacement = TexturePlacement {
    offset: [0.0, 0.0],
    scale: [1.0, 1.0],
    turn: 0.0,
    order: 0,
    centre: [0.5, 0.5],
};

/// The UV transform that lays a texture as `p` places it: stretched and
/// turned about its centre, then moved.
pub(crate) fn uv_transform(p: &TexturePlacement) -> Affine2 {
    let centre = Vec2::from(p.centre);
    Affine2::from_translation(centre + Vec2::from(p.offset))
        * Affine2::from_angle(p.turn)
        * Affine2::from_scale(Vec2::from(p.scale))
        * Affine2::from_translation(-centre)
}

/// What a mesh's material and texture controllers do to it, for meshes
/// that don't move.
pub(crate) fn painting(nif: &Nif, props: &[nif::Ref]) -> Option<Animation> {
    let fade = nif
        .property::<nif::Material>(props)
        .and_then(|m| controller(nif, m.controller, ControllerKind::Alpha))
        .and_then(|c| Some((c.clone(), nif.get_as::<Keys<f32>>(c.data)?.clone())));
    let texturing = nif.property::<nif::Texturing>(props);
    let changes: Vec<_> = texturing
        .into_iter()
        .flat_map(|t| controllers(nif, t.controller))
        .filter_map(|c| match c.kind {
            // Only the base texture is drawn.
            ControllerKind::Texture { slot: 0, change } => {
                Some((c.clone(), change, nif.get_as::<Keys<f32>>(c.data)?.clone()))
            }
            _ => None,
        })
        .collect();
    let texture = (!changes.is_empty()).then(|| TextureMotion {
        rest: texturing
            .and_then(|t| t.base.as_ref()?.placement)
            .unwrap_or(UNPLACED),
        changes,
    });
    (fade.is_some() || texture.is_some()).then_some(Animation {
        motion: Vec::new(),
        fade,
        texture,
    })
}

impl Animation {
    /// `painting`, if there is one, moved by `motion` as well.
    pub(crate) fn moved(motion: Vec<Moved>, painting: Option<Animation>) -> Animation {
        let (fade, texture) = painting.map_or((None, None), |p| (p.fade, p.texture));
        Animation {
            motion,
            fade,
            texture,
        }
    }

    /// Whether instances need their own material to animate.
    pub(crate) fn paints(&self) -> bool {
        self.fade.is_some() || self.texture.is_some()
    }

    fn transform(&self, t: f32) -> Transform {
        let placed = self.motion.iter().fold(nif::Transform::IDENTITY, |at, m| {
            let own = m.keys.apply(&m.base, m.controller.key_time(t));
            at.then(&m.above).then(&own)
        });
        bevy_transform(&placed)
    }
}

/// Plays a part's animation from when it's spawned, `offset` seconds in so
/// neighbouring objects don't move in step.
#[derive(Component)]
pub struct Animated {
    animation: Arc<Animation>,
    /// Seconds into the animation.
    time: f32,
    /// Seconds of animation per second played.
    rate: f32,
    material: PartMaterial,
}

impl Animated {
    pub(crate) fn new(animation: Arc<Animation>, offset: f32, material: PartMaterial) -> Self {
        Animated {
            animation,
            time: offset,
            rate: 1.0,
            material,
        }
    }

    /// One played at `rate` seconds of animation per second, from `offset`.
    pub(crate) fn at_rate(
        animation: Arc<Animation>,
        offset: f32,
        rate: f32,
        material: PartMaterial,
    ) -> Self {
        Animated {
            rate,
            ..Animated::new(animation, offset, material)
        }
    }

    /// Whether it changes its part's material, rather than only moving it.
    pub(crate) fn paints(&self) -> bool {
        self.animation.fade.is_some() || self.animation.texture.is_some()
    }

    /// Paints its part at its time, and gives the part's transform then if
    /// it moves.
    pub(crate) fn pose(&self, materials: &mut PartMaterials) -> Option<Transform> {
        let t = self.time;
        let moved = (!self.animation.motion.is_empty()).then(|| self.animation.transform(t));
        let alpha = self
            .animation
            .fade
            .as_ref()
            .and_then(|(c, keys)| keys.sample(c.key_time(t)));
        let uv = self
            .animation
            .texture
            .as_ref()
            .map(|m| uv_transform(&m.at(t)));
        if alpha.is_none() && uv.is_none() {
            return moved;
        }
        if let Some(m) = materials.base_mut(&self.material) {
            if let Some(alpha) = alpha {
                m.base_color.set_alpha(alpha.clamp(0.0, 1.0));
            }
            if let Some(uv) = uv {
                m.uv_transform = uv;
            }
        }
        moved
    }
}

pub fn animate(
    time: Res<Time>,
    mut parts: Query<(&mut Animated, &mut Transform)>,
    mut stores: MaterialStores,
) {
    let mut materials = stores.parts();
    for (mut a, mut transform) in &mut parts {
        a.time += time.delta_secs() * a.rate;
        if let Some(moved) = a.pose(&mut materials) {
            *transform = moved;
        }
    }
}

/// Plays a model, as spawned under the entity this is on, at this many
/// seconds of its animation per second, as an effect stretched or squeezed
/// to a duration is.
#[derive(Component, Clone, Copy)]
pub struct Rate(pub f32);

/// Sets the pace of the animations and particle systems under a newly
/// [`Rate`]d model.
pub fn pace(
    rated: Query<(Entity, &Rate), Added<Rate>>,
    children: Query<&Children>,
    mut animated: Query<&mut Animated>,
    mut rigs: Query<&mut crate::rig::Playing>,
    mut systems: Query<&mut particles::Particles>,
) {
    for (root, &Rate(rate)) in &rated {
        for e in std::iter::once(root).chain(children.iter_descendants(root)) {
            if let Ok(mut a) = animated.get_mut(e) {
                a.rate = rate;
            }
            if let Ok(mut p) = rigs.get_mut(e) {
                p.rate = rate;
            }
            if let Ok(mut s) = systems.get_mut(e) {
                s.rate = rate;
            }
        }
    }
}

/// What became of an object, best first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Placed {
    Drawn,
    Missing,
    NoModel,
}

/// Loads models and textures once each, however many objects use them.
pub struct Models {
    /// What every model, texture and clip is read from.
    game: Arc<GameData>,
    pub(crate) views: HashMap<Id, Option<View>>,
    /// The faces the object display shows, by id.
    pub(crate) portraits: HashMap<Id, Portrait>,
    /// By path, what they're loaded to play and where their textures are
    /// found.
    models: HashMap<ModelKey, Option<Model>>,
    textures: Textures,
    /// Each player's colour, by id.
    pub(crate) colours: HashMap<u32, [f32; 3]>,
    /// Team-coloured copies of materials, by the material and the colour's
    /// bits.
    painted: HashMap<(AssetId<StandardMaterial>, [u32; 3]), Handle<TeamMaterial>>,
    /// What's measured of each model, by path.
    measures: HashMap<String, Option<Arc<Measures>>>,
    /// `.kf` files by path, each shared by every model that plays it.
    clips: HashMap<String, Option<Arc<Nif>>>,
    /// The model file read last, by path, for whatever asks for it next:
    /// a model's measures and its meshes are often wanted one after the
    /// other.
    last: Option<(String, Arc<Nif>)>,
    /// The folder, beside a model or texture, that holds the map's
    /// biome's own version of it, if the biome has one.
    subfolder: Option<String>,
    /// Whether meshes stay readable off the GPU, for mesh picking.
    pickable: bool,
}

/// How a model's parts are painted as it's spawned.
#[derive(Clone, Copy, Default)]
pub(crate) struct Paint {
    /// Its owner's colour, for the parts that take it.
    pub colour: Option<[f32; 3]>,
    /// Lit by ambient light of this strength, of which each material
    /// reflects its own share: on the map, where it isn't, all reflect
    /// alike.
    pub ambient: Option<f32>,
}

/// Which of its view's animations a model is loaded to play.
#[derive(Clone, Copy)]
pub(crate) struct Plays<'a> {
    /// What it idles with, as a rig; without one it stands as modelled.
    pub idle: Option<&'a tgdata::Animation>,
    pub moving: Option<&'a tgdata::Animation>,
    /// Picks those it plays once when told to.
    pub once: fn(&tgdata::Animation) -> bool,
}

impl<'a> Plays<'a> {
    /// A unit's: idling, moving with `moving`, and using its abilities and
    /// dying once each.
    pub fn unit(view: &'a View, moving: Option<&'a tgdata::Animation>) -> Self {
        Plays {
            idle: idle(view),
            moving,
            once,
        }
    }

    /// The animations of `view`'s it plays once: what a model's cache key
    /// and the clips it loads are both made from.
    fn once_in<'v>(&self, view: &'v View) -> impl Iterator<Item = &'v tgdata::Animation> {
        let once = self.once;
        view.animations.iter().filter(move |a| once(a))
    }
}

/// How an effect's model plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectTiming {
    /// Seconds until the last of its keys has passed.
    pub length: f32,
    /// Some of it plays over and over rather than holding its end.
    pub loops: bool,
}

impl EffectTiming {
    /// The timing of a model's controllers; none if nothing in it is
    /// animated.
    pub fn of(nif: &Nif) -> Option<EffectTiming> {
        let mut out: Option<EffectTiming> = None;
        for object in &nif.objects {
            let Object::Controller(c) = object else {
                continue;
            };
            if c.stop <= c.start || c.frequency <= 0.0 {
                continue;
            }
            let loops = c.cycle() != nif::Cycle::Clamp;
            let length = (c.stop - c.phase) / c.frequency;
            let t = out.get_or_insert(EffectTiming {
                length: 0.0,
                loops: false,
            });
            t.length = t.length.max(length);
            t.loops |= loops;
        }
        out
    }
}

impl Models {
    pub fn new(
        game: Arc<GameData>,
        formats: CompressedImageFormats,
        players: &[Player],
        pickable: bool,
        biome: &str,
    ) -> Self {
        Models {
            subfolder: game.biome_subfolders().get(&Id::from(biome)).cloned(),
            views: game.views(),
            portraits: game.portraits(),
            models: HashMap::new(),
            textures: Textures::new(formats),
            colours: players.iter().map(|p| (p.id, p.rgb)).collect(),
            painted: HashMap::new(),
            measures: HashMap::new(),
            clips: HashMap::new(),
            last: None,
            pickable,
            game,
        }
    }

    /// The view a template's things are drawn with, if it has one.
    pub(crate) fn view(&self, template: &str) -> Option<&View> {
        self.views.get(&Id::from(template))?.as_ref()
    }

    /// The archive path of the model `thing` is drawn with, or with
    /// `packing` its packing model, unpacking if that's true.
    pub fn model_path(&self, thing: &Thing, packing: Option<bool>) -> Option<&str> {
        let view = self.view(&thing.template)?;
        match packing {
            Some(true) => view.unpacking.as_deref(),
            Some(false) => view.packing.as_deref(),
            None if view.models.is_empty() => None,
            None => Some(&view.models[variation(thing) % view.models.len()]),
        }
    }

    /// Where the node named `name`, exactly, is in the model at `path`,
    /// from its origin in its own units; `None` if it has no such node.
    pub fn point(&mut self, path: &str, name: &str) -> Option<Transform> {
        self.measures(path)?.points.get(name).map(bevy_transform)
    }

    /// How far `thing`'s remains sink before they're taken off: the top of
    /// its model for a `building`, else the diagonal of its ground size or,
    /// where the data sets none, the top of its model.
    pub fn remains_depth(&mut self, thing: &Thing, building: bool) -> f32 {
        let Some(view) = self.view(&thing.template) else {
            return 0.0;
        };
        if let Some([across, along]) = view.ground_size.filter(|_| !building) {
            return across.hypot(along);
        }
        let scale = view.scale * thing.scale;
        let Some(path) = self.model_path(thing, None).map(str::to_string) else {
            return 0.0;
        };
        self.extent(&path)
            .map_or(0.0, |[_, high]| high.max(0.0) * scale)
    }

    /// The heights of the lowest and highest of the meshes of the model at
    /// `path`, as modelled, in its own units.
    pub fn extent(&mut self, path: &str) -> Option<[f32; 2]> {
        let extent = self.measures(path)?.extent?;
        Some([extent.low, extent.high])
    }

    /// How the effect model at `path` plays; `None` if it can't be read or
    /// doesn't animate.
    pub fn effect_timing(&mut self, path: &str) -> Option<EffectTiming> {
        self.measures(path)?.timing
    }

    /// What's measured of the model at `path`; `None` if it can't be read.
    pub(crate) fn measures(&mut self, path: &str) -> Option<Arc<Measures>> {
        if let Some(measures) = self.measures.get(path) {
            return measures.clone();
        }
        let measures =
            (self.parse(path)).map(|nif| Arc::new(Measures::of(&nif, &Nodes::new(&nif))));
        self.measures.insert(path.to_string(), measures.clone());
        measures
    }

    /// The model file at `path`, read from the biome's folder where it has
    /// its own; warns if it's there but can't be read.
    fn parse(&mut self, path: &str) -> Option<Arc<Nif>> {
        if let Some((last, nif)) = &self.last
            && last == path
        {
            return Some(nif.clone());
        }
        let nif = Nif::parse(&self.game.read_in(self.subfolder.as_deref(), path)?)
            .map_err(|e| warn!("{path}: {e}"))
            .ok()?;
        if let Some(kind) = &nif.incomplete {
            debug!("{path}: stops at {kind}");
        }
        let nif = Arc::new(nif);
        self.last = Some((path.to_string(), nif.clone()));
        Some(nif)
    }

    /// The `.kf` file at `path`.
    fn clip(&mut self, path: &str) -> Option<Arc<Nif>> {
        cached(&mut self.clips, path, || {
            let kf = Nif::parse(&self.game.read(path)?).map_err(|e| warn!("{path}: {e}"));
            kf.ok().map(Arc::new)
        })
    }

    /// Spawns every object whose template has a readable model, returning
    /// their roots and the rest, each with whether it should have had one:
    /// false for templates that name no model, such as sound emitters, and
    /// for organizations drawn as their members, such as herds and those in
    /// `holding`: ones a building stands in, by id.
    pub fn spawn<'a>(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        things: impl IntoIterator<Item = &'a Thing>,
        holding: &HashSet<u32>,
    ) -> (Vec<Entity>, Vec<(&'a Thing, bool)>) {
        let (mut roots, mut rest) = (Vec::new(), Vec::new());
        let (mut drawn, mut modelless) = (0, 0);
        for t in things {
            let placed = self.place(commands, assets, t, None, holding, &mut roots);
            match placed {
                Placed::Drawn if self.by_members(t, holding) => rest.push((t, false)),
                Placed::Drawn => {}
                Placed::Missing => rest.push((t, true)),
                Placed::NoModel => {
                    rest.push((t, false));
                    modelless += 1;
                }
            }
            drawn += usize::from(placed == Placed::Drawn);
        }
        let mut missing: HashMap<&str, usize> = HashMap::new();
        for (t, _) in rest.iter().filter(|(_, should)| *should) {
            *missing.entry(t.template.as_str()).or_default() += 1;
        }
        info!(
            "{drawn} objects drawn with models, {} missing, {modelless} with no model",
            missing.values().sum::<usize>(),
        );
        debug!("templates without a readable model: {missing:?}");
        (roots, rest)
    }

    /// The archive path of a template's picture in the interface, if it
    /// has one.
    pub fn icon(&self, template: &str) -> Option<&str> {
        self.view(template)?.icon.as_deref()
    }

    /// The outline under which a template's models tilt to follow the
    /// ground, for [`pose`]; `None` for models that stay upright.
    pub fn conform(&self, template: &str) -> Option<Shape> {
        self.view(template)?.conform
    }

    /// The templates that raise an organization.
    pub(crate) fn organization_templates(&self) -> std::collections::HashSet<Id> {
        self.game.organization_components().into_keys().collect()
    }

    /// What height a template's objects stand at, for [`pose`]; on the
    /// ground for templates without a view.
    pub fn footing(&self, template: &str) -> Footing {
        self.view(template).map_or(Footing::Ground, |v| v.footing)
    }

    /// Spawns a unit of the player `owner` that moves at `speed`, its model
    /// playing the moving animation that suits that speed while
    /// [`crate::Playing::set_moving`] says it's moving. `None` if its model
    /// can't be drawn.
    pub fn spawn_unit(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        thing: &Thing,
        owner: u32,
        speed: f32,
    ) -> Option<Entity> {
        let view = self.view(&thing.template)?.clone();
        let moving = moving(&view, speed);
        self.spawn_model(commands, assets, thing, Some(owner), &view, moving)
    }

    /// Spawns an object that stands still, such as a building, painted in
    /// `owner`'s colour. `None` if its model can't be drawn.
    pub fn spawn_object(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        thing: &Thing,
        owner: Option<u32>,
    ) -> Option<Entity> {
        let view = self.view(&thing.template)?.clone();
        self.spawn_model(commands, assets, thing, owner, &view, None)
    }

    /// Spawns the model a building plays once as it packs, or unpacks,
    /// `into` seconds in, painted in its owner's colour. `None` if it has
    /// none that can be drawn.
    pub fn spawn_packing(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        thing: &Thing,
        owner: u32,
        unpacking: bool,
        into: f32,
    ) -> Option<Entity> {
        let view = self.view(&thing.template)?.clone();
        let path = match unpacking {
            true => view.unpacking.clone()?,
            false => view.packing.clone()?,
        };
        let model = self.own_animation(assets, &path, &view)?;
        if !matches!(model.meshes, Meshes::Rig(_)) {
            return None;
        }
        let root = commands
            .spawn((Name::new(path), placed(thing, &view), Visibility::default()))
            .id();
        let paint = Paint {
            colour: self.colours.get(&owner).copied(),
            ambient: None,
        };
        self.spawn_meshes(commands, assets, root, model, paint, into);
        Some(root)
    }

    /// Spawns the model a unit of the player `owner` is drawn with as it
    /// falls, under its parachute, standing still. `None` if it has none
    /// that can be drawn.
    pub fn spawn_falling(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        thing: &Thing,
        owner: u32,
    ) -> Option<Entity> {
        let falling = self.view(&thing.template)?.falling_view()?;
        self.spawn_model(commands, assets, thing, Some(owner), &falling, None)
    }

    /// Whether `thing` is an organization drawn as its members: one whose
    /// template names no model, such as a herd, or one in `holding`, whose
    /// own model is the truck it packs into.
    fn by_members(&self, thing: &Thing, holding: &HashSet<u32>) -> bool {
        let invisible = matches!(
            self.views.get(&Id::from(thing.template.as_str())),
            Some(None)
        );
        invisible || holding.contains(&thing.id)
    }

    /// Spawns one object, or, for an organization drawn as its members, the
    /// members where the map has them standing. Members belong to their
    /// organization's owner, `within`, unless they name their own.
    fn place(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        thing: &Thing,
        within: Option<u32>,
        holding: &HashSet<u32>,
        roots: &mut Vec<Entity>,
    ) -> Placed {
        let owner = owner(thing).or(within);
        if self.by_members(thing, holding) {
            // One member drawn shows the organization, as an HQ's crates
            // can lack a model its building has.
            let members = thing.organization().map_or(&[][..], |o| &o.members);
            return members
                .iter()
                .map(|m| self.place(commands, assets, m, owner, holding, roots))
                .min()
                .unwrap_or(Placed::NoModel);
        }
        let Some(view) = self.view(&thing.template).cloned() else {
            return Placed::Missing;
        };
        match self.spawn_model(commands, assets, thing, owner, &view, None) {
            Some(root) => {
                roots.push(root);
                Placed::Drawn
            }
            None => Placed::Missing,
        }
    }

    /// Spawns an object with one of its view's models, painted in its
    /// owner's colour, returning its root.
    fn spawn_model(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        thing: &Thing,
        owner: Option<u32>,
        view: &View,
        moving: Option<&tgdata::Animation>,
    ) -> Option<Entity> {
        if view.models.is_empty() {
            return None;
        }
        let model = &view.models[variation(thing) % view.models.len()];
        let model = self.model(assets, model, view, Plays::unit(view, moving))?;
        let paint = Paint {
            colour: owner.and_then(|o| self.colours.get(&o).copied()),
            ambient: None,
        };
        let mut root = commands.spawn((
            Name::new(thing.template.clone()),
            MapObject(thing.clone()),
            placed(thing, view),
            Visibility::default(),
        ));
        if let Some(size) = model.selection {
            root.insert(Footprint(size * view.scale * thing.scale));
        }
        // Spread over a minute by the golden ratio, so any run of ids lands
        // well apart.
        let offset = (thing.id as f32 * 0.618_034).fract() * 60.0;
        let root = root.id();
        self.spawn_meshes(commands, assets, root, model, paint, offset);
        Some(root)
    }

    /// Spawns an effect's model, such as the flag marking where an order
    /// was given, playing the animation its own nodes carry, its textures
    /// found beside it or in `texture_dirs`. `None` if it can't be drawn.
    pub fn spawn_effect(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        path: &str,
        texture_dirs: &[String],
        transform: Transform,
    ) -> Option<Entity> {
        let view = View::of_models(vec![path.to_string()], texture_dirs.to_vec());
        let model = self.own_animation(assets, path, &view)?;
        let root = commands
            .spawn((
                Name::new(path.to_string()),
                transform,
                Visibility::default(),
            ))
            .id();
        self.spawn_meshes(commands, assets, root, model, Paint::default(), 0.0);
        Some(root)
    }

    /// A model that plays the animation its own nodes carry, as a rig if
    /// they're animated, its textures found as `view` finds them.
    fn own_animation(
        &mut self,
        assets: &mut ModelAssets,
        path: &str,
        view: &View,
    ) -> Option<Model> {
        let key = ModelKey::own(path, view);
        if let Some(model) = self.models.get(&key) {
            return model.clone();
        }
        let model = self
            .parse(path)
            .and_then(|nif| self.build(assets, &nif, path, view, Some(Clips::default())));
        self.models.insert(key, model.clone());
        model
    }

    /// Spawns a model's meshes and particle systems under `root`, painted
    /// as `paint` says, animations `offset` seconds in.
    pub(crate) fn spawn_meshes(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        root: Entity,
        model: Model,
        paint: Paint,
        offset: f32,
    ) {
        let parts = match model.meshes {
            Meshes::Parts(parts) => {
                particles::spawn(commands, &model.particles, root, |_| None, offset);
                parts
            }
            Meshes::Rig(rig) => {
                let materials = rig
                    .parts()
                    .map(|p| self.own_paint(assets, p, paint))
                    .collect();
                let joints = rig.spawn(commands, root, offset, materials);
                let node = |i| rig.joint(i).map(|j| joints[j]);
                particles::spawn(commands, &model.particles, root, node, offset);
                return;
            }
        };
        let materials: Vec<_> = parts
            .iter()
            .map(|p| self.own_paint(assets, p, paint))
            .collect();
        commands.entity(root).with_children(|parent| {
            for (part, material) in parts.into_iter().zip(materials) {
                let mut child = parent.spawn(Mesh3d(part.mesh));
                if part.blended {
                    child.insert(NotShadowCaster);
                }
                material.insert(&mut child);
                if let Some(animation) = part.animation {
                    child.insert((
                        animation.transform(offset),
                        Animated::new(animation, offset, material),
                    ));
                }
            }
        });
    }

    /// A part's material as [`Self::paint`] gives it, copied if the part's
    /// animation changes it.
    fn own_paint(&mut self, assets: &mut ModelAssets, part: &Part, paint: Paint) -> PartMaterial {
        let material = self.paint(assets, part, paint.colour);
        let material = match part.animation.as_ref().is_some_and(|a| a.paints()) {
            true => assets.materials.copy(&material),
            false => material,
        };
        match paint.ambient {
            Some(light) => reflecting(material, part, light, assets),
            None => material,
        }
    }

    /// A part's material, painted in `colour` if the part takes its owner's.
    fn paint(
        &mut self,
        assets: &mut ModelAssets,
        part: &Part,
        colour: Option<[f32; 3]>,
    ) -> PartMaterial {
        let (PartMaterial::Standard(h), true, Some(colour)) = (&part.material, part.team, colour)
        else {
            return part.material.clone();
        };
        let key = (h.id(), colour.map(f32::to_bits));
        if let Some(painted) = self.painted.get(&key) {
            return PartMaterial::Team(painted.clone());
        }
        // Masked texels would be cut out before the colour shows.
        let Some(base) = assets
            .materials
            .standard
            .get(h)
            .filter(|m| m.alpha_mode == AlphaMode::Opaque)
        else {
            return part.material.clone();
        };
        let base = base.clone();
        let painted = assets.materials.team.add(TeamMaterial {
            base,
            extension: Team::new(colour),
        });
        self.painted.insert(key, painted.clone());
        PartMaterial::Team(painted)
    }

    /// The model at `path`, its textures found as `view` finds them,
    /// loaded to play what `plays` picks.
    pub(crate) fn model(
        &mut self,
        assets: &mut ModelAssets,
        path: &str,
        view: &View,
        plays: Plays,
    ) -> Option<Model> {
        let once = plays.once_in(view);
        let clips = plays.idle.into_iter().chain(plays.moving).chain(once);
        let key = ModelKey::playing(path, clips.map(|a| a.kf.clone()), view);
        if let Some(model) = self.models.get(&key) {
            return model.clone();
        }
        let model = self.parse(path).and_then(|nif| {
            let clips = self.clips(view, plays);
            self.build(assets, &nif, path, view, clips)
        });
        self.models.insert(key, model.clone());
        model
    }

    /// The clips a rig plays, as `plays` picks them from `view`; `None`
    /// without an idle clip that can be read.
    fn clips(&mut self, view: &View, plays: Plays) -> Option<Clips> {
        let idle = plays.idle?;
        let mut clips = Clips {
            idle: Some((self.clip(&idle.kf)?, idle.length)),
            ..Clips::default()
        };
        clips.moving = (plays.moving).and_then(|a| Some((self.clip(&a.kf)?, a.walk_distance?)));
        for a in plays.once_in(view) {
            let Some(kf) = self.clip(&a.kf) else {
                continue;
            };
            match a.kind {
                AnimationKind::Die => clips.deaths.push((a.ids.clone(), kf, a.length)),
                _ => clips.abilities.push((a.ids.clone(), kf, a.length)),
            }
        }
        Some(clips)
    }

    /// A model from its file, as a rig playing `clips` if it has them and
    /// they animate any of its nodes, else standing as modelled.
    fn build(
        &mut self,
        assets: &mut ModelAssets,
        nif: &Nif,
        path: &str,
        view: &View,
        clips: Option<Clips>,
    ) -> Option<Model> {
        let nodes = Nodes::new(nif);
        let selection = selection_box(nif, &nodes.frames);
        let mut loader = self.loader(assets, nif, path, view);
        let particles = loader.particles(&nodes.frames);
        if let Some(clips) = clips {
            let meshes = loader.rig_meshes(&nodes);
            if let Some(rig) = nodes.rig(nif, meshes, &clips, assets) {
                return Some(Model {
                    meshes: Meshes::Rig(Arc::new(rig)),
                    particles,
                    selection,
                });
            }
        }
        // Skinned shapes are drawn in the pose they were modelled in.
        let mut loader = self.loader(assets, nif, path, view);
        let parts = loader.scene_parts(nif.shapes(), true);
        (!parts.is_empty() || !particles.is_empty()).then_some(Model {
            meshes: Meshes::Parts(parts),
            particles,
            selection,
        })
    }

    /// What builds the parts of the model file `nif`, at `path`, with its
    /// textures found as `view` finds them.
    fn loader<'a, 'w>(
        &'a mut self,
        assets: &'a mut ModelAssets<'w>,
        nif: &'a Nif,
        path: &'a str,
        view: &'a View,
    ) -> Loader<'a, 'w> {
        Loader {
            game: &self.game,
            assets,
            textures: &mut self.textures,
            subfolder: self.subfolder.as_deref(),
            pickable: self.pickable,
            nif,
            path,
            view,
        }
    }
}

/// A part's material as it shows under ambient light of strength `light`,
/// reflecting its own share of that rather than as much as it does of
/// light that falls on it: a copy that glows with the difference, where
/// there is one.
fn reflecting(
    material: PartMaterial,
    part: &Part,
    light: f32,
    assets: &mut ModelAssets,
) -> PartMaterial {
    let Some((ambient, diffuse)) = part.reflects else {
        return material;
    };
    let linear = |[r, g, b]: [f32; 3]| Color::srgb(r, g, b).to_linear().to_vec3();
    let more = (linear(ambient) - linear(diffuse) * light).max(Vec3::ZERO);
    if more.max_element() < 1e-3 {
        return material;
    }
    // These reflect their own share of it wherever they're drawn.
    if let PartMaterial::Glow(_) = material {
        return material;
    }
    let copy = assets.materials.copy(&material);
    if let Some(base) = assets.materials.base_mut(&copy) {
        base.emissive = LinearRgba::rgb(more.x, more.y, more.z);
        base.emissive_texture = base.base_color_texture.clone();
    }
    copy
}

/// What a model is loaded once for: the same file is another model when
/// it plays other clips or finds its textures elsewhere.
#[derive(PartialEq, Eq, Hash)]
struct ModelKey {
    path: String,
    /// The `.kf` files it plays, or none if it plays its own nodes'
    /// animation.
    clips: Option<Vec<String>>,
    lookup: Lookup,
}

impl ModelKey {
    /// The model at `path` playing its own nodes' animation, its textures
    /// found as `view` finds them.
    fn own(path: &str, view: &View) -> ModelKey {
        ModelKey {
            path: path.to_string(),
            clips: None,
            lookup: Lookup::of(path, view),
        }
    }

    /// The model at `path` playing `clips`, its textures found as `view`
    /// finds them.
    fn playing(path: &str, clips: impl IntoIterator<Item = String>, view: &View) -> ModelKey {
        ModelKey {
            path: path.to_string(),
            clips: Some(clips.into_iter().collect()),
            lookup: Lookup::of(path, view),
        }
    }
}

/// Where a view finds a model's textures.
#[derive(PartialEq, Eq, Hash, Debug)]
struct Lookup {
    dirs: Vec<String>,
    renames: Option<Vec<(String, String)>>,
}

impl Lookup {
    /// Where `view` finds the textures of the model at `path`.
    fn of(path: &str, view: &View) -> Lookup {
        Lookup {
            dirs: view.texture_dirs.clone(),
            renames: view.texture_renames.get(path).cloned(),
        }
    }
}

/// The size of the hidden box a model marks its selectable extent with:
/// width across its -y facing and length along it.
fn selection_box(nif: &Nif, frames: &Frames) -> Option<Vec2> {
    let (at, shape) = nif.objects.iter().enumerate().find_map(|(i, o)| match o {
        Object::Shape(s) if s.av.name.eq_ignore_ascii_case("SelectionBox") => Some((i, s)),
        _ => None,
    })?;
    let g = nif.get_as::<nif::Geometry>(shape.data)?;
    // Where the nodes above it put it, which may turn it.
    let placed = frames.0.get(&at).map_or(shape.av.transform, |a| a.model);
    let (mut min, mut max) = (Vec2::MAX, Vec2::MIN);
    for &p in &g.positions {
        let [x, y, _] = placed.apply(p);
        (min, max) = (min.min(Vec2::new(x, y)), max.max(Vec2::new(x, y)));
    }
    let size = max - min;
    (size.min_element() > 0.0).then_some(size)
}

/// The value cached for `key`, loading and caching it on first use.
pub(crate) fn cached<K, Q, T>(
    cache: &mut HashMap<K, Option<T>>,
    key: &Q,
    load: impl FnOnce() -> Option<T>,
) -> Option<T>
where
    K: Borrow<Q> + Hash + Eq,
    Q: ToOwned<Owned = K> + Hash + Eq + ?Sized,
    T: Clone,
{
    if let Some(v) = cache.get(key) {
        return v.clone();
    }
    let v = load();
    cache.insert(key.to_owned(), v.clone());
    v
}

/// The first active controller of `kind` in the chain starting at `first`.
pub(crate) fn controller(nif: &Nif, first: nif::Ref, kind: ControllerKind) -> Option<&Controller> {
    controllers(nif, first).find(|c| c.kind == kind)
}

/// The active controllers in the chain starting at `first`.
pub(crate) fn controllers(nif: &Nif, first: nif::Ref) -> impl Iterator<Item = &Controller> {
    let mut next = first;
    std::iter::from_fn(move || {
        let c = nif.get_as::<Controller>(next)?;
        next = c.next;
        Some(c)
    })
    .take(16)
    .filter(|c| c.active())
}

/// The keyframes moving the shape at the end of `path`, on it or any
/// object above it, from the root down, and the transform from the last of
/// them down to the shape; `None` if nothing on the path is animated. The
/// first object's own transform is left out unless `root_placed`.
pub(crate) fn motion(
    nif: &Nif,
    path: &[usize],
    root_placed: bool,
) -> Option<(Vec<Moved>, nif::Transform)> {
    let scene = |i: usize| nif.get(Some(i)).and_then(Object::scene);
    let transform = |i: usize| match path.first() {
        Some(&root) if i == root && !root_placed => Some(nif::Transform::IDENTITY),
        _ => scene(i).map(|av| av.transform),
    };
    let chain = |range: &[usize]| {
        range
            .iter()
            .filter_map(|&i| transform(i))
            .fold(nif::Transform::IDENTITY, |t, av| t.then(&av))
    };
    let mut moved = Vec::new();
    // The first object below the last animated one.
    let mut from = 0;
    for (at, &i) in path.iter().enumerate() {
        let keyed = scene(i)
            .and_then(|av| controller(nif, av.controller, ControllerKind::Keyframe))
            .and_then(|c| Some((c, nif.get_as::<Keyframes>(c.data)?, transform(i)?)));
        let Some((controller, keys, base)) = keyed else {
            continue;
        };
        moved.push(Moved {
            above: chain(&path[from..at]),
            controller: controller.clone(),
            keys: keys.clone(),
            base,
        });
        from = at + 1;
    }
    (!moved.is_empty()).then(|| (moved, chain(&path[from..])))
}

/// A model-space transform in Bevy axes.
pub(crate) fn bevy_transform(t: &nif::Transform) -> Transform {
    let [x, y, z] = t.translation;
    let r = t.rotation;
    // The game's z-up axes map to Bevy's as (x, y, z) -> (x, z, -y); conjugate
    // the rotation by that change of basis.
    let m = Mat3::from_cols(
        Vec3::new(r[0][0], r[2][0], -r[1][0]),
        Vec3::new(r[0][2], r[2][2], -r[1][2]),
        -Vec3::new(r[0][1], r[2][1], -r[1][1]),
    );
    Transform {
        translation: world(x, y, z),
        rotation: Quat::from_mat3(&m),
        scale: Vec3::splat(t.scale),
    }
}

/// Bevy asset stores the models are added to.
pub struct ModelAssets<'a> {
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: PartMaterials<'a>,
    pub images: &'a mut Assets<Image>,
    pub skins: &'a mut Assets<SkinnedMeshInverseBindposes>,
}

/// The moving animation that best suits `speed`: the one whose stride
/// takes as long, played at its natural pace.
fn moving(view: &View, speed: f32) -> Option<&tgdata::Animation> {
    view.animations
        .iter()
        .filter(|a| a.kind == AnimationKind::Move)
        .filter_map(|a| Some((a, a.walk_distance? / a.length - speed)))
        .min_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
        .map(|(a, _)| a)
}

/// Whether an animation is one units play once when told to: using an
/// ability or dying.
fn once(a: &tgdata::Animation) -> bool {
    matches!(a.kind, AnimationKind::Ability | AnimationKind::Die)
}

/// The idle animation a view's models play: the one named `idle`, else the
/// first of its kind.
pub(crate) fn idle(view: &View) -> Option<&tgdata::Animation> {
    let mut idles = view
        .animations
        .iter()
        .filter(|a| a.kind == AnimationKind::Idle);
    idles
        .clone()
        .find(|a| a.ids.eq_ignore_ascii_case("idle"))
        .or_else(|| idles.next())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear(keys: &[(f32, f32)]) -> Keys<f32> {
        Keys {
            interpolation: nif::Interpolation::Linear,
            keys: keys
                .iter()
                .map(|&(time, value)| nif::Key {
                    time,
                    value,
                    tangents: None,
                })
                .collect(),
        }
    }

    fn looping(kind: TextureChange) -> Controller {
        Controller {
            kind: ControllerKind::Texture {
                slot: 0,
                change: kind,
            },
            modifier: String::new(),
            next: None,
            flags: 8,
            frequency: 1.0,
            phase: 0.0,
            start: 0.0,
            stop: 4.0,
            target: None,
            data: None,
        }
    }

    #[test]
    fn textures_turn_and_stretch_about_their_centre() {
        let turned = TexturePlacement {
            turn: std::f32::consts::PI,
            ..UNPLACED
        };
        let at = |p: &TexturePlacement, uv: [f32; 2]| {
            let v = uv_transform(p).transform_point2(Vec2::from(uv));
            [v.x, v.y].map(|c| (c * 1e4).round() / 1e4)
        };
        assert_eq!(at(&turned, [0.5, 0.5]), [0.5, 0.5]);
        assert_eq!(at(&turned, [0.0, 0.0]), [1.0, 1.0]);
        let stretched = TexturePlacement {
            scale: [2.0, 1.0],
            offset: [0.0, 0.25],
            ..UNPLACED
        };
        assert_eq!(at(&stretched, [0.5, 0.5]), [0.5, 0.75]);
        assert_eq!(at(&stretched, [1.0, 0.0]), [1.5, 0.25]);
    }

    #[test]
    fn texture_motion_changes_only_what_it_animates() {
        let rest = TexturePlacement {
            offset: [0.0, 1.0],
            turn: 0.5,
            ..UNPLACED
        };
        let motion = TextureMotion {
            rest,
            changes: vec![
                (
                    looping(TextureChange::OffsetV),
                    TextureChange::OffsetV,
                    linear(&[(0.0, 1.0), (4.0, -1.0)]),
                ),
                (
                    looping(TextureChange::ScaleU),
                    TextureChange::ScaleU,
                    linear(&[]),
                ),
            ],
        };
        let p = motion.at(1.0);
        assert_eq!(p.offset, [0.0, 0.5]);
        assert_eq!((p.turn, p.scale), (0.5, [1.0, 1.0]));
        assert_eq!(motion.at(5.0).offset, [0.0, 0.5]);
    }

    fn scene(name: &str, controller: nif::Ref, transform: nif::Transform) -> nif::SceneObject {
        nif::SceneObject {
            name: name.into(),
            extra_data: Vec::new(),
            controller,
            flags: 0,
            transform,
            properties: Vec::new(),
        }
    }

    fn node(av: nif::SceneObject, children: Vec<nif::Ref>) -> Object {
        Object::Node(nif::Node {
            av,
            children,
            billboard: None,
        })
    }

    fn moved_to(at: [f32; 3]) -> nif::Transform {
        nif::Transform {
            translation: at,
            ..nif::Transform::IDENTITY
        }
    }

    /// Keyframes holding an object at `at`, and the controller playing
    /// them from the object at `data`.
    fn held(at: [f32; 3], data: usize) -> [Object; 2] {
        let keys = Keys {
            interpolation: nif::Interpolation::Linear,
            keys: vec![nif::Key {
                time: 0.0,
                value: at,
                tangents: None,
            }],
        };
        [
            Object::Controller(Controller {
                kind: ControllerKind::Keyframe,
                data: Some(data),
                ..looping(TextureChange::OffsetU)
            }),
            Object::Keyframes(Keyframes {
                translation: keys,
                ..Default::default()
            }),
        ]
    }

    #[test]
    fn every_animated_object_above_a_mesh_moves_it() {
        // A mesh under an arm that its keys lift by 1, itself under a body
        // moved 5 along x that its keys move to 2 along x instead.
        let [body_c, body_k] = held([2.0, 0.0, 0.0], 5);
        let [arm_c, arm_k] = held([0.0, 0.0, 1.0], 7);
        let nif = Nif {
            objects: vec![
                node(
                    scene("root", None, moved_to([0.0, 9.0, 0.0])),
                    vec![Some(1)],
                ),
                node(
                    scene("body", Some(4), moved_to([5.0, 0.0, 0.0])),
                    vec![Some(2)],
                ),
                node(
                    scene("arm", Some(6), nif::Transform::IDENTITY),
                    vec![Some(3)],
                ),
                Object::Shape(nif::Shape {
                    av: scene("mesh", None, moved_to([0.0, 0.5, 0.0])),
                    data: None,
                    skin: None,
                }),
                body_c,
                body_k,
                arm_c,
                arm_k,
            ],
            roots: vec![Some(0)],
            incomplete: None,
        };
        let (motion, below) = motion(&nif, &[0, 1, 2, 3], true).expect("animated");
        assert_eq!(motion.len(), 2);
        assert_eq!(below.translation, [0.0, 0.5, 0.0]);
        let animation = Animation {
            motion,
            fade: None,
            texture: None,
        };
        let at = animation.transform(0.0).translation;
        assert!(at.abs_diff_eq(world(2.0, 9.0, 1.0), 1e-5), "{at}");
        // Unanimated, nothing moves it.
        assert!(super::motion(&nif, &[0], true).is_none());
    }

    #[test]
    fn selection_boxes_turn_with_the_nodes_above_them() {
        // A box 4 wide and 2 long under a node turned a quarter turn.
        let turned = nif::Transform {
            rotation: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            ..nif::Transform::IDENTITY
        };
        let mut hidden = scene("SelectionBox", None, nif::Transform::IDENTITY);
        hidden.flags = 1;
        let nif = Nif {
            objects: vec![
                node(scene("root", None, turned), vec![Some(1)]),
                Object::Shape(nif::Shape {
                    av: hidden,
                    data: Some(2),
                    skin: None,
                }),
                Object::Geometry(nif::Geometry {
                    positions: vec![[-2.0, -1.0, 0.0], [2.0, 1.0, 0.0]],
                    ..Default::default()
                }),
            ],
            roots: vec![Some(0)],
            incomplete: None,
        };
        let size = selection_box(&nif, &Nodes::new(&nif).frames).expect("a box");
        assert!(size.abs_diff_eq(Vec2::new(2.0, 4.0), 1e-5), "{size}");
    }

    #[test]
    fn models_playing_their_own_animation_are_kept_apart_from_those_playing_none() {
        let view = View::of_models(Vec::new(), Vec::new());
        let own = ModelKey::own("Flag.nif", &view);
        assert!(own != ModelKey::playing("Flag.nif", [], &view));
        assert!(own == ModelKey::own("Flag.nif", &view));
    }

    #[test]
    fn models_are_kept_apart_by_where_their_textures_are() {
        let view = |dirs: &[&str]| {
            View::of_models(Vec::new(), dirs.iter().map(|d| d.to_string()).collect())
        };
        let (unit, organization) = (view(&["Shared/SharedTextures"]), view(&[]));
        assert_ne!(
            Lookup::of("Tank.nif", &unit),
            Lookup::of("Tank.nif", &organization)
        );
        assert_eq!(
            Lookup::of("Tank.nif", &unit),
            Lookup::of("Tank.nif", &unit.clone())
        );
        let mut renamed = unit.clone();
        let renames = vec![("Base".to_string(), "Snow".to_string())];
        renamed.texture_renames.insert("Tank.nif".into(), renames);
        assert_ne!(
            Lookup::of("Tank.nif", &unit),
            Lookup::of("Tank.nif", &renamed)
        );
        assert_eq!(
            Lookup::of("Truck.nif", &unit),
            Lookup::of("Truck.nif", &renamed)
        );
    }

    fn ground(height: impl Fn(f32, f32) -> f32) -> Ground {
        let side = 9;
        let heights = (0..side * side)
            .map(|i| height((i % side) as f32 * 2.0, (i / side) as f32 * 2.0))
            .collect();
        Ground::from_heights(heights)
    }

    const TANK: Shape = Shape::Rect {
        width: 1.5,
        height: 2.5,
    };

    #[test]
    fn conforming_on_the_flat_turns_like_upright() {
        let flat = ground(|_, _| 3.0);
        for facing in [[0.0, -1.0], [1.0, 0.0], [0.6, 0.8]] {
            let upright = pose(&flat, [8.0, 8.0], facing, None, Footing::Ground);
            let conform = pose(&flat, [8.0, 8.0], facing, Some(TANK), Footing::Ground);
            assert!(upright.0.abs_diff_eq(conform.0, 1e-5));
            assert!(upright.1.abs_diff_eq(conform.1, 1e-5), "{facing:?}");
        }
    }

    #[test]
    fn climbing_points_the_nose_up_the_slope() {
        let slope = ground(|_, y| y * 0.5);
        let (_, rotation) = pose(&slope, [8.0, 8.0], [0.0, 1.0], Some(TANK), Footing::Ground);
        // The model's -y is its front, which is Bevy's +z.
        let nose = rotation * Vec3::Z;
        let ahead = world(0.0, 1.0, 0.5).normalize();
        assert!(nose.abs_diff_eq(ahead, 1e-5), "{nose}");
    }
}
