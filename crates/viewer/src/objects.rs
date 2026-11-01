//! Placeholder markers for placed objects without a readable model: units,
//! start locations, and objects whose templates name a model.

use std::collections::HashMap;

use bevy::prelude::*;
use tgck::map::Thing;

use crate::terrain::{Ground, world};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Tree,
    Rock,
    Start,
    Unit(usize),
    Other,
}

const NATIONS: [(&str, [f32; 3]); 5] = [
    ("usa", [0.3, 0.45, 0.2]),
    ("gbr", [0.75, 0.6, 0.35]),
    ("ger", [0.45, 0.45, 0.5]),
    ("rus", [0.6, 0.15, 0.1]),
    ("jpn", [0.85, 0.8, 0.2]),
];

fn kind(template: &str) -> Option<Kind> {
    let t = template.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    Some(if has(&["camera"]) {
        return None;
    } else if has(&["start_loc"]) {
        Kind::Start
    } else if has(&["tree", "forest", "bush", "hedge", "palm", "jungle"]) {
        Kind::Tree
    } else if has(&["rock", "boulder"]) {
        Kind::Rock
    } else if let Some(n) = NATIONS
        .iter()
        .position(|(n, _)| t.starts_with("regiment_") && t.contains(&format!("_{n}_")))
    {
        Kind::Unit(n)
    } else {
        Kind::Other
    })
}

fn shape(kind: Kind) -> (Mesh, [f32; 3], f32) {
    match kind {
        Kind::Tree => (Cone::new(1.0, 4.0).into(), [0.13, 0.3, 0.1], 2.0),
        Kind::Rock => (Cuboid::new(1.2, 0.8, 1.2).into(), [0.45, 0.43, 0.4], 0.4),
        Kind::Start => (Cylinder::new(0.8, 12.0).into(), [1.0, 0.1, 0.1], 6.0),
        Kind::Unit(n) => (Cuboid::new(2.5, 1.0, 1.5).into(), NATIONS[n].1, 0.5),
        Kind::Other => (Cuboid::new(3.0, 2.5, 3.0).into(), [0.7, 0.62, 0.5], 1.25),
    }
}

pub fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    ground: &Ground,
    things: &[(&Thing, bool)],
) {
    let mut handles = HashMap::new();
    for &(thing, has_model) in things {
        let Some(kind) = kind(&thing.template) else {
            continue;
        };
        if !has_model && !matches!(kind, Kind::Start | Kind::Unit(_)) {
            continue;
        }
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
        let mut transform = Transform::from_translation(world(x, y, ground.height_at(x, y) + lift));
        let [fx, fy, _] = thing.facing;
        if fx != 0.0 || fy != 0.0 {
            transform.look_to(world(fx, fy, 0.0), Vec3::Y);
        }
        commands.spawn((
            Name::new(thing.template.clone()),
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
        ));
    }
}
