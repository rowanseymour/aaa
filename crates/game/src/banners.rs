//! The banner over each regiment: a plate in its owner's colour showing
//! what the regiment is and the badge of the HQ it's attached to, with
//! crossed rifles behind it while it's engaged, brackets around it while
//! the cursor is on it, and a border while it's out of supply or its side
//! can't pay its upkeep.

use std::collections::{HashMap, HashSet};

use aaa_data::Id;
use aaa_map::Division;
use aaa_render::camera::RtsCamera;
use aaa_render::overlay::BannerImages;
use aaa_render::terrain::world;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use crate::{Drawn, Focus, Loaded, Local, Screen, Sim, TINTS, tint};

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
    out_of_supply: Entity,
    shortage: Entity,
    badge: Entity,
    /// The HQ whose badge it shows.
    wears: Option<u32>,
}

/// Which of its nation's badges each HQ's division wears, by the HQ's id:
/// what the map gives those it starts with, and for each raised since one
/// picked from the list, or the first no other of its side's holds where
/// that one's taken; none where there's none to have.
#[derive(Resource)]
pub struct Badges(HashMap<u32, Option<usize>>);

impl Badges {
    /// The division whose badge a regiment of `template` wears when it's
    /// attached to, or is, the HQ with this id. Any of a nation's
    /// templates finds its badges.
    pub fn division(&self, template: &str, hq: u32) -> Option<Division> {
        Some(Division {
            template: template.to_string(),
            badge: (*self.0.get(&hq)?)?,
        })
    }
}

/// The HQ whose badge the regiment at `r` wears: the one it's attached to,
/// or the one it is.
pub fn wears(sim: &aaa_sim::World, hqs: &[aaa_sim::Hq], r: usize) -> Option<u32> {
    let regiment = &sim.regiments[r];
    let is_hq = hqs.iter().any(|h| h.id == regiment.id);
    regiment.hq.or(is_hq.then_some(regiment.id))
}

/// How long the upkeep shortage's border shows for, and then doesn't, in
/// seconds.
const FLASH: f32 = 0.5;

/// The templates of regiments that fly no banner and that players can't
/// focus or select, such as animal herds.
#[derive(Resource)]
pub struct Scenery(HashSet<Id>);

impl Scenery {
    /// Whether the regiment at this index into the sim's regiments is
    /// scenery.
    pub fn has(&self, sim: &aaa_sim::World, regiment: usize) -> bool {
        self.0
            .contains(&Id::from(sim.regiments[regiment].template.as_str()))
    }
}

/// Loads what banners are drawn with.
pub fn setup_banners(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    commands.insert_resource(BannerImages::load(&mut loaded.game, &mut images, formats.0));
    let things = &loaded.map.things;
    let badge = |t: &tgck::map::Thing| Some((t.id, usize::try_from(t.hq_badge()?).ok()));
    commands.insert_resource(Badges(things.iter().filter_map(badge).collect()));
    commands.insert_resource(Scenery(loaded.game.scenery()));
}

/// One of a banner's images, filling it.
fn layer(image: &Option<Handle<Image>>, colour: Color) -> impl Bundle {
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
}

fn show(on: bool) -> Visibility {
    match on {
        true => Visibility::Inherited,
        false => Visibility::Hidden,
    }
}

/// The banner over an unpacked HQ, by the building's index into the sim's
/// and the HQ's id.
#[derive(Component)]
pub struct HqBanner {
    building: usize,
    hq: u32,
    plate: Entity,
    focus: Entity,
    shortage: Entity,
}

/// How high over the ground under an HQ its banner floats, clear of its
/// roof.
const HQ_HEIGHT: f32 = 6.0;

/// Spawns a banner for each HQ building that has none yet, in its
/// division's badge.
pub fn add_hq_banners(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    sim: Res<Sim>,
    badges: Res<Badges>,
    mut images: (ResMut<BannerImages>, ResMut<Assets<Image>>),
    flying: Query<&HqBanner>,
    mut bare: bevy::prelude::Local<HashSet<usize>>,
) {
    if sim.is_added() {
        bare.clear();
    }
    if !sim.is_changed() {
        return;
    }
    for b in 0..sim.0.buildings.len() {
        if bare.contains(&b) {
            continue;
        }
        let hq = sim.0.hq_of(b).filter(|_| !sim.0.buildings[b].gone());
        let (Some(hq), Some(template)) = (hq, sim.0.hq_template(b)) else {
            continue;
        };
        if flying.iter().any(|f| f.building == b && f.hq == hq) {
            continue;
        }
        let (banners, assets) = (&mut *images.0, &mut *images.1);
        let icon = banners.icon(&mut loaded.game, assets, template);
        let frame = banners.frame(&mut loaded.game, assets, template);
        let badge = badges
            .division(template, hq)
            .and_then(|d| banners.badge(&mut loaded.game, assets, &d));
        // Its template names none, and never will.
        if icon.is_none() {
            bare.insert(b);
            continue;
        }
        let mut root = commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            Visibility::Hidden,
            Pickable::IGNORE,
            RelativeCursorPosition::default(),
            DespawnOnExit(Screen::Match),
        ));
        let plate = root
            .commands()
            .spawn(layer(&banners.plate, Color::WHITE))
            .id();
        let shortage = (layer(&banners.shortage, Color::WHITE), Visibility::Hidden);
        let shortage = root.commands().spawn(shortage).id();
        let focus = (layer(&banners.focus, Color::WHITE), Visibility::Hidden);
        let focus = root.commands().spawn(focus).id();
        root.add_child(plate);
        root.with_children(|banner| {
            banner.spawn(layer(&icon, Color::WHITE));
            banner.spawn(layer(&frame, Color::WHITE));
            banner.spawn(layer(&badge, Color::WHITE));
        });
        root.add_children(&[shortage, focus]);
        root.insert(HqBanner {
            building: b,
            hq,
            plate,
            focus,
            shortage,
        });
    }
}

/// Places each standing HQ's banner over it, in its owner's colour, and
/// takes down those of HQs that are gone.
#[allow(clippy::too_many_arguments)]
pub fn show_hq_banners(
    mut commands: Commands,
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    local: Res<Local>,
    focus: Res<Focus>,
    time: Res<Time<Real>>,
    ui: Res<UiScale>,
    cameras: Query<(&Camera, &Transform), With<RtsCamera>>,
    mut banners: Query<(Entity, &HqBanner, &mut Node, &mut ZIndex, &mut Visibility)>,
    mut layers: Query<(&mut ImageNode, &mut Visibility), Without<HqBanner>>,
) {
    let Ok((camera, transform)) = cameras.single() else {
        return;
    };
    let global = GlobalTransform::from(*transform);
    let side = camera.logical_viewport_size().map_or(0.0, |s| s.y * SIZE);
    // Whether each owner met so far can't pay its upkeep.
    let mut short: HashMap<u32, bool> = HashMap::new();
    for (entity, banner, mut node, mut z, mut visibility) in &mut banners {
        let b = banner.building;
        let building = &sim.0.buildings[b];
        if building.gone() || sim.0.hq_of(b) != Some(banner.hq) {
            commands.entity(entity).despawn();
            continue;
        }
        let [x, y] = building.position;
        let over = world(x, y, loaded.map.ground.height_at(x, y) + HQ_HEIGHT);
        let Ok(p) = camera.world_to_viewport(&global, over) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        crate::place_on_map(&mut node, p - side / 2.0, Vec2::splat(side), &ui);
        // As near as a regiment's banner there, which flies lower, would be.
        let low = over - Vec3::Y * (HQ_HEIGHT - HEIGHT);
        let near = camera.world_to_viewport(&global, low).map_or(p.y, |p| p.y);
        *z = ZIndex(near as i32);
        if let Ok((mut image, _)) = layers.get_mut(banner.plate) {
            let [r, g, b] = (sim.0.sides.player(building.owner)).map_or([1.0; 3], |p| p.rgb);
            image.color = Color::srgb(r, g, b);
        }
        if let Ok((_, mut v)) = layers.get_mut(banner.shortage) {
            let owner = building.owner;
            let short = *(short.entry(owner)).or_insert_with(|| sim.0.short(owner).is_some());
            v.set_if_neq(show(short && time.elapsed_secs() % (2.0 * FLASH) < FLASH));
        }
        if let Ok((mut image, mut v)) = layers.get_mut(banner.focus) {
            let focused = *focus == Focus::Building(b);
            v.set_if_neq(show(focused));
            if focused {
                image.color = TINTS[tint(&sim.0, local.0, building.owner)];
            }
        }
    }
}

/// Spawns a banner, hidden until [`show_banners`] places it, for each
/// regiment that has none yet: those the map starts with and any raised
/// since.
#[allow(clippy::too_many_arguments)]
pub fn add_banners(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    sim: Res<Sim>,
    scenery: Res<Scenery>,
    mut banners: ResMut<BannerImages>,
    mut images: ResMut<Assets<Image>>,
    mut have: bevy::prelude::Local<usize>,
) {
    // Regiments are only ever added, at the end, until another match.
    if sim.is_added() {
        *have = 0;
    }
    let new = sim.0.regiments.iter().enumerate().skip(*have);
    *have = sim.0.regiments.len();
    for (r, regiment) in new {
        if scenery.has(&sim.0, r) {
            continue;
        }
        let [red, green, blue] = sim
            .0
            .sides
            .player(regiment.owner)
            .map_or([1.0; 3], |p| p.rgb);
        let icon = banners.icon(&mut loaded.game, &mut images, &regiment.template);
        let frame = banners.frame(&mut loaded.game, &mut images, &regiment.template);
        // Only organization templates name a banner icon, so a unit outside
        // one, such as a supply truck, flies no banner.
        if icon.is_none() {
            continue;
        }
        let mut root = commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            Visibility::Hidden,
            Pickable::IGNORE,
            RelativeCursorPosition::default(),
            DespawnOnExit(Screen::Match),
        ));
        let mut hidden = |image: &Option<Handle<Image>>| {
            let layer = (layer(image, Color::WHITE), Visibility::Hidden);
            root.commands().spawn(layer).id()
        };
        let engaged = hidden(&banners.engaged);
        let focus = hidden(&banners.focus);
        let out_of_supply = hidden(&banners.out_of_supply);
        let shortage = hidden(&banners.shortage);
        // Over the frame's lower edge.
        let badge = hidden(&None);
        root.add_child(engaged);
        root.with_children(|banner| {
            banner.spawn(layer(&banners.plate, Color::srgb(red, green, blue)));
            banner.spawn(layer(&icon, Color::WHITE));
            banner.spawn(layer(&frame, Color::WHITE));
        });
        root.add_children(&[out_of_supply, shortage, badge, focus]);
        root.insert(Banner {
            regiment: r,
            engaged,
            focus,
            out_of_supply,
            shortage,
            badge,
            wears: None,
        });
    }
}

/// Places each live regiment's banner over where its units stand, nearer
/// ones over farther, and shows its rifles and brackets as they apply.
#[allow(clippy::too_many_arguments)]
pub fn show_banners(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    drawn: Res<Drawn>,
    local: Res<Local>,
    focus: Res<Focus>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time<Real>>,
    ui: Res<UiScale>,
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
    // Whether each owner met so far can't pay its upkeep.
    let mut short: HashMap<u32, bool> = HashMap::new();
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
        // Unpacked, or on its way in or out, its building flies the banner.
        let housed = regiment
            .unpacked_into
            .is_some_and(|b| sim.0.hq_of(b).is_some());
        let at = live().next().filter(|_| !housed).map(|first| {
            let sum = live().fold(Vec2::ZERO, |sum, u| sum + Vec2::from(u.drawn_at(t)));
            let [x, y] = (sum / count as f32).to_array();
            let footing = drawn.0.footing(&first.template);
            world(x, y, ground.stand_at(footing, x, y) + HEIGHT)
        });
        let Some(p) = at.and_then(|p| camera.world_to_viewport(&global, p).ok()) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        crate::place_on_map(&mut node, p - side / 2.0, Vec2::splat(side), &ui);
        // Lower on the screen is nearer the camera.
        *z = ZIndex(p.y as i32);
        if let Ok((_, mut v)) = layers.get_mut(banner.engaged) {
            v.set_if_neq(show(!regiment.engaged.is_empty()));
        }
        if let Ok((_, mut v)) = layers.get_mut(banner.out_of_supply) {
            v.set_if_neq(show(sim.0.lacks_supply(banner.regiment)));
        }
        if let Ok((_, mut v)) = layers.get_mut(banner.shortage) {
            let owner = regiment.owner;
            let short = *(short.entry(owner)).or_insert_with(|| sim.0.short(owner).is_some());
            v.set_if_neq(show(short && time.elapsed_secs() % (2.0 * FLASH) < FLASH));
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

/// Gives each HQ raised since the map's a badge, and shows on each banner
/// that of the HQ its regiment is attached to, or is.
pub fn badge_banners(
    mut loaded: ResMut<Loaded>,
    sim: Res<Sim>,
    mut badges: ResMut<Badges>,
    mut images: (ResMut<BannerImages>, ResMut<Assets<Image>>),
    mut banners: Query<&mut Banner>,
    mut layers: Query<(&mut ImageNode, &mut Visibility), Without<Banner>>,
) {
    // Attachments and HQs change only as the sim does.
    if !sim.is_changed() {
        return;
    }
    let hqs = sim.0.hqs();
    for hq in &hqs {
        if badges.0.contains_key(&hq.id) {
            continue;
        }
        // Its organization's template finds its nation's badges.
        let packed = sim.0.regiments.iter().find(|r| r.id == hq.id);
        let unpacked = || (0..sim.0.buildings.len()).find(|&b| sim.0.hq_of(b) == Some(hq.id));
        let template = match packed {
            Some(r) => Some(r.template.as_str()),
            None => unpacked().and_then(|b| sim.0.hq_template(b)),
        };
        let count = template.map_or(0, |t| images.0.badges(t));
        let worn = |b: usize| {
            let mut others = hqs.iter().filter(|h| h.owner == hq.owner);
            others.any(|h| badges.0.get(&h.id) == Some(&Some(b)))
        };
        // Any of them, the same one every time for the same HQ.
        let pick = (hq.id as usize).wrapping_mul(2_654_435_761) >> 8;
        let pick = pick.checked_rem(count).filter(|&b| !worn(b));
        // None where its nation has none, or every one is held. A lost
        // HQ's is free again by no longer being among those that hold one.
        let badge = pick.or_else(|| (0..count).find(|&b| !worn(b)));
        badges.0.insert(hq.id, badge);
    }
    for mut banner in &mut banners {
        let wears = wears(&sim.0, &hqs, banner.regiment);
        if banner.wears == wears {
            continue;
        }
        banner.wears = wears;
        let Ok((mut image, mut visibility)) = layers.get_mut(banner.badge) else {
            continue;
        };
        let template = &sim.0.regiments[banner.regiment].template;
        let badge = wears
            .and_then(|hq| badges.division(template, hq))
            .and_then(|d| (images.0).badge(&mut loaded.game, &mut images.1, &d));
        *visibility = match badge.is_some() {
            true => Visibility::Inherited,
            false => Visibility::Hidden,
        };
        image.image = badge.unwrap_or_default();
    }
}

/// What [`under_cursor`] reads of a banner.
pub type Pointed<B> = (
    &'static B,
    &'static RelativeCursorPosition,
    &'static ZIndex,
    &'static Visibility,
);

/// The regiment or HQ building whose banner the cursor is on: the nearest
/// where they overlap, and a regiment's over an HQ's as near.
pub fn under_cursor(
    regiments: &Query<Pointed<Banner>>,
    hqs: &Query<Pointed<HqBanner>>,
) -> Option<Focus> {
    let on = |cursor: &RelativeCursorPosition, visibility: &Visibility| {
        cursor.cursor_over && *visibility != Visibility::Hidden
    };
    let hqs = (hqs.iter())
        .filter(|(_, cursor, _, v)| on(cursor, v))
        .map(|(banner, _, z, _)| (Focus::Building(banner.building), z.0));
    let regiments = (regiments.iter())
        .filter(|(_, cursor, _, v)| on(cursor, v))
        .map(|(banner, _, z, _)| (Focus::Regiment(banner.regiment), z.0));
    // Regiments come last, to win where they're as near.
    nearest(hqs.chain(regiments))
}

/// The nearest drawn of what banners fly for, each given with how near, and
/// the last of those as near.
fn nearest(banners: impl Iterator<Item = (Focus, i32)>) -> Option<Focus> {
    banners.max_by_key(|&(_, z)| z).map(|(focus, _)| focus)
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    #[test]
    fn nearest_banner_wins() {
        let (regiment, building) = (Focus::Regiment(3), Focus::Building(1));
        assert_eq!(nearest([].into_iter()), None);
        assert_eq!(nearest([(building, 9)].into_iter()), Some(building));
        // Whichever kind is nearer, whatever order they come in.
        let banners = [(building, 5), (regiment, 2)];
        assert_eq!(nearest(banners.into_iter()), Some(building));
        assert_eq!(nearest(banners.into_iter().rev()), Some(building));
        let banners = [(building, 2), (regiment, 5)];
        assert_eq!(nearest(banners.into_iter()), Some(regiment));
        // As near as each other, the later wins.
        let banners = [(building, 4), (regiment, 4)];
        assert_eq!(nearest(banners.into_iter()), Some(regiment));
    }

    #[test]
    fn cursor_picks_among_the_banners_it_is_on() {
        let mut world = World::new();
        let none = Entity::PLACEHOLDER;
        let pointed = |over: bool, z: i32, visibility: Visibility| {
            let cursor = RelativeCursorPosition {
                cursor_over: over,
                ..default()
            };
            (cursor, ZIndex(z), visibility)
        };
        let regiment = |regiment: usize| Banner {
            regiment,
            engaged: none,
            focus: none,
            out_of_supply: none,
            shortage: none,
            badge: none,
            wears: None,
        };
        let hq = |building: usize| HqBanner {
            building,
            hq: 0,
            plate: none,
            focus: none,
            shortage: none,
        };
        let picked = |world: &mut World| {
            let pick = |regiments: Query<Pointed<Banner>>, hqs: Query<Pointed<HqBanner>>| {
                under_cursor(&regiments, &hqs)
            };
            world.run_system_once(pick).unwrap()
        };
        // Those the cursor is off, or that are hidden, don't count.
        world.spawn((regiment(3), pointed(false, 9, Visibility::Inherited)));
        world.spawn((hq(4), pointed(true, 9, Visibility::Hidden)));
        assert_eq!(picked(&mut world), None);
        world.spawn((hq(1), pointed(true, 2, Visibility::Inherited)));
        assert_eq!(picked(&mut world), Some(Focus::Building(1)));
        // A regiment's wins over an HQ's as near, and a nearer HQ's over it.
        world.spawn((regiment(7), pointed(true, 2, Visibility::Inherited)));
        assert_eq!(picked(&mut world), Some(Focus::Regiment(7)));
        world.spawn((hq(2), pointed(true, 5, Visibility::Inherited)));
        assert_eq!(picked(&mut world), Some(Focus::Building(2)));
    }
}
