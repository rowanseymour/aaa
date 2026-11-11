//! What a side has unlocked: the technologies it holds, and what they and
//! its buildings let a recruiter offer.

use aaa_map::Id;

use crate::World;

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
}

impl Lacking {
    /// Whether nothing's missing, so the recruiter offers it.
    pub fn is_empty(&self) -> bool {
        self.structures.is_empty() && self.properties.is_empty()
    }
}

impl World {
    /// Whether `player`'s side holds `technology`.
    pub fn holds(&self, player: u32, technology: &Id) -> bool {
        self.technologies
            .get(&player)
            .is_some_and(|t| t.contains(technology))
    }

    /// What the building at `building` would need to raise `regiment` for
    /// `player`, checked member by member: a member missing a property it
    /// hides for leaves the regiment out, and where members differ the
    /// later one decides.
    pub fn lacks(&self, player: u32, building: usize, regiment: &aaa_map::Regiment) -> Lacking {
        let recruiter = self.buildings.get(building);
        let carries = |p: &Id| {
            recruiter.is_some_and(|b| {
                let kind = self.kinds.get(&Id::from(b.template.as_str()));
                kind.is_some_and(|k| k.properties.contains(p))
                    || b.properties.iter().any(|h| h.property == *p)
            })
        };
        // Only buildings done unpacking count, and only the player's own,
        // not its allies'.
        let stands = |ids: &[Id]| {
            self.buildings.iter().any(|b| {
                b.owner == player
                    && b.standing()
                    && !b.busy()
                    && ids
                        .iter()
                        .any(|id| id.as_str().eq_ignore_ascii_case(&b.template))
            })
        };
        let mut out = Lacking::default();
        for m in &regiment.members {
            let Some(r) = self.requires.get(&Id::from(m.thing.template.as_str())) else {
                continue;
            };
            let mut structures: Vec<&Id> = (r.structures.iter())
                .filter(|ids| !stands(ids))
                .map(|ids| &ids[0])
                .collect();
            if r.any && structures.len() < r.structures.len() {
                structures.clear();
            }
            let properties: Vec<&Id> = (r.properties.iter())
                .filter(|p| !carries(p) && !self.holds(player, p))
                .collect();
            if structures.is_empty() && properties.is_empty() {
                continue;
            }
            out.hidden = r.hide && !properties.is_empty();
            for id in structures {
                if !out.structures.contains(id) {
                    out.structures.push(id.clone());
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
}
