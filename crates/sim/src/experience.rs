//! The experience a side's general earns and spends on special operations.

use crate::{TICK, World};

/// A side's general's experience.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Commander {
    pub experience: f32,
    /// The most it can hold, if there's a ceiling.
    pub max: Option<f32>,
}

impl Commander {
    fn set(&mut self, experience: f32) {
        let max = self.max.unwrap_or(f32::INFINITY);
        self.experience = experience.clamp(0.0, max);
    }
}

impl World {
    /// Gives `player` a general whose side holds at most `max`, starting
    /// with `experience`.
    pub fn lead(&mut self, player: u32, experience: f32, max: Option<f32>) {
        let mut commander = Commander {
            experience: 0.0,
            max,
        };
        commander.set(experience);
        self.commanders.insert(player, commander);
    }

    /// `player`'s experience, for a side with a general.
    pub fn experience(&self, player: u32) -> Option<f32> {
        self.commanders.get(&player).map(|c| c.experience)
    }

    /// Credits every side with a general a tick's steady gain.
    pub(crate) fn gain_experience(&mut self) {
        let gain = self.rules.experience_per_second * TICK;
        for commander in self.commanders.values_mut() {
            commander.set(commander.experience + gain);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        let pathing = aaa_map::Pathing::uniform(8, aaa_map::Domain::Land);
        World::from_regiments(crate::Sides::default(), pathing, &[])
    }

    #[test]
    fn sides_with_a_general_gain_experience_up_to_its_ceiling() {
        let mut w = world();
        w.rules.experience_per_second = 1.0;
        w.lead(1, 200.0, Some(5000.0));
        w.lead(2, 4999.5, Some(5000.0));
        w.lead(3, 9000.0, None);
        for _ in 0..10 {
            w.tick();
        }
        assert!((w.experience(1).unwrap() - 201.0).abs() < 1e-3);
        assert_eq!(w.experience(2), Some(5000.0));
        assert!((w.experience(3).unwrap() - 9001.0).abs() < 1e-2);
        // A side without a general has none.
        assert_eq!(w.experience(4), None);
    }

    #[test]
    fn a_start_past_the_ceiling_is_kept_to_it() {
        let mut w = world();
        w.lead(1, 6000.0, Some(5000.0));
        w.lead(2, -5.0, Some(5000.0));
        assert_eq!(
            (w.experience(1), w.experience(2)),
            (Some(5000.0), Some(0.0))
        );
    }
}
