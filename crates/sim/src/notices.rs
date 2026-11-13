//! What the match tells its sides as it's played.

use aaa_map::Id;

use crate::{Pause, Piece, World};

/// Something a side is told.
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// Its stockpile has run out, its income `percent` short of its costs.
    Short { player: u32, percent: f32 },
    /// It's no longer short.
    Repaired { player: u32 },
    /// A building of its, by index into [`World::buildings`], has stopped
    /// work on the regiment of `template` at the front of its queue.
    Paused {
        player: u32,
        building: usize,
        template: String,
        why: Pause,
    },
    /// A packed building of its, by index into [`World::regiments`],
    /// couldn't unpack where it was sent.
    CannotUnpack { player: u32, regiment: usize },
    /// A regiment of its, by index into [`World::regiments`], was ordered
    /// to the HQ with this id.
    Attached {
        player: u32,
        hq: u32,
        regiment: usize,
    },
    /// A regiment of its was ordered away from the HQ with this id.
    Detached {
        player: u32,
        hq: u32,
        regiment: usize,
    },
    /// A building of its, by index into [`World::buildings`], was taken by
    /// player `by`; for a city, its centre.
    Lost {
        player: u32,
        building: usize,
        by: u32,
    },
    /// It took a building, by index into [`World::buildings`]; for a city,
    /// its centre.
    Captured { player: u32, building: usize },
    /// A building of its, by index into [`World::buildings`], has put out
    /// the regiment at this index into [`World::regiments`].
    PutOut {
        player: u32,
        building: usize,
        regiment: usize,
    },
    /// A building of its, by index into [`World::buildings`], has finished
    /// researching `technology`.
    Researched {
        player: u32,
        building: usize,
        technology: Id,
    },
    /// It holds a technology it didn't.
    Gained { player: u32, technology: Id },
    /// Something of `owner`'s is destroyed, by whatever cause.
    Destroyed { owner: u32, what: Piece },
    /// A building of its, by index into [`World::buildings`], has been
    /// sold, or destroyed when it could have been, for `paid` money.
    Sold {
        player: u32,
        building: usize,
        paid: f32,
    },
}

impl World {
    /// What sides have been told since these were last taken.
    pub fn notices(&self) -> &[Notice] {
        &self.notices
    }

    /// Takes what sides have been told since these were last taken.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }
}
