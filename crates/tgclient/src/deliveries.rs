//! The aircraft bringing organizations in, flying over the map or diving
//! to the ground shot down, and what they've dropped coming down under
//! its parachute; and the aircraft airfields send out.

use std::collections::HashMap;

use bevy::prelude::*;
use tgdata::Footing;
use tgrender::MapAssets;
use tgrender::models::pose;
use tgsim::Seen;

use crate::views::thing_at;
use crate::{Drawn, Loaded, Local, Screen, Sim, fog};

/// What's drawn of the sim's deliveries: each one's aircraft, by its
/// index, and the falling model of each unit coming down, by its index.
#[derive(Resource, Default)]
pub struct Flights {
    /// `None` for one with no model to draw.
    planes: HashMap<usize, Option<Entity>>,
    falling: HashMap<usize, Option<Entity>>,
    /// The falling model of each building coming down, by its index.
    crates: HashMap<usize, Option<Entity>>,
    /// Each airfield's plane out, by its sortie's index.
    sorties: HashMap<usize, Option<Entity>>,
    /// The scale each of those is drawn at in full.
    full_size: HashMap<Entity, Vec3>,
    /// How many of the sim's deliveries, from the first, are over: gone,
    /// with all they brought on the ground.
    over: usize,
}

/// Draws each aircraft where it flies, until it leaves the map, and each
/// unit it's dropped where it's fallen to, until it lands.
#[allow(clippy::too_many_arguments)]
pub fn show(
    mut commands: Commands,
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    local: Res<Local>,
    fog: Res<fog::Fog>,
    fixed: Res<Time<Fixed>>,
    mut drawn: ResMut<Drawn>,
    mut assets: MapAssets,
    mut flights: ResMut<Flights>,
    mut placed: Query<(&mut Transform, &mut Visibility)>,
) {
    let map = &loaded.map;
    let ground = &map.ground;
    let t = fixed.overstep_fraction();
    let flights = &mut *flights;
    // A side sees its own and its allies' wherever they are.
    let friends = local.0.map(|p| sim.0.sharing_sight(p));
    let seen = |owner: u32, at: [f32; 2]| {
        let ours = friends.as_ref().is_none_or(|f| f.contains(&owner));
        crate::ui::visible(ours || fog.at(at) == Seen::Visible)
    };
    let full_size = &mut flights.full_size;
    // `size` is of the size it was first drawn at.
    let mut place = |e: Entity, at: Vec3, turned: Quat, shown: Visibility, size: f32| {
        if let Ok((mut transform, mut visibility)) = placed.get_mut(e) {
            (transform.translation, transform.rotation) = (at, turned);
            visibility.set_if_neq(shown);
            if size != 1.0 || full_size.contains_key(&e) {
                let full = *full_size.entry(e).or_insert(transform.scale);
                transform.scale = full * size;
            }
        }
    };
    let done = |d: &tgsim::Delivery| {
        (d.left.is_some() || d.down.is_some())
            && d.organization
                .is_none_or(|r| !sim.0.organizations[r].arriving)
            && d.building
                .is_none_or(|b| sim.0.buildings[b].descent.is_none())
    };
    for (d, delivery) in sim.0.deliveries.iter().enumerate().skip(flights.over) {
        // Level over where it's bound, whatever it crosses.
        let level = ground.stand_at(Footing::Ground, delivery.target) + delivery.height;
        // Down, its unit's remains are drawn instead.
        if delivery.left.is_some() || delivery.down.is_some() {
            if let Some(Some(e)) = flights.planes.remove(&d) {
                commands.entity(e).despawn();
            }
        } else {
            let at = delivery.drawn_at(t);
            let plane = *flights.planes.entry(d).or_insert_with(|| {
                let template = delivery.template.as_str();
                let thing = thing_at(
                    ground,
                    Footing::Ground,
                    delivery.id,
                    template,
                    at,
                    delivery.facing,
                );
                let mut models = assets.models();
                let e =
                    drawn
                        .0
                        .spawn_unit(&mut commands, &mut models, &thing, delivery.owner, 0.0)?;
                commands.entity(e).insert(DespawnOnExit(Screen::Match));
                Some(e)
            });
            let (mut over, turned) = pose(ground, at, delivery.facing, None, Footing::Ground);
            over.y = level;
            // Nose down as steeply as it dives: turning the way it's
            // heading back about this axis takes it down towards the ground.
            let [fx, fy] = delivery.facing;
            let axis = tgrender::space::world(fx, fy, 0.0).cross(Vec3::Y);
            let dip = Quat::from_axis_angle(axis.normalize_or_zero(), -delivery.dive().atan());
            let turned = dip * turned;
            if let Some(e) = plane {
                place(e, over, turned, seen(delivery.owner, at), 1.0);
            }
        }
        if let Some(b) = delivery.building {
            let building = &sim.0.buildings[b];
            let Some(descent) = building.descent.filter(|_| !building.gone()) else {
                if let Some(Some(e)) = flights.crates.remove(&b) {
                    commands.entity(e).despawn();
                }
                continue;
            };
            let at = descent.over(t);
            let falling = *flights.crates.entry(b).or_insert_with(|| {
                let template = building.template.as_str();
                let thing = thing_at(
                    ground,
                    Footing::Ground,
                    building.id,
                    template,
                    at,
                    building.facing,
                );
                let mut models = assets.models();
                let e =
                    drawn
                        .0
                        .spawn_falling(&mut commands, &mut models, &thing, delivery.owner)?;
                commands.entity(e).insert(DespawnOnExit(Screen::Match));
                Some(e)
            });
            let (mut over, turned) = pose(ground, at, building.facing, None, Footing::Ground);
            let lands = ground.stand_at(Footing::Ground, descent.lands());
            let let_go = level - delivery.height + descent.dropped_from();
            over.y = lands + (let_go - lands) * descent.left(t);
            if let Some(e) = falling {
                place(e, over, turned, seen(delivery.owner, at), 1.0);
            }
        }
        let Some(r) = delivery.organization else {
            continue;
        };
        for &u in &sim.0.organizations[r].units {
            let unit = &sim.0.units[u];
            let Some(descent) = unit.descent else {
                if let Some(Some(e)) = flights.falling.remove(&u) {
                    commands.entity(e).despawn();
                }
                continue;
            };
            let at = unit.drawn_at(t);
            let falling = *flights.falling.entry(u).or_insert_with(|| {
                let template = unit.template.as_str();
                let thing = thing_at(ground, Footing::Ground, unit.id, template, at, unit.facing);
                let mut models = assets.models();
                let e =
                    drawn
                        .0
                        .spawn_falling(&mut commands, &mut models, &thing, delivery.owner)?;
                commands.entity(e).insert(DespawnOnExit(Screen::Match));
                Some(e)
            });
            // From where the aircraft let it go, however high the ground
            // is there, down to the ground where it lands.
            let (mut over, turned) = pose(ground, at, unit.facing, None, Footing::Ground);
            let lands = ground.stand_at(Footing::Ground, descent.lands());
            let let_go = level - delivery.height + descent.dropped_from();
            over.y = lands + (let_go - lands) * descent.left(t);
            if let Some(e) = falling {
                place(e, over, turned, seen(delivery.owner, at), 1.0);
            }
        }
    }
    // Those over, from the first, are passed over from here on.
    let rest = sim.0.deliveries.iter().skip(flights.over);
    flights.over += rest.take_while(|d| done(d)).count();
    let mut forgotten = Vec::new();
    for (s, sortie) in sim.0.sorties.iter().enumerate() {
        let Some(u) = sortie.plane.filter(|_| !sortie.stage.over()) else {
            if let Some(Some(e)) = flights.sorties.remove(&s) {
                forgotten.push(e);
                commands.entity(e).despawn();
            }
            continue;
        };
        let unit = &sim.0.units[u];
        let at = unit.drawn_at(t);
        let plane = *flights.sorties.entry(s).or_insert_with(|| {
            let template = unit.template.as_str();
            let thing = thing_at(ground, Footing::Ground, unit.id, template, at, unit.facing);
            let mut models = assets.models();
            let e = drawn
                .0
                .spawn_unit(&mut commands, &mut models, &thing, sortie.owner, 0.0)?;
            commands.entity(e).insert(DespawnOnExit(Screen::Match));
            Some(e)
        });
        let (mut over, turned) = pose(ground, at, unit.facing, None, Footing::Ground);
        over.y = ground.stand_at(Footing::Ground, at) + sortie.height;
        if let Some(e) = plane {
            // Smaller near the ground.
            let size = sim.0.flight_scale(s);
            place(e, over, turned, seen(sortie.owner, at), size);
        }
    }
    for e in forgotten {
        flights.full_size.remove(&e);
    }
}

/// Forgets what was drawn of the last match's deliveries.
pub fn reset(mut flights: ResMut<Flights>) {
    *flights = Flights::default();
}
