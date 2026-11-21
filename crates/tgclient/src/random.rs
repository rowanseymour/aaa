//! The client's own random picks, for how things look and sound, which
//! the sim's never draw on.

use std::time::{SystemTime, UNIX_EPOCH};

/// A stream of numbers in `0..1`, by xorshift64.
#[derive(Clone, Copy, Debug)]
pub struct Random(u64);

impl Random {
    /// From `seed`, which mustn't be zero, where xorshift stays.
    pub const fn new(seed: u64) -> Random {
        Random(seed)
    }

    /// Somewhere new each launch, as the game's picks are once everything
    /// else drawing on its numbers has had its turn.
    pub fn of_launch() -> Random {
        let now = SystemTime::now().duration_since(UNIX_EPOCH);
        Random(now.map_or(0, |d| d.as_nanos() as u64) | 1 << 63)
    }

    /// The next number in `0..1`.
    pub fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}
