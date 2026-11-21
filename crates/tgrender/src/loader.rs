//! Builds the parts of a model file: its meshes, their materials and
//! textures, and its particle systems.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::CompressedImageFormats;
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat};
use nif::{Nif, PixelFormat, VertexColors};
use tgdata::{GameData, View};

use crate::glow::{Glow, GlowMaterial};
use crate::models::{
    Animation, ModelAssets, Part, PartMaterial, cached, motion, painting, uv_transform,
};
use crate::particles::{self, Blend, Frames, Look, System};
use crate::rig::Nodes;
use crate::solid::{Solid, SolidMaterial};
use crate::team;
use crate::terrain::world;
use crate::textures;

/// Textures loaded for models: from files by archive path, and those
/// stored in a model by its path and the texture's index in it.
pub(crate) struct Textures {
    loaded: HashMap<TextureSource, Option<Handle<Image>>>,
    particles: particles::Layers,
    /// Bevy's texture formats this GPU can sample compressed.
    formats: CompressedImageFormats,
}

impl Textures {
    /// None loaded yet, for a GPU that samples `formats` compressed.
    pub(crate) fn new(formats: CompressedImageFormats) -> Textures {
        Textures {
            loaded: HashMap::new(),
            particles: particles::Layers::default(),
            formats,
        }
    }
}

/// Where a texture comes from, which it's loaded once for.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum TextureSource {
    /// A file, by its archive path.
    File(String),
    /// Pixels stored in the model at `model`, by their nif index.
    Embedded { model: String, index: usize },
}

/// A texture's source, with the pixels of an embedded one.
enum Source<'a> {
    /// A file, by its archive path.
    File(String),
    /// Pixels stored in the model at `model`, at nif index `index`.
    Embedded {
        model: &'a str,
        index: usize,
        pixels: &'a nif::Pixels,
    },
}

/// Builds the parts of one model file: its meshes, their materials and
/// textures, and its particle systems.
pub(crate) struct Loader<'a, 'w> {
    pub(crate) game: &'a GameData,
    pub(crate) assets: &'a mut ModelAssets<'w>,
    pub(crate) textures: &'a mut Textures,
    /// As [`Models::subfolder`](crate::models::Models::subfolder).
    pub(crate) subfolder: Option<&'a str>,
    /// As [`Models::pickable`](crate::models::Models::pickable).
    pub(crate) pickable: bool,
    pub(crate) nif: &'a Nif,
    /// The file's archive path.
    pub(crate) path: &'a str,
    pub(crate) view: &'a View,
}

impl<'a> Loader<'a, '_> {
    /// The parts of a rig's shapes, each with its index in
    /// [`Nodes::shapes`].
    pub(crate) fn rig_meshes(&mut self, nodes: &Nodes) -> Vec<(Part, usize)> {
        let nif = self.nif;
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
            if let Some(mut part) = self.part(g, props, &transform) {
                part.team = team::marked(nif, &s.av);
                part.animation = painting(nif, props).map(Arc::new);
                meshes.push((part, i));
            }
        }
        meshes
    }

    /// A mesh and its material, with `props` the properties in effect for
    /// it, nearest first.
    fn part(
        &mut self,
        g: &nif::Geometry,
        props: &[nif::Ref],
        transform: &nif::Transform,
    ) -> Option<Part> {
        let nif = self.nif;
        let texturing = nif.property::<nif::Texturing>(props);
        let base = texturing.and_then(|t| t.base.as_ref());
        let texture = self.base_texture(props);
        // A texture drawn instead of the colour beneath it shows as stored,
        // whatever light or vertex colours there are.
        let replaced = texture.is_some() && texturing.is_some_and(|t| t.replaces());
        let material = nif.property::<nif::Material>(props);
        let uv_set = base.map_or(0, |b| b.uv_set as usize);
        // Vertex colours tint a mesh only where they stand for its ambient
        // and diffuse colours, as they do without a property to say.
        let tinted = !replaced
            && nif
                .property::<nif::VertexColor>(props)
                .is_none_or(|v| v.stand_for() == VertexColors::AmbientDiffuse);
        let mut mesh = mesh(g, transform, uv_set, tinted)?;
        if self.pickable {
            mesh.asset_usage = RenderAssetUsages::default();
        }
        let alpha = nif.property::<nif::Alpha>(props);
        let double_sided = nif
            .property::<nif::Stencil>(props)
            .is_some_and(|s| s.double_sided());
        let opacity = material.map_or(1.0, |m| m.alpha);
        let alpha_mode = alpha_mode(alpha, opacity);
        let emissive = material.map_or([0.0; 3], |m| m.emissive);
        // Shapes no light reaches show their emissive colour alone, such
        // as the glow of a lit window.
        let unlit = replaced
            || nif
                .property::<nif::VertexColor>(props)
                .is_some_and(|v| !v.lit());
        let ([r, g, b], emissive) = match (replaced, unlit) {
            (true, _) => ([1.0; 3], [0.0; 3]),
            (false, true) => (emissive, [0.0; 3]),
            (false, false) => (material.map_or([1.0; 3], |m| m.diffuse), emissive),
        };
        // The material's own glow is added to the light it takes, and the
        // texture multiplies the lot. Alpha 0 keeps it from being scaled by
        // the camera's exposure, which the lights are set to undo.
        let emissive = LinearRgba::new(emissive[0], emissive[1], emissive[2], 0.0);
        let standard = StandardMaterial {
            base_color: Color::linear_rgba(r, g, b, opacity),
            emissive_texture: (emissive.to_f32_array_no_alpha() != [0.0; 3])
                .then(|| texture.clone())
                .flatten(),
            emissive,
            unlit,
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
        let reflects = material.filter(|_| !unlit).map(|m| (m.ambient, m.diffuse));
        let glow = (alpha.filter(|_| alpha_mode == AlphaMode::Add))
            .zip(material)
            .and_then(|(a, m)| match unlit {
                true => Glow::new([1.0; 3], [1.0; 3], a.source()),
                false => Glow::new(m.ambient, m.diffuse, a.source()),
            });
        let materials = &mut self.assets.materials;
        let material = match glow {
            Some(glow) => PartMaterial::Glow(materials.glow.add(GlowMaterial {
                // Without highlights, as the game draws them.
                base: StandardMaterial {
                    reflectance: 0.0,
                    perceptual_roughness: 1.0,
                    ..standard
                },
                extension: glow,
            })),
            _ if alpha_mode == AlphaMode::Blend && writes_depth => {
                PartMaterial::Solid(materials.solid.add(SolidMaterial {
                    base: standard,
                    extension: Solid::default(),
                }))
            }
            _ => PartMaterial::Standard(materials.standard.add(standard)),
        };
        Some(Part {
            mesh: self.assets.meshes.add(mesh),
            material,
            animation: None,
            blended: matches!(alpha_mode, AlphaMode::Blend | AlphaMode::Add),
            team: false,
            reflects,
        })
    }

    /// The model's particle systems that can be drawn, its scene objects
    /// being where `frames` has them.
    pub(crate) fn particles(&mut self, frames: &Frames) -> Vec<Arc<System>> {
        let nif = self.nif;
        let placed = nif.particle_systems();
        let old = nif.old_particle_systems();
        let mut out = Vec::new();
        for (_, p) in placed {
            let Some(&at) = p.path.last() else { continue };
            let look = self.particle_look(&p.properties);
            if let Some(system) = System::read(nif, frames, at, look) {
                out.push(Arc::new(system));
            }
        }
        for (particles, p) in old {
            let look = self.particle_look(&p.properties);
            let copies = self.mesh_copies(particles);
            if let Some(system) = System::read_old(nif, frames, &p, look, copies) {
                out.push(Arc::new(system));
            }
        }
        out
    }

    /// The meshes an older-format mesh particle system's particles show,
    /// one set for each in its modifier's list; empty for other systems.
    fn mesh_copies(&mut self, particles: &nif::OldParticles) -> Vec<Arc<Vec<Part>>> {
        let nif = self.nif;
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
            .map(|r| Arc::new(self.scene_parts(nif.shapes_below(r), false)))
            .collect()
    }

    /// The parts of `shapes`, each animated by its own controllers and
    /// those of the objects above it, with the first object on each path
    /// taken as having no transform of its own unless `root_placed`.
    pub(crate) fn scene_parts(&mut self, shapes: Vec<nif::Placed>, root_placed: bool) -> Vec<Part> {
        let nif = self.nif;
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
                        (below, Some(Animation::moved(motion, painting)))
                    }
                    (None, painting) => (placed.transform, painting),
                };
            if let Some(mut part) = self.part(g, props, &mesh_transform) {
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
    fn particle_look(&mut self, props: &[nif::Ref]) -> Look {
        let nif = self.nif;
        let texturing = nif.property::<nif::Texturing>(props);
        let layer = self.particle_layer(props);
        let m = nif.property::<nif::Material>(props);
        let opacity = m.map_or(1.0, |m| m.alpha);
        // Squares that also test alpha still blend: cut out instead, soft
        // smoke turns to specks.
        let alpha = nif.property::<nif::Alpha>(props);
        let blend = match alpha_mode(alpha, opacity) {
            AlphaMode::Mask(_) if alpha.is_some_and(|a| a.blend()) => Blend::Over,
            AlphaMode::Mask(cutoff) => Blend::Opaque { cutoff },
            AlphaMode::Opaque => Blend::Opaque { cutoff: 0.0 },
            AlphaMode::Add => Blend::Add,
            _ => Blend::Over,
        };
        let rgb = |c: Option<[f32; 3]>, a: f32| {
            let [r, g, b] = c.unwrap_or([1.0; 3]);
            Color::linear_rgba(r, g, b, a)
        };
        let emissive = m.map(|m| m.emissive);
        // Without a property, vertex colours stand for ambient and diffuse,
        // lit.
        let (vertex, lit) = nif
            .property::<nif::VertexColor>(props)
            .map_or((VertexColors::AmbientDiffuse, true), |v| {
                (v.stand_for(), v.lit())
            });
        let replace = layer.is_some() && texturing.is_some_and(|t| t.replaces());
        let (base_color, emissive, lit, tinted) = match (replace, vertex, lit) {
            // The texture alone.
            (true, _, _) => (Color::WHITE, None, false, false),
            (false, VertexColors::Emissive, false) => (Color::WHITE, None, false, true),
            // Lit by no light, so only the emissive colour shows.
            (false, _, false) => (rgb(emissive, opacity), None, false, false),
            // The particle's colour is its ambient and diffuse, and alpha.
            (false, VertexColors::AmbientDiffuse, true) => (Color::WHITE, emissive, true, true),
            (false, _, true) => (rgb(m.map(|m| m.diffuse), opacity), emissive, true, false),
        };
        let emissive = emissive.unwrap_or([0.0; 3]);
        Look {
            layer: layer.unwrap_or(0),
            colour: base_color.to_linear().to_f32_array(),
            emissive,
            lit,
            blend,
            tinted,
        }
    }

    /// Where the base texture `props` give a shape comes from.
    fn base_source(&self, props: &[nif::Ref]) -> Option<Source<'a>> {
        let nif = self.nif;
        let base = nif.property::<nif::Texturing>(props)?.base.as_ref()?;
        let source = nif.get_as::<nif::SourceTexture>(base.source)?;
        match (&source.file, nif.get_as(source.pixels)) {
            (Some(file), _) => {
                let (game, model) = (self.game, self.path);
                let Some(path) = game.find_texture(self.subfolder, self.view, model, file) else {
                    warn!("{model}: texture {file} not found");
                    return None;
                };
                Some(Source::File(path))
            }
            (None, Some(pixels)) => Some(Source::Embedded {
                model: self.path,
                index: source.pixels?,
                pixels,
            }),
            _ => None,
        }
    }

    /// The base texture `props` give a shape, from a file or embedded.
    fn base_texture(&mut self, props: &[nif::Ref]) -> Option<Handle<Image>> {
        let source = self.base_source(props)?;
        let (game, formats, images) = (self.game, self.textures.formats, &mut self.assets.images);
        cached(&mut self.textures.loaded, &source.key(), || {
            let image = source.load(game, formats)?;
            Some(images.add(image))
        })
    }

    /// The base texture `props` give a particle system, as a layer of the
    /// particle textures.
    fn particle_layer(&mut self, props: &[nif::Ref]) -> Option<u32> {
        let source = self.base_source(props)?;
        let (game, images) = (self.game, &mut *self.assets.images);
        // Decoded on the CPU, whatever the GPU samples compressed.
        let bc = CompressedImageFormats::BC;
        let layers = &mut self.textures.particles;
        layers.layer(&source.key(), images, || source.load(game, bc))
    }
}

impl Source<'_> {
    /// What it's loaded once for.
    fn key(&self) -> TextureSource {
        match self {
            Source::File(path) => TextureSource::File(path.clone()),
            Source::Embedded { model, index, .. } => TextureSource::Embedded {
                model: model.to_string(),
                index: *index,
            },
        }
    }

    /// Its texture, as an image `formats` can sample.
    fn load(&self, game: &GameData, formats: CompressedImageFormats) -> Option<Image> {
        match self {
            Source::File(path) => load_image(&game.read(path)?, path, formats),
            Source::Embedded { model, pixels, .. } => {
                let image = embedded_image(pixels, formats, false);
                if image.is_none() {
                    warn!(
                        "{model}: unsupported embedded texture format {:?}",
                        pixels.pixel_format()
                    );
                }
                image
            }
        }
    }
}

/// How a shape with `alpha` and its material's `opacity` blends.
fn alpha_mode(alpha: Option<&nif::Alpha>, opacity: f32) -> AlphaMode {
    // An additive shape that tests alpha too, as flashes do, is still
    // added: what its test would drop adds next to nothing. Other blends
    // that test are cut out, which writes depth only where they show.
    match alpha {
        Some(a) if a.blend() && a.adds() => AlphaMode::Add,
        Some(a) if a.test() => AlphaMode::Mask(a.threshold as f32 / 255.0),
        Some(a) if a.blend() => AlphaMode::Blend,
        _ if opacity < 1.0 => AlphaMode::Blend,
        _ => AlphaMode::Opaque,
    }
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
/// every mipmap level when the GPU can sample them. `srgb` for one the
/// interface draws, rather than the world (see [`crate::stored`]).
pub(crate) fn embedded_image(
    p: &nif::Pixels,
    formats: CompressedImageFormats,
    srgb: bool,
) -> Option<Image> {
    let &[width, height, offset] = p.mipmaps.first()?;
    let size = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let pixels = (width * height) as usize;
    let (format, data, levels) = match p.pixel_format() {
        f @ (PixelFormat::Rgb | PixelFormat::Rgba) => {
            let n = if f == PixelFormat::Rgb { 3 } else { 4 };
            let src = p.data.get(offset as usize..offset as usize + pixels * n)?;
            let rgba = src
                .chunks_exact(n)
                .flat_map(|c| [c[0], c[1], c[2], if n == 4 { c[3] } else { 255 }])
                .collect();
            let format = match srgb {
                true => TextureFormat::Rgba8UnormSrgb,
                false => TextureFormat::Rgba8Unorm,
            };
            (format, rgba, 1)
        }
        bc @ (PixelFormat::Dxt1 | PixelFormat::Dxt3 | PixelFormat::Dxt5)
            if formats.contains(CompressedImageFormats::BC) =>
        {
            let format = match (bc, srgb) {
                (PixelFormat::Dxt1, true) => TextureFormat::Bc1RgbaUnormSrgb,
                (PixelFormat::Dxt3, true) => TextureFormat::Bc2RgbaUnormSrgb,
                (_, true) => TextureFormat::Bc3RgbaUnormSrgb,
                (PixelFormat::Dxt1, false) => TextureFormat::Bc1RgbaUnorm,
                (PixelFormat::Dxt3, false) => TextureFormat::Bc2RgbaUnorm,
                (_, false) => TextureFormat::Bc3RgbaUnorm,
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
    let mut image = textures::decode(bytes, name, formats, false)?;
    image.sampler = textures::repeating();
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn additive_shapes_add_even_when_they_test_alpha() {
        let mode = |flags, threshold| alpha_mode(Some(&nif::Alpha { flags, threshold }), 1.0);
        // Source alpha over one, tested: a flash.
        assert_eq!(mode(0x120d, 0), AlphaMode::Add);
        // Source alpha over one minus it, tested: foliage.
        assert_eq!(mode(0x12ed, 128), AlphaMode::Mask(128.0 / 255.0));
        assert_eq!(mode(0x00ed, 0), AlphaMode::Blend);
        assert_eq!(mode(0x0200, 64), AlphaMode::Mask(64.0 / 255.0));
        assert_eq!(alpha_mode(None, 1.0), AlphaMode::Opaque);
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
}
