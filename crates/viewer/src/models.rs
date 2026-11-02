//! Placed objects drawn with the game's models, where their templates name
//! models this viewer can read.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use aaa_data::{Game, Id, View};
use bevy::asset::RenderAssetUsages;
use bevy::image::CompressedImageFormats;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat};
use nif::{Controller, ControllerKind, Keyframes, Keys, Nif, Object};
use tgck::map::Thing;

use crate::terrain::world;
use crate::textures;

/// One mesh of a model, in Bevy axes, ready to instance.
#[derive(Clone)]
struct Part {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    animation: Option<Arc<Animation>>,
    /// Bevy's shadows ignore alpha blending, so a faded or see-through mesh
    /// would cast a solid one.
    blended: bool,
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
    models: HashMap<String, Option<Vec<Part>>>,
    textures: HashMap<String, Option<Handle<Image>>>,
    /// Bevy's texture formats this GPU can sample compressed.
    formats: CompressedImageFormats,
}

impl Models {
    pub fn new(game: &mut Game, formats: CompressedImageFormats) -> Self {
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
        game: &mut Game,
        assets: &mut ModelAssets,
        things: &'a [Thing],
    ) -> Vec<(&'a Thing, bool)> {
        let mut rest = Vec::new();
        let (mut drawn, mut modelless) = (0, 0);
        for t in things {
            let placed = self.place(commands, game, assets, t);
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

    /// Spawns one object, or, for an invisible organization such as a herd,
    /// its members where the map has them standing.
    fn place(
        &mut self,
        commands: &mut Commands,
        game: &mut Game,
        assets: &mut ModelAssets,
        thing: &Thing,
    ) -> Placed {
        let view = match self.views.get(&Id::from(thing.template.as_str())) {
            Some(Some(view)) if !view.models.is_empty() => view.clone(),
            Some(None) => {
                let members = thing.organization().map_or(Vec::new(), |o| o.members);
                return members
                    .iter()
                    .map(|m| self.place(commands, game, assets, m))
                    .max()
                    .unwrap_or(Placed::NoModel);
            }
            _ => return Placed::Missing,
        };
        let (factor, variant) = placement(thing);
        let model =
            &view.models[variant.map_or(thing.id as usize, |v| v as usize) % view.models.len()];
        let Some(parts) = self.model(game, assets, model, &view) else {
            return Placed::Missing;
        };
        // Maps store each object's height as the game places it: on the
        // ground, the seabed or the waterline.
        let [x, y, z] = thing.position;
        // Models face their -y axis, which turns to the object's facing:
        // roads run along it and waves roll back against it to shore.
        let [fx, fy, _] = thing.facing;
        let transform = Transform::from_translation(world(x, y, z))
            .with_rotation(Quat::from_rotation_y(fy.atan2(fx) + FRAC_PI_2))
            .with_scale(Vec3::splat(view.scale * factor));
        commands
            .spawn((
                Name::new(thing.template.clone()),
                transform,
                Visibility::default(),
            ))
            .with_children(|parent| {
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
                    // Spread over a minute by the golden ratio, so any
                    // run of ids lands well apart.
                    let offset = (thing.id as f32 * 0.618_034).fract() * 60.0;
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
        Placed::Drawn
    }

    fn model(
        &mut self,
        game: &mut Game,
        assets: &mut ModelAssets,
        path: &str,
        view: &View,
    ) -> Option<Vec<Part>> {
        if let Some(parts) = self.models.get(path) {
            return parts.clone();
        }
        let parts = self.load(game, assets, path, view);
        self.models.insert(path.to_string(), parts.clone());
        parts
    }

    fn load(
        &mut self,
        game: &mut Game,
        assets: &mut ModelAssets,
        path: &str,
        view: &View,
    ) -> Option<Vec<Part>> {
        let nif = Nif::parse(&game.read(path)?)
            .map_err(|e| warn!("{path}: {e}"))
            .ok()?;
        let mut parts = Vec::new();
        // Skinned shapes are drawn in the pose they were modelled in.
        for placed in nif.shapes() {
            let Some(g) = nif.get_as::<nif::Geometry>(placed.shape.data) else {
                continue;
            };
            let props = &placed.properties;
            let base = nif
                .property::<nif::Texturing>(props)
                .and_then(|t| t.base.as_ref());
            let material = nif.property::<nif::Material>(props);
            let fade = material
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
            let uv_set = base.map_or(0, |b| b.uv_set as usize);
            let Some(mesh) = mesh(g, &mesh_transform, uv_set) else {
                continue;
            };
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
                Some(a) if a.blend() => AlphaMode::Blend,
                _ if opacity < 1.0 => AlphaMode::Blend,
                _ => AlphaMode::Opaque,
            };
            parts.push(Part {
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
                animation: animation.map(Arc::new),
                blended: alpha_mode == AlphaMode::Blend,
            });
        }
        if let Some(kind) = &nif.incomplete {
            debug!("{path}: stops at {kind}");
        }
        (!parts.is_empty()).then_some(parts)
    }

    fn texture(
        &mut self,
        game: &mut Game,
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
fn bevy_transform(t: &nif::Transform) -> Transform {
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
}

/// A placed object's own size factor and model variation, where its record
/// has them. Both follow an 8-byte flag block at the start of the record's
/// undecoded remainder.
fn placement(thing: &Thing) -> (f32, Option<u32>) {
    let field = |at: usize| -> Option<[u8; 4]> { thing.extra.get(at..at + 4)?.try_into().ok() };
    let factor = field(8)
        .map(f32::from_le_bytes)
        .filter(|f| (0.1..10.0).contains(f))
        .unwrap_or(1.0);
    let variant = field(12).map(u32::from_le_bytes).filter(|&v| v < 1024);
    (factor, variant)
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
