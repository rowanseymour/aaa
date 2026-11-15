//! The picture of what's selected: a face seen through the camera its
//! model holds, or a thing's whole model fitted to the picture, drawn to an
//! image of its own, apart from the map.

use aaa_data::{AnimationKind, Id};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Exposure, RenderTarget};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::light::AmbientLight;
use bevy::prelude::*;
use nif::Nif;

use crate::Playing;
use crate::models::{ModelAssets, Models, Paint, Plays, bevy_transform, idle};
use crate::particles::Frames;
use crate::shot;
use crate::terrain::world;

/// The layer sitters are drawn in, which the map's camera and lights
/// leave alone.
const LAYER: usize = 1;

/// Where sitters stand, well under any map, so that nothing of theirs that
/// misses the layer shows over one.
const AT: Vec3 = Vec3::new(0.0, -5000.0, 0.0);

/// How strong the stage's ambient light is, as a colour's brightness:
/// what most models' materials reflect of it, the rest glowing to make up
/// their own share.
const AMBIENT: f32 = 0.5;

/// The name of the camera a face's model holds.
const CAMERA: &str = "PortraitCamera";

/// A fitted model's picture: how many times as wide as it's high, and how
/// far across it the eye sees, as the game's camera on the map does.
pub const FIT_ASPECT: f32 = 4.0 / 3.0;
const FIT_ACROSS: f32 = 15.0;
/// The way a fitted model is seen from, in model space: ahead of it, to
/// one side and above.
const FIT_FROM: [f32; 3] = [1.0, -1.0, 1.0];

/// What a portrait shows.
#[derive(Clone, PartialEq, Debug)]
pub enum Sitter {
    /// A face, by its portrait's id.
    Face(Id),
    /// A template's own model, whole, in its owner's colour.
    Model {
        template: String,
        owner: Option<u32>,
    },
}

/// Where the portrait's sitter is drawn, and the images it's drawn to.
#[derive(Resource)]
pub struct Stage {
    /// A face's picture: square, as its model's camera sees, to be shown
    /// stretched over wherever it goes.
    face: Handle<Image>,
    /// A fitted model's picture, [`FIT_ASPECT`] times as wide as high.
    fitted: Handle<Image>,
    camera: Entity,
    /// What shines on the sitter, from the first to sit.
    light: Option<Entity>,
    /// Who's on it, and the root of their model.
    sitter: Option<(Sitter, Entity)>,
    /// How many have sat, so that no two fidget alike.
    sat: u32,
}

/// How a sitter is seen: from where, and how wide.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Seen {
    /// The eye, from the model's origin.
    transform: Transform,
    /// The angle from the bottom of the picture to its top.
    fov: f32,
    near: f32,
    far: f32,
}

impl Stage {
    /// A stage whose pictures are `width` pixels across, with nobody on
    /// it. Its [`Stage::camera`] lasts until it's despawned.
    pub fn new(commands: &mut Commands, images: &mut Assets<Image>, width: u32) -> Stage {
        let high = (width as f32 / FIT_ASPECT).round() as u32;
        let face = images.add(shot::target(UVec2::splat(width)));
        let fitted = images.add(shot::target(UVec2::new(width, high)));
        let camera = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    // Before the map's, so the picture is ready for the
                    // interface drawn over that.
                    order: -1,
                    is_active: false,
                    clear_color: ClearColorConfig::Custom(Color::BLACK),
                    ..default()
                },
                RenderTarget::from(face.clone()),
                Tonemapping::None,
                DebandDither::Disabled,
                RenderLayers::layer(LAYER),
                // White light all round: see [`crate::color_lights`].
                AmbientLight {
                    color: Color::srgb(AMBIENT, AMBIENT, AMBIENT),
                    brightness: 1.0 / Exposure::default().exposure(),
                    ..default()
                },
            ))
            .id();
        Stage {
            face,
            fitted,
            camera,
            light: None,
            sitter: None,
            sat: 0,
        }
    }

    /// The image the sitter is drawn to, and whether it's a fitted model's
    /// picture, which keeps its shape, rather than a face's, which
    /// stretches; `None` with nobody on the stage.
    pub fn picture(&self) -> Option<(Handle<Image>, bool)> {
        Some(match self.sitter()? {
            Sitter::Face(_) => (self.face.clone(), false),
            Sitter::Model { .. } => (self.fitted.clone(), true),
        })
    }

    pub fn camera(&self) -> Entity {
        self.camera
    }

    /// Who's on it.
    pub fn sitter(&self) -> Option<&Sitter> {
        self.sitter.as_ref().map(|(sitter, _)| sitter)
    }

    /// What it has spawned besides its camera: the model of whoever's on
    /// it and its light.
    pub fn spawned(&self) -> impl Iterator<Item = Entity> {
        let root = self.sitter.as_ref().map(|(_, root)| *root);
        root.into_iter().chain(self.light)
    }

    /// Takes whoever's on it off.
    pub fn clear(&mut self, commands: &mut Commands) {
        if let Some((_, root)) = self.sitter.take() {
            commands.entity(root).despawn();
            commands
                .entity(self.camera)
                .entry::<Camera>()
                .and_modify(|mut c| c.is_active = false);
        }
    }
}

/// A sitter's root: everything under it is drawn in the portrait's layer,
/// and it plays on while the game is paused.
#[derive(Component)]
pub struct Sitting;

/// Breaks a face's idling up with its fidgets.
#[derive(Component)]
pub struct Fidgets {
    /// What it can play, by their animations' ids.
    ids: Vec<String>,
    /// How many times it idles between them: at least the first, and
    /// fewer than the second.
    idle_loops: [u32; 2],
    /// Seconds of idling left before the next.
    left: f32,
    /// What picks the next and how long till it.
    seed: u32,
}

impl Fidgets {
    fn roll(&mut self) -> u32 {
        // Xorshift: any run of numbers that looks unplanned does.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed
    }

    /// How many idle loops come before the next fidget.
    fn loops(&mut self) -> u32 {
        let [least, under] = self.idle_loops;
        match under > least {
            true => least + self.roll() % (under - least),
            false => least,
        }
    }
}

impl Models {
    /// The portrait a template's things show, by id, if it names one.
    pub fn portrait(&self, template: &str) -> Option<&Id> {
        self.view(template)?.portrait.as_ref()
    }

    /// Whether a template's things show their owner's faction's portrait
    /// rather than their own.
    pub fn shows_faction_portrait(&self, template: &str) -> bool {
        self.view(template).is_some_and(|v| v.faction_portrait)
    }

    /// Puts `sitter` on the stage in place of whoever was there, a face
    /// that's `greeting` playing first what it plays on being selected.
    /// The first to sit is lit from `sun`, the way the map's sun shines,
    /// and so are all after. False if it has no model that can be drawn,
    /// which leaves the stage empty.
    pub fn sit(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        stage: &mut Stage,
        sitter: &Sitter,
        greeting: bool,
        sun: Dir3,
    ) -> bool {
        stage.clear(commands);
        stage.sat = stage.sat.wrapping_add(1);
        stage.light.get_or_insert_with(|| {
            let unit = 1.0 / Exposure::default().exposure();
            let light = DirectionalLight {
                illuminance: std::f32::consts::PI * unit,
                ..default()
            };
            let shining = Transform::default().looking_to(sun, Vec3::Y);
            let layer = RenderLayers::layer(LAYER);
            commands.spawn((light, shining, layer)).id()
        });
        let placed = match sitter {
            Sitter::Face(id) => self.sit_face(commands, assets, stage, id, greeting),
            Sitter::Model { template, owner } => self.sit_model(commands, assets, template, *owner),
        };
        let Some((root, seen)) = placed else {
            return false;
        };
        let image = match sitter {
            Sitter::Face(_) => stage.face.clone(),
            Sitter::Model { .. } => stage.fitted.clone(),
        };
        let mut eye = seen.transform;
        eye.translation += AT;
        commands.entity(stage.camera).insert((
            eye,
            RenderTarget::from(image),
            Projection::Perspective(PerspectiveProjection {
                fov: seen.fov,
                near: seen.near,
                far: seen.far,
                ..default()
            }),
        ));
        commands
            .entity(stage.camera)
            .entry::<Camera>()
            .and_modify(|mut c| c.is_active = true);
        stage.sitter = Some((sitter.clone(), root));
        true
    }

    /// Spawns a face, idling and fidgeting, seen by its model's camera.
    fn sit_face(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        stage: &mut Stage,
        id: &Id,
        greeting: bool,
    ) -> Option<(Entity, Seen)> {
        let portrait = self.portraits.get(id)?.clone();
        let path = portrait.view.models.first()?.clone();
        let seen = self.measures(&path)?.camera?;
        let view = &portrait.view;
        let plays = Plays {
            idle: idle(view),
            moving: None,
            once: |a| a.kind != AnimationKind::Idle,
        };
        let model = self.model(assets, &path, view, plays)?;
        let root = commands
            .spawn((
                Name::new(path),
                Sitting,
                Transform::from_translation(AT),
                Visibility::default(),
            ))
            .id();
        self.spawn_meshes(commands, assets, root, model, staged(None), 0.0);
        let of_kind = |kind: AnimationKind| -> Vec<String> {
            let animations = portrait.view.animations.iter();
            animations
                .filter(|a| a.kind == kind)
                .map(|a| a.ids.clone())
                .collect()
        };
        // Any number will do to start from, as long as sittings differ.
        let seed = stage.sat.wrapping_mul(0x9e37_79b9);
        let mut fidgets = Fidgets {
            ids: of_kind(AnimationKind::Fidget),
            idle_loops: portrait.idle_loops,
            left: f32::NAN,
            seed: seed | 1,
        };
        let greetings = of_kind(AnimationKind::Selected);
        if greeting && !greetings.is_empty() {
            let pick = fidgets.roll() as usize % greetings.len();
            commands
                .entity(root)
                .insert(Greeting(greetings[pick].clone()));
        }
        if !fidgets.ids.is_empty() {
            commands.entity(root).insert(fidgets);
        }
        Some((root, seen))
    }

    /// Spawns a template's first model as its file has it, in its owner's
    /// colour, seen from far enough for all of it to fit the picture.
    fn sit_model(
        &mut self,
        commands: &mut Commands,
        assets: &mut ModelAssets,
        template: &str,
        owner: Option<u32>,
    ) -> Option<(Entity, Seen)> {
        let view = self.view(template)?.clone();
        let path = view.models.first()?.clone();
        let extent = self.measures(&path)?.extent?;
        let model = self.model(assets, &path, &view, Plays::unit(&view, None))?;
        let root = commands
            .spawn((
                Name::new(path),
                Sitting,
                Transform::from_translation(AT),
                Visibility::default(),
            ))
            .id();
        let colour = owner.and_then(|o| self.colours.get(&o).copied());
        self.spawn_meshes(commands, assets, root, model, staged(colour), 0.0);
        let reach = view.ground_reach.unwrap_or(extent.foot);
        Some((root, fitted(extent.height(), reach, extent.furthest)))
    }
}

/// Painted in `colour`, under the stage's ambient light.
fn staged(colour: Option<[f32; 3]>) -> Paint {
    Paint {
        colour,
        ambient: Some(Color::srgb(AMBIENT, AMBIENT, AMBIENT).to_linear().red),
    }
}

/// How the camera a face's model holds sees it; `None` if it holds none.
pub(crate) fn seen_by(nif: &Nif, frames: &Frames) -> Option<Seen> {
    let found = nif.objects.iter().enumerate().find_map(|(i, o)| match o {
        nif::Object::Camera(c) if c.av.name.eq_ignore_ascii_case(CAMERA) => Some((i, c)),
        _ => None,
    });
    let (at, camera) = found?;
    let placed = frames.0.get(&at).map_or(camera.av.transform, |a| a.model);
    Some(Seen {
        transform: eye(&placed),
        fov: camera.top.atan() - camera.bottom.atan(),
        near: camera.near,
        far: camera.far,
    })
}

/// Where a model's camera placed at `placed` in the model looks from, as a
/// Bevy camera: the model's looks along its own +x with its +y up.
fn eye(placed: &nif::Transform) -> Transform {
    let r = placed.rotation;
    let axis = |c: usize| world(r[0][c], r[1][c], r[2][c]);
    let rotation = Quat::from_mat3(&Mat3::from_cols(axis(2), axis(1), -axis(0)));
    Transform {
        rotation: rotation.normalize(),
        scale: Vec3::ONE,
        ..bevy_transform(placed)
    }
}

/// The view of a model `height` high whose foot reaches `reach` from its
/// origin: looking at the point half its height above its origin, from far
/// enough that the near edge of its foot sits on the picture's bottom edge.
/// `furthest` is how far from its origin any of it is.
fn fitted(height: f32, reach: f32, furthest: f32) -> Seen {
    let across = (FIT_ACROSS.to_radians() / 2.0).tan();
    let fov = 2.0 * (across / FIT_ASPECT).atan();
    let [x, y, z] = FIT_FROM;
    let from = world(x, y, z).normalize();
    let elevation = from.y.asin();
    let half = height / 2.0;
    // Half the picture's height as an angle, as the game reckons it.
    let below = (FIT_ACROSS * 0.375).to_radians();
    let to_foot = half.atan2(reach);
    let back = reach.hypot(half) * (std::f32::consts::PI - to_foot - below - elevation).sin()
        / below.sin();
    // A model with no size still needs somewhere to be seen from.
    let back = back.max(1.0);
    let middle = Vec3::Y * half;
    let transform = Transform::from_translation(middle + from * back).looking_at(middle, Vec3::Y);
    let around = furthest + half;
    Seen {
        transform,
        fov,
        near: (back - around).max(back * 0.01),
        far: back + around,
    }
}

/// Draws everything under a sitter in the portrait's layer, as it's
/// spawned. Runs before what's visible is worked out.
pub fn layer(
    mut commands: Commands,
    sitters: Query<Entity, With<Sitting>>,
    children: Query<&Children>,
    layered: Query<(), With<RenderLayers>>,
) {
    for root in &sitters {
        for entity in children.iter_descendants(root) {
            if !layered.contains(entity) {
                commands.entity(entity).insert(RenderLayers::layer(LAYER));
            }
        }
    }
}

/// What a face plays once as it's put up, by its animation's ids.
#[derive(Component)]
pub struct Greeting(String);

/// Has each face play its greeting as it's put up, and one of its fidgets
/// after idling a while.
pub fn fidget(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut greeted: Query<(Entity, &Greeting, &mut Playing)>,
    mut faces: Query<(&mut Fidgets, &mut Playing), Without<Greeting>>,
) {
    for (face, greeting, mut playing) in &mut greeted {
        playing.use_ability(&greeting.0);
        commands.entity(face).remove::<Greeting>();
    }
    for (mut fidgets, mut playing) in &mut faces {
        if playing.busy() {
            continue;
        }
        let length = playing.idle_length();
        if fidgets.left.is_nan() {
            fidgets.left = length * fidgets.loops() as f32;
        }
        fidgets.left -= time.delta_secs();
        if fidgets.left > 0.0 {
            continue;
        }
        let pick = fidgets.roll() as usize % fidgets.ids.len();
        playing.use_ability(&fidgets.ids[pick]);
        fidgets.left = length * fidgets.loops() as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_models_camera_looks_along_its_x_with_its_y_up() {
        // Ahead of the model, which faces -y, looking back at it.
        let placed = nif::Transform {
            translation: [0.0, -10.0, 2.0],
            rotation: [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            scale: 1.0,
        };
        let eye = eye(&placed);
        assert!(eye.translation.distance(world(0.0, -10.0, 2.0)) < 1e-5);
        assert!(eye.forward().distance(world(0.0, 1.0, 0.0)) < 1e-5);
        assert!(eye.up().distance(world(0.0, 0.0, 1.0)) < 1e-5);
        assert!(eye.right().distance(world(1.0, 0.0, 0.0)) < 1e-5);
    }

    #[test]
    fn a_fitted_model_stands_on_the_pictures_bottom_edge() {
        let (height, reach) = (4.0, 3.0);
        let seen = fitted(height, reach, 5.0);
        let eye = seen.transform;
        // From ahead, to one side and above, looking at half its height.
        let from = world(1.0, -1.0, 1.0).normalize();
        let middle = Vec3::Y * height / 2.0;
        assert!((eye.translation - middle).normalize().distance(from) < 1e-5);
        assert!(eye.forward().distance(-from) < 1e-5);
        // The nearest point of its foot is as far below the middle of the
        // picture as the game takes half its height to be.
        let level = Vec3::new(from.x, 0.0, from.z).normalize();
        let foot = level * reach - eye.translation;
        let below = foot.angle_between(*eye.forward());
        assert!((below - (FIT_ACROSS * 0.375).to_radians()).abs() < 1e-4);
        let back = eye.translation.distance(middle);
        assert!(seen.near < back - 5.0 && seen.far > back + 5.0);
    }

    #[test]
    fn fidgets_come_after_as_many_idle_loops_as_the_data_allows() {
        let mut fidgets = Fidgets {
            ids: vec!["fidget1".into()],
            idle_loops: [1, 3],
            left: 0.0,
            seed: 12345,
        };
        // Fewer than the most it names.
        let loops: Vec<u32> = (0..200).map(|_| fidgets.loops()).collect();
        assert!(loops.iter().all(|n| (1..=2).contains(n)));
        assert!((1..=2).all(|n| loops.contains(&n)));
        fidgets.idle_loops = [2, 2];
        assert_eq!(fidgets.loops(), 2);
    }
}
