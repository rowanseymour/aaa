//! What a side has unlocked: the technologies it holds and researches, and
//! what they and its buildings let a recruiter offer.

use aaa_map::{Grant, Id, Research};

use crate::{Held, Notice, Refused, World};

/// What a recruiter would need to raise a regiment but hasn't, and so
/// whether its menu offers it, greys it or leaves it out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lacking {
    /// The buildings its owner hasn't standing, by their current ids.
    pub structures: Vec<Id>,
    /// The properties neither it nor, for a technology, its side has.
    pub properties: Vec<Id>,
    /// Whether it's left out of the menu, rather than greyed.
    pub hidden: bool,
    /// Whether the map bars its owner from it, which leaves it out too.
    pub barred: bool,
}

impl Lacking {
    /// Whether nothing's missing, so the recruiter offers it.
    pub fn is_empty(&self) -> bool {
        self.structures.is_empty() && self.properties.is_empty() && !self.barred
    }
}

/// How a building's research menu shows one of the technologies it lists.
#[derive(Debug, Clone, PartialEq)]
pub enum Researchable {
    Ready,
    /// Greyed, for want of these buildings, by their current ids.
    Lacking(Vec<Id>),
    /// Already researched, or being researched: its slot is left empty.
    Taken,
    /// Left out, the items after it taking its place.
    Hidden,
}

/// For good, as a researched property never runs out.
const KEPT: u64 = u64::MAX;

impl World {
    /// Whether the map bars `player` from a technology or template.
    pub fn barred(&self, player: u32, id: &Id) -> bool {
        (self.sides.player(player)).is_some_and(|p| {
            p.barred
                .iter()
                .any(|b| b.as_str().eq_ignore_ascii_case(id.as_str()))
        })
    }

    /// Whether `player`'s side holds `technology`.
    pub fn holds(&self, player: u32, technology: &Id) -> bool {
        self.technologies
            .get(&player)
            .is_some_and(|t| t.contains(technology))
    }

    /// Whether the building at `building` has `property` of its own: its
    /// template's, or one it's been given.
    fn carries(&self, building: usize, property: &Id) -> bool {
        self.buildings.get(building).is_some_and(|b| {
            let kind = self.kinds.get(&Id::from(b.template.as_str()));
            kind.is_some_and(|k| k.properties.contains(property))
                || b.properties.iter().any(|h| h.property == *property)
        })
    }

    /// Whether `player` has a building of one of `ids` standing: done
    /// unpacking, and its own rather than an ally's.
    fn stands(&self, player: u32, ids: &[Id]) -> bool {
        self.buildings.iter().any(|b| {
            b.owner == player
                && b.standing()
                && !b.busy()
                && ids
                    .iter()
                    .any(|id| id.as_str().eq_ignore_ascii_case(&b.template))
        })
    }

    /// Of `structures`, each building by every id it goes by, the first id
    /// of each `player` lacks: none where `any` and one stands.
    fn missing(&self, player: u32, structures: &[Vec<Id>], any: bool) -> Vec<Id> {
        let missing: Vec<Id> = (structures.iter())
            .filter(|ids| !self.stands(player, ids))
            .map(|ids| ids[0].clone())
            .collect();
        match any && missing.len() < structures.len() {
            true => Vec::new(),
            false => missing,
        }
    }

    /// What the building at `building` would need to raise `regiment` for
    /// `player`, checked member by member: a member missing a property it
    /// hides for leaves the regiment out, and where members differ the
    /// later one decides.
    pub fn lacks(&self, player: u32, building: usize, regiment: &aaa_map::Regiment) -> Lacking {
        let mut out = Lacking::default();
        let templates = std::iter::once(regiment.template.as_str())
            .chain(regiment.members.iter().map(|m| m.thing.template.as_str()));
        out.barred = templates
            .into_iter()
            .any(|t| self.barred(player, &Id::from(t)));
        out.hidden = out.barred;
        for m in &regiment.members {
            let Some(r) = self.requires.get(&Id::from(m.thing.template.as_str())) else {
                continue;
            };
            let structures = self.missing(player, &r.structures, r.any);
            let properties: Vec<&Id> = (r.properties.iter())
                .filter(|p| !self.carries(building, p) && !self.holds(player, p))
                .collect();
            if structures.is_empty() && properties.is_empty() {
                continue;
            }
            out.hidden = out.barred || (r.hide && !properties.is_empty());
            for id in structures {
                if !out.structures.contains(&id) {
                    out.structures.push(id);
                }
            }
            for id in properties {
                if !out.properties.contains(id) {
                    out.properties.push(id.clone());
                }
            }
        }
        out
    }

    /// What the building at `building` lists for research, in its menu's
    /// order.
    pub fn research_list(&self, building: usize) -> &[Research] {
        (self.buildings.get(building))
            .and_then(|b| self.research_lists.get(&Id::from(b.template.as_str())))
            .map_or(&[], Vec::as_slice)
    }

    /// How the building at `building` offers `item` to `player`.
    pub fn researchable(&self, player: u32, building: usize, item: &Research) -> Researchable {
        let Some(b) = self.buildings.get(building) else {
            return Researchable::Hidden;
        };
        let tech = &item.technology;
        let queued = |b: &crate::Building| {
            (b.queue.iter())
                .any(|c| matches!(&c.work, crate::Work::Research(r) if r.technology == *tech))
        };
        let taken = match item.grant {
            Grant::Side => {
                self.holds(player, tech)
                    || (self.buildings.iter()).any(|o| o.owner == player && queued(o))
            }
            Grant::Building => self.carries(building, tech) || queued(b),
            // Nothing keeps it, so it's never done.
            Grant::Settlement => queued(b),
        };
        if taken {
            return Researchable::Taken;
        }
        if self.barred(player, tech) {
            return Researchable::Hidden;
        }
        let has = |p: &Id| self.carries(building, p) || self.holds(player, p);
        let properties = match item.properties_any && !item.properties.is_empty() {
            true => item.properties.iter().any(has),
            false => item.properties.iter().all(has),
        };
        if !properties {
            return Researchable::Hidden;
        }
        match self.missing(player, &item.structures, item.structures_any) {
            missing if missing.is_empty() => Researchable::Ready,
            missing => Researchable::Lacking(missing),
        }
    }

    /// The item for `technology` the building at `building` would research
    /// for `player` now, or why it won't.
    pub(crate) fn may_research(
        &self,
        player: u32,
        building: usize,
        technology: &Id,
    ) -> Result<&Research, Refused> {
        let b = self.standing_for(player, building)?;
        let item = (self.research_list(building).iter()).find(|r| r.technology == *technology);
        let Some(item) = item.filter(|_| !b.busy()) else {
            return Err(Refused::Unable);
        };
        match self.researchable(player, building, item) {
            Researchable::Ready => {}
            Researchable::Taken => return Err(Refused::Unable),
            Researchable::Lacking(_) | Researchable::Hidden => return Err(Refused::Lacking),
        }
        if self.research_cost(item) > self.stockpile(player) {
            return Err(Refused::Unaffordable);
        }
        Ok(item)
    }

    /// Gives what the building at `building` has researched to whom its
    /// item says.
    pub(crate) fn researched(&mut self, building: usize, item: &Research) {
        let player = self.buildings[building].owner;
        let technology = item.technology.clone();
        let held = Held {
            property: technology.clone(),
            until: KEPT,
        };
        self.notices.push(Notice::Researched {
            player,
            building,
            technology: technology.clone(),
        });
        match item.grant {
            Grant::Side if !self.holds(player, &technology) => {
                self.grant(player, technology.clone());
                self.notices.push(Notice::Gained { player, technology });
            }
            Grant::Side => {}
            Grant::Building => self.buildings[building].properties.push(held),
            // No building belongs to a settlement, so there's none to have
            // it.
            Grant::Settlement => {}
        }
    }
}
