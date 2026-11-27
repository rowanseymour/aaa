//! The map's triggers moving the person's view: to the cameras the map
//! places or back to the game's own view, and freeing its zoom while
//! cutscene mode is on.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;
use tgmap::Ground;
use tgrender::camera::RtsCamera;
use tgrender::space::{in_world, world};
use tgsim::{Pace, View};

use crate::{Loaded, Local, Sim};

/// What the triggers have done to the view, kept on the match's camera.
#[derive(Component, Default)]
pub(crate) struct Scripted {
    /// How many of the sim's view moves have been looked at.
    seen: usize,
    moving: Option<Move>,
    cutscene: bool,
}

/// A view move under way.
struct Move {
    from: Pose,
    to: Pose,
    /// When it started, in seconds of the clock its pace goes by.
    start: f32,
    seconds: f32,
    pace: Pace,
}

/// Where the camera looks and from where.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    focus: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
}

impl Pose {
    fn of(cam: &RtsCamera) -> Pose {
        Pose {
            focus: cam.focus,
            yaw: cam.yaw,
            pitch: cam.pitch,
            distance: cam.distance,
        }
    }

    /// The game's own view of a point.
    fn default_at(focus: Vec3) -> Pose {
        Pose::of(&RtsCamera::new(focus))
    }

    /// The view a placed camera gives.
    fn camera(camera: &tgmap::Camera) -> Pose {
        let focus = in_world(camera.position);
        let (heading, pitch) = camera.angles();
        Pose {
            // Bevy's yaw turns the way from the focus to the camera from
            // the map's -y, which is a quarter turn back from its heading.
            yaw: heading + PI / 2.0,
            pitch,
            distance: camera.zoom * Pose::default_at(focus).distance,
            focus,
        }
    }

    /// `share` of the way from `self` to `to`, turning the shorter way.
    fn towards(self, to: Pose, share: f32) -> Pose {
        let turn = (to.yaw - self.yaw + PI).rem_euclid(TAU) - PI;
        Pose {
            focus: self.focus.lerp(to.focus, share),
            yaw: self.yaw + turn * share,
            pitch: self.pitch + (to.pitch - self.pitch) * share,
            distance: self.distance + (to.distance - self.distance) * share,
        }
    }

    fn put(self, cam: &mut RtsCamera) {
        let (near, far) = cam.distances();
        cam.focus = self.focus;
        cam.yaw = self.yaw;
        cam.pitch = self.pitch;
        cam.distance = self.distance.clamp(near, far);
    }
}

/// The time now on each clock a move can go by, in seconds.
#[derive(Clone, Copy)]
struct Clocks {
    game: f32,
    real: f32,
}

impl Move {
    /// The share of the way it's gone by now.
    fn share(&self, clocks: Clocks) -> f32 {
        if self.seconds <= 0.0 {
            return 1.0;
        }
        let now = match self.pace {
            Pace::RealTime => clocks.real,
            Pace::Steady | Pace::Eased => clocks.game,
        };
        let t = ((now - self.start) / self.seconds).clamp(0.0, 1.0);
        match self.pace {
            Pace::Eased => t * t * (3.0 - 2.0 * t),
            Pace::Steady | Pace::RealTime => t,
        }
    }

    /// Where it has the view by now.
    fn at(&self, clocks: Clocks) -> Pose {
        self.from.towards(self.to, self.share(clocks))
    }
}

/// Where a move ends, if what it names is still in the map.
fn target(to: View, cameras: &[tgmap::Camera], ground: &Ground) -> Option<Pose> {
    match to {
        View::Camera(id) => cameras.iter().find(|c| c.id == id).map(Pose::camera),
        View::Default([x, y]) => Some(Pose::default_at(world(x, y, ground.height_at([x, y])))),
    }
}

/// Starts the moves made for this side since last looked, each from where
/// the view is, and carries on the one under way.
pub(crate) fn follow(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    local: Res<Local>,
    fixed: Res<Time<Fixed>>,
    real: Res<Time<Real>>,
    mut cameras: Query<(&mut RtsCamera, &mut Transform, &mut Scripted)>,
) {
    let triggers = &sim.0.triggers;
    let game_now = (sim.0.ticks as f32 + fixed.overstep_fraction()) * tgsim::TICK;
    let real_now = real.elapsed_secs();
    for (mut cam, mut transform, mut scripted) in &mut cameras {
        let scripted = &mut *scripted;
        let switched = scripted.cutscene != triggers.cutscene;
        if switched {
            scripted.cutscene = triggers.cutscene;
            cutscene(&mut cam, scripted);
        }
        let new = triggers.views.get(scripted.seen..).unwrap_or_default();
        scripted.seen = triggers.views.len();
        let clocks = Clocks {
            game: game_now,
            real: real_now,
        };
        for made in new.iter().filter(|m| local.0 == Some(m.player)) {
            let Some(to) = target(made.to, &triggers.cameras, &loaded.map.ground) else {
                continue;
            };
            // From where the view is, or would be by the move it cuts short.
            let from = match &scripted.moving {
                Some(moving) => moving.at(clocks),
                None => Pose::of(&cam),
            };
            // From when the sim made it, which may be frames or, for a
            // camera that came later, much longer ago; real time is taken
            // to have gone by as game time has.
            let made_at = made.at as f32 * tgsim::TICK;
            let start = match made.pace {
                Pace::Steady | Pace::Eased => made_at,
                Pace::RealTime => real_now - (game_now - made_at).max(0.0),
            };
            scripted.moving = Some(Move {
                from,
                to,
                start,
                seconds: made.seconds,
                pace: made.pace,
            });
        }
        if let Some(moving) = &scripted.moving {
            moving.at(clocks).put(&mut cam);
            if moving.share(clocks) >= 1.0 {
                scripted.moving = None;
            }
        } else if !switched {
            continue;
        }
        *transform = cam.transform();
    }
}

/// Frees the zoom as cutscene mode comes on; as it goes off, holds it
/// again and puts back the game's own zoom and angle, still looking where
/// the view is.
fn cutscene(cam: &mut RtsCamera, scripted: &mut Scripted) {
    match scripted.cutscene {
        true => cam.unhold_zoom(),
        false => {
            // Which holds the zoom again too.
            cam.default_angles();
            if let Some(moving) = &mut scripted.moving {
                for pose in [&mut moving.from, &mut moving.to] {
                    *pose = Pose {
                        focus: pose.focus,
                        ..Pose::default_at(pose.focus)
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_1_SQRT_2;

    use super::*;

    /// The game's own view's facing.
    fn own_facing() -> [f32; 3] {
        [FRAC_1_SQRT_2, -FRAC_1_SQRT_2, 40f32.to_radians().tan()]
    }

    fn placed(facing: [f32; 3], zoom: f32) -> tgmap::Camera {
        tgmap::Camera {
            id: 1,
            name: String::new(),
            position: [10.0, 20.0, 2.0],
            facing,
            zoom,
        }
    }

    #[test]
    fn a_camera_facing_as_the_game_does_gives_its_own_view() {
        let own = Pose::default_at(world(10.0, 20.0, 2.0));
        let pose = Pose::camera(&placed(own_facing(), 1.0));
        assert!((pose.yaw - own.yaw).abs() < 1e-4);
        assert!((pose.pitch - own.pitch).abs() < 1e-4);
        assert!((pose.distance - own.distance).abs() < 1e-3);
        assert_eq!(pose.focus, own.focus);
        let far = Pose::camera(&placed(own_facing(), 2.0));
        assert!((far.distance - 2.0 * own.distance).abs() < 1e-3);
    }

    #[test]
    fn a_view_turns_the_shorter_way() {
        let from = Pose::default_at(Vec3::ZERO);
        let to = Pose {
            yaw: from.yaw - 1.5 * PI,
            ..from
        };
        // Three quarters of a turn back is a quarter forward.
        let half = from.towards(to, 0.5);
        assert!((half.yaw - (from.yaw + PI / 4.0)).abs() < 1e-4);
    }

    #[test]
    fn an_eased_move_starts_and_stops_slowly() {
        let pose = Pose::default_at(Vec3::ZERO);
        let moving = |pace| Move {
            from: pose,
            to: pose,
            start: 10.0,
            seconds: 4.0,
            pace,
        };
        let at = |game| Clocks { game, real: 0.0 };
        let (eased, steady) = (moving(Pace::Eased), moving(Pace::Steady));
        assert!(eased.share(at(11.0)) < steady.share(at(11.0)));
        assert_eq!(eased.share(at(12.0)), 0.5);
        assert_eq!(eased.share(at(20.0)), 1.0);
        assert_eq!(
            moving(Pace::RealTime).share(at(20.0)),
            0.0,
            "on the other clock"
        );
        let jump = Move {
            seconds: 0.0,
            ..moving(Pace::Steady)
        };
        assert_eq!(jump.share(at(0.0)), 1.0);
    }
}
