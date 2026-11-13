//! Placed objects drawn with the game's models, where their templates name
//! models this crate can read.

use std::collections::{HashMap, HashSet};
use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use aaa_data::{Footing, GameData, Id, Shape, View};
use aaa_map::{Ground, Player, owner};
use bevy::asset::RenderAssetUsages;
use bevy::image::CompressedImageFormats;
use bevy::light::NotShadowCaster;
use bevy::math::Affine2;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat};
use nif::{
    Controller, ControllerKind, Keyframes, Keys, Nif, Object, TextureChange, TexturePlacement,
};
use tgck::map::Thing;

use crate::MapObject;
use crate::glow::{Glow, GlowMaterial, Source};
use crate::particles::{self, Look, System};
use crate::rig::{Nodes, Rig};
use crate::solid::{Solid, SolidMaterial};
use crate::team::{self, Team, TeamMaterial};
use crate::terrain::world;
use crate::textures;

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
    team: bool,
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

    /// A copy of the material of its own, to animate.
    fn copy(&self, assets: &mut ModelAssets) -> Self {
        self.copied(
            assets.materials,
            assets.glow_materials,
            assets.team_materials,
            assets.solid_materials,
        )
    }

    /// [`Self::copy`], into the stores themselves.
    pub(crate) fn copied(
        &self,
        materials: &mut Assets<StandardMaterial>,
        glow_materials: &mut Assets<GlowMaterial>,
        team_materials: &mut Assets<TeamMaterial>,
        solid_materials: &mut Assets<SolidMaterial>,
    ) -> Self {
        fn copy<M: Asset + Clone>(store: &mut Assets<M>, h: &Handle<M>) -> Handle<M> {
            store.get(h).cloned().map_or(h.clone(), |m| store.add(m))
        }
        match self {
            PartMaterial::Standard(h) => PartMaterial::Standard(copy(materials, h)),
            PartMaterial::Glow(h) => PartMaterial::Glow(copy(glow_materials, h)),
            PartMaterial::Team(h) => PartMaterial::Team(copy(team_materials, h)),
            PartMaterial::Solid(h) => PartMaterial::Solid(copy(solid_materials, h)),
        }
    }
}

/// A loaded model.
#[derive(Clone)]
struct Model {
    meshes: Meshes,
    particles: Vec<Arc<System>>,
    /// Its selection box's width and length, in model units.
    selection: Option<Vec2>,
}

/// A model's meshes placed in the model, or a rig of its nodes for one that
/// plays a `.kf` animation.
#[derive(Clone)]
enum Meshes {
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
struct Moved {
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
fn uv_transform(p: &TexturePlacement) -> Affine2 {
    let centre = Vec2::from(p.centre);
    Affine2::from_translation(centre + Vec2::from(p.offset))
        * Affine2::from_angle(p.turn)
        * Affine2::from_scale(Vec2::from(p.scale))
        * Affine2::from_translation(-centre)
}

/// What a mesh's material and texture controllers do to it, for meshes
/// that don't move.
fn painting(nif: &Nif, props: &[nif::Ref]) -> Option<Animation> {
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

    /// Paints its part at its time, and gives the part's transform then if
    /// it moves.
    pub(crate) fn pose(
        &self,
        materials: &mut Assets<StandardMaterial>,
        glow_materials: &mut Assets<GlowMaterial>,
        team_materials: &mut Assets<TeamMaterial>,
        solid_materials: &mut Assets<SolidMaterial>,
    ) -> Option<Transform> {
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
        let base = match &self.material {
            PartMaterial::Standard(h) => materials.get_mut(h).map(|m| m.into_inner()),
            PartMaterial::Glow(h) => glow_materials.get_mut(h).map(|m| &mut m.into_inner().base),
            PartMaterial::Team(h) => team_materials.get_mut(h).map(|m| &mut m.into_inner().base),
            PartMaterial::Solid(h) => solid_materials.get_mut(h).map(|m| &mut m.into_inner().base),
        };
        if let Some(m) = base {
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
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut glow_materials: ResMut<Assets<GlowMaterial>>,
    mut team_materials: ResMut<Assets<TeamMaterial>>,
    mut solid_materials: ResMut<Assets<SolidMaterial>>,
) {
    for (mut a, mut transform) in &mut parts {
        a.time += time.delta_secs() * a.rate;
        let moved = a.pose(
            &mut materials,
            &mut glow_materials,
            &mut team_materials,
            &mut solid_materials,
        );
        if let Some(moved) = moved {
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
    views: HashMap<Id, Option<View>>,
    /// By path, and for rigs the animation's path too.
    models: HashMap<String, Option<Model>>,
    textures: HashMap<String, Option<Handle<Image>>>,
    /// Bevy's texture formats this GPU can sample compressed.
    formats: CompressedImageFormats,
    /// Each player's colour, by id.
    colours: HashMap<u32, [f32; 3]>,
    /// Team-coloured copies of materials, by the material and the colour's
    /// bits.
    painted: HashMap<(AssetId<StandardMaterial>, [u32; 3]), Handle<TeamMaterial>>,
    /// Each model's named nodes, by path, as [`Nodes::points`] has them.
    points: HashMap<String, HashMap<String, nif::Transform>>,
    /// Each model's lowest and highest point, by path.
    extents: HashMap<String, Option<[f32; 2]>>,
    timings: HashMap<String, Option<EffectTiming>>,
    /// Whether meshes stay readable off the GPU, for mesh picking.
    pickable: bool,
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
            // Flag bits 1-2 of 2 hold the end; the rest repeat.
            let loops = (c.flags >> 1) & 3 != 2;
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
        game: &GameData,
        formats: CompressedImageFormats,
        players: &[Player],
        pickable: bool,
    ) -> Self {
        Models {
            views: game.views(),
            models: HashMap::new(),
            textures: HashMap::new(),
            formats,
            colours: players.iter().map(|p| (p.id, p.rgb)).collect(),
            painted: HashMap::new(),
            points: HashMap::new(),
            extents: HashMap::new(),
            timings: HashMap::new(),
            pickable,
        }
    }

    /// The archive path of the model `thing` is drawn with, or with
    /// `packing` its packing model, unpacking if that's true.
    pub fn model_path(&self, thing: &Thing, packing: Option<bool>) -> Option<&str> {
        let view = self
            .views
            .get(&Id::from(thing.template.as_str()))?
            .as_ref()?;
        match packing {
            Some(true) => view.unpacking.as_deref(),
            Some(false) => view.packing.as_deref(),
            None if view.models.is_empty() => None,
            None => Some(&view.models[variation(thing) % view.models.len()]),
        }
    }

    /// Where the node named `name`, exactly, is in the model at `path`,
    /// from its origin in its own units; `None` if it has no such node.
    pub fn point(&mut self, game: &GameData, path: &str, name: &str) -> Option<Transform> {
        let points = self.points.entry(path.to_string()).or_insert_with(|| {
            game.read(path)
                .and_then(|b| Nif::parse(&b).ok())
                .map(|nif| Nodes::new(&nif).points())
                .unwrap_or_default()
        });
        points.get(name).map(bevy_transform)
    }

    /// The heights of the lowest and highest of the meshes of the model at
    /// `path`, as modelled, in its own units.
    pub fn extent(&mut self, game: &GameData, path: &str) -> Option<[f32; 2]> {
        *self.extents.entry(path.to_string()).or_insert_with(|| {
            let nif = Nif::parse(&game.read(path)?).ok()?;
            let mut out: Option<[f32; 2]> = None;
            for placed in nif.shapes() {
                let Some(g) = nif.get_as::<nif::Geometry>(placed.shape.data) else {
                    continue;
                };
                for &p in &g.positions {
                    let z = placed.transform.apply(p)[2];
                    let [lo, hi] = out.get_or_insert([z, z]);
                    *lo = lo.min(z);
                    *hi = hi.max(z);
                }
            }
            out
        })
    }

    /// How the effect model at `path` plays; `None` if it can't be read or
    /// doesn't animate.
    pub fn effect_timing(&mut self, game: &GameData, path: &str) -> Option<EffectTiming> {
        *self.timings.entry(path.to_string()).or_insert_with(|| {
            let nif = Nif::parse(&game.read(path)?).ok()?;
            EffectTiming::of(&nif)
        })
    }

    /// Spawns every object whose template has a readable model and returns
    /// the rest, each with whether it should have had one: false for
    /// templates that name no model, such as sound emitters, and for
    /// organizations drawn as their members, such as herds and those in
    /// `holding`: ones a building stands in, by id.
    pub fn spawn<'a>(
        &mut self,
        commands: &mut Commands,
        game: &GameData,
        assets: &mut ModelAssets,
        things: impl IntoIterator<Item = &'a Thing>,
        holding: &HashSet<u32>,
    ) -> Vec<(&'a Thing, bool)> {
        let mut rest = Vec::new();
        let (mut drawn, mut modelless) = (0, 0);
        for t in things {
            let placed = self.place(commands, game, assets, t, None, holding);
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
        rest
    }

    /// The archive path of a template's picture in the interface, if it
    /// has one.
    pub fn icon(&self, template: &str) -> Option<&str> {
        self.views
            .get(&Id::from(template))?
            .as_ref()?
            .icon
            .as_deref()
    }

    /// The outline under which a template's models tilt to follow the
    /// ground, for [`pose`]; `None` for models that stay upright.
    pub fn conform(&self, template: &str) -> Option<Shape> {
        self.views.get(&Id::from(template))?.as_ref()?.conform
    }

    /// What height a template's objects stand at, for [`pose`]; on the
    /// ground for templates without a view.
    pub fn footing(&self, template: &str) -> Footing {
        let view = self.views.get(&Id::from(template));
        view.and_then(Option::as_ref)
            .map_or(Footing::Ground, |v| v.footing)
    }

    /// Spawns a unit of the player `owner` that moves at `speed`, its model
    /// playing the moving animation that suits that speed while
    /// [`crate::Playing::set_moving`] says it's moving. `None` if its model
    /// can't be drawn.
    pub fn spawn_unit(
        &mut self,
        commands: &mut Commands,
        game: &GameData,
        assets: &mut ModelAssets,
        thing: &Thing,
        owner: u32,
        speed: f32,
    ) -> Option<Entity> {
        let view = self
            .views
            .get(&Id::from(thing.template.as_str()))?
            .clone()?;
        let moving = moving(&view, speed);
        self.spawn_model(commands, game, assets, thing, Some(owner), &view, moving)
    }

    /// Spawns an object that stands still, such as a building. `None` if its
    /// model can't be drawn.
    pub fn spawn_object(
        &mut self,
        commands: &mut Commands,
        game: &GameData,
        assets: &mut ModelAssets,
        thing: &Thing,
    ) -> Option<Entity> {
        let view = self
            .views
            .get(&Id::from(thing.template.as_str()))?
            .clone()?;
        self.spawn_model(commands, game, assets, thing, owner(thing), &view, None)
    }

    /// Spawns a building of the player `owner`'s, such as one that's just
    /// unpacked. `None` if its model can't be drawn.
    pub fn spawn_building(
        &mut self,
        commands: &mut Commands,
        game: &GameData,
        assets: &mut ModelAssets,
        thing: &Thing,
        owner: u32,
    ) -> Option<Entity> {
        let view = self
            .views
            .get(&Id::from(thing.template.as_str()))?
            .clone()?;
        self.spawn_model(commands, game, assets, thing, Some(owner), &view, None)
    }

    /// Spawns the model a building plays once as it packs, or unpacks,
    /// `into` seconds in, painted in its owner's colour. `None` if it has
    /// none that can be drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_packing(
        &mut self,
        commands: &mut Commands,
        game: &GameData,
        assets: &mut ModelAssets,
        thing: &Thing,
        owner: u32,
        unpacking: bool,
        into: f32,
    ) -> Option<Entity> {
        let view = self
            .views
            .get(&Id::from(thing.template.as_str()))?
            .clone()?;
        let path = match unpacking {
            true => view.unpacking.clone()?,
            false => view.packing.clone()?,
        };
        let model = self.own_animation(game, assets, &path, view.clone())?;
        if !matches!(model.meshes, Meshes::Rig(_)) {
            return None;
        }
        let root = commands
            .spawn((Name::new(path), placed(thing, &view), Visibility::default()))
            .id();
        let colour = self.colours.get(&owner).copied();
        self.spawn_meshes(commands, assets, root, model, colour, into);
        Some(root)
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
        game: &GameData,
        assets: &mut ModelAssets,
        thing: &Thing,
        within: Option<u32>,
        holding: &HashSet<u32>,
    ) -> Placed {
        let owner = owner(thing).or(within);
        if self.by_members(thing, holding) {
            // One member drawn shows the organization, as an HQ's crates
            // can lack a model its building has.
            let members = thing.organization().map_or(&[][..], |o| &o.members);
            return members
                .iter()
                .map(|m| self.place(commands, game, assets, m, owner, holding))
                .min()
                .unwrap_or(Placed::NoModel);
        }
        let Some(Some(view)) = self.views.get(&Id::from(thing.template.as_str())) else {
            return Placed::Missing;
        };
        let view = view.clone();
        match self.spawn_model(commands, game, assets, thing, owner, &view, None) {
            Some(_) => Placed::Drawn,
            None => Placed::Missing,
        }
    }

    /// Spawns an object with one of its view's models, painted in its
    /// owner's colour, returning its root.
    #[allow(clippy::too_many_arguments)]
    fn spawn_model(
        &mut self,
        commands: &mut Commands,
        game: &GameData,
        assets: &mut ModelAssets,
        thing: &Thing,
        owner: Option<u32>,
        view: &View,
        moving: Option<&aaa_data::Animation>,
    ) -> Option<Entity> {
        if view.models.is_empty() {
            return None;
        }
        let model = &view.models[variation(thing) % view.models.len()];
        let model = self.model(game, assets, model, view, moving)?;
        let colour = owner.and_then(|o| self.colours.get(&o).copied());
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
        self.spawn_meshes(commands, assets, root, model, colour, offset);
        Some(root)
    }

    /// Spawns an effect's model, such as the flag marking where an order
    /// was given, playing the animation its own nodes carry, its textures
    /// found beside it or in `texture_dirs`. `None` if it can't be drawn.
    pub fn spawn_effect(
        &mut self,
        commands: &mut Commands,
        game: &GameData,
        assets: &mut ModelAssets,
        path: &str,
        texture_dirs: &[String],
        transform: Transform,
    ) -> Option<Entity> {
        let view = View {
            models: vec![path.to_string()],
            texture_dirs: texture_dirs.to_vec(),
            texture_renames: HashMap::new(),
            scale: 1.0,
            animations: Vec::new(),
            conform: None,
            footing: Footing::Ground,
            unpacking: None,
            packing: None,
            icon: None,
        };
        let model = self.own_animation(game, assets, path, view)?;
        let root = commands
            .spawn((
                Name::new(path.to_string()),
                transform,
                Visibility::default(),
            ))
            .id();
        self.spawn_meshes(commands, assets, root, model, None, 0.0);
        Some(root)
    }

    /// A model that plays the animation its own nodes carry, as a rig if
    /// they're animated, its textures found as `view` finds them.
    fn own_animation(
        &mut self,
        game: &GameData,
        assets: &mut ModelAssets,
        path: &str,
        view: View,
    ) -> Option<Model> {
        let key = format!("{path}#own{}", lookup_key(path, &view));
        if let Some(model) = self.models.get(&key) {
            return model.clone();
        }
        let rig = Nif::parse(&game.read(path)?)
            .map_err(|e| warn!("{path}: {e}"))
            .ok()
            .and_then(|nif| {
                let nodes = Nodes::new(&nif);
                let meshes = self.rig_meshes(game, assets, &nif, path, &view, &nodes);
                let rig = nodes.rig(
                    &nif,
                    meshes,
                    None,
                    None,
                    &[],
                    &[],
                    assets.meshes,
                    assets.skins,
                )?;
                Some((rig, self.particles(game, assets, &nif, path, &view)))
            });
        let model = match rig {
            Some((rig, particles)) => Some(Model {
                meshes: Meshes::Rig(Arc::new(rig)),
                particles,
                selection: None,
            }),
            None => self.load(game, assets, path, &view, None, None),
        };
        self.models.insert(key, model.clone());
        model
    }

    /// Spawns a model's meshes and particle systems under `root`, painted
    /// in `colour` where they take their owner's, animations `offset`
    /// seconds in.
    fn spawn_meshes(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        root: Entity,
        model: Model,
        colour: Option<[f32; 3]>,
        offset: f32,
    ) {
        let parts = match model.meshes {
            Meshes::Parts(parts) => {
                particles::spawn(
                    commands,
                    assets.meshes,
                    &model.particles,
                    root,
                    |_| None,
                    offset,
                );
                parts
            }
            Meshes::Rig(rig) => {
                let materials = rig
                    .parts()
                    .map(|p| self.own_paint(assets, p, colour))
                    .collect();
                let joints = rig.spawn(commands, root, offset, materials);
                let node = |i| rig.joint(i).map(|j| joints[j]);
                particles::spawn(
                    commands,
                    assets.meshes,
                    &model.particles,
                    root,
                    node,
                    offset,
                );
                return;
            }
        };
        let materials: Vec<_> = parts
            .iter()
            .map(|p| self.own_paint(assets, p, colour))
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
    fn own_paint(
        &mut self,
        assets: &mut ModelAssets,
        part: &Part,
        colour: Option<[f32; 3]>,
    ) -> PartMaterial {
        let material = self.paint(assets, part, colour);
        match part.animation.as_ref().is_some_and(|a| a.paints()) {
            true => material.copy(assets),
            false => material,
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
            .get(h)
            .filter(|m| m.alpha_mode == AlphaMode::Opaque)
        else {
            return part.material.clone();
        };
        let base = base.clone();
        let painted = assets.team_materials.add(TeamMaterial {
            base,
            extension: Team::new(colour),
        });
        self.painted.insert(key, painted.clone());
        PartMaterial::Team(painted)
    }

    fn model(
        &mut self,
        game: &GameData,
        assets: &mut ModelAssets,
        path: &str,
        view: &View,
        moving: Option<&aaa_data::Animation>,
    ) -> Option<Model> {
        let idle = idle(view);
        let mut key = path.to_string();
        let once = view.animations.iter().filter(|a| once(a));
        for a in idle.into_iter().chain(moving).chain(once) {
            key += &format!("#{}", a.kf);
        }
        key += &lookup_key(path, view);
        if let Some(model) = self.models.get(&key) {
            return model.clone();
        }
        let model = self.load(game, assets, path, view, idle, moving);
        self.models.insert(key, model.clone());
        model
    }

    fn load(
        &mut self,
        game: &GameData,
        assets: &mut ModelAssets,
        path: &str,
        view: &View,
        idle: Option<&aaa_data::Animation>,
        moving: Option<&aaa_data::Animation>,
    ) -> Option<Model> {
        let nif = Nif::parse(&game.read(path)?)
            .map_err(|e| warn!("{path}: {e}"))
            .ok()?;
        if let Some(kind) = &nif.incomplete {
            debug!("{path}: stops at {kind}");
        }
        let selection = selection_box(&nif);
        let particles = self.particles(game, assets, &nif, path, view);
        if let Some(rig) = idle.and_then(|a| self.rig(game, assets, &nif, path, view, a, moving)) {
            return Some(Model {
                meshes: Meshes::Rig(Arc::new(rig)),
                particles,
                selection,
            });
        }
        // Skinned shapes are drawn in the pose they were modelled in.
        let parts = self.scene_parts(game, assets, &nif, path, view, nif.shapes(), true);
        (!parts.is_empty() || !particles.is_empty()).then_some(Model {
            meshes: Meshes::Parts(parts),
            particles,
            selection,
        })
    }

    /// A model as a rig of its nodes idling with `idle`, if its `.kf`
    /// file reads and animates any of them, and moving with `moving`.
    #[allow(clippy::too_many_arguments)]
    fn rig(
        &mut self,
        game: &GameData,
        assets: &mut ModelAssets,
        nif: &Nif,
        path: &str,
        view: &View,
        idle: &aaa_data::Animation,
        moving: Option<&aaa_data::Animation>,
    ) -> Option<Rig> {
        let kf = |a: &aaa_data::Animation| {
            Nif::parse(&game.read(&a.kf)?)
                .map_err(|e| warn!("{}: {e}", a.kf))
                .ok()
        };
        let idle_kf = kf(idle)?;
        let moving = moving.and_then(|a| Some((kf(a)?, a.walk_distance?)));
        let mut abilities = Vec::new();
        let mut deaths = Vec::new();
        for a in view.animations.iter().filter(|a| once(a)) {
            let Some(clip) = kf(a) else {
                continue;
            };
            match a.kind.as_str() {
                "DIE" => deaths.push((clip, a.length)),
                _ => abilities.push((a.ids.clone(), clip, a.length)),
            }
        }
        let nodes = Nodes::new(nif);
        let meshes = self.rig_meshes(game, assets, nif, path, view, &nodes);
        nodes.rig(
            nif,
            meshes,
            Some((&idle_kf, idle.length)),
            moving.as_ref().map(|(kf, distance)| (kf, *distance)),
            &abilities,
            &deaths,
            assets.meshes,
            assets.skins,
        )
    }

    /// The parts of a rig's shapes, each with its index in
    /// [`Nodes::shapes`].
    fn rig_meshes(
        &mut self,
        game: &GameData,
        assets: &mut ModelAssets,
        nif: &Nif,
        path: &str,
        view: &View,
        nodes: &Nodes,
    ) -> Vec<(Part, usize)> {
        let mut meshes = Vec::new();
        for (i, shape) in nodes.shapes.iter().enumerate() {
            let Some(s) = nif.get_as::<nif::Shape>(Some(shape.shape)) else {
                continue;
            };
            let Some(g) = nif.get_as::<nif::Geometry>(s.data) else {
                continue;
            };
            // Skins are bound in their own space, other meshes hang from
            // their node.
            let transform = match s.skin {
                Some(_) => nif::Transform::IDENTITY,
                None => s.av.transform,
            };
            let props = &shape.properties;
            if let Some(mut part) = self.part(game, assets, nif, path, view, g, props, &transform) {
                part.team = team::marked(nif, &s.av);
                part.animation = painting(nif, props).map(Arc::new);
                meshes.push((part, i));
            }
        }
        meshes
    }

    /// A mesh and its material, with `props` the properties in effect for
    /// it, nearest first.
    #[allow(clippy::too_many_arguments)]
    fn part(
        &mut self,
        game: &GameData,
        assets: &mut ModelAssets,
        nif: &Nif,
        path: &str,
        view: &View,
        g: &nif::Geometry,
        props: &[nif::Ref],
        transform: &nif::Transform,
    ) -> Option<Part> {
        let base = nif
            .property::<nif::Texturing>(props)
            .and_then(|t| t.base.as_ref());
        let material = nif.property::<nif::Material>(props);
        let uv_set = base.map_or(0, |b| b.uv_set as usize);
        // Vertex colours tint a mesh only where they stand for its ambient
        // and diffuse colours, as they do without a property to say.
        let tinted = nif
            .property::<nif::VertexColor>(props)
            .is_none_or(|v| v.vertex_mode == 2);
        let mut mesh = mesh(g, transform, uv_set, tinted)?;
        if self.pickable {
            mesh.asset_usage = RenderAssetUsages::default();
        }
        let texture = self.base_texture(game, assets, nif, path, view, props);
        let alpha = nif.property::<nif::Alpha>(props);
        let double_sided = nif
            .property::<nif::Stencil>(props)
            .is_some_and(|s| s.double_sided());
        let [r, g, b] = material.map_or([1.0; 3], |m| m.diffuse);
        let opacity = material.map_or(1.0, |m| m.alpha);
        let alpha_mode = alpha_mode(alpha, opacity);
        let standard = StandardMaterial {
            base_color: Color::srgba(r, g, b, opacity),
            base_color_texture: texture,
            uv_transform: base
                .and_then(|b| b.placement)
                .map_or(Affine2::IDENTITY, |p| uv_transform(&p)),
            alpha_mode,
            double_sided,
            cull_mode: (!double_sided).then_some(Face::Back),
            perceptual_roughness: 0.9,
            reflectance: 0.2,
            ..default()
        };
        // A shape with nothing to say of depth tests and writes it.
        let writes_depth = nif
            .property::<nif::ZBuffer>(props)
            .is_none_or(|z| z.write());
        let source = alpha.filter(|_| alpha_mode == AlphaMode::Add);
        let material = match (source.and_then(|a| Source::of(a.source())), material) {
            (Some(source), Some(m)) => {
                PartMaterial::Glow(assets.glow_materials.add(GlowMaterial {
                    // Without highlights, as the game draws them.
                    base: StandardMaterial {
                        reflectance: 0.0,
                        perceptual_roughness: 1.0,
                        ..standard
                    },
                    extension: Glow::new(m.ambient, m.diffuse, source),
                }))
            }
            _ if alpha_mode == AlphaMode::Blend && writes_depth => {
                PartMaterial::Solid(assets.solid_materials.add(SolidMaterial {
                    base: standard,
                    extension: Solid::default(),
                }))
            }
            _ => PartMaterial::Standard(assets.materials.add(standard)),
        };
        Some(Part {
            mesh: assets.meshes.add(mesh),
            material,
            animation: None,
            blended: matches!(alpha_mode, AlphaMode::Blend | AlphaMode::Add),
            team: false,
        })
    }

    /// A model's particle systems that can be drawn.
    fn particles(
        &mut self,
        game: &GameData,
        assets: &mut ModelAssets,
        nif: &Nif,
        path: &str,
        view: &View,
    ) -> Vec<Arc<System>> {
        let placed = nif.particle_systems();
        let old = nif.old_particle_systems();
        if placed.is_empty() && old.is_empty() {
            return Vec::new();
        }
        let frames = Nodes::new(nif).frames;
        let mut out = Vec::new();
        for (_, p) in placed {
            let Some(&at) = p.path.last() else { continue };
            let look = self.particle_look(game, assets, nif, path, view, &p.properties);
            if let Some(system) = System::read(nif, &frames, at, look) {
                out.push(Arc::new(system));
            }
        }
        for (particles, p) in old {
            let look = self.particle_look(game, assets, nif, path, view, &p.properties);
            let copies = self.mesh_copies(game, assets, nif, path, view, particles);
            if let Some(system) = System::read_old(nif, &frames, &p, look, copies) {
                out.push(Arc::new(system));
            }
        }
        out
    }

    /// The meshes an older-format mesh particle system's particles show,
    /// one set for each in its modifier's list; empty for other systems.
    fn mesh_copies(
        &mut self,
        game: &GameData,
        assets: &mut ModelAssets,
        nif: &Nif,
        path: &str,
        view: &View,
        particles: &nif::OldParticles,
    ) -> Vec<Arc<Vec<Part>>> {
        let meshes = nif
            .get_as::<nif::OldParticleData>(particles.shape.data)
            .is_some_and(|d| matches!(d.kind, nif::OldParticleKind::Meshes { .. }));
        let Some(c) = particles::old_controller(nif, particles).filter(|_| meshes) else {
            return Vec::new();
        };
        let list: Vec<nif::Ref> = particles::old_modifiers(nif, c)
            .flat_map(|m| match &m.kind {
                nif::OldModifierKind::Meshes { meshes } => meshes.clone(),
                _ => Vec::new(),
            })
            .collect();
        list.into_iter()
            .map(|r| {
                let shapes = nif.shapes_below(r);
                Arc::new(self.scene_parts(game, assets, nif, path, view, shapes, false))
            })
            .collect()
    }

    /// The parts of `shapes`, each animated by its own controllers and
    /// those of the objects above it, with the first object on each path
    /// taken as having no transform of its own unless `root_placed`.
    #[allow(clippy::too_many_arguments)]
    fn scene_parts(
        &mut self,
        game: &GameData,
        assets: &mut ModelAssets,
        nif: &Nif,
        path: &str,
        view: &View,
        shapes: Vec<nif::Placed>,
        root_placed: bool,
    ) -> Vec<Part> {
        let mut parts = Vec::new();
        for placed in shapes {
            let Some(g) = nif.get_as::<nif::Geometry>(placed.shape.data) else {
                continue;
            };
            let props = &placed.properties;
            let painting = painting(nif, props);
            // An animated mesh is built relative to the object that moves.
            let (mesh_transform, animation) =
                match (motion(nif, &placed.path, root_placed), painting) {
                    (Some((motion, below)), painting) => {
                        let (fade, texture) =
                            painting.map_or((None, None), |p| (p.fade, p.texture));
                        let animation = Animation {
                            motion,
                            fade,
                            texture,
                        };
                        (below, Some(animation))
                    }
                    (None, painting) => (placed.transform, painting),
                };
            if let Some(mut part) =
                self.part(game, assets, nif, path, view, g, props, &mesh_transform)
            {
                part.animation = animation.map(Arc::new);
                part.team = team::marked(nif, &placed.shape.av);
                parts.push(part);
            }
        }
        parts
    }

    /// How a particle system's squares are drawn: its texture whole, lit
    /// as its vertex colour property says and blended as its alpha
    /// property says.
    fn particle_look(
        &mut self,
        game: &GameData,
        assets: &mut ModelAssets,
        nif: &Nif,
        path: &str,
        view: &View,
        props: &[nif::Ref],
    ) -> Look {
        let texturing = nif.property::<nif::Texturing>(props);
        let texture = self.base_texture(game, assets, nif, path, view, props);
        let m = nif.property::<nif::Material>(props);
        let opacity = m.map_or(1.0, |m| m.alpha);
        // Squares that also test alpha still blend: cut out instead, soft
        // smoke turns to specks.
        let alpha = nif.property::<nif::Alpha>(props);
        let alpha_mode = match alpha_mode(alpha, opacity) {
            AlphaMode::Mask(_) if alpha.is_some_and(|a| a.blend()) => AlphaMode::Blend,
            mode => mode,
        };
        let rgb = |c: Option<[f32; 3]>, a: f32| {
            let [r, g, b] = c.unwrap_or([1.0; 3]);
            Color::srgba(r, g, b, a)
        };
        let emissive = m.map(|m| m.emissive);
        // Without a property, vertex colours stand for ambient and diffuse,
        // lit.
        let (vertex, lit) = nif
            .property::<nif::VertexColor>(props)
            .map_or((2, true), |v| (v.vertex_mode, v.lighting == 1));
        let replace = texture.is_some() && texturing.is_some_and(|t| t.apply_mode == 0);
        let (base_color, emissive, lit, tinted) = match (replace, vertex, lit) {
            // The texture alone.
            (true, _, _) => (Color::WHITE, None, false, false),
            (false, 1, false) => (Color::WHITE, None, false, true),
            // Lit by no light, so only the emissive colour shows.
            (false, _, false) => (rgb(emissive, opacity), None, false, false),
            // The particle's colour is its ambient and diffuse, and alpha.
            (false, 2, true) => (Color::WHITE, emissive, true, true),
            (false, _, true) => (rgb(m.map(|m| m.diffuse), opacity), emissive, true, false),
        };
        // Bevy scales emissive light by the exposure, which the lights undo.
        let unit = 1.0 / bevy::camera::Exposure::default().exposure();
        let emissive = emissive.map_or(LinearRgba::BLACK, |[r, g, b]| {
            LinearRgba::from(Color::srgb(r, g, b)) * unit
        });
        let material = assets.materials.add(StandardMaterial {
            base_color,
            base_color_texture: texture,
            emissive,
            alpha_mode,
            unlit: !lit,
            double_sided: true,
            cull_mode: None,
            // Without highlights, as the game draws them.
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            ..default()
        });
        Look { material, tinted }
    }

    /// The base texture `props` give a shape, from a file or embedded.
    fn base_texture(
        &mut self,
        game: &GameData,
        assets: &mut ModelAssets,
        nif: &Nif,
        path: &str,
        view: &View,
        props: &[nif::Ref],
    ) -> Option<Handle<Image>> {
        let base = nif.property::<nif::Texturing>(props)?.base.as_ref()?;
        let source = nif.get_as::<nif::SourceTexture>(base.source)?;
        match (&source.file, nif.get_as(source.pixels)) {
            (Some(file), _) => self.texture(game, assets, file, view, path),
            (None, Some(p)) => {
                let key = format!("{path}#{}", source.pixels.unwrap_or_default());
                self.embedded(assets, key, p)
            }
            _ => None,
        }
    }

    fn texture(
        &mut self,
        game: &GameData,
        assets: &mut ModelAssets,
        file: &str,
        view: &View,
        model: &str,
    ) -> Option<Handle<Image>> {
        let Some(path) = game.find_texture(view, model, file) else {
            warn!("{model}: texture {file} not found");
            return None;
        };
        let formats = self.formats;
        cached(&mut self.textures, &path, || {
            let image = load_image(&game.read(&path)?, &path, formats)?;
            Some(assets.images.add(image))
        })
    }

    fn embedded(
        &mut self,
        assets: &mut ModelAssets,
        key: String,
        pixels: &nif::Pixels,
    ) -> Option<Handle<Image>> {
        let formats = self.formats;
        cached(&mut self.textures, &key, || {
            let Some(image) = embedded_image(pixels, formats) else {
                warn!(
                    "{key}: unsupported embedded texture format {}",
                    pixels.format
                );
                return None;
            };
            Some(assets.images.add(image))
        })
    }
}

/// What tells the model at `path` as loaded for `view` from the same one
/// loaded for a view that finds its textures elsewhere.
fn lookup_key(path: &str, view: &View) -> String {
    let renames = view.texture_renames.get(path);
    format!("#{:?}#{renames:?}", view.texture_dirs)
}

/// How a shape with `alpha` and its material's `opacity` blends.
fn alpha_mode(alpha: Option<&nif::Alpha>, opacity: f32) -> AlphaMode {
    match alpha {
        Some(a) if a.test() => AlphaMode::Mask(a.threshold as f32 / 255.0),
        Some(a) if a.blend() && a.adds() => AlphaMode::Add,
        Some(a) if a.blend() => AlphaMode::Blend,
        _ if opacity < 1.0 => AlphaMode::Blend,
        _ => AlphaMode::Opaque,
    }
}

/// The size of the hidden box a model marks its selectable extent with:
/// width across its -y facing and length along it.
fn selection_box(nif: &Nif) -> Option<Vec2> {
    let (at, shape) = nif.objects.iter().enumerate().find_map(|(i, o)| match o {
        Object::Shape(s) if s.av.name.eq_ignore_ascii_case("SelectionBox") => Some((i, s)),
        _ => None,
    })?;
    let g = nif.get_as::<nif::Geometry>(shape.data)?;
    // Where the nodes above it put it, which may turn it.
    let frames = Nodes::new(nif).frames;
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
pub(crate) fn cached<T: Clone>(
    cache: &mut HashMap<String, Option<T>>,
    key: &str,
    load: impl FnOnce() -> Option<T>,
) -> Option<T> {
    if let Some(v) = cache.get(key) {
        return v.clone();
    }
    let v = load();
    cache.insert(key.to_string(), v.clone());
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
    // Flag bit 3 marks the controller active.
    .filter(|c| c.flags & 8 != 0)
}

/// The keyframes moving the shape at the end of `path`, on it or any
/// object above it, from the root down, and the transform from the last of
/// them down to the shape; `None` if nothing on the path is animated. The
/// first object's own transform is left out unless `root_placed`.
fn motion(nif: &Nif, path: &[usize], root_placed: bool) -> Option<(Vec<Moved>, nif::Transform)> {
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
    pub materials: &'a mut Assets<StandardMaterial>,
    pub glow_materials: &'a mut Assets<GlowMaterial>,
    pub team_materials: &'a mut Assets<TeamMaterial>,
    pub solid_materials: &'a mut Assets<SolidMaterial>,
    pub images: &'a mut Assets<Image>,
    pub skins: &'a mut Assets<SkinnedMeshInverseBindposes>,
}

/// The moving animation that best suits `speed`: the one whose stride
/// takes as long, played at its natural pace.
fn moving(view: &View, speed: f32) -> Option<&aaa_data::Animation> {
    view.animations
        .iter()
        .filter(|a| a.kind == "MOVE")
        .filter_map(|a| Some((a, a.walk_distance? / a.length - speed)))
        .min_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
        .map(|(a, _)| a)
}

/// Whether an animation is one units play once when told to: using an
/// ability or dying.
fn once(a: &aaa_data::Animation) -> bool {
    a.kind == "ABILITY" || a.kind == "DIE"
}

/// The idle animation a view's models play: the one named `idle`, else the
/// first of its kind.
fn idle(view: &View) -> Option<&aaa_data::Animation> {
    let mut idles = view.animations.iter().filter(|a| a.kind == "IDLE");
    idles
        .clone()
        .find(|a| a.ids.eq_ignore_ascii_case("idle"))
        .or_else(|| idles.next())
}

/// A mesh in model space, converted from the game's z-up axes to Bevy's,
/// with its vertex colours if it's `tinted` by them.
fn mesh(
    g: &nif::Geometry,
    transform: &nif::Transform,
    uv_set: usize,
    tinted: bool,
) -> Option<Mesh> {
    if g.positions.is_empty() || g.triangles.is_empty() {
        return None;
    }
    let to_bevy = |[x, y, z]: [f32; 3]| world(x, y, z).to_array();
    let positions: Vec<[f32; 3]> = g
        .positions
        .iter()
        .map(|&p| to_bevy(transform.apply(p)))
        .collect();
    let indices: Vec<u32> = g
        .triangles
        .iter()
        .filter(|t| t.iter().all(|&i| (i as usize) < positions.len()))
        .flat_map(|t| t.map(u32::from))
        .collect();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_indices(Indices::U32(indices));
    if let Some(uvs) = g.uv_sets.get(uv_set).or(g.uv_sets.first()) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs.clone());
    }
    if tinted && g.colors.len() == g.positions.len() {
        // The model's colours are sRGB, as everywhere else.
        let colours: Vec<[f32; 4]> = g
            .colors
            .iter()
            .map(|&[r, g, b, a]| Color::srgba(r, g, b, a).to_linear().to_f32_array())
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    }
    if g.normals.len() == g.positions.len() {
        let normals: Vec<[f32; 3]> = g
            .normals
            .iter()
            .map(|&n| to_bevy(transform.rotate(n)))
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    } else {
        mesh.compute_smooth_normals();
    }
    Some(mesh)
}

/// A texture stored in a model: uncompressed RGB(A), or DXT blocks with
/// every mipmap level when the GPU can sample them.
pub(crate) fn embedded_image(p: &nif::Pixels, formats: CompressedImageFormats) -> Option<Image> {
    let &[width, height, offset] = p.mipmaps.first()?;
    let size = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let pixels = (width * height) as usize;
    let (format, data, levels) = match p.format {
        0 | 1 => {
            let n = if p.format == 0 { 3 } else { 4 };
            let src = p.data.get(offset as usize..offset as usize + pixels * n)?;
            let rgba = src
                .chunks_exact(n)
                .flat_map(|c| [c[0], c[1], c[2], if n == 4 { c[3] } else { 255 }])
                .collect();
            (TextureFormat::Rgba8UnormSrgb, rgba, 1)
        }
        4..=6 if formats.contains(CompressedImageFormats::BC) => {
            let format = match p.format {
                4 => TextureFormat::Bc1RgbaUnormSrgb,
                5 => TextureFormat::Bc2RgbaUnormSrgb,
                _ => TextureFormat::Bc3RgbaUnormSrgb,
            };
            (format, p.data.clone(), p.mipmaps.len() as u32)
        }
        _ => return None,
    };
    let mut image = Image::new_uninit(
        size,
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = textures::repeating();
    Some(image)
}

fn load_image(bytes: &[u8], name: &str, formats: CompressedImageFormats) -> Option<Image> {
    let mut image = textures::decode(bytes, name, formats, true)?;
    image.sampler = textures::repeating();
    Some(image)
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
        let size = selection_box(&nif).expect("a box");
        assert!(size.abs_diff_eq(Vec2::new(2.0, 4.0), 1e-5), "{size}");
    }

    #[test]
    fn meshes_take_vertex_colours_only_when_tinted() {
        let g = nif::Geometry {
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            colors: vec![[0.5, 0.5, 0.5, 1.0]; 3],
            triangles: vec![[0, 1, 2]],
            ..Default::default()
        };
        let colours = |tinted| {
            mesh(&g, &nif::Transform::IDENTITY, 0, tinted)
                .expect("a mesh")
                .attribute(Mesh::ATTRIBUTE_COLOR)
                .is_some()
        };
        assert!(colours(true));
        assert!(!colours(false));
    }

    #[test]
    fn models_are_kept_apart_by_where_their_textures_are() {
        let view = |dirs: &[&str]| View {
            models: Vec::new(),
            texture_dirs: dirs.iter().map(|d| d.to_string()).collect(),
            texture_renames: HashMap::new(),
            scale: 1.0,
            animations: Vec::new(),
            conform: None,
            footing: Footing::Ground,
            unpacking: None,
            packing: None,
            icon: None,
        };
        let (unit, regiment) = (view(&["Shared/SharedTextures"]), view(&[]));
        assert_ne!(
            lookup_key("Tank.nif", &unit),
            lookup_key("Tank.nif", &regiment)
        );
        assert_eq!(
            lookup_key("Tank.nif", &unit),
            lookup_key("Tank.nif", &unit.clone())
        );
        let mut renamed = unit.clone();
        let renames = vec![("Base".to_string(), "Snow".to_string())];
        renamed.texture_renames.insert("Tank.nif".into(), renames);
        assert_ne!(
            lookup_key("Tank.nif", &unit),
            lookup_key("Tank.nif", &renamed)
        );
        assert_eq!(
            lookup_key("Truck.nif", &unit),
            lookup_key("Truck.nif", &renamed)
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
