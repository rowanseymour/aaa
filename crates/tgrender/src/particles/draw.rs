//! Particles' squares drawn, every system's in one mesh for each layer
//! the cameras draw.

use crate::loader::TextureSource;
use crate::textures;
use bevy::asset::{AssetMut, RenderAssetUsages, uuid_handle};
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use std::collections::HashMap;
use std::ops::Range;

use super::{Blend, ParticleMesh, Particles};

/// The 3D cameras, which aren't particle meshes.
type Cameras = (With<Camera3d>, Without<ParticleMesh>);

/// Every particle texture, as a layer of one array, so one material draws
/// every system and all their squares go in one mesh, in the order they're
/// drawn in.
pub(crate) const TEXTURES: Handle<Image> = uuid_handle!("6b0c3f0e-8d0a-4e57-9a8f-2f3c1d6e9b41");

/// The side of each layer of [`TEXTURES`], which every particle texture is
/// resampled to: the largest they come in.
const SIDE: usize = 128;

/// The particle textures loaded so far, as layers of [`TEXTURES`]: white
/// first, for systems with none.
pub(crate) struct Layers {
    /// By where each was loaded from: none if it couldn't be.
    by_source: HashMap<TextureSource, Option<u32>>,
    /// Every layer's pixels and mipmaps, one after another, as the array
    /// takes them.
    data: Vec<u8>,
    count: u32,
    mips: u32,
}

impl Default for Layers {
    fn default() -> Self {
        let white = white();
        Layers {
            by_source: HashMap::new(),
            mips: white.texture_descriptor.mip_level_count,
            data: white.data.unwrap_or_default(),
            count: 1,
        }
    }
}

impl Layers {
    /// The layer of the texture from `source`, loaded by `load` the first
    /// time: none if it can't be.
    pub fn layer(
        &mut self,
        source: &TextureSource,
        images: &mut Assets<Image>,
        load: impl FnOnce() -> Option<Image>,
    ) -> Option<u32> {
        if let Some(&layer) = self.by_source.get(source) {
            return layer;
        }
        let layer = load().and_then(resampled).and_then(|i| i.data).map(|data| {
            self.data.extend(data);
            self.count += 1;
            // Bevy can't add a layer in place, so the whole array goes up
            // again, but only as models load.
            _ = images.insert(&TEXTURES, self.array());
            self.count - 1
        });
        self.by_source.insert(source.clone(), layer);
        layer
    }

    /// Every layer so far, as an array texture.
    fn array(&self) -> Image {
        let size = UVec2::splat(SIDE as u32);
        let format = textures::rgba8_format(false);
        textures::array_image(self.data.clone(), size, self.mips, format, self.count)
    }
}

/// A layer of plain white.
fn white() -> Image {
    textures::layer_image(vec![255; SIDE * SIDE * 4], SIDE)
}

/// `image` as a layer of [`TEXTURES`]: bilinearly resampled, as it would be
/// sampled at its own size, and mipmapped.
fn resampled(image: Image) -> Option<Image> {
    let image = textures::rgba8(image)?;
    let (w, h) = (image.width() as usize, image.height() as usize);
    let src = image.data?;
    let texel = |x: isize, y: isize, c: usize| {
        let (x, y) = (
            x.rem_euclid(w as isize) as usize,
            y.rem_euclid(h as isize) as usize,
        );
        src[(y * w + x) * 4 + c] as f32
    };
    let mut data = Vec::with_capacity(SIDE * SIDE * 4);
    for i in 0..SIDE * SIDE {
        // Where this texel's centre falls among the source's, which repeat.
        let u = ((i % SIDE) as f32 + 0.5) * w as f32 / SIDE as f32 - 0.5;
        let v = ((i / SIDE) as f32 + 0.5) * h as f32 / SIDE as f32 - 0.5;
        let (x, y) = (u.floor() as isize, v.floor() as isize);
        let (fx, fy) = (u - u.floor(), v - v.floor());
        for c in 0..4 {
            let top = texel(x, y, c) * (1.0 - fx) + texel(x + 1, y, c) * fx;
            let bottom = texel(x, y + 1, c) * (1.0 - fx) + texel(x + 1, y + 1, c) * fx;
            data.push((top * (1.0 - fy) + bottom * fy).round() as u8);
        }
    }
    Some(textures::layer_image(data, SIDE))
}

/// Draws every system's squares, whatever its texture and blending, with
/// [`TEXTURES`].
pub type ParticleMaterial = ExtendedMaterial<StandardMaterial, ParticleTextures>;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct ParticleTextures {
    #[texture(100, dimension = "2d_array")]
    #[sampler(101)]
    textures: Handle<Image>,
}

impl MaterialExtension for ParticleTextures {
    fn vertex_shader() -> ShaderRef {
        "embedded://tgrender/particles.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://tgrender/particles.wesl".into()
    }
}

/// The meshes every system's squares are drawn in: one for each layer the
/// cameras draw. A mesh for each system, refilled every frame, cost more to
/// upload and draw than everything else the systems do.
#[derive(Resource, Default)]
pub struct Draws {
    material: Option<Handle<ParticleMaterial>>,
    /// How many layers [`TEXTURES`] had when the material last bound it.
    layers: u32,
    draws: Vec<Draw>,
    /// Kept between frames for their room.
    squares: Vec<Square>,
    systems: Vec<Drawn>,
}

/// The mesh drawing the systems in one layer, as its camera sees them.
struct Draw {
    layers: Option<RenderLayers>,
    entity: Entity,
    mesh: Handle<Mesh>,
    /// Spawned this frame, so its entity isn't there to place yet.
    new: bool,
    /// Its mesh holds squares, so it has to be emptied once it has none.
    filled: bool,
    view: Option<Eye>,
    /// Its systems' squares this frame, in the order they're drawn.
    runs: Vec<Range<usize>>,
}

/// Where a camera is and which way it looks.
#[derive(Clone, Copy)]
struct Eye {
    at: Vec3,
    forward: Vec3,
    right: Vec3,
    up: Vec3,
}

/// A system's squares this frame.
struct Drawn {
    draw: usize,
    /// How far in front of the camera the middle of its squares is.
    depth: f32,
    squares: Range<usize>,
}

/// A particle's square, in the world.
#[derive(Clone, Copy)]
struct Square {
    at: Vec3,
    half: f32,
    colour: [f32; 4],
    /// Its system's look, as the shader takes it: its texture layer and how
    /// it blends and is lit, and its emissive colour and alpha cutoff.
    look: [f32; 2],
    extra: [f32; 4],
}

/// What [`draw`] needs of a system.
type SystemLook = (
    &'static Particles,
    &'static GlobalTransform,
    &'static InheritedVisibility,
    Option<&'static RenderLayers>,
);

/// The assets [`draw`] makes and changes.
#[derive(SystemParam)]
pub struct DrawStores<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    images: ResMut<'w, Assets<Image>>,
    materials: ResMut<'w, Assets<ParticleMaterial>>,
}

/// What [`draw`] needs of a camera.
type CameraLook = (
    &'static Camera,
    &'static GlobalTransform,
    Option<&'static RenderLayers>,
);

/// Redraws every system's squares to face the camera that sees them. Runs
/// once visibility is propagated, so hidden systems are left out, and
/// before what each camera sees is worked out, so a new mesh is drawn at
/// once.
pub fn draw(
    mut commands: Commands,
    mut draws: ResMut<Draws>,
    systems: Query<SystemLook, Without<ParticleMesh>>,
    cameras: Query<CameraLook, Cameras>,
    mut placed: Query<(&mut Transform, &mut GlobalTransform), With<ParticleMesh>>,
    mut stores: DrawStores,
) {
    let DrawStores {
        meshes,
        images,
        materials,
    } = &mut stores;
    let textures = match images.get(&TEXTURES) {
        Some(image) => image.texture_descriptor.size.depth_or_array_layers,
        None => {
            _ = images.insert(&TEXTURES, Layers::default().array());
            1
        }
    };
    let Draws {
        material,
        layers,
        draws,
        squares,
        systems: drawn,
    } = &mut *draws;
    let material = material.get_or_insert_with(|| {
        materials.add(ParticleMaterial {
            base: StandardMaterial {
                alpha_mode: AlphaMode::Premultiplied,
                double_sided: true,
                cull_mode: None,
                // Without highlights, as the game draws them.
                perceptual_roughness: 1.0,
                reflectance: 0.0,
                ..default()
            },
            extension: ParticleTextures { textures: TEXTURES },
        })
    });
    // A larger array is a new texture, which the material binds only once
    // it's changed itself.
    if std::mem::replace(layers, textures) != textures {
        materials.get_mut(&*material).map(AssetMut::into_inner);
    }
    let eye = |layers: Option<&RenderLayers>| {
        let layers = layers.unwrap_or(&RenderLayers::layer(0)).clone();
        let (_, camera, _) = cameras.iter().find(|(c, _, seen)| {
            c.is_active && seen.unwrap_or(&RenderLayers::layer(0)).intersects(&layers)
        })?;
        Some(Eye {
            at: camera.translation(),
            forward: camera.forward().as_vec3(),
            right: camera.right().as_vec3(),
            up: camera.up().as_vec3(),
        })
    };
    for draw in draws.iter_mut() {
        draw.view = eye(draw.layers.as_ref());
    }
    // The mesh drawing `layers`, and the camera that sees it.
    let mut draw_for = |layers: Option<&RenderLayers>| {
        let i = match draws.iter().position(|d| d.layers.as_ref() == layers) {
            Some(i) => i,
            None => {
                let mut draw = spawn_draw(&mut commands, meshes, material, layers);
                draw.view = eye(layers);
                draws.push(draw);
                draws.len() - 1
            }
        };
        (i, draws[i].view)
    };
    // Made at once, so its pipeline is ready for the first square.
    draw_for(None);
    drawn.clear();
    squares.clear();
    for (particles, own, shown, layers) in &systems {
        let (system, state) = (&particles.system, &particles.state);
        // Mesh particles draw as copies of their meshes instead.
        if !system.copies.is_empty() || !shown.get() || state.particles.is_empty() {
            continue;
        }
        let (draw, Some(view)) = draw_for(layers) else {
            continue;
        };
        let own = own.affine();
        let to_world = match system.world_space {
            true => Affine3A::IDENTITY,
            false => own,
        };
        let scale = own.matrix3.x_axis.length();
        let look = &system.look;
        let (mode, cutoff) = match look.blend {
            Blend::Over => (0, 0.0),
            Blend::Add => (1, 0.0),
            Blend::Opaque { cutoff } => (2, cutoff),
        };
        let flags = mode + if look.lit { 4 } else { 0 };
        let [er, eg, eb] = look.emissive;
        let start = squares.len();
        squares.extend(state.particles.iter().map(|p| Square {
            at: to_world.transform_point3(p.position),
            half: p.radius * p.size * scale,
            colour: match look.tinted {
                true => std::array::from_fn(|i| p.colour[i] * look.colour[i]),
                false => look.colour,
            },
            look: [look.layer as f32, flags as f32],
            extra: [er, eg, eb, cutoff],
        }));
        drawn.push(Drawn {
            draw,
            depth: depth(&squares[start..], &view),
            squares: start..squares.len(),
        });
    }
    far_to_near(drawn);
    for system in drawn.iter() {
        draws[system.draw].runs.push(system.squares.clone());
    }
    draws.retain_mut(|draw| {
        let new = std::mem::take(&mut draw.new);
        let runs = std::mem::take(&mut draw.runs);
        let Some(view) = draw.view.filter(|_| !runs.is_empty() || draw.filled) else {
            return true;
        };
        // At the camera, so it's sorted after every other blended model.
        let at = Transform::from_translation(view.at);
        match placed.get_mut(draw.entity) {
            // Propagation has run, so its place this frame is set here.
            Ok((mut t, mut g)) => (*t, *g) = (at, at.into()),
            Err(_) if new => {
                commands
                    .entity(draw.entity)
                    .insert((at, GlobalTransform::from(at)));
            }
            // Something despawned it, so another is made when needed.
            Err(_) => return false,
        }
        let mut ordered = runs.iter().flat_map(|r| &squares[r.clone()]);
        if let Some(mut mesh) = meshes.get_mut(&draw.mesh) {
            fill(&mut mesh, &mut ordered, &view);
        }
        draw.filled = !runs.is_empty();
        // Kept for its room.
        draw.runs = runs;
        draw.runs.clear();
        true
    });
}

/// How far in front of `view` the middle of the box around `squares` is,
/// which systems are drawn far to near by.
fn depth(squares: &[Square], view: &Eye) -> f32 {
    let (low, high) = squares.iter().fold((Vec3::MAX, Vec3::MIN), |(l, h), s| {
        (l.min(s.at), h.max(s.at))
    });
    ((low + high) / 2.0 - view.at).dot(view.forward)
}

/// Puts systems in the order they're drawn: far to near, as blended squares
/// cover what's drawn before them.
fn far_to_near(systems: &mut [Drawn]) {
    systems.sort_by(|a, b| b.depth.total_cmp(&a.depth));
}

/// Spawns the entity drawing the squares in `layers`.
fn spawn_draw(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<ParticleMaterial>,
    layers: Option<&RenderLayers>,
) -> Draw {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, Vec::<[f32; 2]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, Vec::<[f32; 4]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new())
    .with_inserted_indices(Indices::U32(Vec::new()));
    let nowhere = Eye {
        at: Vec3::ZERO,
        forward: Vec3::NEG_Z,
        right: Vec3::X,
        up: Vec3::Y,
    };
    fill(&mut mesh, &mut std::iter::empty(), &nowhere);
    let mesh = meshes.add(mesh);
    let mut entity = commands.spawn((
        Name::new("particles"),
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        // Visibility has been propagated this frame, and it's drawn from
        // its first.
        Visibility::Visible,
        InheritedVisibility::VISIBLE,
        ParticleMesh,
        // Its bounds change every frame.
        NoFrustumCulling,
        NotShadowCaster,
    ));
    if let Some(layers) = layers {
        entity.insert(layers.clone());
    }
    Draw {
        layers: layers.cloned(),
        entity: entity.id(),
        mesh,
        new: true,
        filled: false,
        view: None,
        runs: Vec::new(),
    }
}

/// A square's corners, across and up the screen, in drawing order.
const CORNERS: [(f32, f32); 4] = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
const UVS: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

/// Refills a mesh in place: a square for each of `squares`, facing `view`
/// and placed from where it is. With no squares it holds one empty
/// triangle, as Bevy can't upload a mesh without vertices.
fn fill<'a>(mesh: &mut Mesh, squares: &mut (impl Iterator<Item = &'a Square> + Clone), view: &Eye) {
    use bevy::mesh::VertexAttributeValues as V;
    let n = squares.clone().count();
    // Each square's value at each of its corners, or a value for each
    // corner of the empty triangle, then for each of the two vertices that
    // centre the mesh's box.
    fn corners<'a, T: Copy>(
        v: &mut Vec<T>,
        squares: &(impl Iterator<Item = &'a Square> + Clone),
        f: impl Fn(&Square) -> [T; 4],
        none: T,
    ) {
        v.clear();
        v.extend(squares.clone().flat_map(&f));
        if v.is_empty() {
            v.extend([none; 3]);
        }
        v.extend([none; 2]);
    }
    let (right, up) = (view.right, view.up);
    if let Some(V::Float32x3(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
        corners(
            v,
            squares,
            |s| {
                let at = s.at - view.at;
                CORNERS.map(|(x, y)| (at + (right * x + up * y) * s.half).to_array())
            },
            [0.0; 3],
        );
        // Bevy sorts a blended mesh by the middle of its box, not where it's
        // placed, so two vertices no square uses put that middle at the
        // camera.
        let reach = v
            .iter()
            .fold(Vec3::ZERO, |r, &p| r.max(Vec3::from(p).abs()));
        let last = v.len() - 1;
        v[last - 1] = (-reach).to_array();
        v[last] = reach.to_array();
    }
    if let Some(V::Float32x3(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL) {
        corners(v, squares, |_| [(-view.forward).to_array(); 4], [0.0; 3]);
    }
    if let Some(V::Float32x2(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
        corners(v, squares, |_| UVS, [0.0; 2]);
    }
    if let Some(V::Float32x2(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_1) {
        corners(v, squares, |s| [s.look; 4], [0.0; 2]);
    }
    if let Some(V::Float32x4(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_TANGENT) {
        corners(v, squares, |s| [s.extra; 4], [0.0; 4]);
    }
    if let Some(V::Float32x4(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        corners(v, squares, |s| [s.colour; 4], [0.0; 4]);
    }
    if let Some(Indices::U32(v)) = mesh.indices_mut() {
        v.clear();
        match n {
            0 => v.extend([0, 1, 2]),
            n => v.extend((0..n as u32).flat_map(|q| {
                let b = q * 4;
                [b, b + 1, b + 2, b, b + 2, b + 3]
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systems_draw_far_to_near_by_the_middle_of_their_box() {
        let view = Eye {
            at: Vec3::ZERO,
            forward: Vec3::NEG_Z,
            right: Vec3::X,
            up: Vec3::Y,
        };
        let square = |z: f32| Square {
            at: Vec3::new(0.0, 0.0, z),
            half: 1.0,
            colour: [1.0; 4],
            look: [0.0; 2],
            extra: [0.0; 4],
        };
        // Mostly near, but reaching far: the middle of its box counts, not
        // of its squares.
        let spread = [square(-1.0), square(-1.0), square(-1.0), square(-9.0)];
        assert_eq!(depth(&spread, &view), 5.0);
        let at = |depth: f32| Drawn {
            draw: 0,
            depth,
            squares: 0..1,
        };
        let mut systems = vec![at(2.0), at(5.0), at(3.0)];
        far_to_near(&mut systems);
        let order: Vec<f32> = systems.iter().map(|s| s.depth).collect();
        assert_eq!(order, [5.0, 3.0, 2.0]);
    }

    #[test]
    fn textures_are_resampled_as_they_would_be_sampled() {
        // Black and white columns, two texels wide.
        let data = [[0, 0, 0, 255], [255; 4]].repeat(2).concat();
        let format = bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb;
        let image = textures::image_2d(UVec2::splat(2), 1, data, format);
        let layer = resampled(image).unwrap();
        assert_eq!((layer.width(), layer.height()), (SIDE as u32, SIDE as u32));
        let red = |x: usize| layer.data.as_ref().unwrap()[x * 4];
        // Each column's centre keeps its colour, blending towards the other
        // between them and, as the texture repeats, across its edges.
        assert!(red(SIDE / 4) < 5 && red(SIDE * 3 / 4) > 250);
        assert!((100..156).contains(&red(SIDE / 2)));
        assert!((100..156).contains(&red(0)));
    }

    #[test]
    fn mesh_box_centres_on_camera() {
        let view = Eye {
            at: Vec3::new(10.0, 50.0, 10.0),
            forward: Vec3::NEG_Y,
            right: Vec3::X,
            up: Vec3::NEG_Z,
        };
        let square = |at| Square {
            at,
            half: 1.0,
            colour: [1.0; 4],
            look: [0.0; 2],
            extra: [0.0; 4],
        };
        let squares = [
            square(Vec3::new(30.0, 0.0, 5.0)),
            square(Vec3::new(12.0, 2.0, 40.0)),
        ];
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new())
        .with_inserted_indices(Indices::U32(Vec::new()));
        fill(&mut mesh, &mut squares.iter(), &view);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        let (low, high) = positions.iter().fold((Vec3::MAX, Vec3::MIN), |(l, h), &p| {
            (l.min(p.into()), h.max(p.into()))
        });
        assert_eq!((low + high) / 2.0, Vec3::ZERO);
        let colours = mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap().len();
        assert_eq!(colours, positions.len());
        // The padding isn't drawn.
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!()
        };
        assert!(indices.iter().all(|&i| (i as usize) < squares.len() * 4));

        // With no squares, every attribute still matches the positions.
        fill(&mut mesh, &mut [].iter(), &view);
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().len();
        assert_eq!(
            mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap().len(),
            positions
        );
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!()
        };
        assert_eq!(indices, &[0, 1, 2]);
    }
}
