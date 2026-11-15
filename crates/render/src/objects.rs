//! Placeholder markers for placed objects without a readable model:
//! organizations in their owner's colour, start locations, magenta boxes for everything else that should have had a
//! model, and red boxes for objects that have none such as sound emitters.

use std::collections::{HashMap, HashSet};

use aaa_data::Id;
use aaa_map::Ground;
use bevy::prelude::*;
use tgck::map::Thing;

use crate::terrain::world;
use crate::{MapObject, Models};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Start,
    /// An organization, by its owner's id; 0 for none.
    Organization(u32),
    Missing,
    Modelless,
}

/// Marks a placeholder marker, which [`toggle_markers`] shows and hides.
#[derive(Component)]
pub struct Marker;

/// The colour of an organization no player owns.
const UNOWNED: [f32; 3] = [0.5, 0.5, 0.5];

fn kind(thing: &Thing, organizations: &HashSet<Id>, should_have_model: bool) -> Kind {
    let t = thing.template.to_ascii_lowercase();
    if t.contains("start_loc") {
        Kind::Start
    } else if organizations.contains(&Id::from(t.as_str())) {
        Kind::Organization(aaa_map::owner(thing).unwrap_or(0))
    } else if should_have_model {
        Kind::Missing
    } else {
        Kind::Modelless
    }
}

fn shape(kind: Kind, colours: &HashMap<u32, [f32; 3]>) -> (Mesh, [f32; 3], f32) {
    match kind {
        Kind::Start => (Cylinder::new(0.8, 12.0).into(), [1.0, 0.1, 0.1], 6.0),
        Kind::Organization(owner) => {
            let colour = colours.get(&owner).copied().unwrap_or(UNOWNED);
            (Cuboid::new(2.5, 1.0, 1.5).into(), colour, 0.5)
        }
        Kind::Missing => (Cuboid::new(3.0, 2.5, 3.0).into(), [1.0, 0.0, 1.0], 1.25),
        Kind::Modelless => (Cuboid::new(1.5, 1.5, 1.5).into(), [0.9, 0.1, 0.1], 0.75),
    }
}

/// Spawns placeholder markers for `things`, each with whether it should
/// have had a model, returning them. `models` gives where each stands and
/// its owner's colour.
pub fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    ground: &Ground,
    models: &Models,
    things: &[(&Thing, bool)],
    show: bool,
) -> Vec<Entity> {
    let organizations: HashSet<Id> = models.game.organization_components().into_keys().collect();
    let mut handles = HashMap::new();
    let mut spawned = Vec::new();
    for &(thing, should_have_model) in things {
        let kind = kind(thing, &organizations, should_have_model);
        let (mesh, material, lift) = handles
            .entry(kind)
            .or_insert_with(|| {
                let (mesh, [r, g, b], lift) = shape(kind, &models.colours);
                (
                    meshes.add(mesh),
                    materials.add(StandardMaterial {
                        base_color: Color::srgb(r, g, b),
                        perceptual_roughness: 0.9,
                        ..default()
                    }),
                    lift,
                )
            })
            .clone();
        let [x, y, _] = thing.position;
        let mut transform = Transform::from_translation(world(
            x,
            y,
            ground.stand_at(models.footing(&thing.template), [x, y]) + lift,
        ));
        let [fx, fy, _] = thing.facing;
        if fx != 0.0 || fy != 0.0 {
            transform.look_to(world(fx, fy, 0.0), Vec3::Y);
        }
        let marker = commands.spawn((
            Name::new(thing.template.clone()),
            MapObject(thing.clone()),
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            Marker,
            if show {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
        ));
        spawned.push(marker.id());
    }
    spawned
}

/// Shows or hides all placeholder markers.
pub fn toggle_markers(
    keys: Res<ButtonInput<KeyCode>>,
    mut markers: Query<&mut Visibility, With<Marker>>,
) {
    if keys.just_pressed(KeyCode::KeyM) {
        for mut v in &mut markers {
            v.toggle_visible_hidden();
        }
    }
}
