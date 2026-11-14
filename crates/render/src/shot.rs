//! Saves frames to files, without a window.
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

/// Points every camera at an image, for [`take`] to save. Given a `path`,
/// saves it there once the scene is ready, then exits. Goes with [`window`].
pub struct ShotPlugin {
    pub path: Option<PathBuf>,
}

/// A frame [`take`] was asked for is on disk, or why it isn't.
#[derive(Event)]
pub struct Taken(pub Result<PathBuf, String>);

/// Counts frames towards the capture and takes it. An app whose scene isn't
/// ready from the start, such as one that loads it in the background, gives
/// this set a run condition to hold the count off until it is.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Capture;

/// How many pipelines the render world is still compiling.
#[derive(Resource, Clone, Default)]
struct Compiling(Arc<AtomicUsize>);

#[derive(Resource)]
struct Shot {
    target: Handle<Image>,
    /// The frame to save and exit on.
    once: Option<Wanted>,
    /// The frames [`take`] was asked for, the first of them being waited on.
    asked: std::collections::VecDeque<Wanted>,
}

/// A frame to save, and how long it has been waited for.
struct Wanted {
    path: PathBuf,
    frames: u32,
    quiet: u32,
    asked: bool,
}

impl Wanted {
    fn new(path: PathBuf) -> Wanted {
        Wanted {
            path,
            frames: 0,
            quiet: 0,
            asked: false,
        }
    }
}

/// Saves the next settled frame of a windowless run to `path`, triggering
/// [`Taken`] once it has; `false` if cameras draw to a window instead.
pub fn take(world: &mut World, path: PathBuf) -> bool {
    let Some(mut shot) = world.get_resource_mut::<Shot>() else {
        return false;
    };
    shot.asked.push_back(Wanted::new(path));
    true
}

#[derive(Debug, PartialEq)]
enum Step {
    Wait,
    Capture,
    GiveUp,
}

impl Wanted {
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
                target,
                once: self.path.clone().map(Wanted::new),
                asked: default(),
            })
            .add_systems(Update, (aim_cameras, capture.in_set(Capture), serve));
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .insert_resource(compiling)
                .add_systems(Render, count_compiling);
        }
    }
}

/// An image for cameras to draw into, which can be copied back off the GPU.
pub(crate) fn target(size: UVec2) -> Image {
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

/// Points every camera that draws to a window at the capture image; every
/// frame, as cameras can come and go, as with a change of screen. Those
/// that draw to an image of their own are left to.
///
/// UI goes to the top camera on the primary window by default, and there is
/// none, so that camera is marked to draw it instead.
fn aim_cameras(
    mut commands: Commands,
    shot: Res<Shot>,
    mut cameras: Query<(Entity, &Camera, &mut RenderTarget, Has<IsDefaultUiCamera>)>,
) {
    let own = |target: &RenderTarget| target.as_image().is_some_and(|i| *i != shot.target);
    let screen = cameras.iter().filter(|(_, _, target, _)| !own(target));
    let top = screen.max_by_key(|(e, c, ..)| (c.order, *e));
    let top = top.map(|(e, ..)| e);
    for (entity, _, mut target, marked) in &mut cameras {
        if own(&target) {
            continue;
        }
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

fn capture(mut commands: Commands, mut shot: ResMut<Shot>, compiling: Res<Compiling>) {
    let shot = &mut *shot;
    if let Some(once) = &mut shot.once {
        advance(&mut commands, once, &shot.target, &compiling, Then::Exit);
    }
}

/// Takes the frames [`take`] was asked for, one at a time.
fn serve(mut commands: Commands, mut shot: ResMut<Shot>, compiling: Res<Compiling>) {
    let shot = &mut *shot;
    if let Some(next) = shot.asked.front_mut() {
        advance(&mut commands, next, &shot.target, &compiling, Then::Tell);
    }
}

/// What follows a frame being saved, or given up on.
#[derive(Clone, Copy)]
enum Then {
    Exit,
    /// Triggers [`Taken`] and goes on to the next asked for.
    Tell,
}

/// Waits a frame more for `wanted`, or takes it.
fn advance(
    commands: &mut Commands,
    wanted: &mut Wanted,
    target: &Handle<Image>,
    compiling: &Compiling,
    then: Then,
) {
    let path = wanted.path.clone();
    match wanted.tick(compiling.0.load(Ordering::Relaxed)) {
        Step::Wait => {}
        Step::GiveUp => {
            let said = format!("no frame for {} after {MAX_FRAMES} frames", path.display());
            commands.queue(move |world: &mut World| finish(world, then, Err(said)));
        }
        Step::Capture => {
            commands.spawn(Screenshot::image(target.clone())).observe(
                move |captured: On<ScreenshotCaptured>, mut commands: Commands| {
                    let saved = save(&captured.image, &path)
                        .map(|_| path.clone())
                        .map_err(|e| format!("{}: {e}", path.display()));
                    commands.queue(move |world: &mut World| finish(world, then, saved));
                },
            );
        }
    }
}

fn finish(world: &mut World, then: Then, saved: Result<PathBuf, String>) {
    match then {
        Then::Exit => {
            let exit = match saved {
                Ok(path) => {
                    info!("saved {}", path.display());
                    AppExit::Success
                }
                Err(e) => {
                    error!("{e}");
                    AppExit::error()
                }
            };
            world.write_message(exit);
        }
        Then::Tell => {
            world.resource_mut::<Shot>().asked.pop_front();
            world.trigger(Taken(saved));
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

    fn shot() -> Wanted {
        Wanted::new("out.png".into())
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
