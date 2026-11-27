//! Orders queued behind what a regiment is doing.

use crate::commands::steered;
use crate::morale::Mind;
use crate::{Command, Order, Piece, Refused, Shelled, World};

/// Where an order is aimed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Aimed {
    Place([f32; 2]),
    Thing(Piece),
}

/// An order as the map marks it: the command it is in the data, such as
/// `move` or `kill`, and where it's aimed.
#[derive(Debug, Clone, PartialEq)]
pub struct Marked {
    pub command: &'static str,
    pub at: Aimed,
}

/// How `command` is marked, for one aimed anywhere.
fn marked(command: &Command) -> Option<Marked> {
    let (command, at) = match *command {
        Command::Move { to, .. } => ("move", Aimed::Place(to)),
        Command::Attack { to, .. } => ("kill", Aimed::Place(to)),
        Command::Run { to, .. } => ("retreat", Aimed::Place(to)),
        Command::Pursue { enemy, .. } => ("kill", Aimed::Thing(Piece::Organization(enemy))),
        Command::Guard { to, .. } => ("guard", Aimed::Place(to)),
        Command::GuardPiece { piece, .. } => ("guard", Aimed::Thing(piece)),
        Command::MoveOnto { piece, .. } => ("move", Aimed::Thing(piece)),
        Command::Bombard { target, .. } => ("bombard", shelled(target)),
        Command::Explore { to, .. } => ("explore", Aimed::Place(to)),
        Command::Unpack { at, .. } => ("unpack", Aimed::Place(at)),
        Command::Build { at, .. } => ("construct", Aimed::Place(at)),
        Command::Repair { piece, .. } => ("repair", Aimed::Thing(piece)),
        _ => return None,
    };
    Some(Marked { command, at })
}

fn shelled(target: Shelled) -> Aimed {
    match target {
        Shelled::Thing(piece) => Aimed::Thing(piece),
        Shelled::Place(at) => Aimed::Place(at),
    }
}

/// The regiment `command` would go on the queue of: one with a target.
/// Stop and Disband have none, and nothing else is a regiment's.
fn queued_for(command: &Command) -> Option<usize> {
    match command {
        Command::Stop { .. } | Command::Disband { .. } => None,
        _ => steered(command),
    }
}

impl World {
    /// The orders the regiment at `r` has, as the map marks them: the one
    /// it's carrying out, if it's aimed anywhere, then those waiting in its
    /// queue, then, for its group's lead, those waiting in its group's.
    pub fn marked_orders(&self, r: usize) -> Vec<Marked> {
        let organization = &self.organizations[r];
        let mut out: Vec<Marked> = self.current_marked(r).into_iter().collect();
        out.extend(organization.queue.iter().filter_map(marked));
        let group = (self.groups.iter()).find(|g| Some(g.id) == organization.group);
        if let Some(g) = group.filter(|g| g.organizations.first() == Some(&r)) {
            for orders in &g.queue {
                let own = orders.iter().find(|o| steered(o) == Some(r));
                out.extend(own.or(orders.first()).and_then(marked));
            }
        }
        out
    }

    /// How the order the regiment at `r` is carrying out is marked, if
    /// it's aimed anywhere.
    fn current_marked(&self, r: usize) -> Option<Marked> {
        let organization = &self.organizations[r];
        let (command, at) = if let Some(piece) = organization.mending {
            ("repair", Aimed::Thing(piece))
        } else if let Some(build) = &organization.build {
            ("construct", Aimed::Place(build.at))
        } else if let Some((at, _)) = organization.unpack_at {
            ("unpack", Aimed::Place(at))
        } else if let Some(b) = &organization.bombard {
            ("bombard", shelled(b.target))
        } else if let Some(e) = &organization.explore {
            ("explore", Aimed::Place(e.point))
        } else if let Some(g) = &organization.guard {
            ("guard", g.of.map_or(Aimed::Place(g.post), Aimed::Thing))
        } else if let Some(piece) = organization.follow {
            ("move", Aimed::Thing(piece))
        } else if let Order::Pursue(e) = organization.order {
            ("kill", Aimed::Thing(Piece::Organization(e)))
        } else if self.underway(r) {
            let command = match organization.order {
                Order::Run => "retreat",
                Order::Attack => "kill",
                _ => "move",
            };
            (command, Aimed::Place(self.destination(r)))
        } else {
            return None;
        };
        Some(Marked { command, at })
    }

    /// Whether `player` may queue `command`, as [`Command::Queued`] has it.
    pub(crate) fn may_queue(&self, player: u32, command: &Command) -> Result<(), Refused> {
        match command {
            Command::Queued { .. } => Err(Refused::Unable),
            _ => self.check(player, command),
        }
    }

    /// Carries out [`Command::Queued`] for `player`.
    pub(crate) fn order_queued(&mut self, player: u32, command: Command) -> Result<(), Refused> {
        match command {
            Command::Queued { .. } => Err(Refused::Unable),
            Command::Group { orders, .. } if self.group_ordered(player, &orders).is_some() => {
                let id = self.group_ordered(player, &orders).expect("just found");
                let g = self.groups.iter().position(|g| g.id == id).expect("found");
                let idle = self.groups[g]
                    .organizations
                    .iter()
                    .all(|&r| self.takes_queued(r));
                let done = self.groups[g].orders.is_empty();
                match idle && done && self.groups[g].queue.is_empty() {
                    true => self.take_up_queued(id, orders),
                    false => {
                        self.groups[g].queue.push(orders);
                        Ok(())
                    }
                }
            }
            Command::Group { orders, .. } => {
                let done: Vec<_> = (orders.into_iter())
                    .map(|o| self.order_queued(player, o))
                    .collect();
                match done.iter().any(Result::is_ok) {
                    true => Ok(()),
                    false => done.into_iter().next().unwrap_or(Err(Refused::Unable)),
                }
            }
            command => match queued_for(&command) {
                Some(r) if !self.takes_queued(r) || !self.organizations[r].queue.is_empty() => {
                    self.may_command(player, r)?;
                    self.may_steer(r)?;
                    self.organizations[r].queue.push(command);
                    Ok(())
                }
                _ => self.give_order(player, command, false),
            },
        }
    }

    /// The group `orders` are to, if they're to every regiment of one of
    /// `player`'s groups and only those.
    fn group_ordered(&self, player: u32, orders: &[Command]) -> Option<u32> {
        let mut ordered: Vec<usize> = orders.iter().filter_map(queued_for).collect();
        ordered.sort_unstable();
        ordered.dedup();
        let id = self.organizations.get(*ordered.first()?)?.group?;
        let g = self
            .groups
            .iter()
            .find(|g| g.id == id && g.owner == player)?;
        let mut members = g.organizations.clone();
        members.sort_unstable();
        (members == ordered && orders.len() == ordered.len()).then_some(id)
    }

    /// Whether the regiment at `r` has run out of orders and would take
    /// the next on its queue: steady and doing nothing, and not exploring.
    fn takes_queued(&self, r: usize) -> bool {
        let organization = &self.organizations[r];
        organization.mind == Mind::Steady && organization.explore.is_none() && self.doing_nothing(r)
    }

    /// Has each regiment that's run out of orders take those it has
    /// queued, in the order given, carrying out the first it can and
    /// dropping any before it that it can't.
    pub(crate) fn take_queued(&mut self) {
        self.take_group_queued();
        for r in 0..self.organizations.len() {
            if self.organizations[r].queue.is_empty() || !self.takes_queued(r) {
                continue;
            }
            let mut queue = std::mem::take(&mut self.organizations[r].queue);
            let owner = self.organizations[r].owner;
            while !queue.is_empty() {
                let next = queue.remove(0);
                if self.give_order(owner, next, false).is_ok() {
                    break;
                }
            }
            self.organizations[r].queue = queue;
        }
    }
}

impl World {
    /// Has each group done with its order whose regiments have all run
    /// out of orders take up the next it has queued, carried out together
    /// and timed as one; what's left queued stays the group's.
    fn take_group_queued(&mut self) {
        for g in 0..self.groups.len() {
            let group = &self.groups[g];
            let ready = !group.queue.is_empty()
                && group.orders.is_empty()
                && group.organizations.iter().all(|&r| self.takes_queued(r));
            if !ready {
                continue;
            }
            let (id, members) = (group.id, group.organizations.clone());
            let mut queue = std::mem::take(&mut self.groups[g].queue);
            while !queue.is_empty() {
                let orders: Vec<Command> = (queue.remove(0).into_iter())
                    .filter(|o| queued_for(o).is_some_and(|r| members.contains(&r)))
                    .collect();
                if self.take_up_queued(id, orders).is_ok() {
                    break;
                }
            }
            self.groups[g].queue = queue;
        }
    }
}

/// Only ever holds orders as given, whose debug form is exact, floats
/// included.
impl crate::hash::Digest for Command {
    fn digest(&self, h: &mut crate::hash::Hasher) {
        format!("{self:?}").digest(h);
    }
}

#[cfg(test)]
mod tests {
    use tgmap::{Domain, Pathing};

    use crate::testing::*;
    use crate::{Command, World, distance};

    fn to(at: [f32; 2]) -> Command {
        Command::Move {
            organization: 0,
            to: at,
        }
    }

    fn queued(command: Command) -> Command {
        Command::Queued {
            command: Box::new(command),
        }
    }

    fn reaches(w: &mut World, at: [f32; 2]) -> bool {
        (0..400).any(|_| {
            w.tick();
            distance(w.centre(0), at) < 1.0 && !w.underway(0)
        })
    }

    #[test]
    fn queued_orders_are_carried_out_in_turn_once_the_last_is_done() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        // To an idle regiment it starts at once.
        w.apply(7, queued(to([10.0, 30.0]))).unwrap();
        assert!(w.organizations[0].queue.is_empty() && w.underway(0));
        w.apply(7, queued(to([30.0, 30.0]))).unwrap();
        w.apply(7, queued(to([30.0, 10.0]))).unwrap();
        assert_eq!(w.organizations[0].queue.len(), 2);
        // Marked in the order they'll be carried out.
        let places = |w: &World| -> Vec<crate::Aimed> {
            w.marked_orders(0).into_iter().map(|m| m.at).collect()
        };
        let at = crate::Aimed::Place;
        assert_eq!(
            places(&w),
            [at([10.0, 30.0]), at([30.0, 30.0]), at([30.0, 10.0])]
        );
        assert!(w.marked_orders(0).iter().all(|m| m.command == "move"));
        assert!(reaches(&mut w, [10.0, 30.0]));
        assert!(reaches(&mut w, [30.0, 30.0]));
        assert!(reaches(&mut w, [30.0, 10.0]));
        assert!(w.organizations[0].queue.is_empty());
        assert!(w.marked_orders(0).is_empty());
    }

    #[test]
    fn an_order_not_queued_throws_the_queue_away() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        w.apply(7, to([10.0, 30.0])).unwrap();
        w.apply(7, queued(to([30.0, 30.0]))).unwrap();
        // Stop has no target, so it's carried out at once even queued.
        w.apply(7, queued(Command::Stop { organization: 0 }))
            .unwrap();
        assert!(w.organizations[0].queue.is_empty());
        w.apply(7, queued(to([30.0, 30.0]))).unwrap();
        w.apply(7, to([10.0, 20.0])).unwrap();
        assert!(w.organizations[0].queue.is_empty());
        // Nor does routing keep it.
        w.apply(7, queued(to([30.0, 30.0]))).unwrap();
        w.rout(0);
        assert!(w.organizations[0].queue.is_empty());
    }

    #[test]
    fn a_queued_order_it_cant_carry_out_is_dropped_for_the_next() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        w.apply(7, to([10.0, 30.0])).unwrap();
        // The squad doesn't explore.
        let explore = Command::Explore {
            organization: 0,
            to: [30.0, 30.0],
        };
        assert!(w.apply(7, explore.clone()).is_err());
        w.apply(7, queued(explore)).unwrap();
        w.apply(7, queued(to([30.0, 10.0]))).unwrap();
        assert!(reaches(&mut w, [10.0, 30.0]));
        assert!(reaches(&mut w, [30.0, 10.0]));
        // A group's orders to regiments that aren't one group are queued
        // each for its own regiment.
        let group = |orders| {
            queued(Command::Group {
                orders,
                selection: None,
            })
        };
        assert!(w.check(7, &queued(queued(to([1.0, 1.0])))).is_err());
        w.apply(7, to([10.0, 30.0])).unwrap();
        w.apply(7, group(vec![to([30.0, 30.0])])).unwrap();
        assert_eq!(w.organizations[0].queue, [to([30.0, 30.0])]);
    }

    #[test]
    fn a_groups_queued_orders_wait_for_all_of_it_and_go_together() {
        let mut w = battle(&[1, 1], vec![soldier(vec![]), soldier(vec![])], at_war());
        let move_to = |r: usize, at: [f32; 2]| Command::Move {
            organization: r,
            to: at,
        };
        let both = |at: [f32; 2]| vec![move_to(0, at), move_to(1, [at[0], at[1] + 10.0])];
        // The second has further to go.
        w.apply(
            1,
            Command::Group {
                orders: vec![move_to(0, [10.0, 15.0]), move_to(1, [30.0, 30.0])],
                selection: None,
            },
        )
        .unwrap();
        let id = w.organizations[0].group.unwrap();
        w.apply(
            1,
            queued(Command::Group {
                orders: both([20.0, 5.0]),
                selection: None,
            }),
        )
        .unwrap();
        let g = |w: &World| {
            w.groups
                .iter()
                .find(|g| Some(g.id) == w.organizations[0].group)
                .cloned()
        };
        assert_eq!(g(&w).unwrap().queue.len(), 1);
        assert!(w.organizations.iter().all(|o| o.queue.is_empty()));
        // The group's waiting order is marked once, with its lead.
        assert_eq!(w.marked_orders(0).len(), 2);
        assert_eq!(w.marked_orders(1).len(), 1);
        // Taken once both are at their places round where they were sent,
        // the nearer having waited.
        let arrived = |w: &World, r: usize, at: [f32; 2]| distance(w.centre(r), at) < 19.0;
        assert!((0..800).any(|_| {
            w.tick();
            g(&w).is_some_and(|g| g.queue.is_empty())
        }));
        assert!(arrived(&w, 0, [10.0, 15.0]) && arrived(&w, 1, [30.0, 30.0]));
        // Queued, it went to the group it already had.
        assert_eq!(w.organizations[0].group, Some(id));
        assert!((0..800).any(|_| {
            w.tick();
            arrived(&w, 0, [20.0, 5.0])
                && arrived(&w, 1, [20.0, 15.0])
                && !w.underway(0)
                && !w.underway(1)
        }));
        // Carried out as a group, queue and all.
        let after = g(&w).unwrap();
        assert!(after.queue.is_empty() && after.organizations.len() == 2);
        // An order to one alone takes it out, leaving the group's queue
        // to the rest.
        w.apply(
            1,
            Command::Group {
                orders: both([30.0, 10.0]),
                selection: None,
            },
        )
        .unwrap();
        w.apply(
            1,
            queued(Command::Group {
                orders: both([10.0, 10.0]),
                selection: None,
            }),
        )
        .unwrap();
        w.apply(1, move_to(0, [5.0, 5.0])).unwrap();
        assert_eq!(w.organizations[0].group, None);
        assert_eq!(g(&w), None);
        let rest = w.groups.iter().find(|g| g.organizations == [1]).unwrap();
        assert_eq!(rest.queue.len(), 1);
    }
}
