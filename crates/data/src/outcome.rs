//! What bears on how a match comes out.

use std::collections::{HashMap, HashSet};

use crate::{GameData, Id, defs};

impl GameData {
    /// The templates whose things keep their side in the match: those
    /// that recruit, and those that unpack into one that does or into a
    /// regiment.
    pub fn sustaining(&self) -> HashSet<Id> {
        let having = |has: fn(&defs::Thing) -> bool| -> HashSet<Id> {
            let found = self.by_template(|t| has(t).then_some(()));
            found.into_keys().collect()
        };
        let recruits = having(|t| t.recruit_component.is_some());
        let regiments = having(|t| t.organization_component.is_some());
        let unpacks = self.by_template(|t| {
            let deployment = t.mobile_deployment_component.as_ref()?;
            deployment.unpack_to_actor_ids.clone()
        });
        sustaining(recruits, &regiments, unpacks)
    }

    /// Whether the sides of a team win and lose together.
    pub fn team_victory(&self) -> bool {
        (self.svars().iter())
            .find_map(|v| v.team_victory)
            .unwrap_or(true)
    }
}

/// The templates that recruit, with those `unpacks` has unpacking into
/// one that does or into a regiment.
fn sustaining(
    recruits: HashSet<Id>,
    regiments: &HashSet<Id>,
    unpacks: HashMap<Id, Id>,
) -> HashSet<Id> {
    let packed: Vec<Id> = (unpacks.into_iter())
        .filter(|(_, into)| recruits.contains(into) || regiments.contains(into))
        .map(|(id, _)| id)
        .collect();
    recruits.into_iter().chain(packed).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_recruits_or_unpacks_into_it_or_a_regiment_keeps_a_side_in() {
        let ids = |ids: &[&str]| ids.iter().map(|&id| Id::from(id)).collect::<HashSet<Id>>();
        let unpacks = [
            ("hq_truck", "hq"),
            ("depot_truck", "depot"),
            ("crate", "squad"),
        ];
        let unpacks = unpacks.map(|(from, to)| (Id::from(from), Id::from(to)));
        let kept = sustaining(ids(&["hq"]), &ids(&["squad"]), unpacks.into());
        assert_eq!(kept, ids(&["hq", "hq_truck", "crate"]));
    }
}
