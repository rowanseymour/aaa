//! Saves a frame to a file and exits, without a window.
//!
//! Cameras draw into an image instead, so a capture can't be lost to a window
//! that is hidden, covered or can't be presented, and needs no display.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, PipelineCache, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::render::{Render, RenderApp};
use bevy::ui::IsDefaultUiCamera;
use bevy::window::ExitCondition;

/// The size of the picture, the default window's.
pub const SIZE: UVec2 = UVec2::new(1280, 720);

/// Frames to let the scene build before looking for a quiet moment.
const SETTLE_FRAMES: u32 = 10;
/// Frames in a row with no shader still compiling before the capture, since
/// whatever uses one is left out of the frame until it is ready.
const QUIET_FRAMES: u32 = 5;
/// Frames to wait for all that before giving up rather than running forever.
const MAX_FRAMES: u32 = 3000;

/// A [`WindowPlugin`] with a window titled `title`, or none for runs that only
/// capture a frame.
pub fn window(title: String, shooting: bool) -> WindowPlugin {
    if !shooting {
        return WindowPlugin {
            primary_window: Some(Window { title, ..default() }),
            ..default()
        };
    }
    WindowPlugin {
        primary_window: None,
        // With no window to close, closing can't be what ends the run.
        exit_condition: ExitCondition::DontExit,
        close_when_requested: false,
        ..default()
    }
}

/// Points every camera at an image and saves it to `path` once the scene is
/// ready, then exits. Goes with [`window`].
pub struct ShotPlugin {
    pub path: PathBuf,
}

/// Present while the scene is still being set up elsewhere, holding off the
/// frame count towards a capture.
#[derive(Resource)]
pub struct Hold;

/// How many pipelines the render world is still compiling.
#[derive(Resource, Clone, Default)]
struct Compiling(Arc<AtomicUsize>);

#[derive(Resource)]
struct Shot {
    path: PathBuf,
    target: Handle<Image>,
    frames: u32,
    quiet: u32,
    asked: bool,
}

#[derive(Debug, PartialEq)]
enum Step {
    Wait,
    Capture,
    GiveUp,
}

impl Shot {
    /// Advances a frame, given how many pipelines are still compiling.
    fn tick(&mut self, compiling: usize) -> Step {
        self.frames += 1;
        self.quiet = if compiling == 0 { self.quiet + 1 } else { 0 };
        if self.frames >= MAX_FRAMES {
            Step::GiveUp
        } else if !self.asked && self.frames >= SETTLE_FRAMES && self.quiet >= QUIET_FRAMES {
            self.asked = true;
            Step::Capture
        } else {
            Step::Wait
        }
    }
}

impl Plugin for ShotPlugin {
    fn build(&self, app: &mut App) {
        let compiling = Compiling::default();
        let target = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(target(SIZE));
        app.insert_resource(compiling.clone())
            .insert_resource(Shot {
                path: self.path.clone(),
                target,
                frames: 0,
                quiet: 0,
                asked: false,
            })
            .add_systems(
                Update,
                (aim_cameras, capture.run_if(not(resource_exists::<Hold>))),
            );
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .insert_resource(compiling)
                .add_systems(Render, count_compiling);
        }
    }
}

/// An image for cameras to draw into, which can be copied back off the GPU.
fn target(size: UVec2) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC | TextureUsages::TEXTURE_BINDING;
    image
}

/// Points every camera at the capture image; every frame, as cameras can come
/// and go, as with a change of screen.
///
/// UI goes to the top camera on the primary window by default, and there is
/// none, so that camera is marked to draw it instead.
fn aim_cameras(
    mut commands: Commands,
    shot: Res<Shot>,
    mut cameras: Query<(Entity, &Camera, &mut RenderTarget, Has<IsDefaultUiCamera>)>,
) {
    let top = cameras.iter().max_by_key(|(e, c, ..)| (c.order, *e));
    let top = top.map(|(e, ..)| e);
    for (entity, _, mut target, marked) in &mut cameras {
        if target.as_image() != Some(&shot.target) {
            *target = shot.target.clone().into();
        }
        match (Some(entity) == top, marked) {
            (true, false) => _ = commands.entity(entity).insert(IsDefaultUiCamera),
            (false, true) => _ = commands.entity(entity).remove::<IsDefaultUiCamera>(),
            _ => {}
        }
    }
}

fn count_compiling(cache: Res<PipelineCache>, compiling: Res<Compiling>) {
    let waiting = cache.waiting_pipelines().count();
    compiling.0.store(waiting, Ordering::Relaxed);
}

fn capture(
    mut commands: Commands,
    mut shot: ResMut<Shot>,
    compiling: Res<Compiling>,
    mut exit: MessageWriter<AppExit>,
) {
    match shot.tick(compiling.0.load(Ordering::Relaxed)) {
        Step::Wait => {}
        Step::GiveUp => {
            error!(
                "no frame for {} after {MAX_FRAMES} frames",
                shot.path.display()
            );
            exit.write(AppExit::error());
        }
        Step::Capture => {
            let path = shot.path.clone();
            commands
                .spawn(Screenshot::image(shot.target.clone()))
                .observe(
                    move |captured: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                        exit.write(match save(&captured.image, &path) {
                            Ok(()) => {
                                info!("saved {}", path.display());
                                AppExit::Success
                            }
                            Err(e) => {
                                error!("{}: {e}", path.display());
                                AppExit::error()
                            }
                        });
                    },
                );
        }
    }
}

/// Saves a captured frame. Bevy's own `save_to_disk` only logs a failure,
/// which would leave a run exiting as if it had worked.
pub fn save(image: &Image, path: &std::path::Path) -> Result<(), String> {
    let picture = image
        .clone()
        .try_into_dynamic()
        .map_err(|e| e.to_string())?;
    picture.to_rgb8().save(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot() -> Shot {
        Shot {
            path: "out.png".into(),
            target: Handle::default(),
            frames: 0,
            quiet: 0,
            asked: false,
        }
    }

    #[test]
    fn captures_once_settled_and_quiet() {
        let mut shot = shot();
        let steps: Vec<_> = (0..SETTLE_FRAMES).map(|_| shot.tick(0)).collect();
        assert!(steps[..steps.len() - 1].iter().all(|s| *s == Step::Wait));
        assert_eq!(steps.last(), Some(&Step::Capture));
        assert_eq!(shot.tick(0), Step::Wait, "captures only once");
    }

    #[test]
    fn waits_for_compiling_to_finish() {
        let mut shot = shot();
        for _ in 0..SETTLE_FRAMES * 2 {
            assert_eq!(shot.tick(3), Step::Wait);
        }
        for _ in 1..QUIET_FRAMES {
            assert_eq!(shot.tick(0), Step::Wait);
        }
        assert_eq!(shot.tick(0), Step::Capture);
    }

    #[test]
    fn gives_up_when_never_quiet() {
        let mut shot = shot();
        for _ in 1..MAX_FRAMES {
            assert_eq!(shot.tick(1), Step::Wait);
        }
        assert_eq!(shot.tick(1), Step::GiveUp);
    }
}
