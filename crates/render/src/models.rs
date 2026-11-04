//! Placed objects drawn with the game's models, where their templates name
//! models this crate can read.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use aaa_data::{GameData, Id, View};
use bevy::asset::RenderAssetUsages;
use bevy::image::CompressedImageFormats;
use bevy::light::NotShadowCaster;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat};
use nif::{Controller, ControllerKind, Keyframes, Keys, Nif, Object};
use tgck::map::Thing;

use crate::rig::{Nodes, Rig};
use crate::terrain::world;
use crate::textures;

/// One mesh of a model, in Bevy axes, ready to instance.
#[derive(Clone)]
pub(crate) struct Part {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    animation: Option<Arc<Animation>>,
    /// Bevy's shadows ignore alpha blending, so a faded or see-through mesh
    /// would cast a solid one.
    pub blended: bool,
}

/// A loaded model: meshes placed in the model, or a rig of its nodes for
/// one that plays a `.kf` animation.
#[derive(Clone)]
enum Model {
    Parts(Vec<Part>),
    Rig(Arc<Rig>),
}

/// What a model's controllers do to one of its meshes.
pub struct Animation {
    /// Moves the mesh's nearest animated ancestor, under `above`, its
    /// parent's transform in the model; the mesh is then built relative
    /// to that ancestor.
    motion: Option<(Controller, Keyframes, nif::Transform)>,
    above: nif::Transform,
    /// Fades the material's alpha; each instance gets its own material.
    fade: Option<(Controller, Keys<f32>)>,
}

impl Animation {
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
    material: Handle<StandardMaterial>,
}

pub fn animate(
    time: Res<Time>,
    mut parts: Query<(&Animated, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (a, mut transform) in &mut parts {
        let t = time.elapsed_secs() + a.offset;
        if a.animation.motion.is_some() {
            *transform = a.animation.transform(t);
        }
        if let Some((c, keys)) = &a.animation.fade
            && let Some(alpha) = keys.sample(c.key_time(t))
            && let Some(mut m) = materials.get_mut(&a.material)
        {
            m.base_color.set_alpha(alpha.clamp(0.0, 1.0));
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
}

impl Models {
    pub fn new(game: &mut GameData, formats: CompressedImageFormats) -> Self {
        Models {
            views: game.views(),
            models: HashMap::new(),
            textures: HashMap::new(),
            formats,
        }
    }

    /// Spawns every object whose template has a readable model and returns
    /// the rest, each with whether it should have had one: false for
    /// templates that name no model, such as sound emitters and herds,
    /// whose members are drawn in their place.
    pub fn spawn<'a>(
        &mut self,
        commands: &mut Commands,
        game: &mut GameData,
        assets: &mut ModelAssets,
        things: impl IntoIterator<Item = &'a Thing>,
        world_version: u32,
    ) -> Vec<(&'a Thing, bool)> {
        let mut rest = Vec::new();
        let (mut drawn, mut modelless) = (0, 0);
        for t in things {
            let placed = self.place(commands, game, assets, t, world_version);
            let invisible = matches!(self.views.get(&Id::from(t.template.as_str())), Some(None));
            match placed {
                Placed::Drawn if invisible => rest.push((t, false)),
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

    /// Spawns a unit that moves at `speed`, its model playing the moving
    /// animation that suits that speed while [`crate::Playing::set_moving`] says
    /// it's moving. `None` if its model can't be drawn.
    pub fn spawn_unit(
        &mut self,
        commands: &mut Commands,
        game: &mut GameData,
        assets: &mut ModelAssets,
        thing: &Thing,
        speed: f32,
    ) -> Option<Entity> {
        let view = self
            .views
            .get(&Id::from(thing.template.as_str()))?
            .clone()?;
        let moving = moving(&view, speed);
        self.spawn_model(commands, game, assets, thing, &view, moving)
    }

    /// Spawns one object, or, for an invisible organization such as a herd,
    /// its members where the map has them standing.
    fn place(
        &mut self,
        commands: &mut Commands,
        game: &mut GameData,
        assets: &mut ModelAssets,
        thing: &Thing,
        world_version: u32,
    ) -> Placed {
        let view = match self.views.get(&Id::from(thing.template.as_str())) {
            Some(Some(view)) if !view.models.is_empty() => view.clone(),
            Some(None) => {
                let members = thing
                    .organization(world_version)
                    .map_or(Vec::new(), |o| o.members);
                return members
                    .iter()
                    .map(|m| self.place(commands, game, assets, m, world_version))
                    .max()
                    .unwrap_or(Placed::NoModel);
            }
            _ => return Placed::Missing,
        };
        match self.spawn_model(commands, game, assets, thing, &view, None) {
            Some(_) => Placed::Drawn,
            None => Placed::Missing,
        }
    }

    /// Spawns an object with one of its view's models, returning its root.
    fn spawn_model(
        &mut self,
        commands: &mut Commands,
        game: &mut GameData,
        assets: &mut ModelAssets,
        thing: &Thing,
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
        // Maps store each object's height as the game places it: on the
        // ground, the seabed or the waterline.
        let [x, y, z] = thing.position;
        // Models face their -y axis, which turns to the object's facing:
        // roads run along it and waves roll back against it to shore.
        let [fx, fy, _] = thing.facing;
        let transform = Transform::from_translation(world(x, y, z))
            .with_rotation(Quat::from_rotation_y(fy.atan2(fx) + FRAC_PI_2))
            .with_scale(Vec3::splat(view.scale * thing.scale));
        let mut root = commands.spawn((
            Name::new(thing.template.clone()),
            transform,
            Visibility::default(),
        ));
        // Spread over a minute by the golden ratio, so any run of ids lands
        // well apart.
        let offset = (thing.id as f32 * 0.618_034).fract() * 60.0;
        let parts = match model {
            Model::Parts(parts) => parts,
            Model::Rig(rig) => {
                let root = root.id();
                rig.spawn(commands, root, offset);
                return Some(root);
            }
        };
        root.with_children(|parent| {
            for part in parts {
                let mut child = parent.spawn(Mesh3d(part.mesh));
                if part.blended {
                    child.insert(NotShadowCaster);
                }
                let Some(animation) = part.animation else {
                    child.insert(MeshMaterial3d(part.material));
                    continue;
                };
                let material = match animation.fade {
                    Some(_) => assets
                        .materials
                        .get(&part.material)
                        .cloned()
                        .map_or(part.material, |m| assets.materials.add(m)),
                    None => part.material,
                };
                child.insert((
                    MeshMaterial3d(material.clone()),
                    animation.transform(offset),
                    Animated {
                        animation,
                        offset,
                        material,
                    },
                ));
            }
        });
        Some(root.id())
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
        for a in idle.iter().chain(&moving) {
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
        if let Some(rig) = idle.and_then(|a| self.rig(game, assets, &nif, path, view, a, moving)) {
            return Some(Model::Rig(Arc::new(rig)));
        }
        let mut parts = Vec::new();
        // Skinned shapes are drawn in the pose they were modelled in.
        for placed in nif.shapes() {
            let Some(g) = nif.get_as::<nif::Geometry>(placed.shape.data) else {
                continue;
            };
            let props = &placed.properties;
            let fade = nif
                .property::<nif::Material>(props)
                .and_then(|m| controller(&nif, m.controller, ControllerKind::Alpha))
                .and_then(|c| Some((c.clone(), nif.get_as::<Keys<f32>>(c.data)?.clone())));
            // An animated mesh is built relative to the object that moves.
            let (mesh_transform, above, motion) = match motion(&nif, &placed.path) {
                Some(m) => (m.below, m.above, Some((m.controller, m.keys, m.base))),
                None => (placed.transform, nif::Transform::IDENTITY, None),
            };
            let animation = (motion.is_some() || fade.is_some()).then_some(Animation {
                motion,
                above,
                fade,
            });
            if let Some(mut part) =
                self.part(game, assets, &nif, path, view, g, props, &mesh_transform)
            {
                part.animation = animation.map(Arc::new);
                parts.push(part);
            }
        }
        (!parts.is_empty()).then_some(Model::Parts(parts))
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
        let nodes = Nodes::new(nif);
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
            if let Some(part) = self.part(game, assets, nif, path, view, g, props, &transform) {
                meshes.push((part, i));
            }
        }
        nodes.rig(
            nif,
            meshes,
            (&idle_kf, idle.length),
            moving.as_ref().map(|(kf, distance)| (kf, *distance)),
            assets.meshes,
            assets.skins,
        )
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
        Some(Part {
            mesh: assets.meshes.add(mesh),
            material: assets.materials.add(StandardMaterial {
                base_color: Color::srgba(r, g, b, opacity),
                base_color_texture: texture,
                alpha_mode,
                double_sided,
                cull_mode: (!double_sided).then_some(Face::Back),
                perceptual_roughness: 0.9,
                reflectance: 0.2,
                ..default()
            }),
            animation: None,
            blended: matches!(alpha_mode, AlphaMode::Blend | AlphaMode::Add),
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

/// The value cached for `key`, loading and caching it on first use.
fn cached<T: Clone>(
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
fn controller(nif: &Nif, first: nif::Ref, kind: ControllerKind) -> Option<&Controller> {
    let mut next = first;
    for _ in 0..16 {
        let c = nif.get_as::<Controller>(next)?;
        // Flag bit 3 marks the controller active.
        if c.kind == kind && c.flags & 8 != 0 {
            return Some(c);
        }
        next = c.next;
    }
    None
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
fn embedded_image(p: &nif::Pixels, formats: CompressedImageFormats) -> Option<Image> {
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
