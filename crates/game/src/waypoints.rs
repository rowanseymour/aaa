//! How orders given at a place show: the order's cursor while it's being
//! aimed, and its waypoint, such as Move's flag, where it was given until
//! the regiments sent there have arrived or been ordered elsewhere.

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

/// Regiments sent to a place, each to its own spot around it.
#[derive(Message)]
pub struct Sent {
    pub to: [f32; 2],
    pub aim: Aim,
    pub regiments: Vec<(usize, [f32; 2])>,
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
    /// The archive path of its waypoint's model.
    waypoints: HashMap<Aim, String>,
}

/// A waypoint, there while any of its regiments is still on its way to
/// its spot.
#[derive(Component)]
pub struct Waypoint(Vec<(usize, [f32; 2])>);

/// The command each kind of aimed order is in the game's data.
fn command(aim: Aim) -> &'static str {
    match aim {
        Aim::Move => "move",
        Aim::Run => "retreat",
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
    for aim in [Aim::Move, Aim::Run] {
        let Some(c) = by_id.get(&Id::from(command(aim))) else {
            continue;
        };
        if let Some(model) = c.marker.as_ref().and_then(|m| models.get(m)) {
            looks.waypoints.insert(aim, model.clone());
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

/// Puts down a waypoint where regiments were sent, taking them off any
/// they were headed for before.
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
        let Some(path) = looks
            .waypoints
            .get(&s.aim)
            .filter(|_| !s.regiments.is_empty())
        else {
            continue;
        };
        let [x, y] = s.to;
        let at = world(x, y, loaded.map.ground.height_at(x, y));
        let mut models = assets.models();
        let spawned = drawn.0.spawn_effect(
            &mut commands,
            &mut loaded.game,
            &mut models,
            path,
            Transform::from_translation(at),
        );
        if let Some(e) = spawned {
            commands
                .entity(e)
                .insert((Waypoint(s.regiments.clone()), DespawnOnExit(Screen::Match)));
        }
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
