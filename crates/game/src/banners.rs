//! The banner over each regiment: a plate in its owner's colour showing
//! what the regiment is and its division's badge, with crossed rifles
//! behind it while it's engaged and brackets around it while the cursor is
//! on it.

use std::collections::HashMap;

use aaa_map::Division;
use aaa_render::camera::RtsCamera;
use aaa_render::overlay::BannerImages;
use aaa_render::terrain::world;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;

use crate::{Focus, Loaded, Local, Screen, Sim, TINTS, tint};

/// How high over the ground under its regiment a banner floats, in world
/// units.
const HEIGHT: f32 = 4.0;

/// A banner's size, square, as a share of the view's height; the same
/// however far away its regiment is.
const SIZE: f32 = 0.075;

/// The banner of the regiment at this index into the sim's regiments.
#[derive(Component)]
pub struct Banner {
    regiment: usize,
    engaged: Entity,
    focus: Entity,
}

/// The division of each regiment the map starts in one, by regiment id.
#[derive(Resource)]
pub struct Divisions(HashMap<u32, Division>);

/// Loads what banners are drawn with.
pub fn setup_banners(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    commands.insert_resource(BannerImages::load(&mut loaded.game, &mut images, formats.0));
    commands.insert_resource(Divisions(aaa_map::divisions(&loaded.map.things)));
}

/// Spawns a banner, hidden until [`show_banners`] places it, for each
/// regiment that has none yet: those the map starts with and any raised
/// since.
pub fn add_banners(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    sim: Res<Sim>,
    divisions: Res<Divisions>,
    mut banners: ResMut<BannerImages>,
    mut images: ResMut<Assets<Image>>,
    existing: Query<&Banner>,
) {
    // Regiments are only ever added, at the end.
    let have = existing.iter().map(|b| b.regiment + 1).max().unwrap_or(0);
    let layer = |image: &Option<Handle<Image>>, colour: Color| {
        (
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            ImageNode {
                image: image.clone().unwrap_or_default(),
                color: colour,
                ..default()
            },
            Pickable::IGNORE,
        )
    };
    for (r, regiment) in sim.0.regiments.iter().enumerate().skip(have) {
        let [red, green, blue] = sim
            .0
            .sides
            .player(regiment.owner)
            .map_or([1.0; 3], |p| p.rgb);
        let icon = banners.icon(&mut loaded.game, &mut images, &regiment.template);
        let badge = divisions
            .0
            .get(&regiment.id)
            .and_then(|d| banners.badge(&mut loaded.game, &mut images, d));
        let mut root = commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            Visibility::Hidden,
            Pickable::IGNORE,
            DespawnOnExit(Screen::Match),
        ));
        let engaged = root
            .commands()
            .spawn(layer(&banners.engaged, Color::WHITE))
            .id();
        let focus = root
            .commands()
            .spawn(layer(&banners.focus, Color::WHITE))
            .id();
        root.add_child(engaged);
        root.with_children(|banner| {
            banner.spawn(layer(&banners.plate, Color::srgb(red, green, blue)));
            if icon.is_some() {
                banner.spawn(layer(&icon, Color::WHITE));
            }
            banner.spawn(layer(&banners.frame, Color::WHITE));
            // Over the frame's lower edge.
            if badge.is_some() {
                banner.spawn(layer(&badge, Color::WHITE));
            }
        });
        root.add_child(focus);
        root.insert(Banner {
            regiment: r,
            engaged,
            focus,
        });
    }
}

/// Places each live regiment's banner over where its units stand, nearer
/// ones over farther, and shows its rifles and brackets as they apply.
#[allow(clippy::too_many_arguments)]
pub fn show_banners(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    local: Res<Local>,
    focus: Res<Focus>,
    fixed: Res<Time<Fixed>>,
    cameras: Query<(&Camera, &Transform), With<RtsCamera>>,
    mut banners: Query<(&Banner, &mut Node, &mut ZIndex, &mut Visibility)>,
    mut layers: Query<(&mut ImageNode, &mut Visibility), Without<Banner>>,
) {
    let Ok((camera, transform)) = cameras.single() else {
        return;
    };
    // This frame's camera, which moves after transforms propagate.
    let global = GlobalTransform::from(*transform);
    let side = camera.logical_viewport_size().map_or(0.0, |s| s.y * SIZE);
    let t = fixed.overstep_fraction();
    let ground = &loaded.map.ground;
    for (banner, mut node, mut z, mut visibility) in &mut banners {
        let regiment = &sim.0.regiments[banner.regiment];
        let live = || {
            regiment
                .units
                .iter()
                .map(|&u| &sim.0.units[u])
                .filter(|u| u.alive())
        };
        let count = live().count();
        let at = (count > 0).then(|| {
            let sum = live().fold(Vec2::ZERO, |sum, u| {
                let (was, is) = (Vec2::from(u.previous), Vec2::from(u.position));
                sum + was.lerp(is, t)
            });
            let [x, y] = (sum / count as f32).to_array();
            world(x, y, ground.height_at(x, y) + HEIGHT)
        });
        let Some(p) = at.and_then(|p| camera.world_to_viewport(&global, p).ok()) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        node.left = px(p.x - side / 2.0);
        node.top = px(p.y - side / 2.0);
        node.width = px(side);
        node.height = px(side);
        // Lower on the screen is nearer the camera.
        *z = ZIndex(p.y as i32);
        let show = |on: bool| match on {
            true => Visibility::Inherited,
            false => Visibility::Hidden,
        };
        if let Ok((_, mut v)) = layers.get_mut(banner.engaged) {
            v.set_if_neq(show(!regiment.engaged.is_empty()));
        }
        if let Ok((mut image, mut v)) = layers.get_mut(banner.focus) {
            let focused = *focus == Focus::Regiment(banner.regiment);
            v.set_if_neq(show(focused));
            if focused {
                image.color = TINTS[tint(&sim.0, local.0, regiment.owner)];
            }
        }
    }
}
