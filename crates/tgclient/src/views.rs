//! What the sim's units and buildings are drawn as, kept in step with
//! it: where they stand, what they're doing, and what's left of them.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use tgck::map::{Detail, Thing};
use tgdata::Footing;
use tgmap::{Ground, Shape};
use tgrender::MapObject;
use tgrender::camera::{self, RtsCamera};
use tgrender::models::{ModelAssets, Models, pose};
use tgrender::space::world;
use tgrender::{MapAssets, Playing};
use tgsim::{State, TICK};

use crate::{Drawn, Loaded, Local, Screen, Sim, effects, fog, sound, ui};

/// How many of the sim's units and buildings are drawn, so those it adds
/// mid-match get drawn too, and how much of the scenery it has cleared is
/// gone from view.
#[derive(Resource, Default)]
pub(crate) struct Viewed {
    units: usize,
    buildings: usize,
    cleared: usize,
    /// The map's objects by id, for clearing scenery without searching.
    objects: HashMap<u32, Entity>,
}

/// Draws one of the simulation's units.
#[derive(Component)]
pub(crate) struct UnitView {
    /// Index into the sim's units.
    pub(crate) unit: usize,
    /// The last of its shots shown.
    shot: Option<(u64, usize)>,
    /// The last of its repairs shown.
    mended: Option<(u64, usize)>,
    /// The last of its fidgets shown.
    fidget: Option<(u64, tgmap::Id)>,
    dead: bool,
    moving: bool,
    /// The outline its model tilts on to follow the ground, if it does.
    conform: Option<Shape>,
    pub(crate) footing: Footing,
    /// How far its remains sink.
    depth: f32,
}

/// Draws the trailer one of the sim's units tows.
#[derive(Component)]
pub(crate) struct TrailerView {
    /// Index into the sim's units.
    unit: usize,
    conform: Option<Shape>,
    footing: Footing,
}

/// Spawns the model of the sim's `unit`, placed as `thing`, moving at up to
/// `speed`, and of any `trailer` it tows; false if the unit has none.
#[allow(clippy::too_many_arguments)]
fn spawn_unit_view(
    commands: &mut Commands,
    models: &mut Models,
    assets: &mut ModelAssets,
    ground: &Ground,
    thing: &Thing,
    speed: f32,
    trailer: Option<&tgsim::Trailer>,
    owner: u32,
    unit: usize,
) -> bool {
    if let Some(t) = trailer {
        let footing = models.footing(t.template.as_str());
        let towed = thing_at(
            ground,
            footing,
            thing.id,
            t.template.as_str(),
            t.position,
            t.facing,
        );
        if let Some(e) = models.spawn_object(commands, assets, &towed, Some(owner)) {
            let conform = models.conform(t.template.as_str());
            commands.entity(e).insert(TrailerView {
                unit,
                conform,
                footing,
            });
        }
    }
    let Some(e) = models.spawn_unit(commands, assets, thing, owner, speed) else {
        return false;
    };
    let depth = models.remains_depth(thing, false);
    commands.entity(e).insert(UnitView {
        unit,
        shot: None,
        mended: None,
        fidget: None,
        dead: false,
        moving: false,
        conform: models.conform(&thing.template),
        footing: models.footing(&thing.template),
        depth,
    });
    true
}

/// Places trailers between where the last two ticks left them, hiding
/// them along with what towed them.
pub(crate) fn show_trailers(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    fog: Res<fog::Fog>,
    fixed: Res<Time<Fixed>>,
    mut views: Query<(&TrailerView, &mut Transform, &mut Visibility)>,
) {
    let t = fixed.overstep_fraction();
    for (view, mut transform, mut visibility) in &mut views {
        let unit = &sim.0.units[view.unit];
        let Some(trailer) = unit.trailer.as_ref().filter(|_| unit.alive()) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(ui::visible(fog.shows_unit(view.unit)));
        (transform.translation, transform.rotation) = pose(
            &loaded.map.ground,
            trailer.drawn_at(t),
            trailer.facing,
            view.conform,
            view.footing,
        );
    }
}

/// Draws one of the simulation's buildings.
#[derive(Component)]
pub(crate) struct BuildingView {
    /// Index into the sim's buildings.
    pub(crate) building: usize,
    /// Where the map placed it.
    pub(crate) placed: Vec3,
    /// The last of its shots shown.
    shot: Option<(u64, usize)>,
    destroyed: bool,
    /// How far its remains sink.
    depth: f32,
    /// The model it plays while it packs or unpacks, shown in its place,
    /// and whether it's unpacking.
    pub(crate) packing: Option<(Entity, bool)>,
}

pub(crate) fn setup(
    mut commands: Commands,
    loaded: Res<Loaded>,
    sim: Res<Sim>,
    local: Res<Local>,
    mut assets: MapAssets,
    mut sounds: ResMut<sound::Sounds>,
) {
    let Loaded { game, map } = &*loaded;
    // The sim's units and buildings are drawn below, trailers with what
    // tows them; what else a Division HQ holds would be left standing when
    // it packs.
    let leave_out: HashSet<u32> = (map.roles())
        .filter(|(_, role)| *role != tgmap::Role::Other)
        .map(|(t, _)| t.id)
        .collect();
    let mut models = tgrender::spawn_map(&mut commands, &mut assets, game, map, false, |t| {
        leave_out.contains(&t.id)
    });
    // Units are drawn as the map stores them, with its variations and
    // scales; members an organization has lost aren't stored, so stand on the
    // ground.
    let stored: HashMap<u32, &Thing> = map.standing().map(|t| (t.id, t)).collect();
    let members = map.organizations.iter().flat_map(|r| &r.members);
    let mut missing = 0;
    for (i, m) in members.enumerate() {
        debug_assert_eq!(sim.0.units[i].id, m.id);
        let owner = sim.0.organizations[sim.0.units[i].organization].owner;
        let thing = match stored.get(&m.id) {
            Some(&t) => Cow::Borrowed(t),
            None => {
                let footing = models.footing(m.template.as_str());
                let at = thing_at(
                    &map.ground,
                    footing,
                    m.id,
                    m.template.as_str(),
                    m.position,
                    m.facing,
                );
                Cow::Owned(at)
            }
        };
        let mut model_assets = assets.models();
        if !spawn_unit_view(
            &mut commands,
            &mut models,
            &mut model_assets,
            &map.ground,
            &thing,
            m.speed,
            sim.0.units[i].trailer.as_ref(),
            owner,
            i,
        ) {
            missing += 1;
        }
    }
    info!(
        "{} organizations of {} units, {missing} without a model",
        sim.0.organizations.len(),
        sim.0.units.len()
    );
    let mut missing = 0;
    for (i, b) in map.buildings.iter().enumerate() {
        debug_assert_eq!(sim.0.buildings[i].id, b.id);
        let thing = stored[&b.id];
        match models.spawn_object(
            &mut commands,
            &mut assets.models(),
            thing,
            tgmap::owner(thing),
        ) {
            Some(e) => {
                let [x, y, z] = thing.position;
                let depth = models.remains_depth(thing, true);
                commands.entity(e).insert(BuildingView {
                    building: i,
                    placed: world(x, y, z),
                    shot: None,
                    destroyed: false,
                    depth,
                    packing: None,
                });
            }
            None => missing += 1,
        }
    }
    info!(
        "{} buildings, {missing} without a model",
        sim.0.buildings.len()
    );
    // Looking at the person's side: an organization of theirs, else a building,
    // else where they start.
    let own = |owner: u32| local.0 == Some(owner);
    let organization = (0..sim.0.organizations.len())
        .find(|&r| local.0.is_some_and(|p| sim.0.commands(p, r)))
        .map(|r| sim.0.centre(r));
    let building = || {
        let mut buildings = sim.0.buildings.iter();
        buildings.find(|b| own(b.owner)).map(|b| b.position)
    };
    let start = || {
        let mut things = map.things.iter();
        let start = things.find(|t| t.template == "start_loc" && tgmap::owner(t).is_some_and(own));
        start.map(|t| [t.position[0], t.position[1]])
    };
    let cam = match organization.or_else(building).or_else(start) {
        Some([x, y]) => RtsCamera::new(world(x, y, map.ground.height_at([x, y]))),
        None => RtsCamera::centred(&map.ground),
    };
    // The letters are the game's hotkeys.
    let cam = RtsCamera {
        letter_keys: false,
        ..cam
    };
    camera::spawn(&mut commands, cam);
    commands.insert_resource(Viewed {
        units: sim.0.units.len(),
        buildings: sim.0.buildings.len(),
        cleared: sim.0.cleared.len(),
        objects: HashMap::new(),
    });
    commands.insert_resource(Drawn(models));
    sounds.load_objects(game);
}

/// A placed object standing on the ground, for drawing what the sim adds.
pub(crate) fn thing_at(
    ground: &Ground,
    footing: Footing,
    id: u32,
    template: &str,
    [x, y]: [f32; 2],
    [fx, fy]: [f32; 2],
) -> Thing {
    Thing {
        template: template.to_string(),
        class: tgck::map::ACTOR,
        id,
        name: String::new(),
        position: [x, y, ground.stand_at(footing, [x, y])],
        facing: [fx, fy, 0.0],
        flags: 0,
        scale: 1.0,
        variation: -1,
        seen_by: 0,
        detail: Detail::None,
    }
}

/// Draws the units and buildings the sim has added since last time, such
/// as those commissioned or unpacked; a building still unpacking plays
/// its unpacking model in its place.
pub(crate) fn spawn_views(
    mut commands: Commands,
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    mut drawn: ResMut<Drawn>,
    mut viewed: ResMut<Viewed>,
    mut assets: MapAssets,
) {
    let map = &loaded.map;
    for u in viewed.units..sim.0.units.len() {
        let unit = &sim.0.units[u];
        let organization = &sim.0.organizations[unit.organization];
        let thing = thing_at(
            &map.ground,
            drawn.0.footing(unit.template.as_str()),
            unit.id,
            unit.template.as_str(),
            unit.position,
            unit.facing,
        );
        let mut models = assets.models();
        spawn_unit_view(
            &mut commands,
            &mut drawn.0,
            &mut models,
            &map.ground,
            &thing,
            organization.speed,
            unit.trailer.as_ref(),
            organization.owner,
            u,
        );
    }
    viewed.units = sim.0.units.len();
    for (b, building) in sim.0.buildings.iter().enumerate().skip(viewed.buildings) {
        let thing = thing_at(
            &map.ground,
            drawn.0.footing(building.template.as_str()),
            building.id,
            building.template.as_str(),
            building.position,
            building.facing,
        );
        let mut models = assets.models();
        let Some(e) =
            drawn
                .0
                .spawn_object(&mut commands, &mut models, &thing, Some(building.owner))
        else {
            continue;
        };
        let [x, y, z] = thing.position;
        let depth = drawn.0.remains_depth(&thing, true);
        commands.entity(e).insert((
            BuildingView {
                building: b,
                placed: world(x, y, z),
                shot: None,
                destroyed: false,
                depth,
                packing: None,
            },
            DespawnOnExit(Screen::Match),
        ));
    }
    viewed.buildings = sim.0.buildings.len();
}

/// Takes away the scenery the sim has cleared from under new buildings.
pub(crate) fn clear_scenery(
    mut commands: Commands,
    sim: Res<Sim>,
    mut viewed: ResMut<Viewed>,
    added: Query<(Entity, &MapObject), Added<MapObject>>,
) {
    let viewed = &mut *viewed;
    for (e, object) in &added {
        viewed.objects.insert(object.0.id, e);
    }
    for id in &sim.0.cleared[viewed.cleared..] {
        if let Some(e) = viewed.objects.remove(id) {
            commands.entity(e).despawn();
        }
    }
    viewed.cleared = sim.0.cleared.len();
}

/// Plays a building's packing or unpacking model in its place while it
/// packs or unpacks, and hides it once it's packed up.
pub(crate) fn show_packing(
    mut commands: Commands,
    sim: Res<Sim>,
    mut drawn: ResMut<Drawn>,
    mut assets: MapAssets,
    mut views: Query<(&mut BuildingView, &MapObject, &mut Visibility)>,
    fog: Res<fog::Fog>,
) {
    for (mut view, object, mut visibility) in &mut views {
        if fog.building(view.building) != fog::Look::Shown {
            continue;
        }
        let building = &sim.0.buildings[view.building];
        let wanted = match (building.unpacking, building.packing) {
            (Some((start, _)), _) => Some((start, true)),
            (None, Some((start, _))) => Some((start, false)),
            _ => None,
        };
        if view.packing.map(|(_, u)| u) != wanted.map(|(_, u)| u)
            && let Some((e, _)) = view.packing.take()
        {
            commands.entity(e).despawn();
        }
        let gone = matches!(building.state, State::Packed(..) | State::Sold(_));
        if let (Some((start, unpacking)), None) = (wanted, view.packing) {
            let into = (sim.0.ticks - start) as f32 * TICK;
            let mut models = assets.models();
            let spawned = drawn.0.spawn_packing(
                &mut commands,
                &mut models,
                &object.0,
                building.owner,
                unpacking,
                into,
            );
            if let Some(e) = spawned {
                commands.entity(e).insert(DespawnOnExit(Screen::Match));
                view.packing = Some((e, unpacking));
            }
        }
        let falling = building.descent.is_some();
        let shown = ui::visible(view.packing.is_none() && !gone && !falling);
        if !matches!(building.state, State::Destroyed(_)) {
            visibility.set_if_neq(shown);
        }
    }
}

/// Places units between where the last two ticks left them, playing their
/// moving animation while they move, their attack as they fire and their
/// death as they die, each from the animations of the look their stance
/// gives them; the dead without one are hidden.
pub(crate) fn show_units(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    fog: Res<fog::Fog>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    mut sounds: ResMut<sound::Sounds>,
    mut views: Query<(
        &mut UnitView,
        &mut Transform,
        &mut Visibility,
        Option<&mut Playing>,
    )>,
) {
    let t = fixed.overstep_fraction();
    for (mut view, mut transform, mut visibility, mut playing) in &mut views {
        let unit = &sim.0.units[view.unit];
        let owner = sim.0.organizations[unit.organization].owner;
        // What's in the air is drawn with its delivery.
        if unit.left.is_some() || unit.awaited || unit.in_air() {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        // Neither seen nor heard in the fog.
        let seen = fog.shows_unit(view.unit);
        // A truck that packed back up from its building is seen again, as
        // is a member that's arrived or been replaced.
        if unit.alive() {
            visibility.set_if_neq(ui::visible(seen));
            if view.dead {
                view.dead = false;
                if let Some(p) = playing.as_mut() {
                    p.revive();
                }
            }
        }
        // Its stance's look, before a death that starts in it.
        if let Some(p) = playing.as_mut() {
            p.set_look(sim.0.stance_look(view.unit).map(|l| l.as_str()));
        }
        if !unit.alive() && !view.dead {
            view.dead = true;
            if seen {
                sounds.cue(
                    &time,
                    unit.template.as_str(),
                    |a| &a.destroyed,
                    unit.position,
                    owner,
                );
            }
            // As the simulation picked it; with none, it lies as it stood.
            if let Some(p) = playing.as_mut() {
                p.die(unit.death.as_ref().map(|d| d.as_str()));
            }
        }
        // Its remains, till they're taken off.
        let sunk = match (unit.alive(), sim.0.unit_remains(view.unit)) {
            (false, Some(r)) => sunk(r, sim.0.ticks, t),
            _ => Some(0.0),
        };
        if !unit.alive() {
            visibility.set_if_neq(ui::visible(seen && sunk.is_some()));
        }
        if unit.shot != view.shot {
            view.shot = unit.shot;
            let weapon = unit
                .shot
                .and_then(|(_, w)| unit.fighter.as_ref()?.weapons.get(w));
            if let (Some(w), Some(target), true) = (weapon, unit.target, seen) {
                let to = sim.0.position(target);
                sounds.shot(&time, &w.sounds, unit.position, to, owner);
            }
            if let (Some(p), Some(ids)) = (
                playing.as_mut(),
                weapon.and_then(|w| w.animation.as_deref()),
            ) {
                p.use_ability(ids);
            }
        }
        if unit.fidget != view.fidget {
            view.fidget = unit.fidget.clone();
            if let (Some(p), Some((_, ids))) = (playing.as_mut(), unit.fidget.as_ref()) {
                p.use_ability(ids.as_str());
            }
        }
        if unit.mended != view.mended {
            view.mended = unit.mended;
            let repair = unit
                .mended
                .and_then(|(_, i)| unit.fighter.as_ref()?.repairs.get(i));
            if let (Some(r), Some(target), true) = (repair, unit.target, seen) {
                let to = sim.0.position(target);
                sounds.shot(&time, &r.sounds, unit.position, to, owner);
            }
            if let (Some(p), Some(ids)) = (
                playing.as_mut(),
                repair.and_then(|r| r.animation.as_deref()),
            ) {
                p.use_ability(ids);
            }
        }
        (transform.translation, transform.rotation) = pose(
            &loaded.map.ground,
            unit.drawn_at(t),
            unit.facing,
            view.conform,
            view.footing,
        );
        transform.translation.y -= view.depth * sunk.unwrap_or(1.0);
        if unit.alive() && unit.moving() != view.moving {
            view.moving = unit.moving();
            let event: fn(&tgdata::ObjectAudio) -> &Vec<_> = match view.moving {
                true => |a| &a.start_moving,
                false => |a| &a.stop_moving,
            };
            if seen {
                sounds.cue(&time, unit.template.as_str(), event, unit.position, owner);
            }
        }
        if let Some(mut playing) = playing.filter(|_| unit.alive()) {
            let speed = sim.0.organizations[unit.organization].speed;
            playing.set_moving(unit.moving().then_some(speed));
        }
    }
}

/// How far into its sinking `remains` are, from 0 while they rot to 1 as
/// they go, a fraction `t` of a tick past `ticks`; `None` once taken off.
pub(crate) fn sunk(remains: tgsim::Remains, ticks: u64, t: f32) -> Option<f32> {
    let now = ticks as f32 + t;
    let [sinking, gone] = [remains.sinking, remains.gone].map(|k| k as f32);
    match now < gone {
        true => Some(((now - sinking) / (gone - sinking)).clamp(0.0, 1.0)),
        false => None,
    }
}

/// Plays buildings' attacks as they fire, sinks the destroyed into the
/// ground, and sinks a site an effect lowers by the share of its health
/// it's yet to gain.
#[allow(clippy::too_many_arguments)]
pub(crate) fn show_buildings(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    fog: Res<fog::Fog>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    mut sounds: ResMut<sound::Sounds>,
    lowering: Query<&effects::Lowers>,
    mut views: Query<(
        Entity,
        &mut BuildingView,
        &mut Transform,
        &mut Visibility,
        Option<&mut Playing>,
    )>,
) {
    // The sound of a fall is for one seen to fall, not first seen fallen.
    const FRESH: f32 = 3.0;
    let lowered: Vec<Entity> = lowering.iter().map(|l| l.0).collect();
    for (e, mut view, mut transform, mut visibility, playing) in &mut views {
        // Left as last seen until seen again.
        if fog.building(view.building) != fog::Look::Shown {
            continue;
        }
        let building = &sim.0.buildings[view.building];
        // What's in the air is drawn with its delivery.
        if building.descent.is_some() {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        // One that came down somewhere other than where it was let go.
        let [x, y] = building.position;
        let here = world(x, y, view.placed.y);
        if here != view.placed {
            let height = loaded
                .map
                .ground
                .stand_at(Footing::Ground, building.position);
            view.placed = world(x, y, height);
        }
        if let (State::Destroyed(fell), false) = (building.state, view.destroyed) {
            view.destroyed = true;
            // Not for one that fell out of sight, first seen fallen now.
            let since = (sim.0.ticks - fell) as f32 * TICK;
            if since < FRESH {
                let (at, owner) = (building.position, building.owner);
                sounds.cue(
                    &time,
                    building.template.as_str(),
                    |a| &a.destroyed,
                    at,
                    owner,
                );
            }
        }
        if let Some(remains) = sim.0.building_remains(view.building) {
            match sunk(remains, sim.0.ticks, fixed.overstep_fraction()) {
                Some(sunk) => transform.translation = view.placed - Vec3::Y * view.depth * sunk,
                None => *visibility = Visibility::Hidden,
            }
            continue;
        }
        let missing = match lowered.contains(&e) {
            true => {
                1.0 - (building.health / sim.0.building_max_health(view.building)).clamp(0.0, 1.0)
            }
            false => 0.0,
        };
        let at = view.placed - Vec3::Y * view.depth * missing;
        if transform.translation != at {
            transform.translation = at;
        }
        if building.shot != view.shot {
            view.shot = building.shot;
            let weapon = building
                .shot
                .and_then(|(_, w)| building.fighter.weapons.get(w));
            if let (Some(w), Some(target)) = (weapon, building.target) {
                let to = sim.0.position(target);
                sounds.shot(&time, &w.sounds, building.position, to, building.owner);
            }
            if let (Some(mut p), Some(ids)) = (playing, weapon.and_then(|w| w.animation.as_deref()))
            {
                p.use_ability(ids);
            }
        }
    }
}
