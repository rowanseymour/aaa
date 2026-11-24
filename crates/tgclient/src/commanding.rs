//! Selecting what's yours and sending it where the player clicks.

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::window::PrimaryWindow;
use tgrender::camera::RtsCamera;
use tgrender::space::world;
use tgsim::World;

use crate::marks::SelectionBox;
use crate::orders::Orders;
use crate::pointing::GroundCursor;
use crate::ui_scale::place_on_map;
use crate::{
    Drawn, Focus, Loaded, Local, Selected, SelectedBuilding, Sim, actions, banners, control_groups,
    fog, minimap, selection, sound, ui, waypoints,
};

/// Plays the callout of an organization of the player's newly added to the
/// selection: others' are selected in silence.
pub(crate) fn selection_sounds(
    selected: Res<Selected>,
    sim: Res<Sim>,
    local: Res<Local>,
    time: Res<Time>,
    mut sounds: ResMut<sound::Sounds>,
    mut before: bevy::prelude::Local<Vec<usize>>,
) {
    if !selected.is_changed() {
        return;
    }
    let fresh = selected.0.iter().find(|r| !before.contains(r));
    if let Some(&r) = fresh.filter(|&&r| Some(sim.0.organizations[r].owner) == local.0) {
        let organization = &sim.0.organizations[r];
        let at = sim.0.centre(r);
        sounds.reply(
            &time,
            organization.template.as_str(),
            |a| &a.selected,
            at,
            organization.owner,
        );
    }
    before.clone_from(&selected.0);
}

/// Lets go of what the player can no longer see of others' things, and of
/// a building that's fallen or packed up. Whose it is doesn't matter: a
/// building captured while selected stays selected.
pub(crate) fn keep_selection(
    sim: Res<Sim>,
    fog: Res<fog::Fog>,
    mut selected: ResMut<Selected>,
    mut chosen: ResMut<SelectedBuilding>,
) {
    let sim = &sim.0;
    // The game keeps a building that drops into the fog as its remembered
    // picture, but the display would show it as it is now, so it goes.
    if let Some(b) = chosen.0
        && (!sim.buildings[b].standing() || fog.building(b) != fog::Look::Shown)
    {
        chosen.0 = None;
    }
    if selected.0.iter().any(|&r| !fog.shows_organization(sim, r)) {
        selected.0.retain(|&r| fog.shows_organization(sim, r));
    }
}

/// Left click selects what's under the cursor, whoever's it is, and
/// dragging what the box covers, by [`selection`]'s rules; with Shift or
/// Ctrl held, a click adds to the selection or takes back out what's in it,
/// and a box adds to it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn select(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &RtsCamera)>,
    minimap: Query<&RelativeCursorPosition, With<minimap::MinimapView>>,
    panels: ui::Panels,
    (loaded, drawn): (Res<Loaded>, Res<Drawn>),
    local: Res<Local>,
    focus: Res<Focus>,
    (sim, scenery, fog): (Res<Sim>, Res<banners::Scenery>, Res<fog::Fog>),
    mut selected: ResMut<Selected>,
    mut chosen: ResMut<SelectedBuilding>,
    mut start: bevy::prelude::Local<Option<Vec2>>,
    ui: Res<UiScale>,
    mut shown: Single<(&mut Node, &mut Visibility), With<SelectionBox>>,
) {
    // How far the cursor goes before a click becomes a drag, in pixels.
    const DRAG: f32 = 4.0;
    let at = windows.single().ok().and_then(|w| w.cursor_position());
    let view = (cameras.single().ok()).and_then(|(c, ..)| c.logical_viewport_rect());
    // The view may be cut narrower than the window: a drag starts in it and
    // stays within it.
    let cursor = at.map(|at| view.map_or(at, |v| at.clamp(v.min, v.max)));
    if buttons.just_pressed(MouseButton::Left)
        && minimap::pointer_away(minimap)
        && ui::pointer_away(panels)
        && at.zip(view).is_none_or(|(at, v)| v.contains(at))
    {
        *start = cursor;
    }
    let released = buttons.just_released(MouseButton::Left);
    // Presses can end unseen, such as while the console is open.
    if !released && !buttons.pressed(MouseButton::Left) {
        *start = None;
    }
    let dragged = start
        .zip(cursor)
        .map(|(a, b)| Rect::from_corners(a, b))
        .filter(|r| r.size().max_element() > DRAG);
    let (node, visibility) = &mut *shown;
    match dragged.filter(|_| !released) {
        Some(r) => {
            place_on_map(node, r.min, r.size(), &ui);
            visibility.set_if_neq(Visibility::Inherited);
        }
        None => {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
    if !released || start.take().is_none() {
        return;
    }
    let sim = &sim.0;
    let rank = |t| selection::rank(sim, local.0, t);
    let adding = queuing(&keys);
    let mut selection = selection::Selection {
        organizations: &mut selected.0,
        building: &mut chosen.0,
    };
    let Some(rect) = dragged else {
        let thing = match *focus {
            Focus::Organization(r) if !sim.destroyed(r) && !scenery.has(sim, r) => {
                Some(selection::Thing::Organization(r))
            }
            Focus::Building(b) if sim.buildings[b].standing() => {
                Some(selection::picked(sim, selection::Thing::Building(b)))
            }
            _ => None,
        };
        match (thing, adding) {
            (Some(thing), true) => selection.toggle(rank, thing),
            (Some(thing), false) => selection.replace(thing),
            (None, true) => {}
            (None, false) => selection.clear(),
        }
        return;
    };
    let Ok((camera, transform, _)) = cameras.single() else {
        return;
    };
    let ground = &loaded.map.ground;
    let drawn_at = |at: [f32; 2], z: f32| {
        camera
            .world_to_viewport(transform, world(at[0], at[1], z))
            .ok()
    };
    let unit_at = |u: usize| {
        let unit = &sim.units[u];
        let z = ground.stand_at(drawn.0.footing(unit.template.as_str()), unit.position);
        drawn_at(unit.position, z)
    };
    let mut inside: Vec<(selection::Thing, Vec2)> = (0..sim.organizations.len())
        .filter(|&r| !scenery.has(sim, r))
        .filter_map(|r| {
            let at = (sim.survivors(r))
                .filter(|&u| fog.shows_unit(u))
                .filter_map(unit_at)
                .find(|&p| rect.contains(p))?;
            Some((selection::Thing::Organization(r), at))
        })
        .collect();
    for (b, building) in sim.buildings.iter().enumerate() {
        let seen = building.standing() && fog.building(b) == fog::Look::Shown;
        let at = drawn_at(building.position, ground.height_at(building.position));
        let Some(at) = at.filter(|&p| seen && rect.contains(p)) else {
            continue;
        };
        // A city once, however many of its buildings the box covers.
        let thing = selection::picked(sim, selection::Thing::Building(b));
        if !inside.iter().any(|&(t, _)| t == thing) {
            inside.push((thing, at));
        }
    }
    if !adding {
        selection.clear();
    }
    // Nearest the middle of what's selected, else of the box.
    let held: Vec<Vec2> = (selection.organizations.iter())
        .filter_map(|&r| sim.survivors(r).find_map(unit_at))
        .collect();
    let middle = match held.len() {
        0 => rect.center(),
        n => held.iter().sum::<Vec2>() / n as f32,
    };
    selection.boxed(rank, inside, middle);
}

/// Whether Shift or Ctrl is held: a click on the map then adds to the
/// selection, or queues the order it gives.
pub(crate) fn queuing(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ])
}

/// Right click sends the selected organizations where it points, each keeping
/// its place relative to the rest, or after the enemy organization it's on,
/// those that can't fight it going there instead; with Shift or Ctrl held,
/// after what they've been told already. With a building that
/// commissions selected, it sets its rally point there.
#[allow(clippy::too_many_arguments)]
pub(crate) fn command(
    (buttons, keys): (Res<ButtonInput<MouseButton>>, Res<ButtonInput<KeyCode>>),
    cursor: GroundCursor,
    loaded: Res<Loaded>,
    local: Res<Local>,
    focus: Res<Focus>,
    looks: Res<waypoints::Looks>,
    sim: Res<Sim>,
    mut orders: ResMut<Orders>,
    (selected, chosen): (Res<Selected>, Res<SelectedBuilding>),
    groups: Res<control_groups::ControlGroups>,
    time: Res<Time>,
    mut sounds: ResMut<sound::Sounds>,
) {
    let Some(player) = local.0 else {
        return;
    };
    // Others' things take no orders.
    if !buttons.just_pressed(MouseButton::Right)
        || !selection::commanded(&sim.0, local.0, &selected.0, chosen.0)
    {
        return;
    }
    let ground = &loaded.map.ground;
    let Some(point) = cursor.at(ground) else {
        return;
    };
    if selected.0.is_empty() {
        // Buildings that don't commission have no rally point to set.
        if let Some(b) = chosen.0 {
            let rally = tgsim::Command::Rally {
                building: b,
                at: Some(point),
            };
            let _ = orders.give(&sim.0, player, rally);
        }
        return;
    }
    acknowledge(&sim.0, &mut sounds, &time, player, &selected.0);
    orders.queue(queuing(&keys));
    // With Group Behavior on, what each is told goes as one group.
    if groups.grouping(&selected.0) {
        orders.gather(player);
    }
    let mut moving = selected.0.clone();
    if let Some(e) = enemy_organization(&sim.0, player, *focus) {
        let (_, rest) = pursue(&sim.0, &mut orders, player, &selected.0, e);
        moving = rest;
    }
    // What can't be attacked there is repaired, by those that can, or
    // else moved onto.
    if let Some(piece) = onto(&sim.0, player, *focus) {
        let (_, rest) = give_each(&sim.0, &mut orders, player, &moving, point, |r| {
            tgsim::Command::Repair {
                organization: r,
                piece,
            }
        });
        let (_, rest) = approach(&sim.0, &mut orders, player, &rest, piece);
        moving = rest;
    }
    send(
        &sim.0,
        &mut orders,
        player,
        &moving,
        point,
        (actions::Aim::Move, looks.together(actions::Aim::Move)),
        ground.size(),
    );
}

/// The enemy organization of `player`'s that `focus` is on, if it's one.
fn enemy_organization(sim: &World, player: u32, focus: Focus) -> Option<usize> {
    let Focus::Organization(e) = focus else {
        return None;
    };
    let owner = sim.organizations[e].owner;
    (sim.sides.relation(player, owner) == tgsim::Relation::Enemy).then_some(e)
}

/// What at `focus` organizations of `player`'s are moved onto, rather than
/// to its place: any organization, and a building that isn't an enemy's.
fn onto(sim: &World, player: u32, focus: Focus) -> Option<tgsim::Piece> {
    focus.piece().filter(|&p| match p {
        tgsim::Piece::Organization(_) => true,
        tgsim::Piece::Building(_) => {
            sim.sides.relation(player, sim.piece_owner(p)) != tgsim::Relation::Enemy
        }
    })
}

/// Moves organizations onto `piece`. Returns those that went, each with
/// where `piece` is, and those that can't.
fn approach(
    sim: &World,
    orders: &mut Orders,
    player: u32,
    organizations: &[usize],
    piece: tgsim::Piece,
) -> (Vec<(usize, [f32; 2])>, Vec<usize>) {
    let there = match piece {
        tgsim::Piece::Organization(e) => sim.centre(e),
        tgsim::Piece::Building(b) => sim.buildings[b].position,
    };
    give_each(sim, orders, player, organizations, there, |r| {
        tgsim::Command::MoveOnto {
            organization: r,
            piece,
        }
    })
}

/// Sends organizations after the enemy organization `e`. Returns those that went,
/// each with where the enemy is, and those that can't.
pub(crate) fn pursue(
    sim: &World,
    orders: &mut Orders,
    player: u32,
    organizations: &[usize],
    e: usize,
) -> (Vec<(usize, [f32; 2])>, Vec<usize>) {
    // What has only bombarding weapons has no Attack, and bombards instead.
    give_each(
        sim,
        orders,
        player,
        organizations,
        sim.centre(e),
        |r| match sim.only_bombards(r) {
            true => tgsim::Command::Bombard {
                organization: r,
                target: tgsim::Shelled::Thing(tgsim::Piece::Organization(e)),
            },
            false => tgsim::Command::Pursue {
                organization: r,
                enemy: e,
            },
        },
    )
}

/// Gives each organization the command `command` makes for it. Returns
/// those that took it, each with `there`, and those that didn't.
pub(crate) fn give_each(
    sim: &World,
    orders: &mut Orders,
    player: u32,
    organizations: &[usize],
    there: [f32; 2],
    command: impl Fn(usize) -> tgsim::Command,
) -> (Vec<(usize, [f32; 2])>, Vec<usize>) {
    let (went, rest): (Vec<usize>, Vec<usize>) =
        (organizations.iter()).partition(|&&r| orders.give(sim, player, command(r)).is_ok());
    (went.into_iter().map(|r| (r, there)).collect(), rest)
}

/// Plays the first of the organizations answering an order.
pub(crate) fn acknowledge(
    sim: &World,
    sounds: &mut sound::Sounds,
    time: &Time,
    player: u32,
    organizations: &[usize],
) {
    if let Some(&first) = organizations.first() {
        let template = &sim.organizations[first].template;
        sounds.reply(
            time,
            template.as_str(),
            |a| &a.commanded,
            sim.centre(first),
            player,
        );
    }
}

/// Where each of `organizations` goes for an order at `[px, py]` on a map
/// `size` across: the point plus its own offset from their centre, so they
/// keep their places relative to each other.
pub(crate) fn spread(
    sim: &World,
    organizations: &[usize],
    [px, py]: [f32; 2],
    size: f32,
) -> Vec<[f32; 2]> {
    let centres: Vec<[f32; 2]> = organizations.iter().map(|&r| sim.centre(r)).collect();
    let n = centres.len() as f32;
    let [cx, cy] = centres
        .iter()
        .fold([0.0, 0.0], |[x, y], [a, b]| [x + a / n, y + b / n]);
    (centres.into_iter())
        .map(|[x, y]| {
            [
                (px + x - cx).clamp(0.0, size),
                (py + y - cy).clamp(0.0, size),
            ]
        })
        .collect()
}

/// Gives organizations an order at a point, each keeping its place relative to
/// the rest within a map `size` across, unless they all go `together` to
/// the point itself. Returns those that went, each with where to.
pub(crate) fn send(
    sim: &World,
    orders: &mut Orders,
    player: u32,
    organizations: &[usize],
    [px, py]: [f32; 2],
    (aim, together): (actions::Aim, bool),
    size: f32,
) -> Vec<(usize, [f32; 2])> {
    let targets = match together {
        true => vec![[px, py]; organizations.len()],
        false => spread(sim, organizations, [px, py], size),
    };
    let mut sent = Vec::new();
    for (&r, target) in organizations.iter().zip(targets) {
        let (organization, to) = (r, target);
        let command = match aim {
            actions::Aim::Move => tgsim::Command::Move { organization, to },
            actions::Aim::Attack => tgsim::Command::Attack { organization, to },
            actions::Aim::Bombard => tgsim::Command::Bombard {
                organization,
                target: tgsim::Shelled::Place(to),
            },
            actions::Aim::Run => tgsim::Command::Run { organization, to },
            actions::Aim::Guard => tgsim::Command::Guard { organization, to },
            actions::Aim::Explore => tgsim::Command::Explore { organization, to },
            actions::Aim::Unpack => tgsim::Command::Unpack {
                organization,
                at: to,
                facing: actions::UNPACK_FACING,
            },
            // Given on a thing.
            actions::Aim::Repair => continue,
        };
        if orders.give(sim, player, command).is_ok() {
            sent.push((r, target));
        }
    }
    sent
}

pub(crate) fn choose_formation(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Res<Selected>,
    local: Res<Local>,
    sim: Res<Sim>,
    mut orders: ResMut<Orders>,
) {
    const KEYS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    // The digits alone are the control groups'.
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let (Some(f), Some(player), true) = (
        KEYS.iter().position(|&k| keys.just_pressed(k)),
        local.0,
        alt,
    ) else {
        return;
    };
    for &r in &selected.0 {
        let organization = &sim.0.organizations[r];
        let Some(formation) = organization.formations.get(f) else {
            continue;
        };
        let command = tgsim::Command::Formation {
            organization: r,
            formation: f,
        };
        if orders.give(&sim.0, player, command).is_ok() {
            debug!("{} to formation {}", organization.template, formation.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organizations_spread_round_a_point_as_they_stand_round_their_centre() {
        let sim = crate::orders::testing::fresh();
        // At 10,10 and 10,20.
        let spots = spread(&sim, &[0, 1], [30.0, 30.0], 100.0);
        assert_eq!(spots, [[30.0, 25.0], [30.0, 35.0]]);
        // Kept on the map.
        let spots = spread(&sim, &[0, 1], [30.0, 2.0], 100.0);
        assert_eq!(spots, [[30.0, 0.0], [30.0, 7.0]]);
    }

    #[test]
    fn a_right_click_moves_onto_any_organization_and_buildings_not_an_enemys() {
        let mut sim = crate::orders::testing::fresh();
        let house = |owner| tgmap::Building {
            id: 9,
            template: "house".into(),
            owner,
            position: [20.0, 20.0],
            facing: [0.0, -1.0],
            fighter: Default::default(),
            health: 1.0,
            zone: None,
            members: Vec::new(),
            obstacle: None,
            deploy: None,
            organization: None,
            awake: false,
        };
        sim.add_buildings(&[house(1), house(2)]);
        let organization = tgsim::Piece::Organization;
        assert_eq!(onto(&sim, 1, Focus::Organization(0)), Some(organization(0)));
        assert_eq!(onto(&sim, 1, Focus::Organization(1)), Some(organization(1)));
        let own = Focus::Building(0);
        assert_eq!(onto(&sim, 1, own), Some(tgsim::Piece::Building(0)));
        // An enemy's building is attacked where it stands instead.
        assert_eq!(onto(&sim, 1, Focus::Building(1)), None);
        assert_eq!(onto(&sim, 1, Focus::Nothing), None);
    }
}
