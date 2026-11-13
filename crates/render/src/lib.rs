//! Draws Axis & Allies (2004) maps with Bevy: the ground and water, placed
//! objects with the game's models, and an RTS camera to look at them with.

pub mod blend;
pub mod camera;
pub mod decals;
mod glow;
pub mod log;
pub mod models;
pub mod objects;
pub mod overlay;
mod particles;
mod rig;
pub mod shot;
mod solid;
mod team;
pub mod terrain;
pub mod textures;
pub mod water;

pub use rig::Playing;

use aaa_data::GameData;
use aaa_map::Map;
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::camera::Exposure;
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use tgck::map::Sun;

use blend::GroundMaterial;
use glow::GlowMaterial;
use models::{ModelAssets, Models};
use solid::SolidMaterial;
use team::TeamMaterial;
pub use team::Tint;
use tgck::map::Thing;
use water::WaterMaterial;

/// What drawing a map needs: the ground material, model animation, what lies
/// beyond the map and the camera's controls.
pub struct MapRenderPlugin {
    pub edge: Edge,
    /// Whether the ground and models can be clicked on with Bevy's mesh
    /// picking, which reads meshes that otherwise live only on the GPU.
    pub pickable: bool,
}

/// [`MapRenderPlugin::pickable`].
#[derive(Resource, Clone, Copy)]
struct MeshPicking(bool);

/// How the map meets what lies beyond it.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    /// A sharp edge against sky blue, to show exactly where the map ends.
    Hard,
    /// Fading into black over the last tiles, as the game draws it.
    Fade,
}

impl Edge {
    /// Distance in from the map's edge that the fade to black takes.
    pub fn fade(self) -> f32 {
        match self {
            Edge::Hard => 0.0,
            Edge::Fade => 2.0 * tgck::map::CELL_SIZE,
        }
    }
}

impl Plugin for MapRenderPlugin {
    fn build(&self, app: &mut App) {
        // The shader has to be registered before the material that loads it.
        embedded_asset!(app, "blend.wesl");
        embedded_asset!(app, "water.wesl");
        embedded_asset!(app, "glow.wesl");
        embedded_asset!(app, "team.wesl");
        let beyond = match self.edge {
            Edge::Hard => Color::srgb(0.55, 0.7, 0.85),
            Edge::Fade => Color::BLACK,
        };
        app.add_plugins(MaterialPlugin::<GroundMaterial>::default())
            .add_plugins(MaterialPlugin::<WaterMaterial>::default())
            .add_plugins(MaterialPlugin::<GlowMaterial>::default())
            .add_plugins(MaterialPlugin::<TeamMaterial>::default())
            .add_plugins(MaterialPlugin::<SolidMaterial>::default())
            .insert_resource(ClearColor(beyond))
            .insert_resource(self.edge)
            .insert_resource(MeshPicking(self.pickable))
            .add_systems(
                PostUpdate,
                (
                    rig::carry_skins.after(bevy::transform::TransformSystems::Propagate),
                    particles::play.after(bevy::transform::TransformSystems::Propagate),
                    particles::place_copies
                        .after(particles::play)
                        .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate)
                        .before(bevy::camera::visibility::VisibilitySystems::CalculateBounds),
                ),
            )
            .add_systems(
                Update,
                (
                    models::pace,
                    (models::animate, rig::play).after(models::pace),
                    camera::update.in_set(camera::Controls),
                    (
                        team::tint::<StandardMaterial>,
                        team::tint::<TeamMaterial>,
                        team::tint::<SolidMaterial>,
                    ),
                ),
            );
    }
}

/// The ground's material, whose fog of war the game draws.
#[derive(Resource, Clone)]
pub struct GroundFog(pub Handle<GroundMaterial>);

/// The map record an object was spawned from, on its root entity.
#[derive(Component)]
pub struct MapObject(pub Thing);

/// The assets [`spawn_map`] adds to.
#[derive(SystemParam)]
pub struct MapAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    glow_materials: ResMut<'w, Assets<GlowMaterial>>,
    team_materials: ResMut<'w, Assets<TeamMaterial>>,
    solid_materials: ResMut<'w, Assets<SolidMaterial>>,
    ground_materials: ResMut<'w, Assets<GroundMaterial>>,
    water_materials: ResMut<'w, Assets<WaterMaterial>>,
    images: ResMut<'w, Assets<Image>>,
    skins: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
    formats: Res<'w, CompressedImageFormatSupport>,
    edge: Res<'w, Edge>,
    picking: Res<'w, MeshPicking>,
}

impl MapAssets<'_> {
    /// The stores [`Models`] add to.
    pub fn models(&mut self) -> ModelAssets<'_> {
        ModelAssets {
            meshes: &mut self.meshes,
            materials: &mut self.materials,
            glow_materials: &mut self.glow_materials,
            team_materials: &mut self.team_materials,
            solid_materials: &mut self.solid_materials,
            images: &mut self.images,
            skins: &mut self.skins,
        }
    }
}

/// Spawns a map's ground, water, objects and lights, but no camera, with
/// placeholder markers for objects that can't be drawn shown if `markers`.
/// Objects `leave_out` picks are left for the caller to spawn, with the
/// [`Models`] returned.
pub fn spawn_map(
    commands: &mut Commands,
    assets: &mut MapAssets,
    game: &mut GameData,
    map: &Map,
    markers: bool,
    leave_out: impl Fn(&Thing) -> bool,
) -> Models {
    let built = terrain::build(
        game,
        &map.ground,
        &map.terrain,
        &mut assets.images,
        assets.edge.fade(),
    );
    let pickable = assets.picking.0;
    if let Some((mut mesh, material)) = built.ground {
        if pickable {
            mesh.asset_usage = RenderAssetUsages::default();
        }
        let material = assets.ground_materials.add(material);
        commands.insert_resource(GroundFog(material.clone()));
        commands.spawn((
            Name::new("ground"),
            Mesh3d(assets.meshes.add(mesh)),
            MeshMaterial3d(material),
            NotShadowCaster,
        ));
    }
    if let Some((mesh, material)) = built.water {
        commands.spawn((
            Name::new("water"),
            Mesh3d(assets.meshes.add(mesh)),
            MeshMaterial3d(assets.water_materials.add(material)),
            NotShadowCaster,
            // Clicks go through to what's under the water.
            Pickable::IGNORE,
        ));
    }
    let mut models = Models::new(game, assets.formats.0, &map.players, pickable);
    let holding: std::collections::HashSet<u32> = (map.roles())
        .filter(|(_, role)| *role == aaa_map::Role::HoldsBuilding)
        .map(|(t, _)| t.id)
        .collect();
    let rest = models.spawn(
        commands,
        game,
        &mut assets.models(),
        map.things.iter().filter(|t| !leave_out(t)),
        &holding,
    );
    let (meshes, materials) = (&mut assets.meshes, &mut assets.materials);
    let footing = |t: &str| models.footing(t);
    objects::spawn(
        commands,
        meshes,
        materials,
        &map.ground,
        footing,
        &rest,
        markers,
    );
    light(commands, &map.sun, map.ground.size());
    models
}

/// Lights a map of `size` across from its sun.
fn light(commands: &mut Commands, sun: &Sun, size: f32) {
    let mut directional = DirectionalLight {
        shadow_maps_enabled: true,
        ..default()
    };
    let mut ambient = GlobalAmbientLight::default();
    color_lights(&mut directional, &mut ambient, sun.diffuse, sun.ambient);
    let [dx, dy, dz] = sun.direction;
    commands.spawn((
        directional,
        Transform::default().looking_to(terrain::world(dx, dy, dz), Vec3::Y),
        CascadeShadowConfigBuilder {
            maximum_distance: size * 1.5,
            ..default()
        }
        .build(),
    ));
    commands.insert_resource(ambient);
}

/// Gives the sun and ambient lights a sun's `diffuse` and `ambient` colours.
///
/// The game lights like Direct3D's fixed pipeline: texture * (ambient +
/// diffuse * N.L), clamped. Undoing the exposure and Lambert's 1/pi in Bevy's
/// PBR gets the same result, as long as the camera skips tonemapping.
pub fn color_lights(
    directional: &mut DirectionalLight,
    ambient: &mut GlobalAmbientLight,
    diffuse: [f32; 3],
    ambient_color: [f32; 3],
) {
    let unit = 1.0 / Exposure::default().exposure();
    let rgb = |[r, g, b]: [f32; 3]| Color::srgb(r, g, b);
    directional.color = rgb(diffuse);
    directional.illuminance = std::f32::consts::PI * unit;
    ambient.color = rgb(ambient_color);
    ambient.brightness = unit;
}
