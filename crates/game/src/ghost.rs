//! While Unpack is being aimed, the building the truck would unpack into
//! follows the cursor where it would stand, on the map or through the
//! minimap, drawn all in red where it wouldn't fit.

use aaa_render::MapAssets;
use aaa_render::models::repaint;
use aaa_render::terrain::world;
use bevy::prelude::*;

use crate::actions::{Aim, Page};
use crate::{Drawn, Loaded, Local, Pointer, Pointing, Screen, Selected, Sim, thing_at};

/// The preview shown: its root, what it is, where, and whether it fits.
#[derive(Resource, Default)]
pub struct Ghost(Option<(Entity, String, [f32; 2], bool)>);

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
    mut loaded: ResMut<Loaded>,
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
        (u, at, sim.0.room_to_unpack(u, at))
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
                let footing = drawn.0.footing(&unpacks.building.template);
                t.translation = world(x, y, ground.stand_at(footing, x, y));
            }
        }
        None => {
            let b = &unpacks.building;
            let footing = drawn.0.footing(&b.template);
            let thing = thing_at(ground, footing, b.id, &b.template, at, b.facing);
            let owner = local.0.unwrap_or(0);
            let Loaded { game, .. } = &mut *loaded;
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
