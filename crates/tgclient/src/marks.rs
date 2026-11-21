//! What's drawn on the map over what's selected or focused: rings,
//! the selection box and health bars, in each side's tint.

use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use tgmap::Shape;
use tgrender::MapObject;
use tgrender::camera::RtsCamera;
use tgrender::models::Footprint;
use tgrender::overlay::{self, BUILDING_HEALTH_BAR, HEALTH_BAR, Mark, OverlayImages, SQUARE_INSET};
use tgrender::space::world;
use tgsim::{Relation, World};

use crate::ui_scale::{OnMap, overlaps_view, place_on_map, within_view};
use crate::views::{BuildingView, UnitView};
use crate::{Focus, Loaded, Local, Screen, Selected, SelectedBuilding, Sim, ui};

/// The flat square marks are drawn on and their materials, one per
/// [`TINTS`].
#[derive(Resource)]
pub(crate) struct Marks {
    square: Handle<Mesh>,
    rings: [Handle<StandardMaterial>; 3],
    squares: [Handle<StandardMaterial>; 3],
}

/// The box drawn while dragging to select organizations.
#[derive(Component)]
pub(crate) struct SelectionBox;

/// The square under a focused building, draped over the ground in a mesh of
/// its own: which building and how big.
#[derive(Component)]
pub(crate) struct DrapedMark(pub(crate) usize, pub(crate) Vec2);

/// One of the health bars drawn over the screen.
#[derive(Component)]
pub(crate) struct HealthBar;

/// Loads what [`show_focus`] and [`show_health`] draw with, and the box
/// [`select`] draws.
pub(crate) fn setup_overlays(
    mut commands: Commands,
    loaded: Res<Loaded>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let images = OverlayImages::load(&loaded.game, &mut images, formats.0);
    let mut tinted = |image: &Option<Handle<Image>>| {
        TINTS.map(|c| materials.add(overlay::mark_material(image.clone(), c)))
    };
    commands.insert_resource(Marks {
        square: meshes.add(overlay::flat_square()),
        rings: tinted(&images.ring),
        squares: tinted(&images.square),
    });
    commands.insert_resource(images);
    commands.spawn((
        SelectionBox,
        Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(px(1)),
            ..default()
        },
        BorderColor::all(OWN),
        BackgroundColor(OWN.with_alpha(0.1)),
        Visibility::Hidden,
        GlobalZIndex(ui::layer::SELECTING),
        Pickable::IGNORE,
        DespawnOnExit(Screen::Match),
    ));
}

pub(crate) const OWN: Color = Color::srgb(0.1, 0.9, 0.1);
const FRIEND: Color = Color::srgb(0.2, 0.55, 1.0);
const OTHER: Color = Color::srgb(0.95, 0.15, 0.1);
/// What marks things by how their owner stands toward the player, by
/// [`tint`].
pub(crate) const TINTS: [Color; 3] = [OWN, FRIEND, OTHER];

/// Which of [`Marks`]' tints suits what `owner` owns.
pub(crate) fn tint(sim: &World, local: Option<u32>, owner: u32) -> usize {
    match local.map(|p| sim.sides.relation(p, owner)) {
        Some(Relation::Itself) => 0,
        Some(Relation::Ally) => 1,
        _ => 2,
    }
}

type ShownMark = (
    &'static mut Transform,
    &'static mut MeshMaterial3d<StandardMaterial>,
    &'static mut Visibility,
);

/// Rings the units of the organization under the cursor and squares the
/// building under it, each mark sized to the model's footprint.
#[allow(clippy::too_many_arguments)]
pub(crate) fn show_focus(
    mut commands: Commands,
    focus: Res<Focus>,
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    local: Res<Local>,
    marks: Res<Marks>,
    fixed: Res<Time<Fixed>>,
    units: Query<(&UnitView, Option<&Footprint>)>,
    buildings: Query<(&BuildingView, &MapObject, Option<&Footprint>)>,
    mut shown: Query<ShownMark, (With<Mark>, Without<DrapedMark>)>,
    mut draped: Query<(Entity, &DrapedMark, &mut MeshMaterial3d<StandardMaterial>)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    // The ring fills its image but for a thin margin.
    const RING_FILL: f32 = 238.0 / 256.0;
    // Infantry are about this wide.
    const SMALLEST: Vec2 = Vec2::splat(0.4);
    let ground = &loaded.map.ground;
    let t = fixed.overstep_fraction();
    let mut wanted = Vec::new();
    let mut square = None;
    match *focus {
        Focus::Nothing => {}
        Focus::Organization(r) => {
            let material = &marks.rings[tint(&sim.0, local.0, sim.0.organizations[r].owner)];
            for (view, footprint) in &units {
                let unit = &sim.0.units[view.unit];
                if unit.organization != r || !unit.alive() {
                    continue;
                }
                let at = unit.drawn_at(t);
                // An oval through the footprint's corners.
                let size =
                    footprint.map_or(SMALLEST, |f| f.0) * std::f32::consts::SQRT_2 / RING_FILL;
                let transform = overlay::mark_transform(ground, at, unit.facing, size);
                wanted.push((transform, material.clone()));
            }
        }
        Focus::Building(b) => {
            let building = &sim.0.buildings[b];
            let material = &marks.squares[tint(&sim.0, local.0, building.owner)];
            if let Some((_, object, footprint)) = buildings.iter().find(|(v, ..)| v.building == b) {
                let [fx, fy, _] = object.0.facing;
                let size = claimed_size(building, footprint) / (1.0 - 2.0 * SQUARE_INSET);
                square = Some((b, size, [fx, fy], material.clone()));
            }
        }
    }
    let mut kept = false;
    for (entity, mark, mut material) in &mut draped {
        match &square {
            Some((b, size, _, m)) if !kept && mark.0 == *b && mark.1 == *size => {
                if material.0 != *m {
                    material.0 = m.clone();
                }
                kept = true;
            }
            _ => commands.entity(entity).despawn(),
        }
    }
    if !kept && let Some((b, size, facing, material)) = square {
        let mesh = overlay::draped_square(ground, sim.0.buildings[b].position, facing, size);
        commands.spawn((
            Mark,
            DrapedMark(b, size),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            DespawnOnExit(Screen::Match),
        ));
    }
    let mut wanted = wanted.into_iter();
    for (mut transform, mut material, mut visibility) in &mut shown {
        match wanted.next() {
            Some((t, m)) => {
                *transform = t;
                if material.0 != m {
                    material.0 = m;
                }
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
    for (transform, material) in wanted {
        commands.spawn((
            Mark,
            Mesh3d(marks.square.clone()),
            MeshMaterial3d(material),
            transform,
            DespawnOnExit(Screen::Match),
        ));
    }
}

/// The ground a building's template claims, which takes in its pad and
/// yard; the model's own box is about its walls.
fn claimed_size(building: &tgsim::Building, footprint: Option<&Footprint>) -> Vec2 {
    let claimed = match building.obstacle.as_ref().map(|o| o.shape) {
        Some(Shape::Rect { width, height }) => Some(Vec2::new(width, height)),
        Some(Shape::Circle { radius }) => Some(Vec2::splat(2.0 * radius)),
        _ => None,
    };
    claimed
        .or(footprint.map(|f| f.0))
        .unwrap_or(Vec2::splat(4.0))
}

/// Draws a health bar below each unit of the selected organizations and the one
/// under the cursor, and in front of the selected building and the one
/// under the cursor, whoever's they are: the width a bar has there but
/// square to the screen and over everything.
#[allow(clippy::too_many_arguments)]
pub(crate) fn show_health(
    mut commands: Commands,
    (selected, chosen, focus): (Res<Selected>, Res<SelectedBuilding>, Res<Focus>),
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    images: Res<OverlayImages>,
    fixed: Res<Time<Fixed>>,
    ui: Res<UiScale>,
    cameras: Query<(&Camera, &Transform), With<RtsCamera>>,
    units: Query<(&UnitView, Option<&Footprint>)>,
    buildings: Query<&BuildingView>,
    mut bars: Query<(&mut Node, &mut ImageNode, &mut Visibility), With<HealthBar>>,
) {
    let mut wanted = Vec::new();
    if let Ok((camera, transform)) = cameras.single() {
        // This frame's camera, which moves after transforms propagate.
        let global = GlobalTransform::from(*transform);
        let toward = (transform.back().as_vec3() * Vec3::new(1.0, 0.0, 1.0)).normalize_or_zero();
        let ground = &loaded.map.ground;
        let t = fixed.overstep_fraction();
        // Where on the ground each bar's middle goes, its size there, and
        // the frame it shows.
        let mut shown = Vec::new();
        let organization_shown =
            |r: usize| selected.0.contains(&r) || *focus == Focus::Organization(r);
        let building_shown = |b: usize| chosen.0 == Some(b) || *focus == Focus::Building(b);
        for (view, footprint) in &units {
            let unit = &sim.0.units[view.unit];
            if !organization_shown(unit.organization) || !unit.alive() {
                continue;
            }
            if unit.fighter.is_none() {
                continue;
            }
            let [x, y] = unit.drawn_at(t);
            // At the edge of its footprint nearest the camera, so the bar
            // clears the model.
            let reach = footprint.map_or(0.0, |f| f.0.length() / 2.0);
            let at = world(x, y, ground.stand_at(view.footing, [x, y])) + toward * reach;
            let image = images.health_bar(unit.health / sim.0.max_health(view.unit));
            shown.push((at, HEALTH_BAR, image, 0.0));
        }
        for view in &buildings {
            let b = view.building;
            let building = &sim.0.buildings[b];
            // A disabled one shows its empty bar.
            if !building_shown(b) || building.gone() {
                continue;
            }
            // On the ground at its middle, which from above reads as in
            // front of it.
            let [x, y] = building.position;
            let at = world(x, y, view.placed.y);
            let image = images.building_health_bar(
                building.health / (building.fighter.health * building.health_factor),
            );
            shown.push((at, BUILDING_HEALTH_BAR, image, 0.0));
            // A relay's supply stockpile, below its health.
            if let Some(supply) = sim
                .0
                .supply_of(b)
                .filter(|s| s.relay && s.stockpile_max > 0.0)
            {
                let share = building.supply.stockpile / supply.stockpile_max;
                let image = images.stockpile_bar(share);
                shown.push((at, BUILDING_HEALTH_BAR, image, 1.0));
            }
        }
        let half = transform.right().as_vec3() / 2.0;
        for (at, size, image, below) in shown {
            let Some(image) = image else {
                continue;
            };
            let (Ok(a), Ok(b)) = (
                camera.world_to_viewport(&global, at - half * size.x),
                camera.world_to_viewport(&global, at + half * size.x),
            ) else {
                continue;
            };
            let width = a.distance(b);
            let height = width * size.y / size.x;
            let centre = (a + b) / 2.0 + Vec2::new(0.0, height * (1.0 + 1.5 * below));
            let rect = Rect::from_center_size(centre, Vec2::new(width, height));
            if !overlaps_view(camera, rect) {
                continue;
            }
            wanted.push((within_view(camera, rect), image.clone()));
        }
    }
    let place = |node: &mut Node, r: Rect| place_on_map(node, r.min, r.size(), &ui);
    let mut wanted = wanted.into_iter();
    for (mut node, mut image, mut visibility) in &mut bars {
        match wanted.next() {
            Some((rect, frame)) => {
                place(&mut node, rect);
                if image.image != frame {
                    image.image = frame;
                }
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
    for (rect, frame) in wanted {
        let mut node = Node {
            position_type: PositionType::Absolute,
            ..default()
        };
        place(&mut node, rect);
        commands.spawn((
            HealthBar,
            node,
            OnMap,
            GlobalZIndex(ui::layer::HEALTH_BARS),
            ImageNode::new(frame).with_mode(NodeImageMode::Stretch),
            Pickable::IGNORE,
            DespawnOnExit(Screen::Match),
        ));
    }
}
