//! The sun's colours changing during a match, as triggers turn them to a
//! sun definition's: see [`tgsim::SunChange`].
//!
//! What the sun shows is worked out afresh from the map's own sun and every
//! change the match has made, so a player who joins late sees what the
//! others do.

use std::collections::HashMap;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use tgdata::GameData;
use tgmap::Id;

use crate::{Loaded, Sim};

/// A sun's diffuse, ambient and specular colours.
type Colours = [[f32; 3]; 3];

/// Where the sun's colours start from, and the sun definitions changes
/// name.
#[derive(Resource)]
pub(crate) struct Sunlight {
    presets: HashMap<Id, Colours>,
    /// The colours shown at the start.
    start: Colours,
    /// A change the start was part-way through: the colours it was heading
    /// for and its seconds left.
    heading: Option<(Colours, f32)>,
    /// How many of the match's changes come before the start, and so don't
    /// count.
    skipped: usize,
    /// What was last shown.
    shown: Option<Colours>,
}

impl Sunlight {
    /// Starts from the map's own sun, carrying on any change it was saved
    /// part-way through.
    pub(crate) fn new(game: &GameData, sun: &tgck::map::Sun) -> Sunlight {
        let presets = (game.sun_presets().into_iter())
            .filter_map(|p| Some((p.ids?, [p.diffuse, p.ambient, p.specular])))
            .collect();
        let heading = (sun.color_fade_time > 0.0).then_some((
            [sun.target_diffuse, sun.target_ambient, sun.target_specular],
            sun.color_fade_time,
        ));
        Sunlight {
            presets,
            start: [sun.diffuse, sun.ambient, sun.specular],
            heading,
            skipped: 0,
            shown: None,
        }
    }

    /// Shows `colours` at once from now on, until the match changes them
    /// again: the match's `changes` so far no longer count.
    pub(crate) fn set(&mut self, colours: Colours, changes: usize) {
        self.start = colours;
        self.heading = None;
        self.skipped = changes;
        self.shown = Some(colours);
    }

    /// The colours shown `now` seconds into the match, given its changes,
    /// and those they're heading for.
    fn at(&self, changes: &[tgsim::SunChange], now: f32) -> (Colours, Colours) {
        // Each change runs from its time towards its colours until the next
        // starts; one naming no sun definition does nothing.
        let own = self.heading.map(|(to, seconds)| (0.0, to, seconds));
        let made = (changes.iter().skip(self.skipped)).filter_map(|c| {
            let to = *self.presets.get(&c.sun)?;
            Some((c.at as f32 * tgsim::TICK, to, c.seconds))
        });
        let changes: Vec<_> = own.into_iter().chain(made).collect();
        let mut shown = self.start;
        let mut heading = self.start;
        for (i, &(from, to, seconds)) in changes.iter().enumerate() {
            let until = changes.get(i + 1).map_or(now, |next| next.0).min(now);
            let t = match seconds > 0.0 {
                true => ((until - from) / seconds).clamp(0.0, 1.0),
                false => 1.0,
            };
            shown = mix(shown, to, t);
            heading = to;
        }
        (shown, heading)
    }
}

/// `a` moved `t` of the way to `b`.
fn mix(a: Colours, b: Colours, t: f32) -> Colours {
    std::array::from_fn(|i| std::array::from_fn(|c| a[i][c] + (b[i][c] - a[i][c]) * t))
}

/// Lights the match with the sun's colours as its changes have left them,
/// showing night effects while it's dark enough.
pub(crate) fn follow(
    sim: Res<Sim>,
    mut loaded: ResMut<Loaded>,
    mut sunlight: ResMut<Sunlight>,
    mut effects: ResMut<crate::effects::Effects>,
    fixed: Res<Time<Fixed>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    // The menu's backdrop has a sun of its own, still up as the match starts.
    mut lights: Query<
        &mut DirectionalLight,
        (Without<RenderLayers>, Without<crate::backdrop::Backdrop>),
    >,
) {
    let now = (sim.0.ticks as f32 + fixed.overstep_fraction()) * tgsim::TICK;
    let (shown, heading) = sunlight.at(&sim.0.triggers.suns, now);
    if sunlight.shown == Some(shown) {
        return;
    }
    sunlight.shown = Some(shown);
    let [diffuse, ambient_color, specular] = shown;
    let sun = &mut loaded.map.sun;
    (sun.diffuse, sun.ambient, sun.specular) = (diffuse, ambient_color, specular);
    [sun.target_diffuse, sun.target_ambient, sun.target_specular] = heading;
    effects.set_night(loaded.game.night(diffuse));
    for mut light in &mut lights {
        tgrender::light::color_lights(&mut light, &mut ambient, diffuse, ambient_color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Colours = [[1.0; 3], [0.8; 3], [0.0; 3]];
    const DUSK: Colours = [[0.8; 3], [0.6; 3], [0.0; 3]];
    const NIGHT: Colours = [[0.4; 3], [0.4; 3], [0.2; 3]];

    fn sunlight() -> Sunlight {
        Sunlight {
            presets: [("sun_dusk".into(), DUSK), ("sun_night".into(), NIGHT)].into(),
            start: DAY,
            heading: None,
            skipped: 0,
            shown: None,
        }
    }

    fn change(sun: &str, at_seconds: f32, seconds: f32) -> tgsim::SunChange {
        tgsim::SunChange {
            sun: sun.into(),
            at: (at_seconds / tgsim::TICK).round() as u64,
            seconds,
        }
    }

    fn near(a: Colours, b: Colours) -> bool {
        (a.iter().flatten().zip(b.iter().flatten())).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn a_late_look_sees_each_change_from_where_the_last_left_it() {
        // Dusk at once, then night over an hour: half an hour in it's
        // halfway from dusk, wherever it was first looked at.
        let changes = [
            change("sun_dusk", 0.0, 0.0),
            change("sun_night", 0.0, 3600.0),
        ];
        let (shown, heading) = sunlight().at(&changes, 1800.0);
        assert!(near(shown, mix(DUSK, NIGHT, 0.5)), "{shown:?}");
        assert!(near(heading, NIGHT));
    }

    #[test]
    fn a_change_starts_from_where_one_under_way_has_got_to() {
        let changes = [
            change("sun_night", 0.0, 100.0),
            change("sun_dusk", 50.0, 0.0),
        ];
        assert!(near(sunlight().at(&changes, 60.0).0, DUSK));
        let changes = [
            change("sun_night", 0.0, 100.0),
            change("sun_dusk", 50.0, 100.0),
        ];
        let halfway = mix(DAY, NIGHT, 0.5);
        assert!(near(
            sunlight().at(&changes, 100.0).0,
            mix(halfway, DUSK, 0.5)
        ));
    }

    #[test]
    fn a_change_naming_no_sun_leaves_one_under_way_going() {
        let changes = [
            change("sun_night", 0.0, 100.0),
            change("sun_typo", 10.0, 0.0),
        ];
        assert!(near(sunlight().at(&changes, 50.0).0, mix(DAY, NIGHT, 0.5)));
    }

    #[test]
    fn colours_set_outright_stand_until_the_next_change() {
        let mut light = sunlight();
        let mut changes = vec![change("sun_night", 0.0, 0.0)];
        light.set(DUSK, changes.len());
        assert!(near(light.at(&changes, 10.0).0, DUSK));
        changes.push(change("sun_night", 20.0, 0.0));
        assert!(near(light.at(&changes, 30.0).0, NIGHT));
    }

    #[test]
    fn the_maps_own_change_under_way_carries_on() {
        let light = Sunlight {
            heading: Some((NIGHT, 10.0)),
            ..sunlight()
        };
        assert!(near(light.at(&[], 5.0).0, mix(DAY, NIGHT, 0.5)));
        assert!(near(light.at(&[], 20.0).0, NIGHT));
    }
}
