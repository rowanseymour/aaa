//! Triggers moving a side's view of the map.

use tgmap::triggers::Value;

use crate::World;

/// A trigger moving one side's view, from wherever it is when the move
/// starts.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewMove {
    /// The side whose player sees it.
    pub player: u32,
    pub to: View,
    /// The tick it was made.
    pub at: u64,
    pub seconds: f32,
    pub pace: Pace,
}

crate::hash::digest!(ViewMove {
    player,
    to,
    at,
    seconds,
    pace
});

/// Where a [`ViewMove`] ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum View {
    /// A camera the map places, by its object's id.
    Camera(u32),
    /// The game's own zoom and angle, looking at a point.
    Default([f32; 2]),
}

/// How a [`ViewMove`] goes, and which clock times it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pace {
    /// Steadily, in game time.
    Steady,
    /// Starting and stopping slowly, in game time.
    Eased,
    /// Steadily, in real time.
    RealTime,
}

/// Only ever holds what triggers gave, whose debug form is exact.
impl crate::hash::Digest for View {
    fn digest(&self, h: &mut crate::hash::Hasher) {
        format!("{self:?}").digest(h);
    }
}

impl crate::hash::Digest for Pace {
    fn digest(&self, h: &mut crate::hash::Hasher) {
        format!("{self:?}").digest(h);
    }
}

impl World {
    /// Has a side's view move, saying whether the side and camera name
    /// anything.
    pub(crate) fn move_view(
        &mut self,
        player: Option<u32>,
        to: Option<View>,
        seconds: f32,
        pace: Pace,
    ) -> bool {
        let (Some(player), Some(to)) = (player, to) else {
            return false;
        };
        if let View::Camera(id) = to
            && !self.triggers.cameras.iter().any(|c| c.id == id)
        {
            return false;
        }
        self.triggers.views.push(ViewMove {
            player,
            to,
            at: self.ticks,
            seconds: seconds.max(0.0),
            pace,
        });
        true
    }
}

/// The camera a parameter names, by id.
pub(crate) fn camera(p: Option<&tgmap::triggers::Parameter>) -> Option<View> {
    match p.map(|p| &p.value) {
        Some(Value::Int(id)) => Some(View::Camera(*id)),
        _ => None,
    }
}
