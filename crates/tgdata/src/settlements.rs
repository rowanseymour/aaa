//! Settlements: the invisible things that group a centre building with the
//! rest of its town.

use std::collections::HashMap;

use crate::{GameData, Id, defs};

impl GameData {
    /// The settlement template that goes with each centre building, by the
    /// centre's template id.
    pub fn settlements(&self) -> HashMap<Id, Id> {
        let centres = self.by_template(|t| t.settlement_component.as_ref().map(centres));
        by_centre(centres)
    }
}

/// The centres a settlement has, one per level.
fn centres(settlement: &defs::SettlementComponent) -> Vec<Id> {
    (settlement.levels.iter())
        .flat_map(|l| &l.level)
        .filter_map(|l| l.settlement_center_ids.clone())
        .collect()
}

/// Each centre with its settlement, the first by id where several name it.
fn by_centre(centres: HashMap<Id, Vec<Id>>) -> HashMap<Id, Id> {
    let mut out: HashMap<Id, Id> = HashMap::new();
    for (settlement, centres) in centres {
        for centre in centres {
            let kept = out.entry(centre).or_insert_with(|| settlement.clone());
            if settlement < *kept {
                *kept = settlement.clone();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_level_names_a_centre_of_its_settlement() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\tids = human_settlement\n\t[SettlementComponent]\n\t{\n\
             \t\t[Levels]\n\t\t{\n\
             \t\t\t[Level]\n\t\t\tsettlement_center_ids = human_center_village\n\
             \t\t\t[Level]\n\t\t\tmax_structures = 3\n\t\t\tsettlement_center_ids = human_center_town\n\
             \t\t}\n\t}\n}\n",
        );
        let files = [("settlements/human_settlement.tgi".to_string(), parsed)];
        let files = crate::scoped(&files);
        let thing = files[0].read::<defs::Thing>("Thing").next().unwrap();
        let found = centres(thing.settlement_component.as_ref().unwrap());
        assert_eq!(
            found,
            [
                Id::from("human_center_village"),
                Id::from("human_center_town")
            ]
        );
    }

    #[test]
    fn a_centre_two_settlements_name_goes_with_the_first_by_id() {
        let ids = |ids: &[&str]| ids.iter().map(|&id| Id::from(id)).collect::<Vec<_>>();
        let found = by_centre(HashMap::from([
            (Id::from("b_settlement"), ids(&["shared", "b_centre"])),
            (Id::from("a_settlement"), ids(&["shared"])),
        ]));
        assert_eq!(found[&Id::from("shared")], Id::from("a_settlement"));
        assert_eq!(found[&Id::from("b_centre")], Id::from("b_settlement"));
    }
}
