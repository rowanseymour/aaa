//! Placed objects drawn with the game's models, where their templates name
//! models this viewer can read.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageType,
};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat};
use nif::{Nif, Object};
use tgck::map::Thing;

use crate::game::{Game, View};
use crate::terrain::{Ground, world};

/// One mesh of a model, in Bevy axes, ready to instance.
#[derive(Clone)]
struct Part {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// Loads models and textures once each, however many objects use them.
pub struct Models {
    views: HashMap<String, View>,
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
    /// the rest, each with whether its template names a model at all (sound
    /// emitters, cameras and the like do not).
    pub fn spawn<'a>(
        &mut self,
        commands: &mut Commands,
        game: &mut Game,
        assets: &mut ModelAssets,
        ground: &Ground,
        things: &'a [Thing],
    ) -> Vec<(&'a Thing, bool)> {
        let mut rest = Vec::new();
        for thing in things {
            let Some(view) = self
                .views
                .get(&thing.template.to_ascii_lowercase())
                .cloned()
            else {
                rest.push((thing, false));
                continue;
            };
            let (factor, variant) = placement(thing);
            let model =
                &view.models[variant.map_or(thing.id as usize, |v| v as usize) % view.models.len()];
            let Some(parts) = self.model(game, assets, model, &view) else {
                rest.push((thing, true));
                continue;
            };
            let [x, y, _] = thing.position;
            let [fx, fy, _] = thing.facing;
            let transform = Transform::from_translation(world(x, y, ground.height_at(x, y)))
                .with_rotation(Quat::from_rotation_y(fy.atan2(fx)))
                .with_scale(Vec3::splat(view.scale * factor));
            commands
                .spawn((
                    Name::new(thing.template.clone()),
                    transform,
                    Visibility::default(),
                ))
                .with_children(|parent| {
                    for part in parts {
                        parent.spawn((Mesh3d(part.mesh), MeshMaterial3d(part.material)));
                    }
                });
        }
        let mut missing: HashMap<&str, usize> = HashMap::new();
        for (t, _) in &rest {
            *missing.entry(t.template.as_str()).or_default() += 1;
        }
        info!(
            "{} objects drawn with models, {} as markers",
            things.len() - rest.len(),
            rest.len()
        );
        debug!("templates without a readable model: {missing:?}");
        rest
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
            let Some(Object::Geometry(g)) = nif.get(placed.shape.data) else {
                continue;
            };
            let props = &placed.properties;
            let texturing = nif.property(props, |o| match o {
                Object::Texturing(t) => Some(t),
                _ => None,
            });
            let base = texturing.and_then(|t| t.base.as_ref());
            let Some(mesh) = mesh(g, &placed.transform, base.map_or(0, |b| b.uv_set as usize))
            else {
                continue;
            };
            let texture = match base.and_then(|b| nif.get(b.source)) {
                Some(Object::SourceTexture(t)) => match (&t.file, nif.get(t.pixels)) {
                    (Some(file), _) => self.texture(game, assets, file, view, path),
                    (None, Some(Object::Pixels(p))) => {
                        let key = format!("{path}#{}", t.pixels.unwrap_or_default());
                        self.embedded(assets, key, p)
                    }
                    _ => None,
                },
                _ => None,
            };
            let material = nif.property(props, |o| match o {
                Object::Material(m) => Some(m),
                _ => None,
            });
            let alpha = nif.property(props, |o| match o {
                Object::Alpha(a) => Some(a),
                _ => None,
            });
            let double_sided = nif
                .property(props, |o| match o {
                    Object::Stencil(s) => Some(s),
                    _ => None,
                })
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
        if let Some(t) = self.textures.get(&path) {
            return t.clone();
        }
        let handle = game
            .read(&path)
            .and_then(|bytes| load_image(&bytes, &path, self.formats))
            .map(|image| assets.images.add(image));
        self.textures.insert(path, handle.clone());
        handle
    }
}

impl Models {
    fn embedded(
        &mut self,
        assets: &mut ModelAssets,
        key: String,
        pixels: &nif::Pixels,
    ) -> Option<Handle<Image>> {
        if let Some(t) = self.textures.get(&key) {
            return t.clone();
        }
        let handle = embedded_image(pixels, self.formats).map(|image| assets.images.add(image));
        if handle.is_none() {
            warn!(
                "{key}: unsupported embedded texture format {}",
                pixels.format
            );
        }
        self.textures.insert(key, handle.clone());
        handle
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
    let to_bevy = |[x, y, z]: [f32; 3]| [x, z, -y];
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
    image.sampler = repeating();
    Some(image)
}

fn repeating() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        anisotropy_clamp: 16,
        ..ImageSamplerDescriptor::linear()
    })
}

fn load_image(bytes: &[u8], name: &str, formats: CompressedImageFormats) -> Option<Image> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    let mut image = Image::from_buffer(
        bytes,
        ImageType::Extension(&ext),
        formats,
        true,
        ImageSampler::Default,
        RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|e| warn!("{name}: {e}"))
    .ok()?;
    image.sampler = repeating();
    Some(image)
}
