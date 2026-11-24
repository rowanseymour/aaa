//! Orders queued behind what a regiment is doing.

use crate::commands::steered;
use crate::morale::Mind;
use crate::{Command, Refused, World};

/// The regiment `command` would go on the queue of: one with a target.
/// Stop and Disband have none, and nothing else is a regiment's.
fn queued_for(command: &Command) -> Option<usize> {
    match command {
        Command::Stop { .. } | Command::Disband { .. } => None,
        _ => steered(command),
    }
}

impl World {
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
            Command::Group { orders } => {
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
        assert!(reaches(&mut w, [10.0, 30.0]));
        assert!(reaches(&mut w, [30.0, 30.0]));
        assert!(reaches(&mut w, [30.0, 10.0]));
        assert!(w.organizations[0].queue.is_empty());
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
        // A group's orders are queued each for its own regiment.
        let group = |orders| queued(Command::Group { orders });
        assert!(w.check(7, &queued(queued(to([1.0, 1.0])))).is_err());
        w.apply(7, to([10.0, 30.0])).unwrap();
        w.apply(7, group(vec![to([30.0, 30.0])])).unwrap();
        assert_eq!(w.organizations[0].queue, [to([30.0, 30.0])]);
    }
}
