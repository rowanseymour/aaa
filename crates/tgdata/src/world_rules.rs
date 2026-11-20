//! The world rules a lobby offers as choices, such as the fog of war and
//! the time of day, and the sun each time of day names.

use crate::strings::localized;
use crate::{GameData, Id, defs};

/// A world rule set to one of a list of choices, as a lobby offers it.
#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceRule {
    /// Its id in the data, e.g. `FogOfWar`.
    pub id: Id,
    /// What the lobby calls it.
    pub name: String,
    /// The choice it's set to unless changed.
    pub default: Option<Id>,
    pub choices: Vec<RuleChoice>,
}

/// One of a [`ChoiceRule`]'s choices.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleChoice {
    pub id: Id,
    pub name: String,
}

impl ChoiceRule {
    /// `chosen`, unless it's what the rule is set to anyway.
    pub fn unless_default(&self, chosen: Option<Id>) -> Option<Id> {
        chosen.filter(|c| Some(c) != self.default.as_ref())
    }
}

impl GameData {
    /// The world rule `ids`, if the game has one that offers choices.
    pub fn choice_rule(&self, ids: &str) -> Option<ChoiceRule> {
        let rule = self.world_rule(ids)?;
        let strings = self.strings();
        let choices: Vec<RuleChoice> = (rule.enum_value.iter())
            .filter_map(|c| {
                let id = c.ids.clone()?;
                let name = localized(&strings, c.name.as_deref()).unwrap_or_else(|| id.to_string());
                Some(RuleChoice { id, name })
            })
            .collect();
        if choices.is_empty() {
            return None;
        }
        Some(ChoiceRule {
            id: Id::from(ids),
            name: localized(&strings, rule.name.as_deref()).unwrap_or_else(|| ids.into()),
            default: rule.default.as_deref().map(|d| Id::from(d.trim())),
            choices,
        })
    }

    /// The sun a match is lit by with `chosen` as its `Sun` world rule's
    /// choice, in place of the map's own; none where it keeps the map's,
    /// as the rule's default does.
    pub fn sun_for(&self, chosen: Option<&Id>) -> Option<defs::Sun> {
        let rule = self.choice_rule("Sun")?;
        let id = rule.unless_default(chosen.cloned())?;
        (self.sun_presets().into_iter()).find(|s| s.ids.as_ref() == Some(&id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_world_rules_choices_are_read() {
        let parsed = tgi::parse(
            "[WorldRule]\n{\n\tids = Sun\n\tname = \"#rule\"\n\tdefault = noon\n\
             \t[EnumValue]\n\tids = noon\n\tname = \"Noon\"\n\
             \t[EnumValue]\n\tids = late\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("WorldRule").next().unwrap();
        let rule: defs::WorldRule = templates.deserialize(block).unwrap();
        let choices: Vec<_> = (rule.enum_value.iter())
            .map(|c| (c.ids.clone().unwrap(), c.name.clone()))
            .collect();
        assert_eq!(
            choices,
            [
                (Id::from("noon"), Some("Noon".into())),
                (Id::from("late"), None)
            ]
        );
        assert_eq!(rule.default.as_deref(), Some("noon"));
    }
}
