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
use crate::rig::{Nodes, Rig};
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
        let translation = world(x, y, ground.stand_at(footing, x, y));
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
    mesh.remove::<(MeshMaterial3d<TeamMaterial>, MeshMaterial3d<GlowMaterial>)>()
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
/// them need their own.
#[derive(Clone)]
pub(crate) enum PartMaterial {
    Standard(Handle<StandardMaterial>),
    Glow(Handle<GlowMaterial>),
    Team(Handle<TeamMaterial>),
}

impl PartMaterial {
    pub fn insert(&self, entity: &mut EntityCommands) {
        match self {
            PartMaterial::Standard(m) => entity.insert(MeshMaterial3d(m.clone())),
            PartMaterial::Glow(m) => entity.insert(MeshMaterial3d(m.clone())),
            PartMaterial::Team(m) => entity.insert(MeshMaterial3d(m.clone())),
        };
    }

    /// A copy of the material of its own, to animate.
    fn copy(&self, assets: &mut ModelAssets) -> Self {
        fn copy<M: Asset + Clone>(store: &mut Assets<M>, h: &Handle<M>) -> Handle<M> {
            store.get(h).cloned().map_or(h.clone(), |m| store.add(m))
        }
        match self {
            PartMaterial::Standard(h) => PartMaterial::Standard(copy(assets.materials, h)),
            PartMaterial::Glow(h) => PartMaterial::Glow(copy(assets.glow_materials, h)),
            PartMaterial::Team(h) => PartMaterial::Team(copy(assets.team_materials, h)),
        }
    }
}

/// A loaded model.
#[derive(Clone)]
struct Model {
    meshes: Meshes,
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
    /// Moves the mesh's nearest animated ancestor, under `above`, its
    /// parent's transform in the model; the mesh is then built relative
    /// to that ancestor.
    motion: Option<(Controller, Keyframes, nif::Transform)>,
    above: nif::Transform,
    /// Fades the material's alpha; each instance gets its own material.
    fade: Option<(Controller, Keys<f32>)>,
    /// Moves, turns or stretches the material's texture, as fades do.
    texture: Option<TextureMotion>,
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
        motion: None,
        above: nif::Transform::IDENTITY,
        fade,
        texture,
    })
}

impl Animation {
    /// Whether instances need their own material to animate.
    fn paints(&self) -> bool {
        self.fade.is_some() || self.texture.is_some()
    }

    fn transform(&self, t: f32) -> Transform {
        match &self.motion {
            Some((c, keys, base)) => {
                bevy_transform(&self.above.then(&keys.apply(base, c.key_time(t))))
            }
            None => Transform::IDENTITY,
        }
    }
}

/// Plays a part's animation, `offset` seconds in so neighbouring objects
/// don't move in step.
#[derive(Component)]
pub struct Animated {
    animation: Arc<Animation>,
    offset: f32,
    material: PartMaterial,
}

impl Animated {
    pub(crate) fn new(animation: Arc<Animation>, offset: f32, material: PartMaterial) -> Self {
        Animated {
            animation,
            offset,
            material,
        }
    }
}

pub fn animate(
    time: Res<Time>,
    mut parts: Query<(&Animated, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut glow_materials: ResMut<Assets<GlowMaterial>>,
    mut team_materials: ResMut<Assets<TeamMaterial>>,
) {
    for (a, mut transform) in &mut parts {
        let t = time.elapsed_secs() + a.offset;
        if a.animation.motion.is_some() {
            *transform = a.animation.transform(t);
        }
        let alpha = a
            .animation
            .fade
            .as_ref()
            .and_then(|(c, keys)| keys.sample(c.key_time(t)));
        let uv = a.animation.texture.as_ref().map(|m| uv_transform(&m.at(t)));
        if alpha.is_some() || uv.is_some() {
            let base = match &a.material {
                PartMaterial::Standard(h) => materials.get_mut(h).map(|m| m.into_inner()),
                PartMaterial::Glow(h) => {
                    glow_materials.get_mut(h).map(|m| &mut m.into_inner().base)
                }
                PartMaterial::Team(h) => {
                    team_materials.get_mut(h).map(|m| &mut m.into_inner().base)
                }
            };
            if let Some(m) = base {
                if let Some(alpha) = alpha {
                    m.base_color.set_alpha(alpha.clamp(0.0, 1.0));
                }
                if let Some(uv) = uv {
                    m.uv_transform = uv;
                }
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

/// Whether `thing` is an organization with one of `buildings` among its
/// members, as an unpacked Division HQ is.
pub fn holds_building(thing: &Thing, buildings: &HashSet<u32>) -> bool {
    thing
        .organization()
        .is_some_and(|o| o.members.iter().any(|m| buildings.contains(&m.id)))
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
}

impl Models {
    pub fn new(game: &mut GameData, formats: CompressedImageFormats, players: &[Player]) -> Self {
        Models {
            views: game.views(),
            models: HashMap::new(),
            textures: HashMap::new(),
            formats,
            colours: players.iter().map(|p| (p.id, p.rgb)).collect(),
            painted: HashMap::new(),
        }
    }

    /// Spawns every object whose template has a readable model and returns
    /// the rest, each with whether it should have had one: false for
    /// templates that name no model, such as sound emitters, and for
    /// organizations drawn as their members, such as herds and those holding
    /// one of `buildings`.
    pub fn spawn<'a>(
        &mut self,
        commands: &mut Commands,
        game: &mut GameData,
        assets: &mut ModelAssets,
        things: impl IntoIterator<Item = &'a Thing>,
        buildings: &HashSet<u32>,
    ) -> Vec<(&'a Thing, bool)> {
        let mut rest = Vec::new();
        let (mut drawn, mut modelless) = (0, 0);
        for t in things {
            let placed = self.place(commands, game, assets, t, None, buildings);
            match placed {
                Placed::Drawn if self.by_members(t, buildings) => rest.push((t, false)),
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
        game: &mut GameData,
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
        game: &mut GameData,
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
        game: &mut GameData,
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
        game: &mut GameData,
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
        self.spawn_meshes(commands, assets, root, model.meshes, colour, into);
        Some(root)
    }

    /// Whether `thing` is an organization drawn as its members: one whose
    /// template names no model, such as a herd, or one holding one of
    /// `buildings`, whose own model is the truck it packs into.
    fn by_members(&self, thing: &Thing, buildings: &HashSet<u32>) -> bool {
        let invisible = matches!(
            self.views.get(&Id::from(thing.template.as_str())),
            Some(None)
        );
        invisible || holds_building(thing, buildings)
    }

    /// Spawns one object, or, for an organization drawn as its members, the
    /// members where the map has them standing. Members belong to their
    /// organization's owner, `within`, unless they name their own.
    fn place(
        &mut self,
        commands: &mut Commands,
        game: &mut GameData,
        assets: &mut ModelAssets,
        thing: &Thing,
        within: Option<u32>,
        buildings: &HashSet<u32>,
    ) -> Placed {
        let owner = owner(thing).or(within);
        if self.by_members(thing, buildings) {
            // One member drawn shows the organization, as an HQ's crates
            // can lack a model its building has.
            let members = thing.organization().map_or(&[][..], |o| &o.members);
            return members
                .iter()
                .map(|m| self.place(commands, game, assets, m, owner, buildings))
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
        game: &mut GameData,
        assets: &mut ModelAssets,
        thing: &Thing,
        owner: Option<u32>,
        view: &View,
        moving: Option<&aaa_data::Animation>,
    ) -> Option<Entity> {
        if view.models.is_empty() {
            return None;
        }
        // Things with no variation set pick one by id, so neighbours differ.
        let variation = usize::try_from(thing.variation).unwrap_or(thing.id as usize);
        let model = &view.models[variation % view.models.len()];
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
        self.spawn_meshes(commands, assets, root, model.meshes, colour, offset);
        Some(root)
    }

    /// Spawns an effect's model, such as the flag marking where an order
    /// was given, playing the animation its own nodes carry. `None` if it
    /// can't be drawn.
    pub fn spawn_effect(
        &mut self,
        commands: &mut Commands,
        game: &mut GameData,
        assets: &mut ModelAssets,
        path: &str,
        transform: Transform,
    ) -> Option<Entity> {
        let view = View {
            models: vec![path.to_string()],
            texture_dirs: path
                .rsplit_once('/')
                .map(|(d, _)| d.to_string())
                .into_iter()
                .collect(),
            texture_renames: Vec::new(),
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
        self.spawn_meshes(commands, assets, root, model.meshes, None, 0.0);
        Some(root)
    }

    /// A model that plays the animation its own nodes carry, as a rig if
    /// they're animated, its textures found as `view` finds them.
    fn own_animation(
        &mut self,
        game: &mut GameData,
        assets: &mut ModelAssets,
        path: &str,
        view: View,
    ) -> Option<Model> {
        let key = format!("{path}#own");
        if let Some(model) = self.models.get(&key) {
            return model.clone();
        }
        let rig = Nif::parse(&game.read(path)?)
            .map_err(|e| warn!("{path}: {e}"))
            .ok()
            .and_then(|nif| {
                let nodes = Nodes::new(&nif);
                let meshes = self.rig_meshes(game, assets, &nif, path, &view, &nodes);
                nodes.rig(
                    &nif,
                    meshes,
                    None,
                    None,
                    &[],
                    &[],
                    assets.meshes,
                    assets.skins,
                )
            });
        let model = match rig {
            Some(rig) => Some(Model {
                meshes: Meshes::Rig(Arc::new(rig)),
                selection: None,
            }),
            None => self.load(game, assets, path, &view, None, None),
        };
        self.models.insert(key, model.clone());
        model
    }

    /// Spawns a model's meshes under `root`, painted in `colour` where
    /// they take their owner's, animations `offset` seconds in.
    fn spawn_meshes(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        root: Entity,
        meshes: Meshes,
        colour: Option<[f32; 3]>,
        offset: f32,
    ) {
        let parts = match meshes {
            Meshes::Parts(parts) => parts,
            Meshes::Rig(rig) => {
                let materials = rig
                    .parts()
                    .map(|p| self.own_paint(assets, p, colour))
                    .collect();
                rig.spawn(commands, root, offset, materials);
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
        game: &mut GameData,
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
        if let Some(model) = self.models.get(&key) {
            return model.clone();
        }
        let model = self.load(game, assets, path, view, idle, moving);
        self.models.insert(key, model.clone());
        model
    }

    fn load(
        &mut self,
        game: &mut GameData,
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
        if let Some(rig) = idle.and_then(|a| self.rig(game, assets, &nif, path, view, a, moving)) {
            return Some(Model {
                meshes: Meshes::Rig(Arc::new(rig)),
                selection,
            });
        }
        let mut parts = Vec::new();
        // Skinned shapes are drawn in the pose they were modelled in.
        for placed in nif.shapes() {
            let Some(g) = nif.get_as::<nif::Geometry>(placed.shape.data) else {
                continue;
            };
            let props = &placed.properties;
            let painting = painting(&nif, props);
            // An animated mesh is built relative to the object that moves.
            let (mesh_transform, animation) = match (motion(&nif, &placed.path), painting) {
                (Some(m), painting) => {
                    let (fade, texture) = painting.map_or((None, None), |p| (p.fade, p.texture));
                    let animation = Animation {
                        motion: Some((m.controller, m.keys, m.base)),
                        above: m.above,
                        fade,
                        texture,
                    };
                    (m.below, Some(animation))
                }
                (None, painting) => (placed.transform, painting),
            };
            if let Some(mut part) =
                self.part(game, assets, &nif, path, view, g, props, &mesh_transform)
            {
                part.animation = animation.map(Arc::new);
                part.team = team::marked(&nif, &placed.shape.av);
                parts.push(part);
            }
        }
        (!parts.is_empty()).then_some(Model {
            meshes: Meshes::Parts(parts),
            selection,
        })
    }

    /// A model as a rig of its nodes idling with `idle`, if its `.kf`
    /// file reads and animates any of them, and moving with `moving`.
    #[allow(clippy::too_many_arguments)]
    fn rig(
        &mut self,
        game: &mut GameData,
        assets: &mut ModelAssets,
        nif: &Nif,
        path: &str,
        view: &View,
        idle: &aaa_data::Animation,
        moving: Option<&aaa_data::Animation>,
    ) -> Option<Rig> {
        let mut kf = |a: &aaa_data::Animation| {
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
        game: &mut GameData,
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
        game: &mut GameData,
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
        let mesh = mesh(g, transform, uv_set)?;
        let source = base.and_then(|b| nif.get_as::<nif::SourceTexture>(b.source));
        let texture = source.and_then(|t| match (&t.file, nif.get_as(t.pixels)) {
            (Some(file), _) => self.texture(game, assets, file, view, path),
            (None, Some(p)) => {
                let key = format!("{path}#{}", t.pixels.unwrap_or_default());
                self.embedded(assets, key, p)
            }
            _ => None,
        });
        let alpha = nif.property::<nif::Alpha>(props);
        let double_sided = nif
            .property::<nif::Stencil>(props)
            .is_some_and(|s| s.double_sided());
        let [r, g, b] = material.map_or([1.0; 3], |m| m.diffuse);
        let opacity = material.map_or(1.0, |m| m.alpha);
        let alpha_mode = match alpha {
            Some(a) if a.test() => AlphaMode::Mask(a.threshold as f32 / 255.0),
            Some(a) if a.blend() && a.adds() => AlphaMode::Add,
            Some(a) if a.blend() => AlphaMode::Blend,
            _ if opacity < 1.0 => AlphaMode::Blend,
            _ => AlphaMode::Opaque,
        };
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

    fn texture(
        &mut self,
        game: &mut GameData,
        assets: &mut ModelAssets,
        file: &str,
        view: &View,
        model: &str,
    ) -> Option<Handle<Image>> {
        let mut file = file.to_string();
        for (from, to) in &view.texture_renames {
            file = file.replace(from.as_str(), to);
        }
        let model_dir = model.rsplit_once('/').map_or("", |(d, _)| d).to_string();
        let dirs: Vec<String> = view
            .texture_dirs
            .iter()
            .cloned()
            .chain([model_dir])
            .collect();
        let Some(path) = game.find_texture(&file, &dirs, model) else {
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

/// The size of the hidden box a model marks its selectable extent with:
/// width across its -y facing and length along it.
fn selection_box(nif: &Nif) -> Option<Vec2> {
    let shape = nif.objects.iter().find_map(|o| match o {
        Object::Shape(s) if s.av.name.eq_ignore_ascii_case("SelectionBox") => Some(s),
        _ => None,
    })?;
    let g = nif.get_as::<nif::Geometry>(shape.data)?;
    let (mut min, mut max) = (Vec2::MAX, Vec2::MIN);
    for &p in &g.positions {
        let [x, y, _] = shape.av.transform.apply(p);
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
fn controllers(nif: &Nif, first: nif::Ref) -> impl Iterator<Item = &Controller> {
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

/// The keyframes on the nearest animated object along a shape's path.
struct Motion {
    controller: Controller,
    keys: Keyframes,
    /// Transforms above the animated object, its own, and below it down to
    /// the shape.
    above: nif::Transform,
    base: nif::Transform,
    below: nif::Transform,
}

fn motion(nif: &Nif, path: &[usize]) -> Option<Motion> {
    let scene = |i: usize| nif.get(Some(i)).and_then(Object::scene);
    let (at, controller, keys) = path.iter().enumerate().rev().find_map(|(at, &i)| {
        let c = controller(nif, scene(i)?.controller, ControllerKind::Keyframe)?;
        Some((at, c, nif.get_as::<Keyframes>(c.data)?))
    })?;
    let chain = |range: &[usize]| {
        range
            .iter()
            .filter_map(|&i| scene(i))
            .fold(nif::Transform::IDENTITY, |t, av| t.then(&av.transform))
    };
    Some(Motion {
        controller: controller.clone(),
        keys: keys.clone(),
        above: chain(&path[..at]),
        base: scene(path[at])?.transform,
        below: chain(&path[at + 1..]),
    })
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

/// A mesh in model space, converted from the game's z-up axes to Bevy's.
fn mesh(g: &nif::Geometry, transform: &nif::Transform, uv_set: usize) -> Option<Mesh> {
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
    if g.colors.len() == g.positions.len() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, g.colors.clone());
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
