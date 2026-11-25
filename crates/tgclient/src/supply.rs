//! Supply as the map shows it: a border along the edge of the ground the
//! player's side is supplied over, and a tint on buildings going without.
//! Its key shows and hides the border, as the options do.

use bevy::image::CompressedImageFormatSupport;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use tgmap::Tile;
use tgrender::{Tint, overlay};

use crate::effects::Effects;
use crate::intent::{Intent, asked};
use crate::keys;
use crate::views::BuildingView;
use crate::{Loaded, Local, Screen, Sim};

/// The tint a building out of supply is drawn in.
const OUT_OF_SUPPLY: &str = "out_of_supply_vfx";

/// The tint a building under siege is drawn in, over that one.
const UNDER_SIEGE: &str = "under_siege_vfx";

/// Whether the supply border is drawn.
#[derive(Resource)]
pub struct Border(pub bool);

impl Default for Border {
    fn default() -> Self {
        Border(true)
    }
}

/// The paths the border follows, for the minimap to draw too; empty while
/// the border's hidden.
#[derive(Resource, Default)]
pub struct Outline(pub Vec<Vec<[f32; 2]>>);

/// How the border looks, from the game's settings.
#[derive(Resource)]
pub struct BorderLook {
    width: f32,
    /// The side of the squares the map is cut into, in world units.
    block: f32,
    material: Handle<StandardMaterial>,
}

/// The border as drawn.
#[derive(Component)]
pub struct BorderLine;

/// The outline the border follows, of the zones of these buildings, by
/// index into the sim's, each with where its zone is laid and how far it
/// reaches; kept while the border's hidden, as where its loops start
/// follows from how the zones came to be.
#[derive(Default)]
pub struct Traced {
    giving: Vec<(usize, Tile, f32)>,
    outline: tgmap::Outline,
}

/// Draws the border round the zones of the player's side and its allies,
/// again whenever one comes, goes, moves or changes its reach.
#[allow(clippy::too_many_arguments)]
pub fn show_border(
    mut commands: Commands,
    border: Res<Border>,
    sim: Res<Sim>,
    local: Res<Local>,
    loaded: Res<Loaded>,
    lines: Query<Entity, With<BorderLine>>,
    look: Res<BorderLook>,
    mut traced: bevy::prelude::Local<Traced>,
    mut outline: ResMut<Outline>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if sim.is_added() {
        *traced = Traced::default();
    }
    let sim = &sim.0;
    let zone = |b: usize| sim.buildings[b].supply.zone.as_ref();
    let laid = |b: usize| {
        let (around, reach) = sim.buildings[b].supply.laid()?;
        Some((b, around, reach))
    };
    let giving: Vec<(usize, Tile, f32)> = match local.0 {
        Some(player) => (0..sim.buildings.len())
            .filter(|&b| sim.shares_supply(player, sim.buildings[b].owner))
            .filter_map(laid)
            .collect(),
        None => Vec::new(),
    };
    let moved = traced.giving != giving;
    if moved {
        let zones = giving.iter().filter_map(|&(b, ..)| zone(b));
        let block = (look.block / tgck::map::CELL_SIZE).round() as i32;
        traced.outline.retrace(zones, block);
        traced.giving = giving;
    }
    let shown = border.0 && !traced.giving.is_empty();
    if !moved && shown != lines.is_empty() {
        return;
    }
    lines.iter().for_each(|e| commands.entity(e).despawn());
    if !shown {
        outline.0.clear();
        return;
    }
    let edges = traced.outline.loops();
    let mesh = overlay::border(&loaded.map.ground, &edges, look.width);
    outline.0 = overlay::border_paths(&edges);
    commands.spawn((
        BorderLine,
        NotShadowCaster,
        Pickable::IGNORE,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(look.material.clone()),
        DespawnOnExit(Screen::Match),
    ));
}

/// On a building tinted: when each of its tints began, in seconds of the
/// wall clock, which their pulses run on; one a stronger tint hides keeps
/// counting meanwhile.
#[derive(Component, Default)]
pub struct Tinted(Vec<(&'static str, f64)>);

/// The steps a tint's strength moves in, so that a pulsing one recolours
/// its materials a few times a second rather than every frame.
pub(crate) const TINT_STEPS: f32 = 16.0;

/// Tints the buildings that are out of supply or under siege, pulsing for
/// as long as they are and plain again at once when they're not. Only the
/// tint with the highest priority shows, the siege's over the other.
pub fn tint_buildings(
    mut commands: Commands,
    sim: Res<Sim>,
    effects: Option<Res<Effects>>,
    time: Res<Time<Real>>,
    mut views: Query<(Entity, &BuildingView, Option<&mut Tinted>, Option<&Tint>)>,
) {
    let Some(effects) = effects else {
        return;
    };
    let now = time.elapsed_secs_f64();
    for (entity, view, tinted, tint) in &mut views {
        let b = view.building;
        let building = &sim.0.buildings[b];
        let standing = !building.gone();
        let active = [
            (OUT_OF_SUPPLY, standing && sim.0.out_of_supply(b)),
            (UNDER_SIEGE, standing && building.besieged.is_some()),
        ];
        let active: Vec<_> = (active.into_iter())
            .filter(|&(_, on)| on)
            .filter_map(|(id, _)| Some((id, effects.tint(id)?)))
            .collect();
        if active.is_empty() {
            if tinted.is_some() {
                commands.entity(entity).remove::<(Tinted, Tint)>();
            }
            continue;
        }
        let mut since = tinted.map_or_else(Vec::new, |t| std::mem::take(&mut t.into_inner().0));
        since.retain(|(id, _)| active.iter().any(|(a, _)| a == id));
        for (id, _) in &active {
            if !since.iter().any(|(s, _)| s == id) {
                since.push((id, now));
            }
        }
        // The first held of the strongest.
        let shown = (since.iter())
            .filter_map(|&(id, at)| Some((at, active.iter().find(|(a, _)| *a == id)?.1)))
            .reduce(|a, b| if b.1.priority > a.1.priority { b } else { a });
        let Some((at, pulse)) = shown else {
            continue;
        };
        let level = (pulse.level((now - at) as f32) * TINT_STEPS).round() / TINT_STEPS;
        let colour = Tint(pulse.colour(level));
        commands.entity(entity).insert(Tinted(since));
        if tint != Some(&colour) {
            commands.entity(entity).insert(colour);
        }
    }
}

/// Loads how the border looks.
pub fn setup(
    mut commands: Commands,
    loaded: Res<Loaded>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &loaded.game;
    let border = game.supply_border();
    let [r, g, b] = border.colour;
    let colour = Color::srgb(r, g, b);
    let image = (border.texture.as_deref())
        .and_then(|path| overlay::border_image(game, path, formats.0))
        .map(|i| images.add(i));
    let material = materials.add(overlay::mark_material(image, colour));
    commands.insert_resource(BorderLook {
        width: border.width,
        block: border.block,
        material,
    });
}

/// Shows or hides the border when asked.
pub fn toggle_border(mut intents: MessageReader<Intent>, mut border: ResMut<Border>) {
    if asked(&mut intents, keys::SUPPLY_BORDER) {
        border.0 = !border.0;
    }
}
