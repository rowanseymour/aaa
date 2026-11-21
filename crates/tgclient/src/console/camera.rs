//! Verbs that point and turn the camera.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;
use tgrender::camera::RtsCamera;
use tgrender::space::{map_point, world as point};

use crate::Loaded;

use super::verbs::{Reply, number, usage};

/// Runs `f` on the match's camera and moves it there, since its controls,
/// which otherwise would, wait while the console is open.
pub(super) fn with_camera(
    world: &mut World,
    f: impl FnOnce(&mut RtsCamera, &tgmap::Ground) -> Reply,
) -> Reply {
    world.resource_scope(|world, loaded: Mut<Loaded>| {
        let mut cameras = world.query::<(&mut RtsCamera, &mut Transform, &mut Projection)>();
        let (mut cam, mut transform, mut projection) =
            cameras.single_mut(world).map_err(|_| "no camera")?;
        let reply = f(&mut cam, &loaded.map.ground)?;
        *transform = cam.transform();
        *projection = cam.projection();
        Ok(reply)
    })
}

/// A place on the map, kept within it.
pub(crate) fn place(ground: &tgmap::Ground, x: &str, y: &str) -> Result<[f32; 2], String> {
    let (x, y) = (number(x)?, number(y)?);
    let size = ground.size();
    if !(0.0..=size).contains(&x) || !(0.0..=size).contains(&y) {
        return Err(format!("{x} {y} is off the map, which runs 0 to {size}"));
    }
    Ok([x, y])
}

pub(crate) fn look_at(world: &mut World, args: &[&str]) -> Reply {
    with_camera(world, |cam, ground| match args {
        [] => {
            let [x, y] = map_point(cam.focus);
            Ok(format!("{x:.1} {y:.1}"))
        }
        [x, y] => {
            let [x, y] = place(ground, x, y)?;
            cam.focus = point(x, y, ground.height_at([x, y]));
            Ok(String::new())
        }
        _ => Err(usage("lookat")),
    })
}

pub(crate) fn camera(world: &mut World, args: &[&str]) -> Reply {
    with_camera(world, |cam, ground| {
        let (x, y, rest) = match args {
            [x, y, rest @ ..] if rest.len() <= 2 => (x, y, rest),
            _ => return Err(usage("camera")),
        };
        let [x, y] = place(ground, x, y)?;
        if let Some(pitch) = rest.get(1) {
            set_pitch(cam, number(pitch)?)?;
        }
        if let Some(height) = rest.first() {
            let (near, far) = cam.distances();
            cam.distance = (number(height)? / cam.pitch.sin()).clamp(near, far);
        }
        cam.focus = point(x, y, ground.height_at([x, y]));
        Ok(String::new())
    })
}

pub(super) fn zoom(world: &mut World, args: &[&str]) -> Reply {
    with_camera(world, |cam, _| {
        let (near, far) = cam.distances();
        // Evenly in ratio, as the mouse wheel zooms.
        match args {
            [] => Ok(format!(
                "{:.2}",
                (cam.distance / far).ln() / (near / far).ln()
            )),
            [level] => {
                let level = number(level)?;
                if !(0.0..=1.0).contains(&level) {
                    return Err("takes 0 to 1".into());
                }
                cam.distance = far * (near / far).powf(level);
                Ok(String::new())
            }
            _ => Err(usage("zoom")),
        }
    })
}

pub(super) fn yaw(world: &mut World, args: &[&str]) -> Reply {
    with_camera(world, |cam, _| {
        // At no yaw the camera looks along the map's y axis.
        cam.yaw = match args {
            [] => RtsCamera::new(cam.focus).yaw,
            [degrees] => number(degrees)?.to_radians() - FRAC_PI_2,
            _ => return Err(usage("yaw")),
        };
        Ok(String::new())
    })
}

fn set_pitch(cam: &mut RtsCamera, degrees: f32) -> Result<(), String> {
    if !(5.0..=89.0).contains(&degrees) {
        return Err("pitch takes 5 to 89 degrees".into());
    }
    cam.pitch = degrees.to_radians();
    Ok(())
}

pub(super) fn pitch(world: &mut World, args: &[&str]) -> Reply {
    with_camera(world, |cam, _| {
        match args {
            [] => cam.pitch = RtsCamera::new(cam.focus).pitch,
            [degrees] => set_pitch(cam, number(degrees)?)?,
            _ => return Err(usage("pitch")),
        }
        Ok(String::new())
    })
}
