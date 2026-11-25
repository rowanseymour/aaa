//! What regiments entrench by: how long they rest or guard first, what
//! speeds that up, and what being entrenched gives them and their members.

use std::collections::HashMap;

use crate::supply::{is, listed};
use crate::{GameData, Id, defs};

/// The state of an entrenched regiment, whose properties it holds.
pub const ENTRENCHED: &str = "organization_entrenched";

/// What entrenching takes and gives.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entrenchment {
    /// The properties an entrenched regiment holds, each with the
    /// properties a thing needs to take it.
    pub properties: Vec<(Id, Vec<Id>)>,
    /// Seconds each organization template rests or guards before it
    /// entrenches, by template id, for those that set it.
    pub times: HashMap<Id, f32>,
    /// What each property adds to how fast it comes, by property id.
    pub rates: HashMap<Id, EntrenchmentRate>,
}

/// What a property adds to how fast a regiment entrenches.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntrenchmentRate {
    /// `ENTRENCHMENT_RATE`: added to the 1 its time is divided by.
    pub rate: f32,
    /// For a technology, the properties a thing of the side needs for it
    /// to be on it.
    pub requires: Vec<Id>,
}

fn rate(p: &defs::Property) -> Option<EntrenchmentRate> {
    let modifiers = p
        .modifier
        .iter()
        .filter(|m| is(&m.attribute, "ENTRENCHMENT_RATE"));
    let rate: f32 = modifiers.filter_map(|m| m.amount).sum();
    (rate != 0.0).then(|| EntrenchmentRate {
        rate,
        requires: listed(p.required_properties.as_deref()),
    })
}

impl GameData {
    pub fn entrenchment(&self) -> Entrenchment {
        let definitions = self.definitions();
        let needs = |id: &Id| {
            let p = definitions.properties.get(id);
            listed(p.and_then(|p| p.required_properties.as_deref()))
        };
        let properties = self
            .state_properties(ENTRENCHED)
            .into_iter()
            .map(|id| {
                let requires = needs(&id);
                (id, requires)
            })
            .collect();
        Entrenchment {
            properties,
            times: self.by_template(|t| t.organization_component.as_ref()?.entrenchment_time),
            rates: self.property_effects(rate),
        }
    }

    /// The properties the state `state` gives what's in it, as the first
    /// of the state files to define it lists them.
    pub fn state_properties(&self, state: &str) -> Vec<Id> {
        given(self.actor_states().iter().cloned(), state)
    }
}

/// The properties the first of `states` that's `state` gives.
fn given(states: impl IntoIterator<Item = defs::ActorState>, state: &str) -> Vec<Id> {
    let found = (states.into_iter()).find(|s| s.ids == Some(Id::from(state)));
    let properties = found.into_iter().flat_map(|s| s.property);
    properties.filter_map(|p| p.property_ids).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read<T: serde::de::DeserializeOwned>(text: &str, kind: &str) -> Vec<T> {
        let parsed = tgi::parse(text);
        let templates = tgi::Templates::new([&parsed.root]);
        let blocks = parsed.root.blocks_of(kind);
        blocks.map(|b| templates.deserialize(b).unwrap()).collect()
    }

    #[test]
    fn the_entrenched_state_gives_the_properties_it_lists() {
        let states = read(
            "[ActorState]\n{\n\tIDS = mind_resting\n\t[Property]\n\tproperty_ids = rest\n}\n\
             [ActorState]\n{\n\tIDS = organization_entrenched\n\tpriority = 6\n\
             \t[Property]\n\tproperty_ids = foot_dug_in\n\
             \t[Property]\n\tproperty_ids = wheels_dug_in\n}\n",
            "ActorState",
        );
        let found = given(states, ENTRENCHED);
        assert_eq!(found, [Id::from("foot_dug_in"), Id::from("wheels_dug_in")]);
    }

    #[test]
    fn entrenchment_rate_comes_from_modifiers_with_what_it_needs() {
        let properties: Vec<defs::Property> = read(
            "[Property]\n{\n\tIDS = spades\n\trequired_properties = org_unit\n\
             \t[Modifier]\n\t{\n\t\tattribute = ENTRENCHMENT_RATE\n\t\tamount = 0.25\n\t}\n}\n\
             [Property]\n{\n\tIDS = boots\n\t[Modifier]\n\t{\n\t\tattribute = SPEED_MAX\n\
             \t\tamount = 0.25\n\t}\n}\n",
            "Property",
        );
        let spades = EntrenchmentRate {
            rate: 0.25,
            requires: vec![Id::from("org_unit")],
        };
        assert_eq!(rate(&properties[0]), Some(spades));
        assert_eq!(rate(&properties[1]), None);
    }
}
