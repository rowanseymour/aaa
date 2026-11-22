//! The match's elapsed game time and a trigger's countdown, as either
//! interface shows them, in game time and moving smoothly between ticks.

use bevy::prelude::*;

use crate::Sim;

/// Whether the elapsed game time is shown, as chosen in the options.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct Elapsed(pub bool);

/// What the time labels say, each `None` while it's off screen.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct Times {
    pub elapsed: Option<String>,
    pub counter: Option<String>,
}

pub struct TimesPlugin;

impl Plugin for TimesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Times>()
            .add_systems(Update, follow.run_if(in_state(crate::Screen::Match)))
            .add_systems(OnExit(crate::Screen::Match), |mut times: ResMut<Times>| {
                *times = Times::default();
            });
    }
}

/// `seconds` as the labels write it: hours, always shown, then minutes and
/// seconds, with what's left of a second dropped.
pub fn written(seconds: f32) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// Keeps the [`Times`] to the match's.
pub fn follow(
    sim: Res<Sim>,
    fixed: Res<Time<Fixed>>,
    elapsed: Res<Elapsed>,
    mut times: ResMut<Times>,
) {
    // As drawn, between the last two ticks.
    let ticks = (sim.0.ticks as f32 - 1.0 + fixed.overstep_fraction()).max(0.0);
    times.set_if_neq(Times {
        elapsed: elapsed.0.then(|| written(ticks * tgsim::TICK)),
        counter: (sim.0.triggers.counter).map(|c| written(c.left(ticks))),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_written_with_hours_and_whole_seconds() {
        assert_eq!(written(0.0), "0:00:00");
        assert_eq!(written(754.9), "0:12:34");
        assert_eq!(written(1199.99), "0:19:59");
        assert_eq!(written(25.0 * 3600.0), "25:00:00");
        assert_eq!(written(-1.0), "0:00:00");
    }

    #[test]
    fn the_labels_follow_the_option_and_the_counter() {
        let mut app = App::new();
        let mut world = crate::special_ops::tests::operating(0.0);
        world.ticks = 600;
        app.insert_resource(Sim(world.clone()))
            .insert_resource(Elapsed(false))
            .init_resource::<Time<Fixed>>()
            .init_resource::<Times>()
            .add_systems(Update, follow);
        app.update();
        assert_eq!(*app.world().resource::<Times>(), Times::default());
        app.insert_resource(Elapsed(true));
        world.triggers.counter = Some(tgsim::Counter {
            at: 300,
            seconds: 1200.0,
        });
        let fresh = tgsim::Counter {
            at: 600,
            seconds: 1200.0,
        };
        // Shown this tick, so none of it has been drawn passing yet.
        assert_eq!(written(fresh.left(599.0)), "0:20:00");
        app.insert_resource(Sim(world));
        app.update();
        let times = app.world().resource::<Times>();
        // A tick behind the sim's, as drawn.
        assert_eq!(times.elapsed.as_deref(), Some("0:00:59"));
        assert_eq!(times.counter.as_deref(), Some("0:19:30"));
    }
}
