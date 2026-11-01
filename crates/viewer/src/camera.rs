//! An RTS-style camera orbiting a focus point on the ground.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;

#[derive(Component)]
pub struct RtsCamera {
    pub focus: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

impl RtsCamera {
    pub fn new(focus: Vec3, distance: f32) -> Self {
        RtsCamera {
            focus,
            yaw: 0.0,
            pitch: 0.9,
            distance,
        }
    }

    pub fn transform(&self) -> Transform {
        let offset = Quat::from_euler(EulerRot::YXZ, self.yaw, -self.pitch, 0.0)
            * Vec3::new(0.0, 0.0, self.distance);
        Transform::from_translation(self.focus + offset).looking_at(self.focus, Vec3::Y)
    }
}

pub const HELP: &str =
    "WASD/arrows/middle drag: pan   Q/E/right drag: rotate   R/F: tilt   wheel: zoom";

pub fn update(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    mut cameras: Query<(&mut RtsCamera, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (mut cam, mut transform) in &mut cameras {
        let pressed = |codes: &[KeyCode]| keys.any_pressed(codes.iter().copied()) as i32 as f32;
        let mut pan = Vec2::new(
            pressed(&[KeyCode::KeyD, KeyCode::ArrowRight])
                - pressed(&[KeyCode::KeyA, KeyCode::ArrowLeft]),
            pressed(&[KeyCode::KeyS, KeyCode::ArrowDown])
                - pressed(&[KeyCode::KeyW, KeyCode::ArrowUp]),
        ) * cam.distance
            * dt;
        if buttons.pressed(MouseButton::Middle) {
            pan -= motion.delta * cam.distance * 0.002;
        }
        let rotation = Quat::from_rotation_y(cam.yaw);
        cam.focus += rotation * Vec3::new(pan.x, 0.0, pan.y);

        cam.yaw += (pressed(&[KeyCode::KeyQ]) - pressed(&[KeyCode::KeyE])) * 1.5 * dt;
        cam.pitch += (pressed(&[KeyCode::KeyR]) - pressed(&[KeyCode::KeyF])) * 1.0 * dt;
        if buttons.pressed(MouseButton::Right) {
            cam.yaw -= motion.delta.x * 0.005;
            cam.pitch += motion.delta.y * 0.005;
        }
        cam.pitch = cam.pitch.clamp(0.1, 1.55);

        let lines = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        cam.distance = (cam.distance * 0.9f32.powf(lines)).clamp(5.0, 2000.0);

        *transform = cam.transform();
    }
}
