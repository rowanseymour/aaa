//! Supply as the map shows it: a border along the edge of the ground the
//! player's side is supplied over, and a tint on buildings going without.

use aaa_render::Tint;
use aaa_render::overlay;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::effects::Effects;
use crate::{BuildingView, Loaded, Local, OWN, Screen, Sim};

/// The tint a building out of supply is drawn in.
const OUT_OF_SUPPLY: &str = "out_of_supply_vfx";

/// Whether the supply border is drawn.
#[derive(Resource)]
pub struct Border(pub bool);

impl Default for Border {
    fn default() -> Self {
        Border(true)
    }
}

/// The border as drawn, around the zones of these buildings, by index into
/// the sim's.
#[derive(Component)]
pub struct BorderLine(Vec<usize>);

/// Draws the border round the zones of the player's side and its allies,
/// again whenever one comes or goes.
#[allow(clippy::too_many_arguments)]
pub fn show_border(
    mut commands: Commands,
    border: Res<Border>,
    sim: Res<Sim>,
    local: Res<Local>,
    loaded: Res<Loaded>,
    lines: Query<(Entity, &BorderLine)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material: bevy::prelude::Local<Option<Handle<StandardMaterial>>>,
) {
    let sim = &sim.0;
    let zone = |b: usize| sim.buildings[b].supply.zone.as_ref();
    let giving: Vec<usize> = match local.0.filter(|_| border.0) {
        Some(player) => (0..sim.buildings.len())
            .filter(|&b| zone(b).is_some() && sim.shares_supply(player, sim.buildings[b].owner))
            .collect(),
        None => Vec::new(),
    };
    let drawn = lines.iter().next();
    if drawn.map_or(&[][..], |(_, line)| &line.0) == giving {
        return;
    }
    lines.iter().for_each(|(e, _)| commands.entity(e).despawn());
    if giving.is_empty() {
        return;
    }
    let edges = aaa_map::outline(giving.iter().filter_map(|&b| zone(b)));
    let mesh = overlay::border(&loaded.map.ground, &edges);
    let material = material
        .get_or_insert_with(|| materials.add(overlay::mark_material(None, OWN)))
        .clone();
    commands.spawn((
        BorderLine(giving),
        NotShadowCaster,
        Pickable::IGNORE,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        DespawnOnExit(Screen::Match),
    ));
}

/// Tints the buildings that are out of supply, for as long as they are.
pub fn tint_buildings(
    mut commands: Commands,
    sim: Res<Sim>,
    effects: Option<Res<Effects>>,
    views: Query<(Entity, &BuildingView, Has<Tint>)>,
) {
    let Some(colour) = effects.and_then(|e| e.tint(OUT_OF_SUPPLY)) else {
        return;
    };
    for (entity, view, tinted) in &views {
        let b = view.building;
        let out = sim.0.out_of_supply(b) && !sim.0.buildings[b].gone();
        match (out, tinted) {
            (true, false) => commands.entity(entity).insert(Tint(colour)),
            (false, true) => commands.entity(entity).remove::<Tint>(),
            _ => continue,
        };
    }
}
