//! Organizations moved onto something, keeping near it.

use crate::supply::BEAT;
use crate::{PURSUIT_DRIFT, PURSUIT_REACH, Piece, World, normalised, squared_distance, within};

/// The share of its reach short of what it's moved onto that an
/// organization makes for: a little inside, so that it's within reach once
/// there despite rounding.
const SHORT: f32 = 0.99;

impl World {
    /// Where the organization at `r` makes for to be near `piece`: its
    /// place round it in its group, or else on the way there,
    /// [`PURSUIT_REACH`] of its control range short of it; and the way it
    /// faces there.
    pub(crate) fn near(&self, r: usize, piece: Piece) -> ([f32; 2], [f32; 2]) {
        let reach = SHORT * self.reach_of(r);
        let (here, there) = (self.centre(r), self.middle(piece));
        let way = normalised([there[0] - here[0], there[1] - here[1]]);
        let way = way.unwrap_or(self.organizations[r].facing);
        match (within(here, there, reach), self.group_aim(r)) {
            (true, _) => (here, way),
            (false, Some(aim)) => (aim, way),
            (false, None) => ([there[0] - way[0] * reach, there[1] - way[1] * reach], way),
        }
    }

    /// How near an organization moved onto something comes to it.
    fn reach_of(&self, r: usize) -> f32 {
        PURSUIT_REACH * self.organizations[r].control(&self.rules.guard_rules)
    }

    /// Has each organization moved onto something stop once it's near,
    /// done with a building, and on its beat set off again after what's
    /// moved [`PURSUIT_DRIFT`] off where it was headed. Done once what it's near
    /// is gone; a fight comes first.
    pub(crate) fn keep_company(&mut self) {
        for r in 0..self.organizations.len() {
            let Some(piece) = self.organizations[r].follow else {
                continue;
            };
            if self.destroyed(r) {
                continue;
            }
            if self.left(piece) {
                self.organizations[r].follow = None;
                continue;
            }
            if !self.organizations[r].engaged.is_empty() {
                continue;
            }
            let (here, there) = (self.centre(r), self.middle(piece));
            let reach = self.reach_of(r);
            if within(here, there, reach) {
                let organization = &self.organizations[r];
                // It stops as soon as it's near, even short of where it was
                // headed should what it's near have come its way.
                if !within(organization.position, there, reach) {
                    let facing = organization.facing;
                    self.send(r, here, facing);
                }
                if matches!(piece, Piece::Building(_)) {
                    self.organizations[r].follow = None;
                }
                continue;
            }
            if !(self.ticks + r as u64).is_multiple_of(BEAT) {
                continue;
            }
            let (to, facing) = self.near(r, piece);
            let off = squared_distance(self.organizations[r].position, to);
            if off > PURSUIT_DRIFT * squared_distance(here, to) {
                self.send(r, to, facing);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::*;
    use crate::{Command, Piece, Refused, distance};

    /// Two regiments of player 1's, the second 30 up from the first.
    fn pair() -> crate::World {
        let mut w = battle(
            &[1, 1],
            vec![soldier(Vec::new()), soldier(Vec::new())],
            at_war(),
        );
        w.units[1].position = [10.0, 40.0];
        w.organizations[1].position = [10.0, 40.0];
        w
    }

    #[test]
    fn moved_onto_a_regiment_it_rests_near_it_and_follows_it_off() {
        let mut w = pair();
        let onto = Command::MoveOnto {
            organization: 0,
            piece: Piece::Organization(1),
        };
        w.apply(1, onto).unwrap();
        run(&mut w, 30.0);
        // It stops within 0.6 of its control range of 18, short of the
        // regiment, and rests there.
        let gap = distance(w.centre(0), w.centre(1));
        assert!(gap <= 10.8 && gap > 5.0, "{gap}");
        assert!(!w.underway(0));
        assert_eq!(w.state(0), Some(tgmap::RESTING));
        assert_eq!(w.organizations[0].follow, Some(Piece::Organization(1)));
        let away = Command::Move {
            organization: 1,
            to: [10.0, 75.0],
        };
        w.apply(1, away).unwrap();
        run(&mut w, 40.0);
        let gap = distance(w.centre(0), w.centre(1));
        assert!(w.centre(0)[1] > 60.0 && gap <= 10.8, "{gap}");
        assert!(!w.underway(0));
        // A new order ends it.
        let stop = Command::Stop { organization: 0 };
        w.apply(1, stop).unwrap();
        assert_eq!(w.organizations[0].follow, None);
    }

    #[test]
    fn it_stops_as_soon_as_whatever_its_moved_onto_comes_its_way() {
        let mut w = pair();
        let onto = Command::MoveOnto {
            organization: 0,
            piece: Piece::Organization(1),
        };
        w.apply(1, onto).unwrap();
        let towards = Command::Move {
            organization: 1,
            to: [10.0, 10.0],
        };
        w.apply(1, towards).unwrap();
        run(&mut w, 6.0);
        // Within reach well short of where it set off for, it's stopped.
        assert!(distance(w.centre(0), w.centre(1)) <= 10.8);
        assert!(w.organizations[0].position[1] < 22.0);
        assert!(!w.underway(0));
    }

    #[test]
    fn moved_onto_a_building_it_stops_short_and_is_done() {
        let w = battle(&[1], vec![soldier(Vec::new())], at_war());
        let mut w = with_buildings(w, &[building(9, 1, [10.0, 50.0])]);
        let onto = Command::MoveOnto {
            organization: 0,
            piece: Piece::Building(0),
        };
        w.apply(1, onto).unwrap();
        run(&mut w, 30.0);
        let gap = distance(w.centre(0), [10.0, 50.0]);
        assert!(gap <= 10.8 && gap > 5.0, "{gap}");
        assert_eq!(w.organizations[0].follow, None);
        assert!(w.resting(0));
    }

    #[test]
    fn regiments_moved_onto_one_thing_stop_on_the_side_each_came_from() {
        let w = battle(
            &[1, 1],
            vec![soldier(Vec::new()), soldier(Vec::new())],
            at_war(),
        );
        let mut w = with_buildings(w, &[building(9, 1, [40.0, 40.0])]);
        for (r, at) in [(0, [10.0, 40.0]), (1, [70.0, 40.0])] {
            w.units[r].position = at;
            w.organizations[r].position = at;
        }
        for organization in 0..2 {
            let onto = Command::MoveOnto {
                organization,
                piece: Piece::Building(0),
            };
            w.apply(1, onto).unwrap();
        }
        run(&mut w, 30.0);
        let (west, east) = (w.centre(0), w.centre(1));
        assert!(west[0] < 40.0 && east[0] > 40.0, "{west:?} {east:?}");
        for at in [west, east] {
            let gap = distance(at, [40.0, 40.0]);
            assert!(gap <= 10.8 && gap > 5.0, "{gap}");
        }
    }

    #[test]
    fn nothing_moves_onto_itself_or_what_is_gone() {
        let mut w = pair();
        let onto = |piece| Command::MoveOnto {
            organization: 0,
            piece,
        };
        let itself = onto(Piece::Organization(0));
        assert_eq!(w.apply(1, itself), Err(Refused::Unable));
        w.units[1].die(0);
        assert_eq!(
            w.apply(1, onto(Piece::Organization(1))),
            Err(Refused::Unable)
        );
    }
}
