//! Kohan II's heroes: the units on the `Hero` template a side holds in its
//! pool and puts at the head of its companies.

use std::collections::HashMap;

use crate::{Definitions, GameData, Id};

/// A hero a side can hold.
#[derive(Debug, Clone, PartialEq)]
pub struct Hero {
    pub id: Id,
    /// Whether a side may be given it at random.
    pub random: bool,
    /// The faction whose it is, by the group of its templates; none for
    /// one that fits every side.
    pub group: Option<Id>,
}

impl GameData {
    /// Every hero, in the order they're defined.
    pub fn heroes(&self) -> Vec<Hero> {
        heroes(&self.definitions())
    }

    /// The group of the templates that are each faction's, by faction id.
    pub fn faction_groups(&self) -> HashMap<Id, Id> {
        faction_groups(&self.definitions())
    }
}

fn heroes(definitions: &Definitions) -> Vec<Hero> {
    (definitions.things.iter())
        .filter(|t| t.lineage.iter().any(|l| l == "hero"))
        .map(|t| Hero {
            id: t.id.clone(),
            random: (t.thing.element_component.as_ref())
                .and_then(|e| e.random_hero)
                .unwrap_or(true),
            group: (t.thing.placement.as_ref()).and_then(|p| p.placement_group_ids.clone()),
        })
        .collect()
}

fn faction_groups(definitions: &Definitions) -> HashMap<Id, Id> {
    (definitions.factions.iter())
        .filter_map(|f| {
            Some((
                f.ids.clone()?,
                crate::not_null(f.placement_group_ids.clone())?,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heroes_are_what_inherits_the_hero_template() {
        let parsed = tgi::parse(
            "[Template Unit]\n[Template Hero Inherit=Unit]\n\
             [Template K2Hero template=Hero]\n{\n\t[ElementComponent]\n}\n\
             [Template K2CeyahHero template=K2Hero]\n{\n\t[Placement]\n\t\
             placement_group_IDS = group_faction_ceyah\n}\n\
             [Unit Template=K2CeyahHero]\n{\n\tIDS = belphegor\n}\n\
             [Unit Template=K2Hero]\n{\n\tIDS = cutscene\n\t[ElementComponent]\n\t\
             random_hero = false\n}\n\
             [Unit Template=Unit]\n{\n\tIDS = pikeman\n}\n\
             [Faction]\n{\n\tids = ceyah\n\tplacement_group_IDS = group_faction_ceyah\n}\n",
        );
        let definitions = Definitions::new(&[("units/test.tgi".to_string(), parsed)]);
        let hero = |id: &str, random, group: Option<&str>| Hero {
            id: Id::from(id),
            random,
            group: group.map(Id::from),
        };
        assert_eq!(
            heroes(&definitions),
            [
                hero("belphegor", true, Some("group_faction_ceyah")),
                hero("cutscene", false, None)
            ]
        );
        let groups = faction_groups(&definitions);
        assert_eq!(
            groups.get(&Id::from("ceyah")),
            Some(&Id::from("group_faction_ceyah"))
        );
    }
}
