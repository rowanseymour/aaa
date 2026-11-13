//! The properties things hold for a time.

use aaa_map::Id;

use crate::{Enemy, TICK, World};

/// A property something has for a time.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub property: Id,
    /// The tick its time is up on. It's taken off at its holder's next
    /// look after.
    pub until: u64,
}

impl World {
    /// Gives a regiment or building a property for `seconds` from now,
    /// unless it still has it.
    pub fn give(&mut self, to: Enemy, property: Id, seconds: f32) {
        let until = self.ticks + (seconds / TICK).round() as u64;
        let held = match to {
            Enemy::Regiment(r) => &mut self.regiments[r].properties,
            Enemy::Building(b) => &mut self.buildings[b].properties,
        };
        if !held.iter().any(|h| h.property == property) {
            held.push(Held { property, until });
        }
    }
}
