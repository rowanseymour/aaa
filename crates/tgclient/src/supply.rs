//! Supply as the map shows it: a border along the edge of the ground the
//! player's side is supplied over, and a tint on buildings going without.
//! Its key shows and hides the border, as the options do. Another steps
//! through the player's organizations that have no HQ, as either
//! interface's button does.

use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use tgmap::Tile;
use tgrender::camera::RtsCamera;
use tgrender::terrain::world;
use tgrender::{Tint, overlay};

use crate::effects::Effects;
use crate::intent::{Intent, asked};
use crate::keys;
use crate::{BuildingView, Loaded, Local, Screen, Selected, SelectedBuilding, Sim};

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

/// The paths the border follows, for the minimap to draw too; empty while
/// the border's hidden.
#[derive(Resource, Default)]
pub struct Outline(pub Vec<Vec<[f32; 2]>>);

/// How the border looks, from the game's settings.
#[derive(Resource)]
pub struct BorderLook {
    pub colour: Color,
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

/// On a building out of supply: when it went out, in seconds of the wall
/// clock, which its tint's pulse runs on.
#[derive(Component)]
pub struct OutOfSupply(f64);

/// The steps a tint's strength moves in, so that a pulsing one recolours
/// its materials a few times a second rather than every frame.
pub(crate) const TINT_STEPS: f32 = 16.0;

/// Tints the buildings that are out of supply, pulsing for as long as they
/// are and plain again at once when they're not.
pub fn tint_buildings(
    mut commands: Commands,
    sim: Res<Sim>,
    effects: Option<Res<Effects>>,
    time: Res<Time<Real>>,
    views: Query<(Entity, &BuildingView, Option<&OutOfSupply>, Option<&Tint>)>,
) {
    let Some(pulse) = effects.and_then(|e| e.tint(OUT_OF_SUPPLY)) else {
        return;
    };
    let now = time.elapsed_secs_f64();
    for (entity, view, since, tint) in &views {
        let b = view.building;
        let out = sim.0.out_of_supply(b) && !sim.0.buildings[b].gone();
        let since = match (out, since) {
            (true, Some(&OutOfSupply(since))) => since,
            (true, None) => {
                commands.entity(entity).insert(OutOfSupply(now));
                now
            }
            (false, Some(_)) => {
                commands.entity(entity).remove::<(OutOfSupply, Tint)>();
                continue;
            }
            (false, None) => continue,
        };
        let level = (pulse.level((now - since) as f32) * TINT_STEPS).round() / TINT_STEPS;
        let colour = Tint(pulse.colour(level));
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
        colour,
        width: border.width,
        block: border.block,
        material,
    });
}

/// What stepping through the detached organizations reads and changes.
#[derive(SystemParam)]
pub struct CycleParts<'w, 's> {
    sim: Res<'w, Sim>,
    local: Res<'w, Local>,
    loaded: Res<'w, Loaded>,
    selected: ResMut<'w, Selected>,
    chosen: ResMut<'w, SelectedBuilding>,
    scenery: Res<'w, crate::banners::Scenery>,
    cameras: Query<'w, 's, &'static mut RtsCamera>,
}

impl CycleParts<'_, '_> {
    /// Selects the player's next detached organization after the one selected,
    /// round to the first again, and looks at it.
    fn step(&mut self) {
        let Some(player) = self.local.0 else {
            return;
        };
        // Those that can't be selected are passed over.
        let mut detached = self.sim.0.detached(player);
        detached.retain(|&r| !self.scenery.has(&self.sim.0, r));
        let after = self.selected.0.first().map_or(0, |&now| now + 1);
        let next = detached.iter().find(|&&r| r >= after).or(detached.first());
        let Some(&r) = next.filter(|&&r| self.selected.0 != [r]) else {
            return;
        };
        self.selected.0 = vec![r];
        self.chosen.0 = None;
        let [x, y] = self.sim.0.leader_position(r);
        for mut camera in &mut self.cameras {
            camera.focus = world(x, y, self.loaded.map.ground.height_at([x, y]));
        }
    }
}

/// Steps through the detached organizations when asked.
pub fn cycle_detached(mut intents: MessageReader<Intent>, mut cycle: CycleParts) {
    if asked(&mut intents, keys::CYCLE_DETACHED) {
        cycle.step();
    }
}

/// Shows or hides the border when asked.
pub fn toggle_border(mut intents: MessageReader<Intent>, mut border: ResMut<Border>) {
    if asked(&mut intents, keys::SUPPLY_BORDER) {
        border.0 = !border.0;
    }
}
