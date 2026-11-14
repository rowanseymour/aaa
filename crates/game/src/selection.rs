//! What a selection may hold: things of any side, but only of one standing
//! towards the player and one kind at a time, ranked so that what's picked
//! either joins the selection, replaces it or is passed over. Only a
//! selection of the player's own things takes orders.

use aaa_sim::{Relation, World};
use bevy::math::Vec2;

use crate::object_display::MAX_SELECTED;

/// How a thing's owner stands towards the player, lowest first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Standing {
    Neutral,
    Ally,
    Enemy,
    Own,
}

/// What sort of thing it is, lowest first. Many regiments go together,
/// but only one building.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Kind {
    Building,
    Regiment,
}

/// Something that can be selected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Thing {
    Regiment(usize),
    Building(usize),
}

/// What's selected: regiments, or a building on its own.
pub struct Selection<'a> {
    pub regiments: &'a mut Vec<usize>,
    pub building: &'a mut Option<usize>,
}

pub fn standing(sim: &World, local: Option<u32>, owner: u32) -> Standing {
    let Some(player) = local else {
        return Standing::Neutral;
    };
    match sim.sides.relation(player, owner) {
        Relation::Itself => Standing::Own,
        Relation::Enemy => Standing::Enemy,
        Relation::Ally => Standing::Ally,
        Relation::Neutral => Standing::Neutral,
    }
}

/// Where a thing ranks in a selection: by standing, then by kind.
pub type Rank = (Standing, Kind);

pub fn rank(sim: &World, local: Option<u32>, thing: Thing) -> Rank {
    match thing {
        Thing::Regiment(r) => (standing(sim, local, sim.regiments[r].owner), Kind::Regiment),
        Thing::Building(b) => (standing(sim, local, sim.buildings[b].owner), Kind::Building),
    }
}

/// What a click on `thing` selects: a city's building stands for its city.
pub fn picked(sim: &World, thing: Thing) -> Thing {
    match thing {
        Thing::Building(b) => sim.city_of(b).map_or(thing, Thing::Building),
        Thing::Regiment(_) => thing,
    }
}

/// Whether the player may give the selection orders: it's their own.
pub fn commanded(
    sim: &World,
    local: Option<u32>,
    regiments: &[usize],
    building: Option<usize>,
) -> bool {
    let Some(player) = local else {
        return false;
    };
    match (regiments.first(), building) {
        (Some(&r), _) => sim.regiments[r].owner == player,
        (None, Some(b)) => sim.buildings[b].owner == player,
        (None, None) => false,
    }
}

impl Selection<'_> {
    fn first(&self) -> Option<Thing> {
        match (self.regiments.first(), *self.building) {
            (Some(&r), _) => Some(Thing::Regiment(r)),
            (None, Some(b)) => Some(Thing::Building(b)),
            (None, None) => None,
        }
    }

    fn holds(&self, thing: Thing) -> bool {
        match thing {
            Thing::Regiment(r) => self.regiments.contains(&r),
            Thing::Building(b) => *self.building == Some(b),
        }
    }

    pub fn clear(&mut self) {
        self.regiments.clear();
        *self.building = None;
    }

    fn take(&mut self, thing: Thing) {
        match thing {
            Thing::Regiment(r) => self.regiments.push(r),
            Thing::Building(b) => *self.building = Some(b),
        }
    }

    /// Selects `thing` alone.
    pub fn replace(&mut self, thing: Thing) {
        self.clear();
        self.take(thing);
    }

    /// Adds `thing` if it ranks with what's selected and there's room,
    /// selects it alone if it outranks it, and else leaves things be.
    pub fn add(&mut self, rank: impl Fn(Thing) -> Rank, thing: Thing) {
        let theirs = rank(thing);
        match self.first().map(&rank) {
            Some(ours) if ours > theirs => {}
            Some(ours) if ours == theirs => {
                let room = self.regiments.len() < MAX_SELECTED;
                if theirs.1 == Kind::Regiment && room && !self.holds(thing) {
                    self.take(thing);
                }
            }
            _ => self.replace(thing),
        }
    }

    /// Takes `thing` back out if it's selected, else adds it.
    pub fn toggle(&mut self, rank: impl Fn(Thing) -> Rank, thing: Thing) {
        match thing {
            Thing::Regiment(r) if self.regiments.contains(&r) => self.regiments.retain(|&s| s != r),
            Thing::Building(b) if *self.building == Some(b) => *self.building = None,
            _ => self.add(rank, thing),
        }
    }

    /// Takes in what a dragged box covers, each with where it's drawn: only
    /// the highest-ranked of them, and while there's no room for all, those
    /// nearest `middle`.
    pub fn boxed(
        &mut self,
        rank: impl Fn(Thing) -> Rank,
        mut inside: Vec<(Thing, Vec2)>,
        middle: Vec2,
    ) {
        let Some(top) = inside.iter().map(|&(t, _)| rank(t)).max() else {
            return;
        };
        inside.retain(|&(t, _)| rank(t) == top && !self.holds(t));
        inside.sort_by(|a, b| a.1.distance(middle).total_cmp(&b.1.distance(middle)));
        for (thing, _) in inside {
            self.add(&rank, thing);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regiment 0 is the player's, regiments 1 and 2 and building 0 an
    /// enemy's, building 1 an ally's.
    fn ranked(thing: Thing) -> Rank {
        match thing {
            Thing::Regiment(0) => (Standing::Own, Kind::Regiment),
            Thing::Regiment(_) => (Standing::Enemy, Kind::Regiment),
            Thing::Building(0) => (Standing::Enemy, Kind::Building),
            Thing::Building(_) => (Standing::Ally, Kind::Building),
        }
    }

    #[test]
    fn a_selection_holds_one_standing_and_kind() {
        let (mut regiments, mut building) = (Vec::new(), None);
        let mut s = Selection {
            regiments: &mut regiments,
            building: &mut building,
        };
        s.replace(Thing::Regiment(1));
        s.add(ranked, Thing::Regiment(2));
        assert_eq!(*s.regiments, [1, 2]);
        // An enemy's building ranks below its regiments.
        s.add(ranked, Thing::Building(0));
        assert_eq!((&*s.regiments, *s.building), (&vec![1, 2], None));
        // One's own outranks them all, and nothing of others joins it.
        s.add(ranked, Thing::Regiment(0));
        assert_eq!(*s.regiments, [0]);
        s.add(ranked, Thing::Regiment(1));
        assert_eq!(*s.regiments, [0]);
        s.toggle(ranked, Thing::Regiment(0));
        assert!(s.regiments.is_empty());
        // Only one building at a time, and an enemy's outranks an ally's.
        s.add(ranked, Thing::Building(1));
        s.add(ranked, Thing::Building(0));
        assert_eq!(*s.building, Some(0));
        s.add(ranked, Thing::Building(1));
        assert_eq!(*s.building, Some(0));
    }

    #[test]
    fn a_selection_takes_no_more_than_it_can_show() {
        let (mut regiments, mut building) = (Vec::new(), None);
        let mut s = Selection {
            regiments: &mut regiments,
            building: &mut building,
        };
        for r in 1..=MAX_SELECTED + 1 {
            s.add(ranked, Thing::Regiment(r));
        }
        assert_eq!(s.regiments.len(), MAX_SELECTED);
    }

    #[test]
    fn a_box_takes_only_its_highest_ranked_things_nearest_first() {
        let (mut regiments, mut building) = (Vec::new(), None);
        let mut s = Selection {
            regiments: &mut regiments,
            building: &mut building,
        };
        let inside = vec![
            (Thing::Building(0), Vec2::ZERO),
            (Thing::Regiment(2), Vec2::new(5.0, 0.0)),
            (Thing::Regiment(1), Vec2::new(1.0, 0.0)),
        ];
        s.boxed(ranked, inside, Vec2::ZERO);
        assert_eq!((&*s.regiments, *s.building), (&vec![1, 2], None));
    }
}
