//! What the cursor is over: the ground, a unit or a building.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::window::PrimaryWindow;
use tgmap::Ground;
use tgrender::camera::RtsCamera;
use tgrender::space::map_point;
use tgsim::World;

use crate::{Focus, Loaded, Sim, banners, fog, minimap, ui};

/// What the cursor points at on the map, on the ground or through the
/// minimap, and not through the action grid.
#[derive(SystemParam)]
pub(crate) struct Pointer<'w, 's> {
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    cameras: Query<
        'w,
        's,
        (
            &'static Camera,
            &'static GlobalTransform,
            &'static RtsCamera,
        ),
    >,
    panels: ui::Panels<'w, 's>,
    pub(crate) minimap: Res<'w, minimap::Minimap>,
    views: Query<'w, 's, &'static RelativeCursorPosition, With<minimap::MinimapView>>,
}

/// A point on the map the cursor's at, in world units.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Pointing {
    Ground([f32; 2]),
    Minimap([f32; 2]),
}

impl Pointing {
    pub(crate) fn point(self) -> [f32; 2] {
        match self {
            Pointing::Ground(p) | Pointing::Minimap(p) => p,
        }
    }
}

impl Pointer<'_, '_> {
    pub(crate) fn at(&self, ground: &Ground) -> Option<Pointing> {
        if self.panels.under_cursor() {
            return None;
        }
        match self.minimap.spot(&self.views) {
            minimap::Spot::Off => {
                pointed(&self.windows, &self.cameras, ground).map(Pointing::Ground)
            }
            minimap::Spot::OffMap => None,
            minimap::Spot::At(p) => Some(Pointing::Minimap(p)),
        }
    }
}

/// The organization, of those `taking` takes, with the living unit nearest
/// `point`, if one's within `reach`.
pub(crate) fn nearest_organization(
    sim: &World,
    point: [f32; 2],
    reach: f32,
    taking: impl Fn(usize) -> bool,
) -> Option<usize> {
    nearest(
        sim.units
            .iter()
            .filter(|u| u.alive() && taking(u.organization))
            .map(|u| (u.organization, u.position)),
        point,
        reach,
    )
}

pub(crate) fn nearest(
    units: impl Iterator<Item = (usize, [f32; 2])>,
    point: [f32; 2],
    reach: f32,
) -> Option<usize> {
    let near = |p: [f32; 2]| (p[0] - point[0]).hypot(p[1] - point[1]);
    units
        .map(|(r, p)| (r, near(p)))
        .filter(|(_, d)| *d < reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r)
}

/// The point on the ground under the cursor, in world units.
pub(crate) fn pointed(
    windows: &Query<&Window, With<PrimaryWindow>>,
    cameras: &Query<(&Camera, &GlobalTransform, &RtsCamera)>,
    ground: &Ground,
) -> Option<[f32; 2]> {
    let (Ok(window), Ok((camera, transform, _))) = (windows.single(), cameras.single()) else {
        return None;
    };
    let cursor = window.cursor_position()?;
    // Not beside the view, where it's cut narrower than the window.
    if !camera.logical_viewport_rect()?.contains(cursor) {
        return None;
    }
    let ray = camera.viewport_to_world(transform, cursor).ok()?;
    pick_ground(ground, ray)
}

/// Finds what the cursor is over: the organization or HQ whose banner it's on,
/// else the organization of the nearest unit within reach of the ground it
/// points at, else the building standing there; nothing while it's on the
/// minimap or the interface's panels, which cover the map.
#[allow(clippy::too_many_arguments)]
pub(crate) fn focus(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &RtsCamera)>,
    banners: Query<banners::Pointed<banners::Banner>>,
    hq_banners: Query<banners::Pointed<banners::HqBanner>>,
    minimap: Query<&RelativeCursorPosition, With<minimap::MinimapView>>,
    panels: ui::Panels,
    loaded: Res<Loaded>,
    sim: Res<Sim>,
    scenery: Res<banners::Scenery>,
    fog: Res<fog::Fog>,
    mut focus: ResMut<Focus>,
) {
    const REACH: f32 = 2.0;
    // Beside the view, where it's cut narrower than the window, a banner
    // at its edge reaches out of sight.
    let beside = windows
        .single()
        .ok()
        .and_then(Window::cursor_position)
        .zip(cameras.single().ok())
        .is_some_and(|(at, (c, ..))| c.logical_viewport_rect().is_some_and(|r| !r.contains(at)));
    if !minimap::pointer_away(minimap) || panels.under_cursor() || beside {
        focus.set_if_neq(Focus::Nothing);
        return;
    }
    // An HQ's banner may outlast it by a frame.
    let flying = |found: &Focus| match *found {
        Focus::Building(b) => !sim.0.buildings[b].gone() && sim.0.hq_of(b).is_some(),
        _ => true,
    };
    if let Some(found) = banners::under_cursor(&banners, &hq_banners).filter(flying) {
        focus.set_if_neq(found);
        return;
    }
    let Some(point) = pointed(&windows, &cameras, &loaded.map.ground) else {
        focus.set_if_neq(Focus::Nothing);
        return;
    };
    let unit = nearest(
        (sim.0.units.iter().enumerate())
            .filter(|&(u, unit)| unit.alive() && fog.shows_unit(u))
            .filter(|(_, unit)| !scenery.has(&sim.0, unit.organization))
            .map(|(_, unit)| (unit.organization, unit.position)),
        point,
        REACH,
    );
    let building = || {
        sim.0.buildings.iter().enumerate().position(|(i, b)| {
            !b.gone()
                && fog.building(i) == fog::Look::Shown
                && b.obstacle
                    .as_ref()
                    .is_some_and(|o| o.distance(point) == 0.0)
        })
    };
    let found = match unit {
        Some(r) => Focus::Organization(r),
        None => building().map_or(Focus::Nothing, Focus::Building),
    };
    focus.set_if_neq(found);
}

/// Where a ray from the camera first meets the ground, in world units.
fn pick_ground(ground: &Ground, ray: Ray3d) -> Option<[f32; 2]> {
    let above = |p: Vec3| {
        let [x, y] = map_point(p);
        p.y - ground.height_at([x, y])
    };
    let step = 0.5;
    let mut t = 0.0;
    let mut was = ray.origin;
    let limit = ground.size() * 4.0;
    while t < limit {
        t += step;
        let p = ray.get_point(t);
        if above(p) <= 0.0 {
            // Halve the step down to where it crossed.
            let (mut a, mut b) = (was, p);
            for _ in 0..16 {
                let mid = (a + b) / 2.0;
                if above(mid) > 0.0 { a = mid } else { b = mid }
            }
            let size = ground.size();
            let [x, y] = map_point(b);
            return ((0.0..=size).contains(&x) && (0.0..=size).contains(&y)).then_some([x, y]);
        }
        was = p;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_unit_within_reach_is_pointed_at() {
        let units = || [(0, [0.0, 0.0]), (1, [3.0, 0.0]), (2, [1.5, 0.0])].into_iter();
        assert_eq!(nearest(units(), [2.0, 0.0], 1.0), Some(2));
        assert_eq!(nearest(units(), [2.9, 0.0], 1.0), Some(1));
        assert_eq!(nearest(units(), [0.0, 5.0], 1.0), None);
    }
}
