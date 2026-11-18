//! What plays a side in place of a person.

use crate::{Command, World};

/// Gives the orders of one side nobody else plays. Only whatever hosts
/// the match runs one, and what it orders reaches every copy of the
/// match as a person's orders do, so it needn't decide the same way
/// twice. It thinks in the host's turn, holding up everyone, so it has to
/// be quick: work that takes longer is spread over turns, or done apart and
/// taken up once it's done.
pub trait ComputerPlayer: Send + Sync {
    /// The orders it gives side `player` as the match stands in `world`,
    /// before each tick it's played.
    fn think(&mut self, world: &World, player: u32) -> Vec<Command>;
}

/// A computer player that gives no orders, leaving its side's things to
/// do what they do unordered.
#[derive(Debug, Default, Clone, Copy)]
pub struct Idle;

impl ComputerPlayer for Idle {
    fn think(&mut self, _: &World, _: u32) -> Vec<Command> {
        Vec::new()
    }
}
