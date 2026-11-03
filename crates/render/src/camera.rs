//! An RTS-style camera orbiting a focus point on the ground.
//!
//! It starts in the game's own view: the map seen corner-on from a fixed
//! angle, which only pans and zooms. Tab frees it to rotate and tilt.

use std::f32::consts::FRAC_PI_4;

use aaa_map::Ground;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;

use crate::terrain::world;

// Estimated from screenshots of the game, not read from its data.
const GAME_YAW: f32 = FRAC_PI_4;
const GAME_PITCH: f32 = 0.85;
const GAME_FOV: f32 = 0.52;
const GAME_DISTANCE: (f32, f32) = (40.0, 250.0);

const FREE_FOV: f32 = FRAC_PI_4;
const FREE_DISTANCE: (f32, f32) = (5.0, 2000.0);

#[derive(Component)]
pub struct RtsCamera {
    pub focus: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub free: bool,
}

impl RtsCamera {
    pub fn new(focus: Vec3) -> Self {
        RtsCamera {
            focus,
            yaw: GAME_YAW,
            pitch: GAME_PITCH,
            distance: 150.0,
            free: false,
        }
    }

    /// The game's view of the middle of a map.
    pub fn centred(ground: &Ground) -> Self {
        let half = ground.size() / 2.0;
        RtsCamera::new(world(half, half, ground.height_at(half, half)))
    }

    pub fn transform(&self) -> Transform {
        let offset = Quat::from_euler(EulerRot::YXZ, self.yaw, -self.pitch, 0.0)
            * Vec3::new(0.0, 0.0, self.distance);
        Transform::from_translation(self.focus + offset).looking_at(self.focus, Vec3::Y)
    }

    pub fn projection(&self) -> Projection {
        Projection::Perspective(PerspectiveProjection {
            fov: if self.free { FREE_FOV } else { GAME_FOV },
            ..default()
        })
    }
}

/// Spawns the camera that looks at a map.
pub fn spawn(commands: &mut Commands, cam: RtsCamera) {
    // crate::light matches the game only without tonemapping.
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        cam.projection(),
        cam.transform(),
        cam,
    ));
}

pub fn update(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    mut cameras: Query<(&mut RtsCamera, &mut Transform, &mut Projection)>,
) {
    let dt = time.delta_secs();
    for (mut cam, mut transform, mut projection) in &mut cameras {
        if keys.just_pressed(KeyCode::Tab) {
            cam.free = !cam.free;
            if !cam.free {
                cam.yaw = GAME_YAW;
                cam.pitch = GAME_PITCH;
            }
            *projection = cam.projection();
        }

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

        if cam.free {
            cam.yaw += (pressed(&[KeyCode::KeyQ]) - pressed(&[KeyCode::KeyE])) * 1.5 * dt;
            cam.pitch += (pressed(&[KeyCode::KeyR]) - pressed(&[KeyCode::KeyF])) * 1.0 * dt;
            if buttons.pressed(MouseButton::Right) {
                cam.yaw -= motion.delta.x * 0.005;
                cam.pitch += motion.delta.y * 0.005;
            }
            cam.pitch = cam.pitch.clamp(0.1, 1.55);
        }

        let lines = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        let (near, far) = if cam.free {
            FREE_DISTANCE
        } else {
            GAME_DISTANCE
        };
        cam.distance = (cam.distance * 0.9f32.powf(lines)).clamp(near, far);

        *transform = cam.transform();
    }
}
