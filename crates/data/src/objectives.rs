//! How the Objectives window draws an objective, by how it stands.

use crate::GameData;
use crate::messages::colour;

const LAYOUT: &str = "UI/Game/objectives_display.tgi";
const WINDOW: &str = "ObjectivesDisplay";

/// The text colour and box image of an objective in one state.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectiveLook {
    pub colour: [f32; 3],
    /// The path of the box drawn before it.
    pub icon: Option<String>,
}

/// How each state of objective is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectiveLooks {
    /// Active, and primary or secondary.
    pub required: ObjectiveLook,
    pub optional: ObjectiveLook,
    pub completed: ObjectiveLook,
    pub failed: ObjectiveLook,
}

impl GameData {
    /// How the Objectives window's layout draws each state, white with no
    /// box where it doesn't say.
    pub fn objective_looks(&mut self) -> ObjectiveLooks {
        // Not in the load lists.
        let parsed = self.read(LAYOUT).map(|b| tgi::parse_bytes(&b));
        let block = (parsed.as_ref()).and_then(|p| p.root.blocks_of(WINDOW).next());
        let look = |state: &str| {
            let value = |key: String| block.and_then(|b| b.value(&key)).map(str::trim);
            let icon = value(format!("icon_{state}")).map(str::to_string);
            ObjectiveLook {
                colour: value(format!("text_color_{state}"))
                    .and_then(colour)
                    .unwrap_or([1.0; 3]),
                icon: icon.filter(|i| self.exists(i)),
            }
        };
        ObjectiveLooks {
            required: look("required"),
            optional: look("optional"),
            completed: look("completed"),
            failed: look("failed"),
        }
    }

    /// Whether a side's objectives are kept in order of their text, rather
    /// than in the order they're given.
    pub fn sort_objectives(&mut self) -> bool {
        (self.svars().iter())
            .find_map(|v| v.sort_objectives)
            .unwrap_or(true)
    }
}
