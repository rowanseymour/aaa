//! How orders given at a place show: the order's cursor while it's being
//! aimed, and its waypoints, such as Move's flags, until the organizations sent
//! there have arrived or been ordered elsewhere. Orders that send each
//! organization to its own spot mark each spot; those that send all to the one
//! place mark it once. A selected building's rally point is marked too.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow, SystemCursorIcon};
use tgdata::Id;
use tgrender::MapAssets;
use tgrender::space::world;

use crate::actions::{self, Aim, Page};
use crate::pointing::Pointer;
use crate::{Drawn, Loaded, Screen, SelectedBuilding, Sim};

/// The effect marking a building's rally point.
const RALLY_MARKER: &str = "recruit_marker_rally_point";

/// Organizations, each with the spot it was sent to.
type Spots = Vec<(usize, [f32; 2])>;

/// Organizations sent to a place, each to its own spot around it, or after an
/// enemy, which marks nowhere.
#[derive(Message)]
pub struct Sent {
    pub to: Option<[f32; 2]>,
    pub aim: Aim,
    pub organizations: Spots,
}

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
    rally_cursor: Option<Frames>,
    waypoints: HashMap<Aim, Marking>,
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

/// How an order marks where it sends organizations.
struct Marking {
    /// The archive path of its waypoint's model.
    model: String,
    /// Whether each organization's spot gets its own.
    each: bool,
}

/// A waypoint, there while any of its organizations is still on its way to
/// its spot.
#[derive(Component)]
pub struct Waypoint(Spots);

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
    let mut frames_of = |c: &tgdata::Command| {
        let cursor = c.cursor.as_ref().and_then(|c| cursors.get(c))?;
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
        .and_then(&mut frames_of);
    for aim in Aim::ALL {
        let Some(c) = by_id.get(&Id::from(command(aim))) else {
            continue;
        };
        if !c.group_offset {
            looks.together.push(aim);
        }
        if let Some(model) = c.marker.as_ref().and_then(|m| models.get(m)) {
            let marking = Marking {
                model: model.clone(),
                each: c.group_offset,
            };
            looks.waypoints.insert(aim, marking);
        }
        if let Some(frames) = frames_of(c) {
            looks.cursors.insert(aim, frames);
        }
    }
    commands.insert_resource(looks);
}

/// Shows the aimed order's cursor over the map and the minimap's diamond,
/// the attack cursor for an armed special operation, and the usual one
/// elsewhere.
pub fn cursor(
    mut commands: Commands,
    (page, armed): (Res<Page>, Res<crate::special_ops::Armed>),
    looks: Res<Looks>,
    time: Res<Time>,
    pointer: Pointer,
    loaded: Res<Loaded>,
    window: Single<(Entity, Option<&CursorIcon>), With<PrimaryWindow>>,
) {
    let over_map = pointer.at(&loaded.map.ground).is_some();
    let frames = match *page {
        Page::Aiming(aim) if over_map => looks.cursors.get(&aim),
        Page::Rally if over_map => looks.rally_cursor.as_ref(),
        Page::AirAttack if over_map => looks.cursors.get(&Aim::Attack),
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

/// Puts down waypoints where organizations were sent, taking them off any they
/// were headed for before.
pub fn mark(
    mut commands: Commands,
    mut sent: MessageReader<Sent>,
    mut earlier: Query<&mut Waypoint>,
    looks: Res<Looks>,
    mut drawn: ResMut<Drawn>,
    loaded: Res<Loaded>,
    mut assets: MapAssets,
) {
    for s in sent.read() {
        for mut w in &mut earlier {
            w.0.retain(|(r, _)| !s.organizations.iter().any(|(sent, _)| sent == r));
        }
        let Some(marking) = looks.waypoints.get(&s.aim) else {
            continue;
        };
        for (at, organizations) in places(s, marking.each) {
            let [x, y] = at;
            let at = world(x, y, loaded.map.ground.height_at([x, y]));
            let mut models = assets.models();
            let spawned = drawn.0.spawn_effect(
                &mut commands,
                &mut models,
                &marking.model,
                &[],
                Transform::from_translation(at),
            );
            if let Some(e) = spawned {
                commands
                    .entity(e)
                    .insert((Waypoint(organizations), DespawnOnExit(Screen::Match)));
            }
        }
    }
}

/// Where to put waypoints for organizations sent somewhere, each with the
/// organizations it's for: at each one's spot, or once where they were sent.
fn places(sent: &Sent, each: bool) -> Vec<([f32; 2], Spots)> {
    let Some(to) = sent.to.filter(|_| !sent.organizations.is_empty()) else {
        return Vec::new();
    };
    match each {
        true => sent
            .organizations
            .iter()
            .map(|&(r, to)| (to, vec![(r, to)]))
            .collect(),
        false => vec![(to, sent.organizations.clone())],
    }
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

/// Takes up waypoints once none of their organizations is still headed for
/// them.
pub fn clear(mut commands: Commands, sim: Res<Sim>, waypoints: Query<(Entity, &Waypoint)>) {
    for (e, w) in &waypoints {
        if !w.0.iter().any(|&(r, to)| headed(&sim.0, r, to)) {
            commands.entity(e).despawn();
        }
    }
}

/// Whether an organization is still on its way to `to`.
fn headed(sim: &tgsim::World, organization: usize, [x, y]: [f32; 2]) -> bool {
    let [dx, dy] = sim.destination(organization);
    !sim.destroyed(organization) && sim.underway(organization) && (dx - x).hypot(dy - y) < 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waypoints_mark_each_spot_or_the_one_place() {
        let sent = Sent {
            to: Some([10.0, 10.0]),
            aim: Aim::Move,
            organizations: vec![(0, [8.0, 10.0]), (1, [12.0, 10.0])],
        };
        let each = places(&sent, true);
        assert_eq!(each.len(), 2);
        assert_eq!(each[1], ([12.0, 10.0], vec![(1, [12.0, 10.0])]));
        assert_eq!(
            places(&sent, false),
            vec![([10.0, 10.0], sent.organizations.clone())]
        );
        let after = Sent {
            to: None,
            aim: Aim::Attack,
            organizations: sent.organizations.clone(),
        };
        assert!(places(&after, false).is_empty());
        let none = Sent {
            organizations: Vec::new(),
            ..sent
        };
        assert!(places(&none, true).is_empty());
    }
}
