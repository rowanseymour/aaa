//! The banner over each regiment: a plate in its owner's colour showing
//! what the regiment is, with crossed rifles behind it while it's engaged
//! and brackets around it while the cursor is on it.

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

/// Spawns every regiment's banner, hidden until [`show_banners`] places it.
pub fn setup_banners(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    sim: Res<Sim>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let templates = sim.0.regiments.iter().map(|r| r.template.as_str());
    let banners = BannerImages::load(&mut loaded.game, &mut images, formats.0, templates);
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
    for (r, regiment) in sim.0.regiments.iter().enumerate() {
        let [red, green, blue] = sim
            .0
            .sides
            .player(regiment.owner)
            .map_or([1.0; 3], |p| p.rgb);
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
            if let Some(icon) = banners.icons.get(&regiment.template) {
                banner.spawn(layer(&Some(icon.clone()), Color::WHITE));
            }
            banner.spawn(layer(&banners.frame, Color::WHITE));
        });
        root.add_child(focus);
        root.insert(Banner {
            regiment: r,
            engaged,
            focus,
        });
    }
    commands.insert_resource(banners);
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
