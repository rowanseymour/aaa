//! Between game space (x east, y north, z up) and Bevy's (x east, y up,
//! -z north).

use std::f32::consts::FRAC_PI_2;

use bevy::math::Affine3A;
use bevy::prelude::*;

/// A point in game space in Bevy's.
pub fn world(x: f32, y: f32, height: f32) -> Vec3 {
    Vec3::new(x, height, -y)
}

/// A point or direction in game space, as its three coordinates, in Bevy's.
pub fn in_world([x, y, z]: [f32; 3]) -> Vec3 {
    world(x, y, z)
}

/// Bevy's space back to game space's x and y; the inverse of [`world`].
pub fn map_point(v: Vec3) -> [f32; 2] {
    [v.x, -v.z]
}

/// Column `c` of a game-space rotation: where it turns that axis to, in
/// Bevy's space.
pub(crate) fn column(rotation: &[[f32; 3]; 3], c: usize) -> Vec3 {
    world(rotation[0][c], rotation[1][c], rotation[2][c])
}

/// `facing` on the ground, or the way an unturned model faces, its -y, if
/// it has no length.
pub(crate) fn heading(facing: [f32; 2]) -> Vec2 {
    Vec2::from(facing).try_normalize().unwrap_or(Vec2::NEG_Y)
}

/// The turn about the vertical that has a model, which faces its -y, face
/// `facing` on the ground.
pub(crate) fn facing_turn([fx, fy]: [f32; 2]) -> Quat {
    Quat::from_rotation_y(fy.atan2(fx) + FRAC_PI_2)
}

/// A model-space transform in Bevy axes.
pub(crate) fn bevy_transform(t: &nif::Transform) -> Transform {
    // Conjugating the rotation by the change of basis: each game axis's
    // column becomes the Bevy axis it maps to.
    let c = |i| column(&t.rotation, i);
    Transform {
        translation: in_world(t.translation),
        rotation: Quat::from_mat3(&Mat3::from_cols(c(0), c(2), -c(1))),
        scale: Vec3::splat(t.scale),
    }
}

/// A model-space transform as a Bevy matrix.
pub(crate) fn affine(t: &nif::Transform) -> Affine3A {
    Affine3A::from_mat4(bevy_transform(t).to_matrix())
}
