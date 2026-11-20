//! The keys that pause the match and set how fast it's played, and what
//! either interface's pause button shows and asks for, as the key does:
//! Escape's menu leaves a match hosted elsewhere playing on.

use std::ops::RangeInclusive;

use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use tgsim::Action;

use crate::displays::Displays;
use crate::intent::{Asked, Intent};
use crate::{Loaded, Orders};

/// What the hotkey files call pausing or unpausing, stepping the speed up
/// or down, and setting it.
pub(crate) const PAUSE: &str = "gamepause";
const SPEED_UP: &str = "SpeedUp";
const SPEED_DOWN: &str = "SpeedDown";
const SPEED: &str = "GameSpeed";

/// Pauses or unpauses the match, or steps or sets its speed, as asked.
pub fn keys(
    mut intents: MessageReader<Intent>,
    mut orders: ResMut<Orders>,
    loaded: Res<Loaded>,
    mut displays: Option<ResMut<Displays>>,
) {
    let asked = Asked::read(&mut intents);
    if asked.is_empty() {
        return;
    }
    let speeds = loaded.game.game_speeds();
    // Pausing goes by the match as it is, not as asked, so that pressing
    // again before the host answers flips a vote back as each press does.
    let (paused, speed) = (orders.paused(), orders.speeding());
    let wanted = wanted(|name| asked.wants(name), paused, speed, &speeds);
    ask(&mut orders, displays.as_deref_mut(), wanted);
}

/// Asks for those of `actions` the match's host would carry out. A pause
/// asked for while a panel's up holds once it closes, as the panel only
/// lets go of a pause it made itself.
pub(crate) fn ask(orders: &mut Orders, mut displays: Option<&mut Displays>, actions: Vec<Action>) {
    for action in actions {
        if !orders.may(&action) {
            continue;
        }
        if action == Action::Pause(true)
            && let Some(displays) = displays.as_deref_mut()
        {
            displays.keep_paused();
        }
        orders.act(action);
    }
}

/// What the keys `wants` names were pressed for ask of a match `paused`
/// or not and played `speed` times as fast, within `speeds`.
fn wanted(
    wants: impl Fn(&str) -> bool,
    paused: bool,
    speed: f32,
    speeds: &RangeInclusive<f32>,
) -> Vec<Action> {
    let now = speed.log2();
    let mut wanted = Vec::new();
    if wants(PAUSE) {
        wanted.push(Action::Pause(!paused));
    }
    for (name, to) in [
        (SPEED_UP, stepped(now, 0.5, speeds)),
        (SPEED_DOWN, stepped(now, -0.5, speeds)),
        (SPEED, 0.0f32.clamp(*speeds.start(), *speeds.end())),
    ] {
        if wants(name) {
            wanted.push(Action::Speed(to.exp2()));
        }
    }
    wanted
}

/// The speed, as a power of two, a step `by` from `now` comes to: from
/// `now` to the nearest half, kept within `speeds`.
fn stepped(now: f32, by: f32, speeds: &RangeInclusive<f32>) -> f32 {
    let nearest = (now * 2.0).round() / 2.0;
    (nearest + by).clamp(*speeds.start(), *speeds.end())
}

/// What either interface's pause button is [`called`](crate::ui::called),
/// so a script works with both.
pub(crate) const PAUSE_BUTTON: &str = "clock.pause";

/// A button, of either interface, that pauses and unpauses the match.
#[derive(Component, Default)]
pub struct Pauses;

/// What the pause buttons show: whether the match is paused, whoever
/// paused it, and whether the player may pause it at all.
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub struct PauseState {
    pub paused: bool,
    pub usable: bool,
}

/// Keeps [`PauseState`] to the match's.
pub fn follow(orders: Res<Orders>, mut state: ResMut<PauseState>) {
    state.set_if_neq(PauseState {
        paused: orders.paused(),
        usable: orders.allowed(&Action::Pause(true)),
    });
}

/// Lets the pause buttons be pressed only by those who may pause, such
/// as not anyone only watching.
pub fn disable_buttons(
    mut commands: Commands,
    state: Res<PauseState>,
    buttons: Query<(Entity, Has<InteractionDisabled>), With<Pauses>>,
) {
    for (id, disabled) in &buttons {
        match (state.usable, disabled) {
            (true, true) => _ = commands.entity(id).remove::<InteractionDisabled>(),
            (false, false) => _ = commands.entity(id).insert(InteractionDisabled),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_key_asks_for_its_pause_or_speed() {
        let speeds = -2.0..=2.0;
        let pressed = |key: &'static str| move |name: &str| name == key;
        let asks = |key, paused, speed| wanted(pressed(key), paused, speed, &speeds);
        assert_eq!(asks(PAUSE, false, 1.0), [Action::Pause(true)]);
        // Paused, pressing again asks to unpause, which for anyone but the
        // pauser is a vote, however soon after.
        assert_eq!(asks(PAUSE, true, 1.0), [Action::Pause(false)]);
        assert_eq!(asks(SPEED_UP, false, 1.0), [Action::Speed(2f32.sqrt())]);
        assert_eq!(asks(SPEED_DOWN, false, 2.0), [Action::Speed(2f32.sqrt())]);
        assert_eq!(asks(SPEED, true, 4.0), [Action::Speed(1.0)]);
        assert_eq!(asks("ActorCommand stop", false, 1.0), []);
    }

    #[test]
    fn a_step_goes_from_the_nearest_half_and_stays_within_the_speeds() {
        let speeds = -2.0..=2.0;
        assert_eq!(stepped(0.0, 0.5, &speeds), 0.5);
        assert_eq!(stepped(0.0, -0.5, &speeds), -0.5);
        // 3 times as fast is nearest 2^1.5.
        assert_eq!(stepped(3.0f32.log2(), 0.5, &speeds), 2.0);
        assert_eq!(stepped(2.0, 0.5, &speeds), 2.0);
        assert_eq!(stepped(-2.0, -0.5, &speeds), -2.0);
        // From beyond them, back within.
        assert_eq!(stepped(4.0, -0.5, &speeds), 2.0);
        // Nine speeds from slowest to fastest.
        let mut at = -2.0;
        let mut seen = vec![at];
        while at < 2.0 {
            at = stepped(at, 0.5, &speeds);
            seen.push(at);
        }
        assert_eq!(seen.len(), 9);
    }
}
