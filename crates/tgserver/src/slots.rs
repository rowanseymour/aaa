//! What a lobby has chosen for each of a map's sides, and how it changes as
//! people and computer players move between them.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use tgdata::Id;

/// What's chosen for the sides, by player id. A side is held by a person,
/// left open for nobody, or else played by a computer player.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Slots {
    /// The nation, faction and handicap chosen for each side, where one is.
    pub nations: BTreeMap<u32, Id>,
    pub factions: BTreeMap<u32, Id>,
    pub handicaps: BTreeMap<u32, Id>,
    /// The profile chosen for each side's computer player; one at random
    /// where none is.
    pub computers: BTreeMap<u32, Id>,
    /// The sides nobody holds.
    pub open: BTreeSet<u32>,
}

impl Slots {
    /// Every one of `sides` left open, with nothing chosen.
    pub fn left_open(sides: &[u32]) -> Slots {
        Slots {
            open: sides.iter().copied().collect(),
            ..Slots::default()
        }
    }

    /// Whether nobody holds side `player`.
    pub fn is_open(&self, player: u32) -> bool {
        self.open.contains(&player)
    }

    /// Puts a computer player on side `player`, playing by profile `sai`
    /// or one at random.
    pub fn computer(&mut self, player: u32, sai: Option<Id>) {
        self.open.remove(&player);
        chosen(&mut self.computers, player, sai);
    }

    /// Leaves side `player` for nobody, with nothing chosen for it.
    pub fn empty(&mut self, player: u32) {
        self.nations.remove(&player);
        self.factions.remove(&player);
        self.handicaps.remove(&player);
        self.computers.remove(&player);
        self.open.insert(player);
    }

    /// Moves a person from side `from` to side `to`, none being to watch.
    /// Their nation and faction go with them. A computer player on `to`
    /// trades places with them, taking its profile, handicap, nation and
    /// faction to `from`, or goes where they come from no side; a side
    /// they leave with nobody coming to it is left open.
    pub fn seat(&mut self, from: Option<u32>, to: Option<u32>) {
        if from == to {
            return;
        }
        let computer = to.filter(|&t| !self.is_open(t));
        let took =
            |map: &mut BTreeMap<u32, Id>, side: Option<u32>| side.and_then(|s| map.remove(&s));
        let mine = (
            took(&mut self.nations, from),
            took(&mut self.factions, from),
        );
        let theirs = (took(&mut self.nations, to), took(&mut self.factions, to));
        let sai = took(&mut self.computers, computer);
        let handicap = took(&mut self.handicaps, to);
        if let Some(to) = to {
            self.open.remove(&to);
            put(&mut self.nations, to, mine.0);
            put(&mut self.factions, to, mine.1);
        }
        let Some(from) = from else {
            return;
        };
        if computer.is_none() {
            self.empty(from);
            return;
        }
        put(&mut self.nations, from, theirs.0);
        put(&mut self.factions, from, theirs.1);
        put(&mut self.computers, from, sai);
        put(&mut self.handicaps, from, handicap);
    }
}

/// Sets `map`'s entry for `player` to `id`, or clears it with none.
fn chosen(map: &mut BTreeMap<u32, Id>, player: u32, id: Option<Id>) {
    match id {
        Some(id) => _ = map.insert(player, id),
        None => _ = map.remove(&player),
    }
}

/// Sets `map`'s entry for `player` to `id` where there's one.
fn put(map: &mut BTreeMap<u32, Id>, player: u32, id: Option<Id>) {
    if let Some(id) = id {
        map.insert(player, id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> Id {
        Id::from(s)
    }

    /// A person on side 1 as the USA, and Konev's computer player on side
    /// 2 as Russia on Easy.
    fn slots() -> Slots {
        Slots {
            nations: [(1, id("usa")), (2, id("rus"))].into(),
            factions: [(1, id("patton")), (2, id("konev"))].into(),
            handicaps: [(2, id("easy"))].into(),
            computers: [(2, id("konev_sai"))].into(),
            open: BTreeSet::from([3]),
        }
    }

    #[test]
    fn moving_onto_a_computer_players_side_trades_places_with_it() {
        let mut slots = slots();
        slots.seat(Some(1), Some(2));
        assert_eq!(slots.nations, [(1, id("rus")), (2, id("usa"))].into());
        assert_eq!(slots.factions, [(1, id("konev")), (2, id("patton"))].into());
        assert_eq!(slots.handicaps, [(1, id("easy"))].into());
        assert_eq!(slots.computers, [(1, id("konev_sai"))].into());
        assert_eq!(slots.open, BTreeSet::from([3]));
    }

    #[test]
    fn moving_onto_an_open_side_leaves_the_old_one_open() {
        let mut slots = slots();
        slots.seat(Some(1), Some(3));
        assert_eq!(slots.nations, [(2, id("rus")), (3, id("usa"))].into());
        assert_eq!(slots.open, BTreeSet::from([1]));
        // And to watch, open too.
        slots.seat(Some(3), None);
        assert_eq!(slots.open, BTreeSet::from([1, 3]));
        assert_eq!(slots.nations, [(2, id("rus"))].into());
    }

    #[test]
    fn coming_from_no_side_puts_out_the_computer_player() {
        let mut slots = slots();
        slots.seat(None, Some(2));
        assert!(slots.computers.is_empty() && slots.handicaps.is_empty());
        assert_eq!(slots.nations, [(1, id("usa"))].into());
    }

    #[test]
    fn a_computer_player_on_an_open_side_takes_it() {
        let mut slots = slots();
        slots.computer(3, Some(id("chuikov_sai")));
        assert!(slots.open.is_empty());
        slots.empty(2);
        assert_eq!(slots.open, BTreeSet::from([2]));
        assert!(!slots.nations.contains_key(&2) && !slots.computers.contains_key(&2));
    }
}
