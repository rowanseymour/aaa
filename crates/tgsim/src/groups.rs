//! Groups: regiments ordered as one with Group Behavior on, timed so that
//! those moving arrive together.

use crate::Piece;

use crate::supply::BEAT;
use crate::{Command, Refused, World, distance};

/// The least radius of a group's zone, which the regiments taking part in
/// its timing are inside.
const ZONE_RANGE_MIN: f32 = 18.0;

/// The least speed a group hands a regiment; one it would hand less stands
/// and waits for the rest.
const SPEED_MIN: f32 = 1.0;

/// How far outside the supply zone of an enemy thing a group heads for its
/// regiments time their arrival at.
const SUPPLY_ZONE_BUFFER: f32 = 5.0;

/// Regiments ordered together as one.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub id: u32,
    pub owner: u32,
    /// Indices into [`World::organizations`], in the group's order.
    pub organizations: Vec<usize>,
    /// The building it was last sent onto, by index into
    /// [`World::buildings`].
    pub heading_for: Option<usize>,
}

crate::digest!(Group {
    id,
    owner,
    organizations,
    heading_for,
});

impl World {
    /// Gives `orders` to the regiments they're to as one group: a fresh
    /// group of them all, each leaving any it was in, timed together from
    /// now on. Refused only if every order is.
    pub(crate) fn order_group(&mut self, player: u32, orders: Vec<Command>) -> Result<(), Refused> {
        let mut ordered: Vec<usize> = Vec::new();
        for o in orders.iter().filter_map(crate::commands::steered) {
            if self.commands(player, o) && !ordered.contains(&o) {
                ordered.push(o);
            }
        }
        if ordered.len() >= 2 {
            for &r in &ordered {
                self.leave_group(r);
            }
            let id = self.next_group;
            self.next_group += 1;
            let heading_for = orders.iter().find_map(|o| match o {
                Command::MoveOnto {
                    piece: Piece::Building(b),
                    ..
                } => Some(*b),
                _ => None,
            });
            for &r in &ordered {
                self.organizations[r].group = Some(id);
            }
            self.groups.push(Group {
                id,
                owner: player,
                organizations: ordered,
                heading_for,
            });
        }
        let mut done = Err(Refused::Unable);
        for order in orders {
            match self.give_order(player, order, true) {
                Ok(()) => done = Ok(()),
                Err(e) if done.is_err() => done = Err(e),
                Err(_) => {}
            }
        }
        if let Some(g) = self.groups.last().map(|g| g.id) {
            self.mark_taking_part(g);
        }
        done
    }

    /// Takes the organization at `r` out of its group, which goes if
    /// that leaves it empty.
    pub(crate) fn leave_group(&mut self, r: usize) {
        let organization = &mut self.organizations[r];
        organization.group_speed = None;
        let Some(id) = organization.group.take() else {
            return;
        };
        if let Some(g) = self.groups.iter_mut().find(|g| g.id == id) {
            g.organizations.retain(|&o| o != r);
        }
        self.groups.retain(|g| !g.organizations.is_empty());
    }

    /// Marks those of the group `id`'s regiments inside its zone as
    /// taking part in its timing, and the rest as not: the circle round
    /// the average of their positions as wide as the widest control zone
    /// among them, and no narrower than [`ZONE_RANGE_MIN`].
    fn mark_taking_part(&mut self, id: u32) {
        let Some(g) = self.groups.iter().find(|g| g.id == id) else {
            return;
        };
        let centre = self.group_centre(&g.organizations);
        let reach = (g.organizations.iter())
            .map(|&r| self.organizations[r].control_range)
            .fold(ZONE_RANGE_MIN, f32::max);
        let inside: Vec<(usize, bool)> = (g.organizations.iter())
            .map(|&r| (r, distance(self.centre(r), centre) <= reach))
            .collect();
        for (r, inside) in inside {
            self.organizations[r].taking_part = inside;
        }
    }

    fn group_centre(&self, organizations: &[usize]) -> [f32; 2] {
        let n = organizations.len().max(1) as f32;
        (organizations.iter().map(|&r| self.centre(r)))
            .fold([0.0, 0.0], |[x, y], [a, b]| [x + a / n, y + b / n])
    }

    /// Once a second, for each group: takes out what's routing,
    /// exhausted, gone or no longer its owner's; and unless it's in a
    /// fight, which marks again who takes part and times nothing, hands
    /// each regiment taking part that's moving the speed that has it
    /// arrive when the furthest out in time does.
    pub(crate) fn keep_groups(&mut self) {
        let ticks = self.ticks;
        let leaving: Vec<usize> = (0..self.organizations.len())
            .filter(|&r| {
                let o = &self.organizations[r];
                let owner = self
                    .groups
                    .iter()
                    .find(|g| Some(g.id) == o.group)
                    .map(|g| g.owner);
                o.group.is_some()
                    && (o.routing()
                        || o.mind == crate::morale::Mind::Exhausted
                        || self.destroyed(r)
                        || owner != Some(o.owner))
            })
            .collect();
        for r in leaving {
            self.leave_group(r);
        }
        let ids: Vec<u32> = self.groups.iter().map(|g| g.id).collect();
        for id in ids {
            if !(ticks + u64::from(id)).is_multiple_of(BEAT) {
                continue;
            }
            if self.group_in_fight(id) {
                self.mark_taking_part(id);
                let g = self.groups.iter().find(|g| g.id == id).expect("listed");
                for r in g.organizations.clone() {
                    self.organizations[r].group_speed = None;
                }
                continue;
            }
            self.time_group(id);
        }
    }

    /// Whether the group `id` is in a fight: whether the first of its
    /// regiments taking part that is engaged is engaged with something
    /// other than what it's going after.
    fn group_in_fight(&self, id: u32) -> bool {
        let g = self.groups.iter().find(|g| g.id == id).expect("listed");
        let first = (g.organizations.iter())
            .map(|&r| &self.organizations[r])
            .find(|o| o.taking_part && !o.engaged.is_empty());
        first.is_some_and(|o| match o.order {
            crate::Order::Pursue(t) => !o.engaged.iter().any(|&p| self.piece_of(p, t)),
            _ => true,
        })
    }

    /// Whether `piece` is the organization at `t`.
    fn piece_of(&self, piece: Piece, t: usize) -> bool {
        piece == Piece::Organization(t)
    }

    /// Hands each of the group `id`'s regiments taking part and moving the
    /// speed that makes its time there the longest's: its own speed times
    /// its time over the longest. Heading for an enemy thing with a supply
    /// zone, each times its arrival at the zone's edge, a little out, less
    /// how far ahead of the group's centre it is.
    fn time_group(&mut self, id: u32) {
        let g = self
            .groups
            .iter()
            .find(|g| g.id == id)
            .expect("listed")
            .clone();
        let zone = g.heading_for.and_then(|b| {
            let range = self.supply_range(b)?;
            let enemy = self.buildings[b].owner != g.owner;
            enemy.then(|| (self.buildings[b].position, range))
        });
        let centre = self.group_centre(&g.organizations);
        let facing =
            zone.and_then(|(at, _)| crate::normalised([at[0] - centre[0], at[1] - centre[1]]));
        let times: Vec<(usize, f32, f32)> = (g.organizations.iter())
            .filter(|&&r| self.organizations[r].taking_part && self.underway(r))
            .filter_map(|&r| {
                let speed = self.unhurried_speed(r);
                let lp = self.pacer(r)?;
                let mut left = self.units[self.organizations[r].units[lp]].route_left();
                if let (Some((_, range)), Some(facing)) = (zone, facing) {
                    let ahead = tgmap::to_local(centre, facing, self.centre(r))[1];
                    left = (left - (range + SUPPLY_ZONE_BUFFER + ahead)).max(0.0);
                }
                (speed > 0.0).then_some((r, speed, left / speed))
            })
            .collect();
        let longest = times.iter().map(|&(_, _, t)| t).fold(0.0, f32::max);
        for &r in &g.organizations {
            self.organizations[r].group_speed = None;
        }
        if longest <= 0.0 {
            return;
        }
        for (r, speed, time) in times {
            self.organizations[r].group_speed = Some(speed * time / longest);
        }
    }

    /// Whether the organization at `r` is waiting for the rest of its
    /// group: handed too low a speed to crawl at, it stands.
    pub(crate) fn waiting_for_group(&self, r: usize) -> bool {
        self.organizations[r]
            .group_speed
            .is_some_and(|s| s < SPEED_MIN)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::*;
    use crate::{Command, SECOND};

    use tgmap::{Domain, Pathing};

    /// Player 7's two squads of one man each, side by side.
    fn grouped() -> crate::World {
        let mut w = world(Pathing::uniform(60, Domain::Land));
        let mut other = w.organizations[0].clone();
        other.units = Vec::new();
        let second = w.organizations.len();
        w.organizations.push(other);
        // The slower man moves into a squad of his own.
        w.organizations[0].units = vec![0];
        w.organizations[second].units = vec![1];
        w.units[1].organization = second;
        w.organizations[0].leader = Some(0);
        w.organizations[second].leader = Some(0);
        for r in [0, second] {
            w.organizations[r].speed = 2.0;
        }
        w.units[0].speed_max = 2.0;
        w.units[1].speed_max = 2.0;
        w
    }

    fn moves(to: [[f32; 2]; 2]) -> Command {
        Command::Group {
            orders: vec![
                Command::Move {
                    organization: 0,
                    to: to[0],
                },
                Command::Move {
                    organization: 1,
                    to: to[1],
                },
            ],
        }
    }

    #[test]
    fn a_group_slows_its_nearer_regiments_to_arrive_with_the_furthest() {
        let mut w = grouped();
        let order = moves([[9.0, 50.0], [11.0, 30.0]]);
        w.apply(7, order).unwrap();
        assert_eq!(w.organizations[0].group, w.organizations[1].group);
        run(&mut w, 2.0);
        // Half as far to go, it goes at half the speed.
        let (near, far) = (
            w.organizations[1].group_speed,
            w.organizations[0].group_speed,
        );
        assert!((far.unwrap() - 2.0).abs() < 1e-3, "{far:?}");
        assert!((near.unwrap() - 1.0).abs() < 0.2, "{near:?}");
        run(&mut w, 25.0);
        let [a, b] = [w.units[0].position[1], w.units[1].position[1]];
        assert!(a > 40.0 && b > 25.0, "{a} {b}");
    }

    #[test]
    fn an_order_to_one_regiment_takes_it_out_of_its_group() {
        let mut w = grouped();
        let order = moves([[9.0, 50.0], [11.0, 30.0]]);
        w.apply(7, order).unwrap();
        w.apply(
            7,
            Command::Move {
                organization: 1,
                to: [11.0, 40.0],
            },
        )
        .unwrap();
        assert_eq!(w.organizations[1].group, None);
        // Left alone, the group goes.
        assert_eq!(w.organizations[0].group.is_some(), w.groups.len() == 1);
        w.ticks += SECOND;
    }

    #[test]
    fn a_regiment_handed_too_low_a_speed_waits() {
        let mut w = grouped();
        let order = moves([[9.0, 55.0], [11.0, 12.0]]);
        w.apply(7, order).unwrap();
        run(&mut w, 2.0);
        assert!(w.waiting_for_group(1));
        let at = w.units[1].position;
        run(&mut w, 1.0);
        assert_eq!(w.units[1].position, at);
    }
}
