//! The views a map's triggers move the camera to.

use tgck::map::{Detail, Thing};

/// A view placed in the map, by the `camera` record of its object.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    /// Its object's id, which triggers name it by.
    pub id: u32,
    pub name: String,
    /// The point it looks at.
    pub position: [f32; 3],
    /// The way from that point to where it looks from, as `rmp.ksy`
    /// describes a camera's.
    pub facing: [f32; 3],
    pub zoom: f32,
}

impl Camera {
    /// The camera a map object is, if it's one.
    pub fn of(thing: &Thing) -> Option<Camera> {
        let Detail::Camera(camera) = &thing.detail else {
            return None;
        };
        Some(Camera {
            id: thing.id,
            name: camera.name.clone(),
            position: thing.position,
            facing: thing.facing,
            zoom: camera.zoom,
        })
    }

    /// Which way it looks from the point it looks at, turned about the
    /// vertical from the map's +x towards +y, and how far above the
    /// horizontal, both in radians.
    pub fn angles(&self) -> (f32, f32) {
        let [x, y, rise] = self.facing;
        let across = libm::hypotf(x, y);
        (libm::atan2f(y, x), libm::atan2f(rise, across))
    }
}
