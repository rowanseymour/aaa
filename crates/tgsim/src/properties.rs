//! The properties things hold for a time.

use std::collections::HashMap;

use tgmap::Id;

use crate::targets::Thing;
use crate::{Piece, TICK, World};

/// For good, as a researched property never runs out.
pub(crate) const KEPT: u64 = u64::MAX;

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
    /// Each property bears once, however many copies of it there are.
    pub(crate) fn bearing<'a, E>(
        &'a self,
        table: &'a HashMap<Id, E>,
        requires: fn(&E) -> &[Id],
        owner: u32,
        template: &Id,
        held: impl IntoIterator<Item = &'a Held> + 'a,
    ) -> impl Iterator<Item = &'a E> {
        let ids = self.bearing_ids(table, requires, owner, template, held, false);
        ids.into_iter().filter_map(|p| table.get(p))
    }

    /// [`World::bearing`] after its template's own properties, each
    /// property once whichever of them it comes from.
    pub(crate) fn bearing_with_own<'a, E>(
        &'a self,
        table: &'a HashMap<Id, E>,
        requires: fn(&E) -> &[Id],
        owner: u32,
        template: &Id,
        held: impl IntoIterator<Item = &'a Held> + 'a,
    ) -> impl Iterator<Item = &'a E> {
        let ids = self.bearing_ids(table, requires, owner, template, held, true);
        ids.into_iter().filter_map(|p| table.get(p))
    }

    /// The properties of `table` that [`World::bearing`] gives, its
    /// template's own first where `own`, each once.
    fn bearing_ids<'a, E>(
        &'a self,
        table: &'a HashMap<Id, E>,
        requires: fn(&E) -> &[Id],
        owner: u32,
        template: &Id,
        held: impl IntoIterator<Item = &'a Held> + 'a,
        own: bool,
    ) -> Vec<&'a Id> {
        let kind = self.rules.kinds.get(template);
        let has = |p: &Id| kind.is_some_and(|k| k.properties.contains(p));
        let on = |p: &Id| table.get(p).is_some_and(|e| requires(e).iter().all(has));
        let mut ids: Vec<&'a Id> = match (own, kind) {
            (true, Some(k)) => (k.properties.iter().enumerate())
                .filter(|&(i, p)| !k.properties[..i].contains(p))
                .map(|(_, p)| p)
                .collect(),
            _ => Vec::new(),
        };
        for p in self.technologies_of(owner).filter(|p| on(p)) {
            if !ids.contains(&p) {
                ids.push(p);
            }
        }
        for h in held {
            if !ids.contains(&&h.property) {
                ids.push(&h.property);
            }
        }
        ids
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

impl World {
    /// The properties `t` holds of its own, for a time or for good; props
    /// hold none.
    fn held_mut(&mut self, t: Thing) -> Option<&mut Vec<Held>> {
        match t {
            Thing::Organization(r) => Some(&mut self.organizations[r].properties),
            Thing::Unit(u) => Some(&mut self.units[u].properties),
            Thing::Building(b) => Some(&mut self.buildings[b].properties),
            Thing::Prop(_) => None,
        }
    }

    /// Whether `property` can be on `t`: its template has properties, and
    /// those `property` needs. A regiment has them when it or any of its
    /// living members does.
    fn can_carry(&self, t: Thing, property: &Id) -> bool {
        let templates: Vec<&Id> = match t {
            Thing::Organization(r) => {
                let o = &self.organizations[r];
                let members = (o.units.iter().map(|&u| &self.units[u]))
                    .filter(|u| u.alive())
                    .map(|u| &u.template);
                std::iter::once(&o.template).chain(members).collect()
            }
            Thing::Unit(u) => vec![&self.units[u].template],
            Thing::Building(b) => vec![&self.buildings[b].template],
            Thing::Prop(_) => return false,
        };
        let kinds = &self.rules.kinds;
        if !kinds.contains_key(templates[0]) {
            return false;
        }
        let needs = self.rules.property_requires.get(property);
        let needs = needs.map_or(&[][..], Vec::as_slice);
        (templates.iter().filter_map(|t| kinds.get(*t)))
            .any(|k| needs.iter().all(|p| k.properties.contains(p)))
    }

    /// Gives `t` a copy of `property` of its own for good, as if its
    /// template listed it, unless it can't be on `t`. It counts for `t`
    /// alone, whoever holds `t`, and isn't barred by the map.
    pub(crate) fn gain_own(&mut self, t: Thing, property: Id) {
        if !self.can_carry(t, &property) {
            return;
        }
        if let Some(held) = self.held_mut(t) {
            held.push(Held {
                property,
                until: KEPT,
                by: None,
            });
        }
    }

    /// Takes away one of `t`'s own copies of `property` it has for good.
    /// Returns whether there was one.
    pub(crate) fn lose_own(&mut self, t: Thing, property: &Id) -> bool {
        let Some(held) = self.held_mut(t) else {
            return false;
        };
        let copy = (held.iter()).rposition(|h| h.property == *property && h.until == KEPT);
        copy.map(|i| held.remove(i)).is_some()
    }
}

crate::hash::digest!(Held {
    property,
    until,
    by
});
