//! The settings regiments keep guard by.

use std::collections::HashMap;

use crate::{GameData, Id};

/// The property a regiment resting at the post it guards has.
pub const GUARDING: &str = "actor_state_guarding";

/// The guard range of a thing that sends out units of its own and sets
/// none.
const GUARD_RANGE: f32 = 10.0;

/// The settings regiments keep guard by.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GuardRules {
    /// How much nearer the intruder a guard is already after counts than
    /// it is.
    pub bias: f32,
    /// What a regiment's control range is multiplied by while it rests at
    /// its post.
    pub control: f32,
}

impl Default for GuardRules {
    fn default() -> Self {
        GuardRules {
            bias: 3.0,
            control: 1.25,
        }
    }
}

impl GameData {
    pub fn guard_rules(&self) -> GuardRules {
        let default = GuardRules::default();
        let bias = self.svar(|v| v.guard_old_target_bias);
        let definitions = self.definitions();
        let control = definitions.properties.get(&Id::from(GUARDING)).map(|p| {
            let ranges = p.modifier.iter().filter(|m| {
                (m.attribute.as_deref()).is_some_and(|a| a.eq_ignore_ascii_case("control_range"))
            });
            1.0 + ranges.filter_map(|m| m.amount).sum::<f32>()
        });
        GuardRules {
            bias: bias.unwrap_or(default.bias),
            control: control.unwrap_or(default.control),
        }
    }

    /// How far around each template's things their guard zone reaches,
    /// for those that send out units of their own, by template id.
    pub fn guard_zones(&self) -> HashMap<Id, f32> {
        self.by_template(|t| {
            let denizen = t.denizen_component.as_ref()?;
            Some(denizen.guard_range.unwrap_or(GUARD_RANGE))
        })
    }
}
