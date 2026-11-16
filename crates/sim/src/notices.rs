//! What the match tells its sides as it's played.

use aaa_map::Id;

use crate::{Pause, Piece, World};

/// Something a side is told.
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// Chat, as [`Action::Chat`](crate::Action::Chat) gave it: everyone's
    /// to tell, by who it reaches.
    Chat {
        from: Option<u32>,
        to: crate::Channel,
        words: String,
    },
    /// Its stockpile has run out, its income `percent` short of its costs.
    Short { player: u32, percent: f32 },
    /// It's no longer short.
    Repaired { player: u32 },
    /// A building of its, by index into [`World::buildings`], has stopped
    /// work on the organization of `template` at the front of its queue.
    Paused {
        player: u32,
        building: usize,
        template: String,
        why: Pause,
    },
    /// A packed building of its, by index into [`World::organizations`],
    /// couldn't unpack where it was sent.
    CannotUnpack { player: u32, organization: usize },
    /// An organization of its, by index into [`World::organizations`], was
    /// ordered to the HQ with this id.
    Attached {
        player: u32,
        hq: u32,
        organization: usize,
    },
    /// An organization of its was ordered away from the HQ with this id.
    Detached {
        player: u32,
        hq: u32,
        organization: usize,
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
    /// the organization at this index into [`World::organizations`].
    PutOut {
        player: u32,
        building: usize,
        organization: usize,
    },
    /// A building of its, by index into [`World::buildings`], has sent
    /// for the delivery at this index into [`World::deliveries`].
    Recruited {
        player: u32,
        building: usize,
        delivery: usize,
    },
    /// A building of its, by index into [`World::buildings`], has finished
    /// researching `technology`.
    Researched {
        player: u32,
        building: usize,
        technology: Id,
    },
    /// What a special operation of its would summon, of `template`, had
    /// nowhere to go.
    SummonFailed { player: u32, template: Id },
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
    /// It's lost the match, which goes on.
    Defeated { player: u32 },
    /// It's won the match.
    Victorious { player: u32 },
    /// Every side of the team at this index into
    /// [`Sides::teams`](crate::Sides::teams) has lost the match, which
    /// goes on.
    TeamDefeated { team: usize },
    /// The team at this index into [`Sides::teams`](crate::Sides::teams)
    /// has won the match.
    TeamVictorious { team: usize },
    /// It holds, with its team, the share of the map's cities that wins
    /// the match: this many.
    CitiesHeld { player: u32, cities: usize },
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
