//! The properties things hold for a time.

use std::collections::HashMap;

use tgmap::Id;

use crate::targets::Thing;
use crate::{Piece, TICK, World};

/// For good, as a researched property never runs out.
pub(crate) const KEPT: u64 = u64::MAX;

/// A copy of a property a trigger took from a thing that it would have
/// had otherwise.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Lost {
    pub on: Thing,
    pub property: Id,
    /// The side whose research or trigger grant put it there; none for
    /// one its template lists.
    pub side: Option<u32>,
}

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
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn bearing<'a, E, H: IntoIterator<Item = &'a Held> + 'a>(
        &'a self,
        table: &'a HashMap<Id, E>,
        requires: fn(&E) -> &[Id],
        owner: u32,
        template: &Id,
        held: H,
        on: &[Thing],
    ) -> impl Iterator<Item = &'a E> + use<'a, E, H> {
        let ids = self.bearing_ids(table, requires, owner, template, held, on, false);
        ids.into_iter().filter_map(|p| table.get(p))
    }

    /// [`World::bearing`] after its template's own properties, each
    /// property once whichever of them it comes from.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn bearing_with_own<'a, E, H: IntoIterator<Item = &'a Held> + 'a>(
        &'a self,
        table: &'a HashMap<Id, E>,
        requires: fn(&E) -> &[Id],
        owner: u32,
        template: &Id,
        held: H,
        on: &[Thing],
    ) -> impl Iterator<Item = &'a E> + use<'a, E, H> {
        let ids = self.bearing_ids(table, requires, owner, template, held, on, true);
        ids.into_iter().filter_map(|p| table.get(p))
    }

    /// The properties of `table` that [`World::bearing`] gives, its
    /// template's own first where `own`, each once, less the copies taken
    /// from what it's for, `on`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn bearing_ids<'a, E>(
        &'a self,
        table: &'a HashMap<Id, E>,
        requires: fn(&E) -> &[Id],
        owner: u32,
        template: &Id,
        held: impl IntoIterator<Item = &'a Held> + 'a,
        things: &[Thing],
        own: bool,
    ) -> Vec<&'a Id> {
        let kind = self.rules.kinds.get(template);
        let has = |p: &Id| kind.is_some_and(|k| k.properties.contains(p));
        let on = |p: &Id| table.get(p).is_some_and(|e| requires(e).iter().all(has));
        let mut ids: Vec<&'a Id> = match (own, kind) {
            (true, Some(k)) => (k.properties.iter().enumerate())
                .filter(|&(i, p)| !k.properties[..i].contains(p) && self.keeps_own(things, p))
                .map(|(_, p)| p)
                .collect(),
            _ => Vec::new(),
        };
        let left = |p: &Id| self.keeps_side_copy(owner, things, p);
        for p in self.technologies_of(owner).filter(|p| on(p) && left(p)) {
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

    /// How many copies of `property` triggers have taken from `things`:
    /// those `side`'s grants put there, or for none those its template
    /// lists. A regiment's loss of one it keeps from its members is its
    /// own, so only the first of `things` counts for those.
    fn lost_copies(&self, things: &[Thing], property: &Id, side: Option<u32>) -> usize {
        let things = match self.rules.kept_from_members.contains(property) {
            true => &things[..things.len().min(1)],
            false => things,
        };
        (self.lost.iter())
            .filter(|l| things.contains(&l.on) && l.property == *property && l.side == side)
            .count()
    }

    /// Whether `things` keep the copy of `property` their template lists,
    /// if it does.
    pub(crate) fn keeps_own(&self, things: &[Thing], property: &Id) -> bool {
        self.lost_copies(things, property, None) == 0
    }

    /// Whether `things`, of `owner`'s, keep a copy of `technology` from
    /// their side's grants, if it holds any.
    pub(crate) fn keeps_side_copy(&self, owner: u32, things: &[Thing], technology: &Id) -> bool {
        let (given, kept) = self.side_copies(owner, technology);
        kept || given > self.lost_copies(things, technology, Some(owner))
    }

    /// The copies of `technology` `player`'s grants put on each thing it's
    /// on: how many a trigger can take, one for each grant from research
    /// or a trigger, and whether there's one it can't.
    fn side_copies(&self, player: u32, technology: &Id) -> (usize, bool) {
        let grants = self
            .technologies
            .get(&player)
            .map_or(&[][..], Vec::as_slice);
        let of = || grants.iter().filter(|g| g.technology == *technology);
        let given = of().filter(|g| g.source == crate::Source::Given).count();
        (given, of().any(|g| g.source != crate::Source::Given))
    }

    /// What a thing's properties are weighed for: a unit takes its
    /// organization's as well as its own, and a building is named twice.
    pub(crate) fn weighed(&self, t: crate::Target) -> [Thing; 2] {
        match t {
            crate::Target::Unit(u) => [
                Thing::Unit(u),
                Thing::Organization(self.units[u].organization),
            ],
            crate::Target::Building(b) => [Thing::Building(b); 2],
        }
    }

    /// The properties `t` holds for a time: for a unit, its
    /// organization's, which it shares, then its own, each once.
    pub(crate) fn timed_on(&self, t: crate::Target) -> impl Iterator<Item = &Held> + Clone {
        let (handed, own) = match t {
            crate::Target::Unit(u) => {
                let unit = &self.units[u];
                (Some(unit.organization), &unit.properties)
            }
            crate::Target::Building(b) => (None, &self.buildings[b].properties),
        };
        let handed = handed.into_iter().flat_map(|r| self.handed_down(r));
        let shared = handed.clone();
        let fresh = move |h: &&Held| !shared.clone().any(|o| o.property == h.property);
        handed.chain(own.iter().filter(fresh))
    }

    /// What the organization at `r` holds for a time that its members
    /// hold with it: all but those it keeps from them.
    pub(crate) fn handed_down(&self, r: usize) -> impl Iterator<Item = &Held> + Clone {
        let kept = &self.rules.kept_from_members;
        (self.organizations[r].properties.iter()).filter(move |h| !kept.contains(&h.property))
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
        let on = self.weighed(t);
        self.bearing(table, requires, owner, template, self.timed_on(t), &on)
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

    /// Whether `property` can be on the unit at `u`, as
    /// [`World::can_carry`] says.
    pub fn unit_may_hold(&self, u: usize, property: &Id) -> bool {
        self.can_carry(Thing::Unit(u), property)
    }

    /// Whether `property` can be on `on`, as [`World::can_carry`] says.
    pub fn may_hold(&self, on: Piece, property: &Id) -> bool {
        let t = match on {
            Piece::Organization(r) => Thing::Organization(r),
            Piece::Building(b) => Thing::Building(b),
        };
        self.can_carry(t, property)
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

    /// Takes away one of `t`'s copies of `property` that a trigger can
    /// take: one of its own it has for good, else the copy its side's
    /// research or trigger grant put on it, else the one its template
    /// lists. Returns whether there was one.
    pub(crate) fn lose_own(&mut self, t: Thing, property: &Id) -> bool {
        let Some(held) = self.held_mut(t) else {
            return false;
        };
        let copy = (held.iter()).rposition(|h| h.property == *property && h.until == KEPT);
        if copy.map(|i| held.remove(i)).is_some() {
            return true;
        }
        let Some((owner, template)) = self.owner_and_template(t) else {
            return false;
        };
        let things = [t];
        let lost = |side| self.lost_copies(&things, property, side);
        let kind = self.rules.kinds.get(template);
        let has = |p: &Id| kind.is_some_and(|k| k.properties.contains(p));
        let needs = self.rules.property_requires.get(property);
        let on_kind = kind.is_some() && needs.is_none_or(|n| n.iter().all(has));
        let (given, _) = self.side_copies(owner, property);
        let side = match on_kind && given > lost(Some(owner)) {
            true => Some(owner),
            false if has(property) && lost(None) == 0 => None,
            false => return false,
        };
        self.lost.push(Lost {
            on: t,
            property: property.clone(),
            side,
        });
        true
    }

    /// Who holds `t` and what it's made from; none for a prop.
    fn owner_and_template(&self, t: Thing) -> Option<(u32, &Id)> {
        match t {
            Thing::Organization(r) => {
                let o = &self.organizations[r];
                Some((o.owner, &o.template))
            }
            Thing::Unit(u) => {
                let unit = &self.units[u];
                Some((self.organizations[unit.organization].owner, &unit.template))
            }
            Thing::Building(b) => Some((self.buildings[b].owner, &self.buildings[b].template)),
            Thing::Prop(_) => None,
        }
    }
}

crate::hash::digest!(Lost { on, property, side });

crate::hash::digest!(Held {
    property,
    until,
    by
});
