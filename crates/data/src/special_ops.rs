//! Special operations as a side's panel shows them, and the rules for the
//! experience that pays for them.

use std::collections::HashMap;

use crate::properties::{Pace, fill, line, number};
use crate::strings::localized;
use crate::supply::is;
use crate::{GameData, Id, defs, scoped};

/// A special operation as its button and tooltip show it.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecialOperation {
    pub name: String,
    pub description: Option<String>,
    /// Archive path of its icon, where the archives have it.
    pub icon: Option<String>,
    /// Seconds after use before it's ready again.
    pub recharge: f32,
    /// The game's word for the size of the area it lands on, if it has one.
    pub area: Option<String>,
    /// What its effects do, a line each, with the lines of what an
    /// enchantment gives indented under it.
    pub effects: Vec<EffectLine>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectLine {
    pub text: String,
    /// It's one of the lines under an enchantment's.
    pub indented: bool,
}

/// How a side's experience starts and grows, none at all where the data
/// doesn't say.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ExperienceRules {
    /// What every side with a general starts a match with.
    pub starting: f32,
    /// What every side with a general gains a second.
    pub per_second: f32,
}

/// What the tooltips need to put an operation's effects into words.
struct Words<'a> {
    strings: &'a crate::Strings,
    paces: &'a [Pace],
    properties: &'a HashMap<Id, defs::Property>,
    /// The names of templates and of thing categories, by id.
    things: &'a HashMap<Id, String>,
    categories: &'a HashMap<Id, String>,
}

impl Words<'_> {
    fn said(&self, key: &str) -> Option<&str> {
        self.strings.get(key).map(String::as_str)
    }

    /// The lines an effect gives, none for one hidden from tooltips or
    /// of a kind the game has no words for.
    fn effect(&self, e: &defs::Effect) -> Vec<EffectLine> {
        if e.hide_from_tooltip {
            return Vec::new();
        }
        let amount = number(e.amount.unwrap_or(0.0));
        let kind = |k: &str| is(&e.kind, k);
        let main = if kind("MAIN_DAMAGE") {
            self.said("ability_effect_desc_main_damage_glyph_long|cf")
                .map(|f| fill(f, &[&amount]))
        } else if kind("ADD_DAMAGE") {
            self.said("ability_effect_desc_add_damage_glyph_long|cf")
                .map(|f| fill(f, &[&amount]))
        } else if kind("MORALE_GAIN") {
            self.said("ability_effect_desc_morale_gain|f")
                .map(|f| fill(f, &[&amount]))
        } else if kind("MORALE_LOSS") {
            self.said("ability_effect_desc_morale_loss|f")
                .map(|f| fill(f, &[&amount]))
        } else if kind("HEAL") {
            let key = match is(&e.heal_type, "REPAIR") {
                true => "ability_effect_desc_repair|f",
                false => "ability_effect_desc_heal|f",
            };
            self.said(key).map(|f| fill(f, &[&amount]))
        } else if kind("SUMMON") {
            let thing = e.object_ids.as_ref().and_then(|t| self.things.get(t));
            (self.said("ability_effect_desc_summon|s").zip(thing)).map(|(f, t)| fill(f, &[t]))
        } else if kind("DESCRIBE_ECONOMY") {
            self.said("ability_effect_desc_describe_economy")
                .map(str::to_string)
        } else if kind("ENCHANTMENT") {
            let mut lines = self.enchantment(e);
            if let Some(first) = lines.first_mut() {
                first.text = self.with_versus(std::mem::take(&mut first.text), e);
            }
            return lines;
        } else {
            None
        };
        let Some(text) = main else {
            return Vec::new();
        };
        vec![EffectLine {
            text: self.with_versus(text, e),
            indented: false,
        }]
    }

    /// An effect's line, with "vs" and its targets after it where it has
    /// its own.
    fn with_versus(&self, text: String, e: &defs::Effect) -> String {
        match self.versus(e.targets.as_deref()) {
            Some(vs) => format!("{text} {vs}"),
            None => text,
        }
    }

    /// "Resupply for 30 Seconds" and the lines of what the property does.
    fn enchantment(&self, e: &defs::Effect) -> Vec<EffectLine> {
        let Some(p) = &e.enchantment_property else {
            return Vec::new();
        };
        let shared = p.ids.as_ref().and_then(|id| self.properties.get(id));
        let name = (p.name.as_deref())
            .or_else(|| shared?.name.as_deref())
            .and_then(|n| localized(self.strings, Some(n)))
            .unwrap_or_default();
        let seconds = p.duration.or_else(|| shared?.duration).unwrap_or(0.0);
        let Some(format) = self.said("ability_effect_desc_enchantment|sf") else {
            return Vec::new();
        };
        let modifiers = match p.modifier.is_empty() {
            true => shared.map_or(&p.modifier, |s| &s.modifier),
            false => &p.modifier,
        };
        let title = EffectLine {
            text: fill(format, &[&name, &number(seconds)]),
            indented: false,
        };
        let under = (modifiers.iter())
            .filter_map(|m| line(self.strings, self.paces, m))
            .map(|text| EffectLine {
                text,
                indented: true,
            });
        std::iter::once(title).chain(under).collect()
    }

    /// "vs Building" for an effect with targets of its own: the required
    /// ones, then "Not" and the excluded ones.
    fn versus(&self, targets: Option<&str>) -> Option<String> {
        let targets = targets.filter(|t| !t.trim().eq_ignore_ascii_case("null"))?;
        let word = |t: &str| {
            let name = self.categories.get(&Id::from(t)).cloned();
            name.unwrap_or_else(|| t.to_string())
        };
        let mut required = Vec::new();
        let mut excluded = Vec::new();
        for t in targets.split([',', ' ', '\t']).filter(|t| !t.is_empty()) {
            match t.strip_prefix('-') {
                Some(t) => excluded.push(word(t)),
                None => required.push(word(t.trim_start_matches('+'))),
            }
        }
        let mut words = Vec::new();
        let prefix = |key| self.said(key).filter(|p| !p.is_empty());
        words.extend(prefix("ability_effect_desc_target_prefix_required"));
        words.extend(required.iter().map(String::as_str));
        if !excluded.is_empty() {
            words.extend(prefix("ability_effect_desc_target_prefix_excluded"));
            words.extend(excluded.iter().map(String::as_str));
        }
        let format = self.said("ability_effect_desc_vs_targets|s")?;
        Some(fill(format, &[&words.join(" ")]))
    }
}

/// The game's word for an area of `radius`, from the descriptions of
/// `category`, the first range to hold it, its upper end left out.
fn area_word(
    strings: &crate::Strings,
    descriptions: &[defs::Description],
    category: &str,
    radius: f32,
) -> Option<String> {
    let d = (descriptions.iter()).find(|d| {
        let holds = d.range.is_some_and(|[lo, hi]| (lo..hi).contains(&radius));
        is(&d.category, category) && holds
    })?;
    localized(strings, d.description.as_deref())
}

impl GameData {
    /// Every special operation defined on its own, by id.
    pub fn special_operations(&self) -> HashMap<Id, SpecialOperation> {
        let strings = self.strings();
        let paces = self.paces(&strings);
        let definitions = self.definitions();
        let things: HashMap<Id, String> = (definitions.things.iter())
            .filter_map(|t| Some((t.id.clone(), localized(&strings, t.thing.name.as_deref())?)))
            .collect();
        let files = self.parse_tgis(|n| n.ends_with("thing_categories.tgi"));
        let categories: HashMap<Id, String> = (scoped(&files).iter())
            .flat_map(|f| {
                f.read::<defs::ThingCategory>("ThingCategory")
                    .collect::<Vec<_>>()
            })
            .filter_map(|c| Some((c.ids?, localized(&strings, c.name.as_deref())?)))
            .collect();
        let words = Words {
            strings: &strings,
            paces: &paces,
            properties: &definitions.properties,
            things: &things,
            categories: &categories,
        };
        let mut out: HashMap<Id, SpecialOperation> = (definitions.abilities.iter())
            .map(|(id, a)| (id.clone(), operation(&words, &definitions.descriptions, a)))
            .collect();
        for op in out.values_mut() {
            op.icon = op.icon.take().and_then(|path| self.wide_icon(&path));
        }
        out
    }

    /// How a side's experience starts and grows: the match's Starting
    /// Experience setting and the tuning's gain a second.
    pub fn experience_rules(&self) -> ExperienceRules {
        let files =
            self.parse_tgis(|n| n == "game/world_rules_aa.tgi" || n.starts_with("templates/"));
        let starting = (scoped(&files).iter())
            .flat_map(|f| f.read::<defs::WorldRule>("WorldRule").collect::<Vec<_>>())
            .find(|r| {
                (r.ids.as_ref())
                    .is_some_and(|id| id.as_str().eq_ignore_ascii_case("StartingExperience"))
            })
            .and_then(|r| r.default?.trim().parse().ok());
        let svars = self.svars();
        let per_second = svars
            .iter()
            .find_map(|s| s.experience_faction_gain_periodic);
        let default = ExperienceRules::default();
        ExperienceRules {
            starting: starting.unwrap_or(default.starting),
            per_second: per_second.unwrap_or(default.per_second),
        }
    }
}

fn operation(
    words: &Words,
    descriptions: &[defs::Description],
    a: &defs::AbilityDefinition,
) -> SpecialOperation {
    let id = a.ids.as_ref().map_or("", Id::as_str);
    let name = localized(words.strings, a.name.as_deref()).unwrap_or_else(|| id.to_string());
    let icon = (a.icon.as_deref())
        .and_then(|i| i.split_whitespace().next())
        .filter(|i| !i.eq_ignore_ascii_case("null"))
        .map(|i| i.trim_start_matches('/').to_string());
    let (category, radius) = match a.volley_area > a.area {
        true => ("AREA_VOLLEY", a.volley_area),
        false => ("AREA", a.area),
    };
    let area = (radius > 0.0)
        .then(|| area_word(words.strings, descriptions, category, radius))
        .flatten();
    SpecialOperation {
        name,
        description: localized(words.strings, a.description.as_deref()),
        icon,
        recharge: a.recharge_time,
        area,
        effects: a.effect.iter().flat_map(|e| words.effect(e)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings() -> crate::Strings {
        [
            ("ability_effect_desc_enchantment|sf", "%s for %g Seconds"),
            ("ability_effect_desc_summon|s", "Summons %s"),
            (
                "ability_effect_desc_add_damage_glyph_long|cf",
                "%lc+%g Extra Damage",
            ),
            ("ability_effect_desc_vs_targets|s", "vs %s"),
            ("ability_effect_desc_target_prefix_required", ""),
            ("ability_effect_desc_target_prefix_excluded", "Not"),
            ("modifier_desc_flag_supplied", "Supplied"),
            (
                "modifier_desc_multiply_resupply_rate|f",
                "%+g%% to Resupply Rate",
            ),
            ("descriptions_area_large", "Large"),
            ("resupply_name", "Resupply"),
            ("op_name", "Armored Resupply"),
            (
                "op_description",
                "Provides supply to target armor regiments.",
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    fn definition(text: &str) -> defs::AbilityDefinition {
        let parsed = tgi::parse(text);
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("AbilityDefinition").next().unwrap();
        templates.deserialize(block).unwrap()
    }

    fn shown(a: &defs::AbilityDefinition) -> SpecialOperation {
        let strings = strings();
        let categories = HashMap::from([(Id::from("building"), "Building".to_string())]);
        let things = HashMap::from([(Id::from("crate"), "Supply Crate".to_string())]);
        let words = Words {
            strings: &strings,
            paces: &[],
            properties: &HashMap::new(),
            things: &things,
            categories: &categories,
        };
        let descriptions = [defs::Description {
            category: Some("AREA".into()),
            description: Some("#descriptions_area_large".into()),
            range: Some([4.0, 1000.0]),
        }];
        operation(&words, &descriptions, a)
    }

    #[test]
    fn enchantments_say_what_they_give_and_for_how_long() {
        let a = definition(
            "[AbilityDefinition]\n{\n\tIDS = resupply_tank\n\tname = \"#op_name\"\n\
             \tdescription = \"#op_description\"\n\ticon = /UI/%s/Game/SpecialOps/IconResupply.png\n\
             \trecharge_time = 300\n\tarea = 15\n\
             \t[Effect]\n\t{\n\t\ttype = ENCHANTMENT\n\t\t[EnchantmentProperty]\n\t\t{\n\
             \t\t\tIDS = resupply\n\t\t\tname = \"#resupply_name\"\n\t\t\tduration = 30\n\
             \t\t\t[Modifier]\n\t\t\t{\n\t\t\t\ttype = FLAG\n\t\t\t\tmodifier_flag = SUPPLIED\n\t\t\t}\n\
             \t\t\t[Modifier]\n\t\t\t{\n\t\t\t\ttype = MULTIPLY\n\t\t\t\tattribute = RESUPPLY_RATE\n\
             \t\t\t\tamount = 0.25\n\t\t\t}\n\t\t}\n\t}\n}\n",
        );
        let op = shown(&a);
        assert_eq!(op.name, "Armored Resupply");
        assert_eq!(
            op.icon.as_deref(),
            // Found in the archives by `special_operations`.
            Some("UI/%s/Game/SpecialOps/IconResupply.png")
        );
        assert_eq!((op.recharge, op.area.as_deref()), (300.0, Some("Large")));
        let lines: Vec<(&str, bool)> = (op.effects.iter())
            .map(|l| (l.text.as_str(), l.indented))
            .collect();
        assert_eq!(
            lines,
            [
                ("Resupply for 30 Seconds", false),
                ("Supplied", true),
                ("+25% to Resupply Rate", true)
            ]
        );
    }

    #[test]
    fn hidden_effects_say_nothing_and_targets_follow_vs() {
        let a = definition(
            "[AbilityDefinition]\n{\n\tIDS = bomb\n\
             \t[Effect]\n\t{\n\t\ttype = SUMMON\n\t\tobject_ids = crate\n\t\thide_from_tooltip = true\n\t}\n\
             \t[Effect]\n\t{\n\t\ttype = SUMMON\n\t\tobject_ids = crate\n\t}\n\
             \t[Effect]\n\t{\n\t\ttype = ADD_DAMAGE\n\t\tamount = 900\n\t\ttargets = building, -friend\n\t}\n}\n",
        );
        let op = shown(&a);
        let lines: Vec<&str> = op.effects.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            lines,
            [
                "Summons Supply Crate",
                "+900 Extra Damage vs Building Not friend"
            ]
        );
        // No name of its own, and no area to name.
        assert_eq!((op.name.as_str(), op.area), ("bomb", None));
    }

    #[test]
    fn enchantments_with_targets_of_their_own_say_so() {
        let a = definition(
            "[AbilityDefinition]\n{\n\tIDS = sandbag\n\
             \t[Effect]\n\t{\n\t\ttype = ENCHANTMENT\n\t\ttargets = building\n\
             \t\t[EnchantmentProperty]\n\t\t{\n\t\t\tname = \"#resupply_name\"\n\
             \t\t\tduration = 30\n\t\t}\n\t}\n}\n",
        );
        let op = shown(&a);
        let lines: Vec<&str> = op.effects.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(lines, ["Resupply for 30 Seconds vs Building"]);
    }

    #[test]
    fn formats_fill_in_order() {
        assert_eq!(
            fill("Recharging (%d:%.2d left)", &["4", "05"]),
            "Recharging (4:05 left)"
        );
        assert_eq!(fill("%lc%g Damage", &["20"]), "20 Damage");
    }
}
