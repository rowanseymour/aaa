//! The aircraft bringing regiments in, flying over the map, and what
//! they've dropped coming down under its parachute.

use std::collections::HashMap;

use aaa_data::Footing;
use aaa_render::MapAssets;
use aaa_render::models::pose;
use aaa_sim::Seen;
use bevy::prelude::*;

use crate::{Drawn, Loaded, Local, Screen, Sim, fog, thing_at};

/// What's drawn of the sim's deliveries: each one's aircraft, by its
/// index, and the falling model of each unit coming down, by its index.
#[derive(Resource, Default)]
pub struct Flights {
    /// `None` for one with no model to draw.
    planes: HashMap<usize, Option<Entity>>,
    falling: HashMap<usize, Option<Entity>>,
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
    let Loaded { game, map } = &*loaded;
    let ground = &map.ground;
    let t = fixed.overstep_fraction();
    let flights = &mut *flights;
    // A side sees its own and its allies' wherever they are.
    let friends = local.0.map(|p| sim.0.sharing_sight(p));
    let seen = |owner: u32, at: [f32; 2]| {
        let ours = friends.as_ref().is_none_or(|f| f.contains(&owner));
        match ours || fog.at(at) == Seen::Visible {
            true => Visibility::Inherited,
            false => Visibility::Hidden,
        }
    };
    let mut place = |e: Entity, at: Vec3, turned: Quat, shown: Visibility| {
        if let Ok((mut transform, mut visibility)) = placed.get_mut(e) {
            (transform.translation, transform.rotation) = (at, turned);
            visibility.set_if_neq(shown);
        }
    };
    let done = |d: &aaa_sim::Delivery| {
        d.left.is_some() && d.regiment.is_none_or(|r| !sim.0.regiments[r].arriving)
    };
    for (d, delivery) in sim.0.deliveries.iter().enumerate().skip(flights.over) {
        // Level over where it's bound, whatever it crosses.
        let level = ground.stand_at(Footing::Ground, delivery.target) + delivery.height;
        if delivery.left.is_some() {
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
                let e = drawn.0.spawn_unit(
                    &mut commands,
                    game,
                    &mut models,
                    &thing,
                    delivery.owner,
                    0.0,
                )?;
                commands.entity(e).insert(DespawnOnExit(Screen::Match));
                Some(e)
            });
            let (mut over, turned) = pose(ground, at, delivery.facing, None, Footing::Ground);
            over.y = level;
            if let Some(e) = plane {
                place(e, over, turned, seen(delivery.owner, at));
            }
        }
        let Some(r) = delivery.regiment else {
            continue;
        };
        for &u in &sim.0.regiments[r].units {
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
                let e = drawn.0.spawn_falling(
                    &mut commands,
                    game,
                    &mut models,
                    &thing,
                    delivery.owner,
                )?;
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
                place(e, over, turned, seen(delivery.owner, at));
            }
        }
    }
    // Those over, from the first, are passed over from here on.
    let rest = sim.0.deliveries.iter().skip(flights.over);
    flights.over += rest.take_while(|d| done(d)).count();
}

/// Forgets what was drawn of the last match's deliveries.
pub fn reset(mut flights: ResMut<Flights>) {
    *flights = Flights::default();
}
