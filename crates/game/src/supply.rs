//! Supply as the map shows it: a border along the edge of the ground the
//! player's side is supplied over, and a tint on buildings going without.
//! F5, or the button over the minimap, steps through the player's
//! regiments that have no HQ.

use aaa_render::camera::RtsCamera;
use aaa_render::terrain::world;
use aaa_render::{Tint, overlay, textures};
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{Activate, Button};

use crate::actions::Blocks;
use crate::effects::Effects;
use crate::ui::Hint;
use crate::{BuildingView, Loaded, Local, OWN, Screen, Selected, SelectedBuilding, Sim, minimap};

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

/// The edges the border runs along, as [`aaa_map::outline`] gives them, for
/// the minimap to draw too; empty while the border's hidden.
#[derive(Resource, Default)]
pub struct Outline(pub Vec<Vec<[f32; 2]>>);

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
    mut outline: ResMut<Outline>,
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
        outline.0.clear();
        return;
    }
    let edges = aaa_map::outline(giving.iter().filter_map(|&b| zone(b)));
    let mesh = overlay::border(&loaded.map.ground, &edges);
    outline.0 = edges;
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

/// The button over the minimap that steps through the player's detached
/// regiments, as F5 does; there only while there are some.
#[derive(Component)]
pub struct CycleDetached;

const CYCLE_ICON: &str = "UI/Game/Floaters/ButtonDetachedRegimentUnpressed.tga";
const CYCLE_HELP: &str = "idle_buttons_IdleButton_tooltip_2";
const CYCLE_SIZE: f32 = 42.0;

/// Spawns the button that cycles through detached regiments.
pub fn spawn(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &mut loaded.game;
    let icon = game
        .read(CYCLE_ICON)
        .and_then(|bytes| textures::decode(&bytes, CYCLE_ICON, formats.0, true))
        .map(|i| images.add(i));
    let help = game.strings().get(CYCLE_HELP).cloned();
    commands
        .spawn((
            CycleDetached,
            Blocks,
            Button,
            RelativeCursorPosition::default(),
            Hint(help.unwrap_or_else(|| "Cycles through all detached regiments.".into())),
            ImageNode::new(icon.unwrap_or_default()),
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                left: px(crate::MARGIN),
                bottom: px(crate::MARGIN + minimap::DISC + crate::MARGIN),
                width: px(CYCLE_SIZE),
                height: px(CYCLE_SIZE),
                ..default()
            },
            GlobalZIndex(1),
            DespawnOnExit(Screen::Match),
        ))
        .observe(|_: On<Activate>, mut cycle: CycleParts| {
            cycle.step();
        });
}

/// What stepping through the detached regiments reads and changes.
#[derive(SystemParam)]
pub struct CycleParts<'w, 's> {
    sim: Res<'w, Sim>,
    local: Res<'w, Local>,
    loaded: Res<'w, Loaded>,
    selected: ResMut<'w, Selected>,
    chosen: ResMut<'w, SelectedBuilding>,
    cameras: Query<'w, 's, &'static mut RtsCamera>,
}

impl CycleParts<'_, '_> {
    /// Selects the player's next detached regiment after the one selected,
    /// round to the first again, and looks at it.
    fn step(&mut self) {
        let Some(player) = self.local.0 else {
            return;
        };
        let detached = self.sim.0.detached(player);
        let after = self.selected.0.first().map_or(0, |&now| now + 1);
        let next = detached.iter().find(|&&r| r >= after).or(detached.first());
        let Some(&r) = next.filter(|&&r| self.selected.0 != [r]) else {
            return;
        };
        self.selected.0 = vec![r];
        self.chosen.0 = None;
        let [x, y] = self.sim.0.leader_position(r);
        for mut camera in &mut self.cameras {
            camera.focus = world(x, y, self.loaded.map.ground.height_at(x, y));
        }
    }
}

/// Steps through the detached regiments on F5.
pub fn cycle_detached(keys: Res<ButtonInput<KeyCode>>, mut cycle: CycleParts) {
    if keys.just_pressed(KeyCode::F5) {
        cycle.step();
    }
}

/// Shows the button that cycles through detached regiments while there's
/// one that an HQ of the player's could take, places aside.
pub fn show_cycle_button(
    sim: Res<Sim>,
    local: Res<Local>,
    mut button: Single<&mut Node, With<CycleDetached>>,
) {
    if !sim.is_changed() && !local.is_changed() {
        return;
    }
    let any = local.0.is_some_and(|p| sim.0.attachable(p));
    let display = match any {
        true => Display::Flex,
        false => Display::None,
    };
    if button.display != display {
        button.display = display;
    }
}
