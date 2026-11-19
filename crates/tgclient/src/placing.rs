//! Placing a building the selection's to put up, chosen from its Build
//! menu: a click on the map orders it there, and pressing and dragging
//! before letting go turns it along the drag.

use bevy::prelude::*;

use crate::actions::{self, Page};
use crate::{Loaded, Local, Orders, Pointer, Pointing, Selected, Sim};

/// How far a drag has to go before it turns the building.
const TURNING: f32 = 1.0;

/// Where the building being placed was pressed down on, while the press
/// lasts.
#[derive(Resource, Default)]
pub struct Anchor(pub Option<[f32; 2]>);

/// What's being placed: its template, where and facing which way, and
/// whether it would go up there; `None` while nothing is, or the cursor's
/// off the map.
pub(crate) fn placed(
    page: Page,
    sim: &tgsim::World,
    selected: &[usize],
    player: Option<u32>,
    anchor: Option<[f32; 2]>,
    cursor: Option<[f32; 2]>,
) -> Option<(tgmap::Id, [f32; 2], [f32; 2], bool)> {
    let Page::Placing(i) = page else {
        return None;
    };
    let template = actions::builds(sim, selected).get(i)?.clone();
    let (at, facing) = match (anchor, cursor) {
        (Some(at), Some(to)) => (at, turned(at, to)),
        (Some(at), None) => (at, actions::UNPACK_FACING),
        (None, Some(at)) => (at, actions::UNPACK_FACING),
        (None, None) => return None,
    };
    let fits = player.is_some_and(|p| sim.room_to_build(p, &template, at, facing));
    Some((template, at, facing, fits))
}

/// The way a building pressed down on at `at` faces, dragged to `to`: along
/// the drag, or the default way without one.
fn turned(at: [f32; 2], to: [f32; 2]) -> [f32; 2] {
    let way = [to[0] - at[0], to[1] - at[1]];
    let length = way[0].hypot(way[1]);
    match length >= TURNING {
        true => [way[0] / length, way[1] / length],
        false => actions::UNPACK_FACING,
    }
}

/// Orders the building being placed where the press let go, if it would
/// go up there; a right click calls placing off.
#[allow(clippy::too_many_arguments)]
pub fn place(
    buttons: Res<ButtonInput<MouseButton>>,
    pointer: Pointer,
    loaded: Res<Loaded>,
    local: Res<Local>,
    selected: Res<Selected>,
    sim: Res<Sim>,
    mut orders: ResMut<Orders>,
    mut page: ResMut<Page>,
    mut anchor: ResMut<Anchor>,
    time: Res<Time>,
    mut sounds: ResMut<crate::sound::Sounds>,
) {
    if !matches!(*page, Page::Placing(_)) {
        anchor.0 = None;
        return;
    }
    if buttons.just_pressed(MouseButton::Right) {
        *page = Page::Orders;
        anchor.0 = None;
        return;
    }
    // A press let go over the interface, which this doesn't hear, is dropped.
    if !buttons.pressed(MouseButton::Left) && !buttons.just_released(MouseButton::Left) {
        anchor.0 = None;
    }
    let cursor = match pointer.at(&loaded.map.ground) {
        Some(Pointing::Ground(p)) => Some(p),
        _ => None,
    };
    if buttons.just_pressed(MouseButton::Left) {
        anchor.0 = cursor;
        return;
    }
    if !buttons.just_released(MouseButton::Left) {
        return;
    }
    let (Some(player), Some(at)) = (local.0, anchor.0.take()) else {
        return;
    };
    let Some((template, at, facing, true)) =
        placed(*page, &sim.0, &selected.0, Some(player), Some(at), cursor)
    else {
        return;
    };
    let Some(organization) = actions::builder(&sim.0, &selected.0) else {
        return;
    };
    let build = tgsim::Command::Build {
        organization,
        template,
        at,
        facing,
    };
    match orders.give(&sim.0, player, build) {
        Ok(()) => crate::acknowledge(&sim.0, &mut sounds, &time, player, &[organization]),
        Err(e) => debug!("can't build: {e:?}"),
    }
    *page = Page::Orders;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_turns_the_building_along_it_and_a_short_one_doesnt() {
        assert_eq!(turned([5.0, 5.0], [5.0, 9.0]), [0.0, 1.0]);
        assert_eq!(turned([5.0, 5.0], [2.0, 5.0]), [-1.0, 0.0]);
        assert_eq!(turned([5.0, 5.0], [5.5, 5.0]), actions::UNPACK_FACING);
    }
}
