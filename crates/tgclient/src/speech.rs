//! The lines of speech the map's triggers play for every player.

use bevy::prelude::*;
use tgsim::Speech;

use crate::sound::Sounds;
use crate::{Loaded, Orders, Screen, Sim};

pub(crate) struct SpeechPlugin;

impl Plugin for SpeechPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Match), |mut commands: Commands| {
            commands.insert_resource(Heard::default());
        })
        .add_systems(
            Update,
            (
                speak.run_if(resource_exists::<Heard>),
                crate::sound::hush_speech.run_if(hurried),
            )
                .run_if(in_state(Screen::Match)),
        );
    }
}

/// How many of the sim's lines have been looked at.
#[derive(Resource, Default)]
struct Heard(usize);

/// The line to play of those asked for since last looked: the last, as
/// each takes the place of any still waiting, unless it was asked for too
/// long ago to start.
fn due(new: &[Speech], ticks: u64, timeout: f32) -> Option<&Speech> {
    let line = new.last()?;
    let waited = ticks.saturating_sub(line.at) as f32 * tgsim::TICK;
    (waited <= timeout).then_some(line)
}

/// Whether a scripted scene is being hurried on under the letterbox,
/// when speech isn't played.
fn hurried(orders: Option<Res<Orders>>, sim: Option<Res<Sim>>, fixed: Res<Time<Fixed>>) -> bool {
    let letterboxed = sim.is_some_and(|s| crate::curtains::hides_interface(&s, &fixed));
    orders.is_some_and(|o| o.skipping()) && letterboxed
}

#[allow(clippy::too_many_arguments)]
fn speak(
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    orders: Option<Res<Orders>>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    mut heard: ResMut<Heard>,
    mut sounds: ResMut<Sounds>,
    mut started: MessageWriter<crate::subtitles::Started>,
) {
    let lines = &sim.0.triggers.speech;
    let new = lines.get(heard.0..).unwrap_or_default();
    heard.0 = lines.len();
    let Some(line) = due(new, sim.0.ticks, loaded.game.audio_drop_timeout()) else {
        return;
    };
    let skipping = orders.is_some_and(|o| o.skipping());
    let played = !(skipping && crate::curtains::hides_interface(&sim, &fixed));
    if played {
        sounds.speak(&time, &line.audio);
    }
    started.write(crate::subtitles::Started {
        audio: line.audio.clone(),
        played,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(audio: &str, at: u64) -> Speech {
        Speech {
            audio: audio.into(),
            at,
        }
    }

    #[test]
    fn of_lines_asked_for_together_the_last_plays_if_its_not_too_late() {
        let new = [line("first", 10), line("second", 10)];
        assert_eq!(due(&new, 12, 5.0).map(|l| l.audio.as_str()), Some("second"));
        assert_eq!(due(&new, 70, 5.0), None, "asked for six seconds ago");
        assert_eq!(due(&[], 12, 5.0), None);
    }
}
