//! What's measured of a model file: read once, however often it's asked.

use std::collections::HashMap;

use bevy::prelude::*;
use nif::Nif;

use crate::models::EffectTiming;
use crate::portrait::{self, Seen};
use crate::rig::Nodes;

/// How far above a model's lowest point still counts as its foot.
const FOOT: f32 = 0.1;

pub(crate) struct Measures {
    /// Where each named node is, as [`Nodes::points`] has them.
    pub points: HashMap<String, nif::Transform>,
    /// How big its meshes are, as modelled.
    pub extent: Option<Extent>,
    /// How it plays as an effect.
    pub timing: Option<EffectTiming>,
    /// How the camera it holds for its portrait sees it.
    pub camera: Option<Seen>,
}

impl Measures {
    pub fn of(nif: &Nif, nodes: &Nodes) -> Measures {
        let mut points = Vec::new();
        for placed in nif.shapes() {
            let Some(g) = nif.get_as::<nif::Geometry>(placed.shape.data) else {
                continue;
            };
            points.extend(g.positions.iter().map(|&p| placed.transform.apply(p)));
        }
        Measures {
            points: nodes.points(),
            extent: Extent::of(&points),
            timing: EffectTiming::of(nif),
            camera: portrait::seen_by(nif, &nodes.frames),
        }
    }
}

/// How big a model is, in its own units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Extent {
    /// The height of its lowest point and its highest.
    pub low: f32,
    pub high: f32,
    /// How far along either of its ground axes its foot reaches from its
    /// origin.
    pub foot: f32,
    /// How far from its origin its furthest point is.
    pub furthest: f32,
}

impl Extent {
    /// The extent of a model with these points, up being their last axis.
    pub fn of(points: &[[f32; 3]]) -> Option<Extent> {
        let heights = points.iter().map(|p| p[2]);
        let low = heights.clone().reduce(f32::min)?;
        let high = heights.reduce(f32::max)?;
        let foot = points.iter().filter(|p| p[2] <= low + FOOT);
        Some(Extent {
            low,
            high,
            foot: foot.fold(0.0, |r, p| r.max(p[0].abs()).max(p[1].abs())),
            furthest: points
                .iter()
                .fold(0.0, |r, &p| r.max(Vec3::from(p).length())),
        })
    }

    pub fn height(&self) -> f32 {
        self.high - self.low
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(name: &str, translation: [f32; 3]) -> nif::SceneObject {
        nif::SceneObject {
            name: name.into(),
            extra_data: Vec::new(),
            controller: None,
            flags: 0,
            transform: nif::Transform {
                translation,
                ..nif::Transform::IDENTITY
            },
            properties: Vec::new(),
        }
    }

    #[test]
    fn a_model_is_measured_where_its_nodes_put_things() {
        // A turret 2 up holding a gun 1 ahead, and a mesh under the turret
        // reaching from its node to 3 above it.
        let node = |name, at, children| {
            nif::Object::Node(nif::Node {
                av: scene(name, at),
                children,
                billboard: None,
            })
        };
        let nif = Nif {
            objects: vec![
                node("root", [0.0; 3], vec![Some(1)]),
                node("turret", [0.0, 0.0, 2.0], vec![Some(2), Some(3)]),
                node("gun", [0.0, -1.0, 0.0], Vec::new()),
                nif::Object::Shape(nif::Shape {
                    av: scene("hull", [0.0; 3]),
                    data: Some(4),
                    skin: None,
                }),
                nif::Object::Geometry(nif::Geometry {
                    positions: vec![[0.0; 3], [0.0, 0.0, 3.0]],
                    ..Default::default()
                }),
            ],
            roots: vec![Some(0)],
            incomplete: None,
        };
        let measures = Measures::of(&nif, &Nodes::new(&nif));
        let gun = measures.points.get("gun").map(|t| t.translation);
        assert_eq!(gun, Some([0.0, -1.0, 2.0]));
        let extent = measures.extent.expect("an extent");
        assert_eq!((extent.low, extent.high), (2.0, 5.0));
        assert_eq!(measures.timing, None);
        assert!(measures.camera.is_none());
    }

    #[test]
    fn a_models_extent_is_its_height_and_its_foots_reach() {
        let points = [
            [3.0, -1.0, 0.5],
            [-2.0, 4.0, 0.55],
            // Above the foot, so its reach doesn't count.
            [9.0, 0.0, 2.0],
            [0.0, 0.0, 6.5],
        ];
        let extent = Extent::of(&points).unwrap();
        assert_eq!((extent.low, extent.high), (0.5, 6.5));
        assert_eq!((extent.height(), extent.foot), (6.0, 4.0));
        assert!((extent.furthest - 85f32.sqrt()).abs() < 1e-5);
        assert_eq!(Extent::of(&[]), None);
    }
}
