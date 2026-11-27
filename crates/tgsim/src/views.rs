//! Triggers changing how the map is seen: moving a side's view, and the
//! letterbox and screen fade every side sees.

use tgmap::triggers::Value;

use crate::{TICK, World};

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

/// The letterbox or the screen fade, as a trigger last switched it: coming
/// in or going out over `seconds` of game time from tick `at`, from
/// wherever it was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slide {
    pub on: bool,
    pub at: u64,
    pub seconds: f32,
}

crate::hash::digest!(Slide { on, at, seconds });

impl Slide {
    /// How far in it is at `ticks` into the match, which may fall between
    /// two: from 0, gone, to 1, fully in. Switching restarts it from the
    /// far end, so going out starts from fully in.
    pub fn shown(&self, ticks: f32) -> f32 {
        let gone = match self.seconds > 0.0 {
            true => ((ticks - self.at as f32) * TICK / self.seconds).clamp(0.0, 1.0),
            false => 1.0,
        };
        match self.on {
            true => gone,
            false => 1.0 - gone,
        }
    }
}

impl World {
    /// The letterbox or screen fade as a trigger switches it now.
    pub(crate) fn slide(
        &self,
        on: Option<&tgmap::triggers::Parameter>,
        seconds: Option<f32>,
    ) -> Option<Slide> {
        let Some(Value::Int(on)) = on.map(|p| &p.value) else {
            return None;
        };
        Some(Slide {
            on: *on != 0,
            at: self.ticks,
            seconds: seconds.unwrap_or(0.0).max(0.0),
        })
    }
}
