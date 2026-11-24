//! The player's workers standing idle: builders, and trucks that unpack
//! into buildings, with nothing to do, for the interface to step through.

use crate::morale::Mind;
use crate::{Order, World};

impl World {
    /// `player`'s organizations, regiments or lone things alike, that
    /// build or are a truck and are doing nothing at all, in the order
    /// they're kept.
    pub fn idle_builders(&self, player: u32) -> Vec<usize> {
        (0..self.organizations.len())
            .filter(|&r| self.organizations[r].owner == player && self.idle_worker(r))
            .collect()
    }

    /// Whether the organization at `r` is a builder or truck, or holds a
    /// builder, with nothing to do.
    fn idle_worker(&self, r: usize) -> bool {
        let organization = &self.organizations[r];
        // The packed side of a pack pair; the unpacked side is a building.
        let truck = organization.unpacks.is_some();
        let builder = (self.survivors(r)).any(|u| {
            self.units[u]
                .fighter
                .as_ref()
                .is_some_and(|f| !f.builds.is_empty())
        });
        (truck || builder) && self.doing_nothing(r)
    }

    /// Whether the organization at `r` is carrying out no order, isn't
    /// fighting or getting over a fight, and isn't on its way anywhere.
    pub(crate) fn doing_nothing(&self, r: usize) -> bool {
        let organization = &self.organizations[r];
        matches!(organization.mind, Mind::Steady | Mind::Exhausted)
            && organization.order == Order::Move
            && organization.engaged.is_empty()
            && !organization.chasing()
            && organization.guard.is_none()
            && organization.follow.is_none()
            && organization.build.is_none()
            && organization.mending.is_none()
            && organization.unpack_at.is_none()
            && organization.unpacked_into.is_none()
            && !organization.bombarding
            && !organization.arriving
            && !organization.travelling
            && !self.underway(r)
            && !self.destroyed(r)
    }
}

#[cfg(test)]
mod tests {
    use tgmap::Id;

    use crate::Command;
    use crate::morale::Mind;
    use crate::testing::*;

    /// Player 1's engineer, rifleman and truck, and player 2's engineer.
    fn workers() -> crate::World {
        let mut engineer = soldier(Vec::new());
        engineer.builds = vec![Id::from("bunker")];
        let mut w = battle(
            &[1, 1, 2],
            vec![engineer.clone(), soldier(Vec::new()), engineer],
            at_war(),
        );
        w.units[2].position = [35.0, 35.0];
        w.organizations[2].position = [35.0, 35.0];
        let mut t = truck();
        t.owner = 1;
        t.renumber(20);
        t.place([30.0, 10.0], [0.0, 1.0]);
        w.add_organization(&t);
        w
    }

    #[test]
    fn builders_and_trucks_doing_nothing_are_idle_in_the_order_kept() {
        let w = workers();
        assert_eq!(w.idle_builders(1), [0, 3]);
        assert_eq!(w.idle_builders(2), [2]);
        assert!(w.idle_builders(3).is_empty());
    }

    #[test]
    fn a_regiment_holding_a_builder_counts_whatever_its_others_are() {
        let mut w = workers();
        // The rifleman joins the engineer's regiment, and his own holds
        // the engineer.
        w.organizations[1].units = vec![1, 0];
        assert_eq!(w.idle_builders(1), [0, 1, 3]);
    }

    #[test]
    fn an_order_or_a_fight_keeps_them_busy_until_its_over() {
        let mut w = workers();
        let to = [10.0, 30.0];
        w.apply(
            1,
            Command::Move {
                organization: 0,
                to,
            },
        )
        .unwrap();
        assert_eq!(w.idle_builders(1), [3]);
        run(&mut w, 20.0);
        assert_eq!(w.idle_builders(1), [0, 3]);
        // Still on its way, whether or not its men are moving this moment.
        w.organizations[0].travelling = true;
        assert_eq!(w.idle_builders(1), [3]);
        w.organizations[0].travelling = false;
        w.organizations[3].mind = Mind::Reforming { until: u64::MAX };
        assert_eq!(w.idle_builders(1), [0]);
        w.organizations[3].mind = Mind::Steady;
        w.organizations[0]
            .engaged
            .push(crate::Piece::Organization(2));
        assert_eq!(w.idle_builders(1), [3]);
    }

    #[test]
    fn the_lost_are_never_idle() {
        let mut w = workers();
        w.destroy(0);
        assert_eq!(w.idle_builders(1), [3]);
    }
}
