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
    /// The side that gave it, for one a side's special operation did.
    pub by: Option<u32>,
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
        held: impl IntoIterator<Item = &'a Held> + 'a,
    ) -> impl Iterator<Item = &'a E> {
        let kind = self.rules.kinds.get(template);
        let on = move |e: &&E| {
            let has = |p| kind.is_some_and(|k| k.properties.contains(p));
            requires(e).iter().all(has)
        };
        let technologies = self.technologies.get(&owner).into_iter().flatten();
        let timed = held.into_iter().map(|h| &h.property);
        let of = |p| table.get(p);
        (technologies.filter_map(of).filter(on)).chain(timed.filter_map(of))
    }

    /// The properties `t` holds for a time: for a unit, its
    /// organization's, which it shares, then its own, each once.
    pub(crate) fn timed_on(&self, t: crate::Target) -> impl Iterator<Item = &Held> + Clone {
        let (handed, own): (&[Held], &[Held]) = match t {
            crate::Target::Unit(u) => {
                let unit = &self.units[u];
                (
                    &self.organizations[unit.organization].properties,
                    &unit.properties,
                )
            }
            crate::Target::Building(b) => (&[], &self.buildings[b].properties),
        };
        let fresh = move |h: &&Held| !handed.iter().any(|o| o.property == h.property);
        handed.iter().chain(own.iter().filter(fresh))
    }

    /// What of `table` bears on `t` past its template's own properties:
    /// its side's technologies on its kind, then what it holds for a time.
    pub(crate) fn borne<'a, E>(
        &'a self,
        table: &'a HashMap<Id, E>,
        requires: fn(&E) -> &[Id],
        t: crate::Target,
    ) -> impl Iterator<Item = &'a E> {
        let (owner, template) = match t {
            crate::Target::Unit(u) => {
                let unit = &self.units[u];
                (self.organizations[unit.organization].owner, &unit.template)
            }
            crate::Target::Building(b) => (self.buildings[b].owner, &self.buildings[b].template),
        };
        self.bearing(table, requires, owner, template, self.timed_on(t))
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
            held.push(Held {
                property,
                until,
                by: None,
            });
        }
    }
}

crate::hash::digest!(Held {
    property,
    until,
    by
});
