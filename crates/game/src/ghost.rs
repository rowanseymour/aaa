//! While Unpack is being aimed, the building the truck would unpack into
//! follows the cursor where it would stand, on the map or through the
//! minimap, drawn all in red where it wouldn't fit.

use aaa_render::MapAssets;
use aaa_render::models::repaint;
use aaa_render::terrain::world;
use bevy::prelude::*;

use crate::actions::{self, Aim, Page};
use crate::{Drawn, Loaded, Local, Pointer, Pointing, Screen, Selected, Sim, thing_at};

/// The preview shown: its root, what it is, where, and whether it fits.
#[derive(Resource, Default)]
pub struct Ghost(Option<(Entity, aaa_data::Id, [f32; 2], bool)>);

/// On a preview that doesn't fit, for [`redden`] to paint.
#[derive(Component)]
pub struct Unfit;

/// A mesh [`redden`] has painted.
#[derive(Component)]
pub struct Reddened;

/// Puts the preview where the cursor points, or takes it away.
#[allow(clippy::too_many_arguments)]
pub fn show(
    mut commands: Commands,
    page: Res<Page>,
    selected: Res<Selected>,
    local: Res<Local>,
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    mut drawn: ResMut<Drawn>,
    mut assets: MapAssets,
    mut ghost: ResMut<Ghost>,
    pointer: Pointer,
    mut transforms: Query<&mut Transform>,
) {
    let unpacks = match (*page, &selected.0[..]) {
        (Page::Aiming(Aim::Unpack), &[r]) => sim.0.regiments[r].unpacks.as_ref(),
        _ => None,
    };
    let point = pointer.at(&loaded.map.ground).map(Pointing::point);
    let wanted = unpacks.zip(point).map(|(u, p)| {
        let at = p.map(f32::round);
        (u, at, sim.0.room_to_unpack(u, at, actions::UNPACK_FACING))
    });
    // A different building, or one that's turned to fit or not, is drawn
    // afresh.
    let keep = match (&ghost.0, &wanted) {
        (Some((_, t, _, fits)), Some((u, _, f))) => *t == u.building.template && fits == f,
        _ => false,
    };
    if !keep && let Some((e, ..)) = ghost.0.take() {
        commands.entity(e).despawn();
    }
    let Some((unpacks, at, fits)) = wanted else {
        return;
    };
    let ground = &loaded.map.ground;
    let [x, y] = at;
    match &mut ghost.0 {
        Some((e, _, placed, _)) => {
            if *placed != at
                && let Ok(mut t) = transforms.get_mut(*e)
            {
                *placed = at;
                let footing = drawn.0.footing(unpacks.building.template.as_str());
                t.translation = world(x, y, ground.stand_at(footing, [x, y]));
            }
        }
        None => {
            let b = &unpacks.building;
            let footing = drawn.0.footing(b.template.as_str());
            let thing = thing_at(
                ground,
                footing,
                b.id,
                b.template.as_str(),
                at,
                actions::UNPACK_FACING,
            );
            let owner = local.0.unwrap_or(0);
            let Loaded { game, .. } = &*loaded;
            let mut models = assets.models();
            let Some(e) = drawn
                .0
                .spawn_building(&mut commands, game, &mut models, &thing, owner)
            else {
                return;
            };
            commands.entity(e).insert(DespawnOnExit(Screen::Match));
            if !fits {
                commands.entity(e).insert(Unfit);
            }
            ghost.0 = Some((e, b.template.clone(), at, fits));
        }
    }
}

/// Paints the meshes of a preview that doesn't fit flat red.
pub fn redden(
    mut commands: Commands,
    unfit: Query<Entity, With<Unfit>>,
    children: Query<&Children>,
    meshes: Query<(), (With<Mesh3d>, Without<Reddened>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut red: bevy::prelude::Local<Option<Handle<StandardMaterial>>>,
) {
    for root in &unfit {
        for e in children.iter_descendants(root) {
            if !meshes.contains(e) {
                continue;
            }
            let red = red
                .get_or_insert_with(|| {
                    materials.add(StandardMaterial {
                        base_color: Color::srgb(0.9, 0.0, 0.0),
                        unlit: true,
                        ..default()
                    })
                })
                .clone();
            let mut mesh = commands.entity(e);
            repaint(&mut mesh, red);
            mesh.insert(Reddened);
        }
    }
}

/// The preview of a regiment being aimed for delivery: its members'
/// models, its template, where it's centred and whether it can land
/// there.
#[derive(Resource, Default)]
pub struct Landing(Option<(Vec<Entity>, aaa_data::Id, [f32; 2], bool)>);

/// While a delivery is being aimed, the regiment follows the cursor over
/// the map, each member where it would stand, drawn all in red where it
/// couldn't land.
#[allow(clippy::too_many_arguments)]
pub fn land(
    mut commands: Commands,
    page: Res<Page>,
    chosen: Res<crate::SelectedBuilding>,
    looks: Res<actions::Looks>,
    local: Res<Local>,
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    mut drawn: ResMut<Drawn>,
    mut assets: MapAssets,
    mut landing: ResMut<Landing>,
    pointer: Pointer,
    mut transforms: Query<&mut Transform>,
) {
    let regiment = looks.delivered(&sim.0, chosen.0, *page);
    // The minimap doesn't aim one: it stays where it was.
    let point = match pointer.at(&loaded.map.ground) {
        Some(Pointing::Ground(p)) => Some(p),
        _ => landing.0.as_ref().map(|(_, _, at, _)| *at),
    };
    let player = local.0.unwrap_or(0);
    let wanted = regiment.zip(point).map(|(r, at)| {
        let fits = sim.0.room_to_land(player, &r.template, at);
        (r, at, fits)
    });
    let keep = match (&landing.0, &wanted) {
        (Some((_, t, _, fits)), Some((r, _, f))) => *t == r.template && fits == f,
        _ => false,
    };
    if !keep && let Some((members, ..)) = landing.0.take() {
        for e in members {
            commands.entity(e).despawn();
        }
    }
    let Some((regiment, at, fits)) = wanted else {
        return;
    };
    let ground = &loaded.map.ground;
    let mut standing = regiment.clone();
    standing.place(at, LANDING_FACING);
    match &mut landing.0 {
        Some((members, _, placed, _)) => {
            if *placed == at {
                return;
            }
            *placed = at;
            for (&e, m) in members.iter().zip(&standing.members) {
                if let Ok(mut t) = transforms.get_mut(e) {
                    let footing = drawn.0.footing(m.template.as_str());
                    let [x, y] = m.position;
                    t.translation = world(x, y, ground.stand_at(footing, m.position));
                }
            }
        }
        None => {
            let Loaded { game, .. } = &*loaded;
            let mut models = assets.models();
            let members = standing.members.iter().filter_map(|m| {
                let template = m.template.as_str();
                let footing = drawn.0.footing(template);
                let thing = thing_at(ground, footing, m.id, template, m.position, m.facing);
                let e = drawn.0.spawn_unit(
                    &mut commands,
                    game,
                    &mut models,
                    &thing,
                    player,
                    m.speed,
                )?;
                commands.entity(e).insert(DespawnOnExit(Screen::Match));
                if !fits {
                    commands.entity(e).insert(Unfit);
                }
                Some(e)
            });
            let members = members.collect();
            landing.0 = Some((members, regiment.template.clone(), at, fits));
        }
    }
}

/// The way a regiment being aimed for delivery faces.
const LANDING_FACING: [f32; 2] = [0.0, -1.0];
