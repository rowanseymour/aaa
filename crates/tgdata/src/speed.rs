//! How fast a match may be played.

use std::ops::RangeInclusive;

use crate::GameData;

impl GameData {
    /// The speeds a match may be played at, as powers of two: 0 for as
    /// fast as the game's own time, 1 for twice as fast.
    pub fn game_speeds(&self) -> RangeInclusive<f32> {
        let first = |of: fn(&crate::defs::SVars) -> Option<f32>| self.svar(of);
        let slowest = first(|s| s.game_speed_min).unwrap_or(-2.0);
        let fastest = first(|s| s.game_speed_max).unwrap_or(2.0);
        slowest.min(fastest)..=fastest.max(slowest)
    }
}
