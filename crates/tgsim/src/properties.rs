//! The properties things hold for a time.

use std::collections::HashMap;

use tgmap::Id;

use crate::{Piece, TICK, World};

/// A property something has for a time.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub property: Id,
    /// The tick its time is up on. It's taken off at its holder's next
    /// look after.
    pub until: u64,
}

impl World {
    /// What of `table`, by property, bears on something of `owner`'s made
    /// from `template` holding `held`: its side's technologies whose
    /// `requires` its kind has, then the properties it holds for a time.
    pub(crate) fn bearing<'a, E>(
        &'a self,
        table: &'a HashMap<Id, E>,
        requires: fn(&E) -> &[Id],
        owner: u32,
        template: &Id,
        held: &'a [Held],
    ) -> impl Iterator<Item = &'a E> {
        let kind = self.rules.kinds.get(template);
        let on = move |e: &&E| {
            let has = |p| kind.is_some_and(|k| k.properties.contains(p));
            requires(e).iter().all(has)
        };
        let technologies = self.technologies.get(&owner).into_iter().flatten();
        let timed = held.iter().map(|h| &h.property);
        let of = |p| table.get(p);
        (technologies.filter_map(of).filter(on)).chain(timed.filter_map(of))
    }

    /// Gives an organization or building a property for `seconds` from now,
    /// unless it still has it.
    pub fn give(&mut self, to: Piece, property: Id, seconds: f32) {
        let until = self.ticks + (seconds / TICK).round() as u64;
        let held = match to {
            Piece::Organization(r) => &mut self.organizations[r].properties,
            Piece::Building(b) => &mut self.buildings[b].properties,
        };
        if !held.iter().any(|h| h.property == property) {
            held.push(Held { property, until });
        }
    }
}

crate::hash::digest!(Held { property, until });
