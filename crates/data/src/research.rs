//! The technologies buildings research, and what each needs.

use std::collections::HashMap;

use crate::{Amounts, GameData, Id, defs, supply::listed};

/// Who a researched technology goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant {
    /// The side, for good.
    Side,
    /// The building that researched it.
    Building,
    /// The settlement the building belongs to, a grouping no building
    /// has: a city doesn't count.
    Settlement,
}

/// A technology a building can research.
#[derive(Debug, Clone, PartialEq)]
pub struct Research {
    pub technology: Id,
    pub grant: Grant,
    pub seconds: f32,
    pub cost: Amounts,
    /// Each building it needs, by every id it may go by on a map: its
    /// current one first, then any it was renamed from.
    pub structures: Vec<Vec<Id>>,
    /// Whether one of `structures` is enough, rather than all.
    pub structures_any: bool,
    /// Properties the building or, for a technology, its side needs.
    pub properties: Vec<Id>,
    pub properties_any: bool,
}

impl GameData {
    /// What each template that researches lists, in its menu's order, by
    /// template id.
    pub fn research_lists(&self) -> HashMap<Id, Vec<Research>> {
        let aliases = self.structure_aliases();
        self.by_template(|t| {
            let items = &t.research_component.as_ref()?.research_item;
            let list: Vec<Research> = items.iter().filter_map(|i| research(i, &aliases)).collect();
            (!list.is_empty()).then_some(list)
        })
    }
}

/// An item as researched, with `aliases` giving every id a building may
/// go by; `None` for one naming no property.
pub(crate) fn research(
    item: &defs::ResearchItem,
    aliases: &impl Fn(&Id) -> Vec<Id>,
) -> Option<Research> {
    let technology = item.property_ids.clone()?;
    let grant = match item.kind.as_deref() {
        Some(k) if k.eq_ignore_ascii_case("LOCAL") => Grant::Building,
        Some(k) if k.eq_ignore_ascii_case("SUPPORT") => Grant::Settlement,
        _ => Grant::Side,
    };
    Some(Research {
        technology,
        grant,
        seconds: item.research_time.unwrap_or(0.0),
        cost: item
            .research_cost
            .as_ref()
            .map(Amounts::from)
            .unwrap_or_default(),
        structures: listed(item.required_structures.as_deref())
            .iter()
            .map(aliases)
            .collect(),
        structures_any: item.required_structures_any,
        properties: listed(item.required_properties.as_deref()),
        properties_any: item.required_properties_any,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_read_who_gets_them_what_they_cost_and_need() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\tIDS = motor_pool\n\t[ResearchComponent]\n\t{\n\
             \t\t[ResearchItem]\n\t\t{\n\t\t\tproperty_IDS = tech_heavy\n\
             \t\t\ttype = GLOBAL\n\t\t\tresearch_time = 60\n\
             \t\t\t[ResearchCost]\n\t\t\t{\n\t\t\t\tmoney = 150\n\t\t\t}\n\t\t}\n\
             \t\t[ResearchItem]\n\t\t{\n\t\t\tproperty_IDS = tech_tiger\n\
             \t\t\ttype = local\n\t\t\tresearch_time = 30\n\
             \t\t\trequired_properties = tech_heavy, NULL\n\
             \t\t\trequired_structures = depot,corps\n\
             \t\t\trequired_structures_any = true\n\t\t}\n\
             \t\t[ResearchItem]\n\t\t{\n\t\t\ttype = SUPPORT\n\t\t}\n\t}\n}\n",
        );
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = tgi::from_block(block).unwrap();
        let aliases = |id: &Id| match id.as_str() {
            "depot" => vec![id.clone(), Id::from("old_depot")],
            _ => vec![id.clone()],
        };
        let items = &thing.research_component.unwrap().research_item;
        let list: Vec<Research> = items.iter().filter_map(|i| research(i, &aliases)).collect();
        // One naming nothing to research is passed over.
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].technology, Id::from("tech_heavy"));
        assert_eq!(list[0].grant, Grant::Side);
        assert_eq!((list[0].seconds, list[0].cost.get("money")), (60.0, 150.0));
        assert!(list[0].properties.is_empty() && list[0].structures.is_empty());
        assert_eq!(list[1].grant, Grant::Building);
        assert_eq!(list[1].properties, [Id::from("tech_heavy")]);
        let ids = |of: &[&str]| -> Vec<Id> { of.iter().map(|&s| Id::from(s)).collect() };
        assert_eq!(
            list[1].structures,
            [ids(&["depot", "old_depot"]), ids(&["corps"])]
        );
        assert!(list[1].structures_any && !list[1].properties_any);
    }
}
