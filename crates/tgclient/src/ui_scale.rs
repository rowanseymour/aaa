//! How big the interface is drawn, and placing it over the view of the
//! map rather than the whole window.

use crate::hud::modern;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use tgrender::camera::RtsCamera;
use tgrender::shot;

use crate::{Screen, connection, hud, pause};

/// A node drawn over the map, which [`target_map`] has the map's camera
/// draw: cut to its view, and under any panels over it.
#[derive(Component)]
pub struct OnMap;

/// Has the map's camera draw each [`OnMap`] node, once there is one.
pub(crate) fn target_map(
    mut commands: Commands,
    nodes: Query<Entity, (With<OnMap>, Without<UiTargetCamera>)>,
    cameras: Query<Entity, With<RtsCamera>>,
) {
    let Ok(map) = cameras.single() else {
        return;
    };
    for node in &nodes {
        commands.entity(node).insert(UiTargetCamera(map));
    }
}

/// What [`UiScaleSetting`] may be: smaller for a small window, bigger to
/// suit a large one.
pub(crate) const UI_SCALES: std::ops::RangeInclusive<f32> = 0.5..=2.0;
/// Between the screen's edges and the minimap, object display and action
/// grid along its bottom, in logical pixels.
pub(crate) const MARGIN: f32 = 12.0;

/// How big the player wants the interface drawn; [`UiScale`] is this, less
/// as a narrow window needs to fit the bottom of the screen in.
#[derive(Resource)]
pub(crate) struct UiScaleSetting(pub(crate) f32);

/// How wide our interface's row along the top of the window is at its
/// widest, in logical pixels, for the match being played with it.
#[derive(Resource)]
pub(crate) struct TopRow(pub(crate) f32);

/// How much of the window's height the bottom of the screen may take.
const UI_HEIGHT_SHARE: f32 = 1.0 / 3.0;

/// Sets [`UiScale`] from [`UiScaleSetting`], shrunk until our object
/// display, centred, clears the minimap and action grid either side of it,
/// the [`TopRow`] fits across, and they take no more than
/// [`UI_HEIGHT_SHARE`] of the window's height. A match played with the
/// original's interface takes no setting, as the original has none: what's
/// drawn over it scales with its layouts' screen, as its own panels do. The
/// pause menu and the connection dialog over it keep the setting, as menus
/// aren't the interface's.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fit_ui(
    setting: Res<UiScaleSetting>,
    windows: Query<Ref<Window>, With<PrimaryWindow>>,
    screen: Option<Res<bevy::state::state::State<Screen>>>,
    hud: Option<Res<bevy::state::state::State<hud::InMatch>>>,
    menu: Option<Res<pause::PauseMenu>>,
    dialog: Option<Res<connection::Dialog>>,
    top: Option<Res<TopRow>>,
    frame: Option<Res<shot::Frame>>,
    mut scale: ResMut<UiScale>,
) {
    let window = windows.single().ok();
    let changed = setting.is_changed()
        || window.as_ref().is_some_and(|w| w.is_changed())
        || screen.as_ref().is_some_and(|s| s.is_changed())
        || hud.as_ref().is_some_and(|h| h.is_changed())
        || menu.as_ref().is_some_and(|m| m.is_changed())
        || dialog.as_ref().is_some_and(|d| d.is_changed())
        || top.as_ref().is_some_and(|t| t.is_changed());
    if !changed {
        return;
    }
    // Without a window, a screenshot's image.
    let frame = frame.map_or(shot::SIZE, |f| f.0).as_vec2();
    let size = window.map_or(frame, |w| w.size());
    if size.min_element() <= 0.0 {
        return;
    }
    let menu_up = menu.is_some_and(|m| m.is_open()) || dialog.is_some_and(|d| d.is_open());
    let original = hud.is_some_and(|h| h.0 == hud::Hud::Original) && !menu_up;
    let wanted = match original {
        true => hud::original::layout_scale(size),
        false => fitted(setting.0, size, top.map(|t| t.0)),
    };
    if scale.0 != wanted {
        scale.0 = wanted;
    }
}

/// `setting`, shrunk as [`fit_ui`] says for a window `size` across, and
/// for the `top` row as wide as that, if there is one.
pub(crate) fn fitted(setting: f32, size: Vec2, top: Option<f32>) -> f32 {
    let side = MARGIN + modern::minimap::DISC.max(modern::actions::SIZE.x) + MARGIN;
    let wide = size.x / (2.0 * side + modern::object_display::SIZE.x);
    let wide = top.map_or(wide, |top| wide.min(size.x / top));
    let tallest = modern::minimap::DISC
        .max(modern::actions::SIZE.y)
        .max(modern::object_display::SIZE.y);
    let tall = size.y * UI_HEIGHT_SHARE / (MARGIN + tallest);
    setting.min(wide).min(tall).max(*UI_SCALES.start())
}

/// Whether any of `rect`, in viewport pixels, is within what `camera`
/// shows.
pub(crate) fn overlaps_view(camera: &Camera, rect: Rect) -> bool {
    camera
        .logical_viewport_rect()
        .is_some_and(|r| !r.intersect(rect).is_empty())
}

/// Where `rect`, in viewport pixels, falls among [`OnMap`] nodes: the map's
/// camera lays out what it draws from the corner of its view.
pub(crate) fn within_view(camera: &Camera, rect: Rect) -> Rect {
    let corner = camera.logical_viewport_rect().map_or(Vec2::ZERO, |r| r.min);
    Rect::from_corners(rect.min - corner, rect.max - corner)
}

/// Places an [`OnMap`] node at a rectangle in viewport pixels.
pub(crate) fn place_in_view(node: &mut Node, camera: &Camera, rect: Rect, ui: &UiScale) {
    let rect = within_view(camera, rect);
    place_on_map(node, rect.min, rect.size(), ui);
}

/// Places a node over the map at a rectangle in viewport pixels, which
/// [`UiScale`] mustn't scale.
pub(crate) fn place_on_map(node: &mut Node, corner: Vec2, size: Vec2, ui: &UiScale) {
    node.left = px(corner.x / ui.0);
    node.top = px(corner.y / ui.0);
    node.width = px(size.x / ui.0);
    node.height = px(size.y / ui.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_interface_shrinks_for_whichever_row_is_wider() {
        let size = Vec2::new(1024.0, 768.0);
        let bottom = fitted(1.0, size, None);
        assert!(bottom < 1.0);
        // A top row narrower than the bottom changes nothing.
        assert_eq!(fitted(1.0, size, Some(800.0)), bottom);
        assert_eq!(fitted(1.0, size, Some(1280.0)), 0.8);
        // Nor does either where there's room for both.
        assert_eq!(fitted(1.0, Vec2::new(1920.0, 1080.0), Some(1280.0)), 1.0);
    }
}
