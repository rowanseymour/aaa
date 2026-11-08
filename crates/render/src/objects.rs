//! Placeholder markers for placed objects without a readable model: units,
//! start locations, magenta boxes for everything else that should have had a
//! model, and red boxes for objects that have none such as sound emitters.

use std::collections::HashMap;

use aaa_data::Footing;
use aaa_map::Ground;
use bevy::prelude::*;
use tgck::map::Thing;

use crate::MapObject;
use crate::terrain::world;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Start,
    Unit(usize),
    Missing,
    Modelless,
}

/// Marks a placeholder marker, which [`toggle_markers`] shows and hides.
#[derive(Component)]
pub struct Marker;

const NATIONS: [(&str, [f32; 3]); 5] = [
    ("usa", [0.3, 0.45, 0.2]),
    ("gbr", [0.75, 0.6, 0.35]),
    ("ger", [0.45, 0.45, 0.5]),
    ("rus", [0.6, 0.15, 0.1]),
    ("jpn", [0.85, 0.8, 0.2]),
];

fn kind(template: &str, should_have_model: bool) -> Kind {
    let t = template.to_ascii_lowercase();
    if t.contains("start_loc") {
        Kind::Start
    } else if let Some(n) = NATIONS
        .iter()
        .position(|(n, _)| t.starts_with("regiment_") && t.contains(&format!("_{n}_")))
    {
        Kind::Unit(n)
    } else if should_have_model {
        Kind::Missing
    } else {
        Kind::Modelless
    }
}

fn shape(kind: Kind) -> (Mesh, [f32; 3], f32) {
    match kind {
        Kind::Start => (Cylinder::new(0.8, 12.0).into(), [1.0, 0.1, 0.1], 6.0),
        Kind::Unit(n) => (Cuboid::new(2.5, 1.0, 1.5).into(), NATIONS[n].1, 0.5),
        Kind::Missing => (Cuboid::new(3.0, 2.5, 3.0).into(), [1.0, 0.0, 1.0], 1.25),
        Kind::Modelless => (Cuboid::new(1.5, 1.5, 1.5).into(), [0.9, 0.1, 0.1], 0.75),
    }
}

pub fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    ground: &Ground,
    footing: impl Fn(&str) -> Footing,
    things: &[(&Thing, bool)],
    show: bool,
) {
    let mut handles = HashMap::new();
    for &(thing, should_have_model) in things {
        let kind = kind(&thing.template, should_have_model);
        let (mesh, material, lift) = handles
            .entry(kind)
            .or_insert_with(|| {
                let (mesh, [r, g, b], lift) = shape(kind);
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
            ground.stand_at(footing(&thing.template), x, y) + lift,
        ));
        let [fx, fy, _] = thing.facing;
        if fx != 0.0 || fy != 0.0 {
            transform.look_to(world(fx, fy, 0.0), Vec3::Y);
        }
        commands.spawn((
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
    }
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
