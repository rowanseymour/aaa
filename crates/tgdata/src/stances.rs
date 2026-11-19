//! The stances things take: each `[Stance]` a template lists, which
//! applies at or below a share of health and in every state it names, and
//! gives a property while it's held.

use std::collections::HashMap;

use crate::{GameData, Id, defs};

/// One of a template's stances.
#[derive(Debug, Clone, PartialEq)]
pub struct Stance {
    pub ids: Id,
    /// The most health, as a share of its maximum, a thing in it has.
    pub health: f32,
    /// The states a thing has to be in, all of them.
    pub states: Vec<Id>,
    /// What a thing holds while in it.
    pub property: Option<Id>,
}

/// A template's stances, in the order it lists them; none for one with no
/// body.
fn stances(t: &defs::Thing) -> Option<Vec<Stance>> {
    t.body_component.as_ref()?;
    fn named(v: &Option<String>) -> Option<&str> {
        (v.as_deref().map(str::trim)).filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("NULL"))
    }
    let stances: Vec<Stance> = (t.stance.iter())
        .map(|s| Stance {
            ids: Id::from(s.ids.as_deref().unwrap_or("")),
            health: s.health.unwrap_or(1.0),
            states: (named(&s.actor_state).into_iter())
                .flat_map(|v| v.split(','))
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(Id::from)
                .collect(),
            property: named(&s.property_ids).map(Id::from),
        })
        .collect();
    (!stances.is_empty()).then_some(stances)
}

impl GameData {
    /// Each template's stances, by template id, for those that list any.
    pub fn stances(&self) -> HashMap<Id, Vec<Stance>> {
        self.by_template(stances)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stances_need_every_state_they_name() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\t[BodyComponent]\n\t{\n\t}\n\
             \t[Stance]\n\t{\n\t\tids = kneel\n\t\tproperty_ids = infantry_kneel\n\
             \t\thealth = 1.0\n\t\tactor_state = mind_engaged, under_siege\n\t}\n\
             \t[Stance]\n\t{\n\t\tids = prone\n\t\tproperty_ids = NULL\n\t\thealth = 0.5\n\
             \t\tactor_state = NULL\n\t}\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        let found = stances(&thing).unwrap();
        assert_eq!(
            found[0].states,
            [Id::from("mind_engaged"), Id::from("under_siege")]
        );
        assert_eq!(found[0].property, Some(Id::from("infantry_kneel")));
        assert_eq!((found[1].health, found[1].property.clone()), (0.5, None));
        assert!(found[1].states.is_empty());
    }
}
