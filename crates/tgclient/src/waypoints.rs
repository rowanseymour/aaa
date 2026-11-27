//! How orders show on the map: the order's cursor while it's being aimed,
//! a marker at the target of each order a selected regiment has, the one
//! it's carrying out and those waiting, numbered while any wait, and a
//! selected building's rally point.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow, SystemCursorIcon};
use tgdata::Id;
use tgrender::MapAssets;
use tgrender::camera::RtsCamera;
use tgrender::space::world;

use crate::actions::{self, Aim, Page};
use crate::pointing::Pointer;
use crate::{Drawn, Loaded, Screen, SelectedBuilding, Sim};

/// The effect marking a building's rally point.
const RALLY_MARKER: &str = "recruit_marker_rally_point";

/// A cursor's frames, which loop.
struct Frames {
    images: Vec<Handle<Image>>,
    hotspot: (u16, u16),
    /// Seconds one loop takes.
    period: f32,
}

/// How each order given at a place shows.
#[derive(Resource, Default)]
pub struct Looks {
    cursors: HashMap<Aim, Frames>,
    /// Those shown instead over where an order can't be given, for the
    /// orders that have one.
    invalid_cursors: HashMap<Aim, Frames>,
    rally_cursor: Option<Frames>,
    /// The archive path of each command's order marker's model, by the
    /// command's id.
    markers: HashMap<Id, String>,
    /// The archive path of the rally point marker's model.
    rally: Option<String>,
    /// The orders that send every organization to the very place pointed at,
    /// instead of each keeping its place relative to the rest.
    together: Vec<Aim>,
}

impl Looks {
    /// Whether `aim` sends every organization to the very place pointed at.
    pub fn together(&self, aim: Aim) -> bool {
        self.together.contains(&aim)
    }
}

/// The marker at the target of one of a selected regiment's orders: the
/// regiment, the order's place among them, and how it's marked.
#[derive(Component)]
pub struct OrderMarker {
    organization: usize,
    index: usize,
    command: Id,
    at: tgsim::Aimed,
    /// The number shown over it, while orders are waiting.
    number: Option<Entity>,
}

/// The marker on a building's rally point, by index into
/// [`tgsim::World::buildings`].
#[derive(Component)]
pub struct RallyMarker(usize, [f32; 2]);

/// The command each kind of aimed order is in the game's data.
fn command(aim: Aim) -> &'static str {
    match aim {
        Aim::Move => "move",
        Aim::Attack => "kill",
        Aim::Bombard => "bombard",
        Aim::Run => "retreat",
        Aim::Guard => "guard",
        Aim::Unpack => "unpack",
        Aim::Repair => "repair",
        Aim::Explore => "explore",
    }
}

pub fn load(
    mut commands: Commands,
    loaded: Res<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &loaded.game;
    let by_id: HashMap<Id, tgdata::Command> = game
        .commands()
        .into_iter()
        .map(|c| (c.ids.clone(), c))
        .collect();
    let cursors = game.cursors();
    let models: HashMap<Id, String> = (game.vfx().iter())
        .filter_map(|(id, v)| Some((id.clone(), v.model.clone()?)))
        .collect();
    let mut looks = Looks {
        rally: models.get(&Id::from(RALLY_MARKER)).cloned(),
        ..default()
    };
    let mut frames_of = |cursor: Option<&Id>| {
        let cursor = cursor.and_then(|c| cursors.get(c))?;
        let frames: Vec<Handle<Image>> = cursor
            .frames
            .iter()
            .filter_map(|f| {
                let mut image = crate::ui::read_image(game, f, formats.0)?;
                // The window reads the cursor's pixels, so they stay here too.
                image.asset_usage = RenderAssetUsages::all();
                Some(images.add(image))
            })
            .collect();
        let [x, y] = cursor.hotspot;
        (!frames.is_empty() && frames.len() == cursor.frames.len()).then_some(Frames {
            images: frames,
            hotspot: (x, y),
            period: cursor.period,
        })
    };
    looks.rally_cursor = by_id
        .get(&Id::from(actions::RALLY))
        .and_then(|c| frames_of(c.cursor.as_ref()));
    for aim in Aim::ALL {
        let Some(c) = by_id.get(&Id::from(command(aim))) else {
            continue;
        };
        if !c.group_offset {
            looks.together.push(aim);
        }
        if let Some(frames) = frames_of(c.cursor.as_ref()) {
            looks.cursors.insert(aim, frames);
        }
        if let Some(frames) = frames_of(c.invalid_cursor.as_ref()) {
            looks.invalid_cursors.insert(aim, frames);
        }
    }
    for (id, c) in &by_id {
        if let Some(model) = c.marker.as_ref().and_then(|m| models.get(m)) {
            looks.markers.insert(id.clone(), model.clone());
        }
    }
    commands.insert_resource(looks);
}

/// Shows the aimed order's cursor over the map and the minimap's diamond,
/// the attack cursor for an armed special operation, and the usual one
/// elsewhere. Bombard shows its invalid cursor over a place none of those
/// it's for can bombard; no cursor tests whether a place can be reached.
#[allow(clippy::too_many_arguments)]
pub fn cursor(
    mut commands: Commands,
    (page, armed): (Res<Page>, Res<crate::special_ops::Armed>),
    looks: Res<Looks>,
    time: Res<Time>,
    pointer: Pointer,
    loaded: Res<Loaded>,
    (sim, selected, aimed_for): (Res<Sim>, Res<crate::Selected>, Res<actions::AimedFor>),
    window: Single<(Entity, Option<&CursorIcon>), With<PrimaryWindow>>,
) {
    let pointed = pointer.at(&loaded.map.ground);
    let over_map = pointed.is_some();
    let out_of_range = |aim: Aim| {
        let Some(at) = pointed.map(crate::pointing::Pointing::point) else {
            return false;
        };
        let given = match aimed_for.0 {
            Some(r) => vec![r],
            None => selected.0.clone(),
        };
        aim == Aim::Bombard && !given.iter().any(|&r| sim.0.bombards_place(r, at))
    };
    let frames = match *page {
        Page::Aiming(aim) if over_map && out_of_range(aim) => {
            (looks.invalid_cursors.get(&aim)).or_else(|| looks.cursors.get(&aim))
        }
        Page::Aiming(aim) if over_map => looks.cursors.get(&aim),
        Page::Rally if over_map => looks.rally_cursor.as_ref(),
        Page::AirAttack if over_map => looks.cursors.get(&Aim::Attack),
        Page::AirRecon if over_map => looks.cursors.get(&Aim::Explore),
        Page::Delivering(_) if over_map => looks.cursors.get(&Aim::Move),
        _ if over_map && armed.0.is_some() => looks.cursors.get(&Aim::Attack),
        _ => None,
    };
    let icon = match frames {
        Some(f) => {
            let n = f.images.len();
            let i = ((time.elapsed_secs() / f.period).fract() * n as f32) as usize;
            CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
                handle: f.images[i.min(n - 1)].clone(),
                hotspot: f.hotspot,
                ..default()
            }))
        }
        None => CursorIcon::System(SystemCursorIcon::Default),
    };
    let (entity, current) = *window;
    if current != Some(&icon) {
        commands.entity(entity).insert(icon);
    }
}

/// How high over a thing its order's marker sits.
const OVER_THING: f32 = 4.0;

/// The game's camera, for placing numbers over markers.
type Viewing<'w, 's> =
    Query<'w, 's, (&'static Camera, &'static Transform), (With<RtsCamera>, Without<OrderMarker>)>;

/// A marker one of the selected regiments' orders wants.
struct Wanted {
    organization: usize,
    index: usize,
    command: Id,
    at: tgsim::Aimed,
    /// Where it shows.
    shown: Vec3,
    number: Option<usize>,
}

impl Wanted {
    fn is(&self, marker: &OrderMarker) -> bool {
        (self.organization, self.index, &self.command, self.at)
            == (
                marker.organization,
                marker.index,
                &marker.command,
                marker.at,
            )
    }
}

/// Keeps a marker at the target of each order the selected regiments of
/// the player's own or allies' have, each numbered in the order they'll be
/// carried out while any are waiting. An order aimed at an enemy shows the
/// attack marker; one at a thing that can move and isn't seen has none.
#[allow(clippy::too_many_arguments)]
pub fn mark(
    mut commands: Commands,
    (sim, fog, local, selected): (
        Res<Sim>,
        Res<crate::fog::Fog>,
        Res<crate::Local>,
        Res<crate::Selected>,
    ),
    looks: Res<Looks>,
    mut markers: Query<(Entity, &mut OrderMarker, &mut Transform)>,
    mut numbers: Query<(&mut Node, &mut Visibility), With<MarkerNumber>>,
    cameras: Viewing,
    ui: Res<UiScale>,
    mut drawn: ResMut<Drawn>,
    loaded: Res<Loaded>,
    mut assets: MapAssets,
) {
    let sim = &sim.0;
    let ground = &loaded.map.ground;
    let on_ground = |[x, y]: [f32; 2], over: f32| world(x, y, ground.height_at([x, y]) + over);
    let mut wanted: Vec<Wanted> = Vec::new();
    for &r in &selected.0 {
        let owner = sim.organizations[r].owner;
        let friendly = local.0.is_some_and(|p| {
            matches!(
                sim.sides.relation(p, owner),
                tgsim::Relation::Itself | tgsim::Relation::Ally
            )
        });
        if !friendly {
            continue;
        }
        let orders = sim.marked_orders(r);
        let numbered = orders.len() > 1;
        for (i, order) in orders.into_iter().enumerate() {
            let enemy = |owner| {
                sim.sides.relation(owner, sim.organizations[r].owner) == tgsim::Relation::Enemy
            };
            let (command, shown) = match order.at {
                tgsim::Aimed::Place(p) => (order.command, on_ground(p, 0.0)),
                tgsim::Aimed::Thing(tgsim::Piece::Organization(o)) => {
                    if !fog.shows_organization(sim, o) || sim.destroyed(o) {
                        continue;
                    }
                    let command = match enemy(sim.organizations[o].owner) {
                        true => "kill",
                        false => order.command,
                    };
                    (command, on_ground(sim.centre(o), OVER_THING))
                }
                tgsim::Aimed::Thing(tgsim::Piece::Building(b)) => {
                    let building = &sim.buildings[b];
                    if fog.building(b) == crate::fog::Look::Hidden || building.gone() {
                        continue;
                    }
                    let command = match enemy(building.owner) {
                        true => "kill",
                        false => order.command,
                    };
                    (command, on_ground(building.position, OVER_THING))
                }
            };
            wanted.push(Wanted {
                organization: r,
                index: i,
                command: Id::from(command),
                at: order.at,
                shown,
                number: numbered.then_some(i + 1),
            });
        }
    }
    let camera = cameras.single().ok();
    let mut kept = vec![false; wanted.len()];
    for (e, mut marker, mut transform) in &mut markers {
        let found = (wanted.iter().enumerate()).position(|(k, w)| !kept[k] && w.is(&marker));
        let Some(k) = found else {
            if let Some(n) = marker.number {
                commands.entity(n).despawn();
            }
            commands.entity(e).despawn();
            continue;
        };
        kept[k] = true;
        let Wanted { shown, number, .. } = &wanted[k];
        transform.translation = *shown;
        let label = marker.number.and_then(|n| numbers.get_mut(n).ok());
        match (number, label) {
            (Some(_), Some((mut node, mut visibility))) => {
                let over = *shown + Vec3::Y * NUMBER_RAISE;
                let shown = camera
                    .and_then(|(c, t)| c.world_to_viewport(&GlobalTransform::from(*t), over).ok());
                match shown {
                    Some(p) => {
                        crate::ui_scale::place_on_map(
                            &mut node,
                            p - NUMBER_SIZE / 2.0,
                            NUMBER_SIZE,
                            &ui,
                        );
                        visibility.set_if_neq(Visibility::Inherited);
                    }
                    None => {
                        visibility.set_if_neq(Visibility::Hidden);
                    }
                }
            }
            (Some(_), None) => {
                let n = commands.spawn(number_label(number.unwrap_or(0))).id();
                marker.number = Some(n);
            }
            (None, Some(_)) => {
                if let Some(n) = marker.number.take() {
                    commands.entity(n).despawn();
                }
            }
            (None, None) => {}
        }
    }
    for (k, w) in wanted.into_iter().enumerate() {
        if kept[k] {
            continue;
        }
        let Some(model) = looks.markers.get(&w.command) else {
            continue;
        };
        let mut models = assets.models();
        let spawned = drawn.0.spawn_effect(
            &mut commands,
            &mut models,
            model,
            &[],
            Transform::from_translation(w.shown),
        );
        if let Some(e) = spawned {
            let number = w.number.map(|n| commands.spawn(number_label(n)).id());
            commands.entity(e).insert((
                OrderMarker {
                    organization: w.organization,
                    index: w.index,
                    command: w.command,
                    at: w.at,
                    number,
                },
                DespawnOnExit(Screen::Match),
            ));
        }
    }
}

/// How far over its marker's foot an order's number shows: the stock
/// markers' height, about that of a flag, and `UIWaypointNumberZOffset`
/// over that.
const NUMBER_RAISE: f32 = 3.5 + 1.0;

/// The room an order's number takes on screen, in pixels.
const NUMBER_SIZE: Vec2 = Vec2::new(16.0, 16.0);

/// The number over an order's marker.
#[derive(Component)]
pub struct MarkerNumber;

/// An order's number `n`, placed over its marker once that's seen.
fn number_label(n: usize) -> impl Bundle {
    (
        MarkerNumber,
        Text::new(n.to_string()),
        TextLayout::new(Justify::Center, LineBreak::NoWrap),
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        Visibility::Hidden,
        DespawnOnExit(Screen::Match),
    )
}

/// Keeps a marker on the selected building's rally point, if it has one.
#[allow(clippy::too_many_arguments)]
pub fn rally(
    mut commands: Commands,
    sim: Res<Sim>,
    chosen: Res<SelectedBuilding>,
    local: Res<crate::Local>,
    looks: Res<Looks>,
    markers: Query<(Entity, &RallyMarker)>,
    mut drawn: ResMut<Drawn>,
    loaded: Res<Loaded>,
    mut assets: MapAssets,
) {
    // Only the player sees where their own send what they raise.
    let wanted = (chosen.0)
        .filter(|&b| Some(sim.0.buildings[b].owner) == local.0)
        .and_then(|b| Some((b, sim.0.buildings[b].rally?)));
    let mut kept = false;
    for (e, m) in &markers {
        match Some((m.0, m.1)) == wanted && !kept {
            true => kept = true,
            false => commands.entity(e).despawn(),
        }
    }
    let (Some((b, at)), Some(model), false) = (wanted, &looks.rally, kept) else {
        return;
    };
    let [x, y] = at;
    let at = world(x, y, loaded.map.ground.height_at([x, y]));
    let mut models = assets.models();
    let spawned = drawn.0.spawn_effect(
        &mut commands,
        &mut models,
        model,
        &[],
        Transform::from_translation(at),
    );
    if let Some(e) = spawned {
        commands
            .entity(e)
            .insert((RallyMarker(b, [x, y]), DespawnOnExit(Screen::Match)));
    }
}
