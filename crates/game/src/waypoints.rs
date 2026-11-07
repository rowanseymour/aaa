//! How orders given at a place show: the order's cursor while it's being
//! aimed, and its waypoints, such as Move's flags, until the regiments sent
//! there have arrived or been ordered elsewhere. Orders that send each
//! regiment to its own spot mark each spot; those that send all to the one
//! place mark it once.

use std::collections::HashMap;

use aaa_data::Id;
use aaa_render::MapAssets;
use aaa_render::terrain::world;
use aaa_render::textures;
use bevy::asset::RenderAssetUsages;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow, SystemCursorIcon};

use crate::actions::{self, Aim, Page};
use crate::{Drawn, Loaded, Screen, Sim, minimap};

/// Regiments, each with the spot it was sent to.
type Spots = Vec<(usize, [f32; 2])>;

/// Regiments sent to a place, each to its own spot around it, or after an
/// enemy, which marks nowhere.
#[derive(Message)]
pub struct Sent {
    pub to: Option<[f32; 2]>,
    pub aim: Aim,
    pub regiments: Spots,
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
    waypoints: HashMap<Aim, Marking>,
}

/// How an order marks where it sends regiments.
struct Marking {
    /// The archive path of its waypoint's model.
    model: String,
    /// Whether each regiment's spot gets its own.
    each: bool,
}

/// A waypoint, there while any of its regiments is still on its way to
/// its spot.
#[derive(Component)]
pub struct Waypoint(Spots);

/// The command each kind of aimed order is in the game's data.
fn command(aim: Aim) -> &'static str {
    match aim {
        Aim::Move => "move",
        Aim::Attack => "kill",
        Aim::Run => "retreat",
        Aim::Unpack => "unpack",
    }
}

pub fn load(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &mut loaded.game;
    let by_id: HashMap<Id, aaa_data::Command> = game
        .commands()
        .into_iter()
        .map(|c| (c.ids.clone(), c))
        .collect();
    let cursors = game.cursors();
    let models = game.vfx_models();
    let mut looks = Looks::default();
    for aim in Aim::ALL {
        let Some(c) = by_id.get(&Id::from(command(aim))) else {
            continue;
        };
        if let Some(model) = c.marker.as_ref().and_then(|m| models.get(m)) {
            let marking = Marking {
                model: model.clone(),
                each: c.group_offset,
            };
            looks.waypoints.insert(aim, marking);
        }
        let Some(cursor) = c.cursor.as_ref().and_then(|c| cursors.get(c)) else {
            continue;
        };
        let frames: Vec<Handle<Image>> = cursor
            .frames
            .iter()
            .filter_map(|f| {
                let mut image = textures::decode(&game.read(f)?, f, formats.0, true)?;
                // The window reads the cursor's pixels, so they stay here too.
                image.asset_usage = RenderAssetUsages::all();
                Some(images.add(image))
            })
            .collect();
        if frames.len() == cursor.frames.len() {
            let [x, y] = cursor.hotspot;
            looks.cursors.insert(
                aim,
                Frames {
                    images: frames,
                    hotspot: (x, y),
                    period: cursor.period,
                },
            );
        }
    }
    commands.insert_resource(looks);
}

/// Shows the aimed order's cursor over the map, and the usual one
/// elsewhere.
pub fn cursor(
    mut commands: Commands,
    page: Res<Page>,
    looks: Res<Looks>,
    time: Res<Time>,
    grid: Query<(&RelativeCursorPosition, &InheritedVisibility), With<actions::Grid>>,
    minimap: Query<&RelativeCursorPosition, With<minimap::MinimapView>>,
    window: Single<(Entity, Option<&CursorIcon>), With<PrimaryWindow>>,
) {
    let over_map = actions::pointer_away(grid) && minimap::pointer_away(minimap);
    let frames = match *page {
        Page::Aiming(aim) if over_map => looks.cursors.get(&aim),
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

/// Puts down waypoints where regiments were sent, taking them off any they
/// were headed for before.
pub fn mark(
    mut commands: Commands,
    mut sent: MessageReader<Sent>,
    mut earlier: Query<&mut Waypoint>,
    looks: Res<Looks>,
    mut drawn: ResMut<Drawn>,
    mut loaded: ResMut<Loaded>,
    mut assets: MapAssets,
) {
    for s in sent.read() {
        for mut w in &mut earlier {
            w.0.retain(|(r, _)| !s.regiments.iter().any(|(sent, _)| sent == r));
        }
        let Some(marking) = looks.waypoints.get(&s.aim) else {
            continue;
        };
        for (at, regiments) in places(s, marking.each) {
            let [x, y] = at;
            let at = world(x, y, loaded.map.ground.height_at(x, y));
            let mut models = assets.models();
            let spawned = drawn.0.spawn_effect(
                &mut commands,
                &mut loaded.game,
                &mut models,
                &marking.model,
                Transform::from_translation(at),
            );
            if let Some(e) = spawned {
                commands
                    .entity(e)
                    .insert((Waypoint(regiments), DespawnOnExit(Screen::Match)));
            }
        }
    }
}

/// Where to put waypoints for regiments sent somewhere, each with the
/// regiments it's for: at each one's spot, or once where they were sent.
fn places(sent: &Sent, each: bool) -> Vec<([f32; 2], Spots)> {
    let Some(to) = sent.to.filter(|_| !sent.regiments.is_empty()) else {
        return Vec::new();
    };
    match each {
        true => sent
            .regiments
            .iter()
            .map(|&(r, to)| (to, vec![(r, to)]))
            .collect(),
        false => vec![(to, sent.regiments.clone())],
    }
}

/// Takes up waypoints once none of their regiments is still headed for
/// them.
pub fn clear(mut commands: Commands, sim: Res<Sim>, waypoints: Query<(Entity, &Waypoint)>) {
    for (e, w) in &waypoints {
        if !w.0.iter().any(|&(r, to)| headed(&sim.0, r, to)) {
            commands.entity(e).despawn();
        }
    }
}

/// Whether a regiment is still on its way to `to`.
fn headed(sim: &aaa_sim::World, regiment: usize, [x, y]: [f32; 2]) -> bool {
    let [dx, dy] = sim.destination(regiment);
    !sim.destroyed(regiment) && sim.underway(regiment) && (dx - x).hypot(dy - y) < 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waypoints_mark_each_spot_or_the_one_place() {
        let sent = Sent {
            to: Some([10.0, 10.0]),
            aim: Aim::Move,
            regiments: vec![(0, [8.0, 10.0]), (1, [12.0, 10.0])],
        };
        let each = places(&sent, true);
        assert_eq!(each.len(), 2);
        assert_eq!(each[1], ([12.0, 10.0], vec![(1, [12.0, 10.0])]));
        assert_eq!(
            places(&sent, false),
            vec![([10.0, 10.0], sent.regiments.clone())]
        );
        let after = Sent {
            to: None,
            aim: Aim::Attack,
            regiments: sent.regiments.clone(),
        };
        assert!(places(&after, false).is_empty());
        let none = Sent {
            regiments: Vec::new(),
            ..sent
        };
        assert!(places(&none, true).is_empty());
    }
}
