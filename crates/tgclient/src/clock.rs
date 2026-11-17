//! The keys that pause the match and set how fast it's played.

use std::ops::RangeInclusive;

use bevy::prelude::*;
use tgsim::Action;

use crate::intent::{Asked, Intent};
use crate::{Loaded, Orders};

/// What the hotkey files call pausing or unpausing, stepping the speed up
/// or down, and setting it.
const PAUSE: &str = "gamepause";
const SPEED_UP: &str = "SpeedUp";
const SPEED_DOWN: &str = "SpeedDown";
const SPEED: &str = "GameSpeed";

/// Pauses or unpauses the match, or steps or sets its speed, as asked.
/// Only what the match's host would carry out is asked of it.
pub fn keys(mut intents: MessageReader<Intent>, mut orders: ResMut<Orders>, loaded: Res<Loaded>) {
    let asked = Asked::read(&mut intents);
    if asked.is_empty() {
        return;
    }
    let speeds = loaded.game.game_speeds();
    let now = orders.speeding().log2();
    let mut wanted = Vec::new();
    if asked.wants(PAUSE) {
        wanted.push(Action::Pause(!orders.pausing()));
    }
    for (name, to) in [
        (SPEED_UP, stepped(now, 0.5, &speeds)),
        (SPEED_DOWN, stepped(now, -0.5, &speeds)),
        (SPEED, 0.0f32.clamp(*speeds.start(), *speeds.end())),
    ] {
        if asked.wants(name) {
            wanted.push(Action::Speed(to.exp2()));
        }
    }
    for action in wanted {
        if orders.may(&action) {
            orders.act(action);
        }
    }
}

/// The speed, as a power of two, a step `by` from `now` comes to: from
/// `now` to the nearest half, kept within `speeds`.
fn stepped(now: f32, by: f32, speeds: &RangeInclusive<f32>) -> f32 {
    let nearest = (now * 2.0).round() / 2.0;
    (nearest + by).clamp(*speeds.start(), *speeds.end())
}

#[cfg(test)]
mod tests {
    use super::*;

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
