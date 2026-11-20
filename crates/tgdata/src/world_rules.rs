//! The world rules a lobby offers, such as the fog of war, the time of day
//! and the money each side starts with, and the sun each time of day
//! names.

use std::collections::BTreeMap;

use log::warn;

use crate::strings::localized;
use crate::{GameData, Id, defs};

/// The world rules the engine itself applies to a match, by id.
pub const FOG_OF_WAR: &str = "FogOfWar";
pub const SUN: &str = "Sun";
pub const STOCKPILE: &str = "ResourceStockpileDefault";
pub const STARTING_EXPERIENCE: &str = "StartingExperience";
pub const CITY_VICTORY_RATIO: &str = "CityVictoryRatio";

/// What world rules are set to, as the data writes them, by the rule's
/// id.
pub type RuleValues = BTreeMap<Id, String>;

/// A world rule as a lobby offers it.
#[derive(Debug, Clone, PartialEq)]
pub struct LobbyRule {
    /// Its id in the data, e.g. `FogOfWar`.
    pub id: Id,
    /// What the lobby calls it.
    pub name: String,
    /// What it's set to unless changed.
    pub default: String,
    pub kind: RuleKind,
}

/// How a [`LobbyRule`] is set.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleKind {
    /// To one of these.
    Choice(Vec<RuleChoice>),
    /// To a whole number typed in, from 0 to `max`.
    Typed { max: u32 },
    /// To a whole number on a slider from `min` to `max`, in `step`s.
    Slid { min: u32, max: u32, step: u32 },
}

/// One of a [`LobbyRule`]'s choices.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleChoice {
    pub id: Id,
    pub name: String,
}

impl LobbyRule {
    /// What it's set to in `values`, at its default where it's not there.
    pub fn value<'a>(&'a self, values: &'a RuleValues) -> &'a str {
        values.get(&self.id).unwrap_or(&self.default)
    }

    /// What the lobby shows for it set as in `values`: a choice's name, or
    /// the number.
    pub fn shown(&self, values: &RuleValues) -> String {
        let value = self.value(values);
        match &self.kind {
            RuleKind::Choice(choices) => {
                let id = Id::from(value);
                let choice = choices.iter().find(|c| c.id == id);
                choice.map_or_else(|| value.to_string(), |c| c.name.clone())
            }
            _ => number(value).map_or_else(|| value.to_string(), |n| n.to_string()),
        }
    }
}

/// A whole number as a world rule's value writes it.
pub fn number(value: &str) -> Option<u32> {
    value
        .trim()
        .parse::<f32>()
        .ok()
        .map(|n| n.max(0.0).round() as u32)
}

impl GameData {
    /// The world rules a lobby lets players change, in the order the
    /// data has them.
    pub fn lobby_rules(&self) -> Vec<LobbyRule> {
        let strings = self.strings();
        let named = |ids: &Id, name: Option<&str>| {
            localized(&strings, name).unwrap_or_else(|| ids.to_string())
        };
        let choices = |found: &[defs::WorldRuleChoice]| -> Vec<RuleChoice> {
            (found.iter())
                .filter_map(|c| {
                    let id = c.ids.clone()?;
                    let name = named(&id, c.name.as_deref());
                    Some(RuleChoice { id, name })
                })
                .collect()
        };
        let mut out = Vec::new();
        for rule in self.world_rules() {
            let Some(id) = rule.ids.clone() else {
                continue;
            };
            if rule.never_editable == Some(true) {
                continue;
            }
            let kind = match rule.kind.as_deref().map(str::to_ascii_lowercase).as_deref() {
                Some("enum") => RuleKind::Choice(choices(&rule.enum_value)),
                Some("enum_specified") => RuleKind::Choice(choices(&self.rule_values(&id))),
                Some("fixed" | "int") if rule.use_slider == Some(true) => {
                    // A bound not given is the default, and a slider with
                    // nowhere to go isn't offered.
                    let default = rule.default.as_deref().and_then(number).unwrap_or(0);
                    let bound = |b: &Option<String>| b.as_deref().and_then(number);
                    let (min, max) = (
                        bound(&rule.slider_min).unwrap_or(default),
                        bound(&rule.slider_max).unwrap_or(default),
                    );
                    if min >= max {
                        continue;
                    }
                    let step = rule.step_size.map_or(1, |n| n.max(1.0) as u32);
                    RuleKind::Slid { min, max, step }
                }
                Some("fixed" | "int") => RuleKind::Typed {
                    // Below 0 for no cap.
                    max: (rule.editbox_max_value.filter(|n| *n >= 0.0))
                        .map_or(u32::MAX, |n| n as u32),
                },
                other => {
                    warn!("world rule {id}: can't offer one of type {other:?}");
                    continue;
                }
            };
            if matches!(&kind, RuleKind::Choice(c) if c.is_empty()) {
                continue;
            }
            out.push(LobbyRule {
                name: named(&id, rule.name.as_deref()),
                default: rule.default.unwrap_or_default().trim().to_string(),
                id,
                kind,
            });
        }
        out
    }

    /// The choices of a rule whose values are blocks named as its id, such
    /// as `DifficultyLevel`.
    fn rule_values(&self, ids: &Id) -> Vec<defs::WorldRuleChoice> {
        let files = self.parse_tgis(|n| n.starts_with("game/") || n.starts_with("templates/"));
        let files = crate::scoped(&files);
        files.iter().flat_map(|f| f.read(ids.as_str())).collect()
    }

    /// The sun a match is lit by with its `Sun` world rule set to `chosen`,
    /// in place of the map's own; none where it keeps the map's, as the
    /// rule's default does.
    pub fn sun_for(&self, chosen: Option<&str>) -> Option<defs::Sun> {
        let chosen = Id::from(chosen?);
        let rule = self.world_rule(SUN)?;
        if rule.default.is_some_and(|d| Id::from(d.trim()) == chosen) {
            return None;
        }
        (self.sun_presets().into_iter()).find(|s| s.ids.as_ref() == Some(&chosen))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_shows_its_value_else_its_default() {
        let money = LobbyRule {
            id: Id::from(STOCKPILE),
            name: "Money".into(),
            default: "500".into(),
            kind: RuleKind::Typed { max: 99999 },
        };
        let values: RuleValues = [(Id::from(STOCKPILE), "750.0".into())].into();
        assert_eq!(money.shown(&values), "750");
        assert_eq!(money.shown(&RuleValues::new()), "500");

        let fog = LobbyRule {
            id: Id::from(FOG_OF_WAR),
            name: "Fog of War".into(),
            default: "EXPLORED".into(),
            kind: RuleKind::Choice(vec![RuleChoice {
                id: Id::from("NONE"),
                name: "None".into(),
            }]),
        };
        let values: RuleValues = [(Id::from(FOG_OF_WAR), "none".into())].into();
        assert_eq!(fog.shown(&values), "None");
    }

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
