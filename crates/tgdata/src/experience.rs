//! What a regiment's experience is worth and the levels it earns, and what
//! the properties those levels give do in a fight.

use std::collections::HashMap;
use std::sync::Arc;

use crate::supply::{is, listed};
use crate::{GameData, Id, defs};

/// How much experience the things that earn it gain, by their `SVars`
/// names less the `Experience` they start with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExperienceGains {
    /// A kill's, capture's or rout's gain is kept within these.
    pub gain_minimum: f32,
    pub gain_maximum: f32,
    pub kill_multiplier: f32,
    pub capture_multiplier: f32,
    pub rout_multiplier: f32,
    /// A second's engagement.
    pub engage_multiplier: f32,
    /// What a victim with no combat value counts as, when its
    /// [`Levels::fake_value`] is set.
    pub default_value: f32,
    /// Of what a thing gains, the share its side's faction gains too.
    pub faction_gain_multiplier: f32,
}

impl Default for ExperienceGains {
    /// AA's.
    fn default() -> Self {
        ExperienceGains {
            gain_minimum: 0.0,
            gain_maximum: 10.0,
            kill_multiplier: 1.0,
            capture_multiplier: 1.0,
            rout_multiplier: 1.0,
            engage_multiplier: 1.0,
            default_value: 1.0,
            faction_gain_multiplier: 1.0,
        }
    }
}

/// The levels a template's things earn with experience.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Levels {
    /// In order; a thing starts at the first.
    pub levels: Vec<Level>,
    /// Set where it stands in for a combat value it doesn't have.
    pub fake_value: Option<f32>,
}

/// One of a template's experience levels.
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    /// The experience reaching it takes.
    pub required: f32,
    /// The property it gives, as [`level_property`] names it.
    pub property: Id,
}

/// The id this project gives the property a template's `level`th
/// experience level gives, counting from 0, which the data leaves
/// unnamed.
pub fn level_property(template: &Id, level: usize) -> Id {
    Id::from(format!("{template}_level_{level}"))
}

/// What a property does to its holder's fighting.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CombatEffect {
    /// `AV`: added to each attack's main damage.
    pub attack: f32,
    /// `AV_MELEE`: added to the main damage of each attack that fires no
    /// projectile.
    pub melee: f32,
    /// `AV_RANGED`: added to the main damage of each attack that fires one.
    pub ranged: f32,
    /// `DV`: added to defense.
    pub defense: f32,
    /// `HEALTH`: added to the 1 maximum health is multiplied by.
    pub health: f32,
    /// `RECHARGE_TIME`: added to the 1 the time between attacks is divided
    /// by.
    pub recharge: f32,
    /// `PROJECTILE_ABILITY_RANGE`: added to the range of each attack that
    /// fires a projectile, as its combat value counts it.
    pub projectile_range: f32,
    /// `BOMBARD_ABILITY_RANGE`: the same for each `BOMBARD` attack.
    pub bombard_range: f32,
    pub efficiency: Efficiency,
    /// For a technology, the properties a thing of the side needs for it
    /// to be on it.
    pub requires: Vec<Id>,
}

impl CombatEffect {
    /// The two together, as a thing holding both has them.
    pub fn with(self, other: &CombatEffect) -> CombatEffect {
        CombatEffect {
            attack: self.attack + other.attack,
            melee: self.melee + other.melee,
            ranged: self.ranged + other.ranged,
            defense: self.defense + other.defense,
            health: self.health + other.health,
            recharge: self.recharge + other.recharge,
            projectile_range: self.projectile_range + other.projectile_range,
            bombard_range: self.bombard_range + other.bombard_range,
            efficiency: self.efficiency.with(other.efficiency),
            requires: Vec::new(),
        }
    }
}

/// What a property sets its holder's attack efficiency to: each the
/// largest of its multipliers on that attribute, 1 plus its amount.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Efficiency {
    /// `ATTACK_EFFICIENCY_ACTUAL`, which wins over `desired`.
    pub actual: Option<f32>,
    /// `ATTACK_EFFICIENCY_DESIRED`.
    pub desired: Option<f32>,
}

impl Efficiency {
    /// The two together, as a thing holding both has them.
    pub fn with(self, other: Efficiency) -> Efficiency {
        let most = |a: Option<f32>, b: Option<f32>| match (a, b) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        Efficiency {
            actual: most(self.actual, other.actual),
            desired: most(self.desired, other.desired),
        }
    }

    /// The efficiency it sets, if it sets one.
    pub fn value(self) -> Option<f32> {
        self.actual.or(self.desired)
    }

    /// What `p`'s modifiers set.
    pub fn of(p: &defs::Property) -> Efficiency {
        combat_effect(p).map_or_else(Efficiency::default, |e| e.efficiency)
    }
}

fn combat_effect(p: &defs::Property) -> Option<CombatEffect> {
    let mut out = CombatEffect::default();
    for m in &p.modifier {
        let amount = m.amount.unwrap_or(0.0);
        let add = is(&m.kind, "ADD");
        let multiply = is(&m.kind, "MULTIPLY");
        let most = |v: Option<f32>| Some(v.map_or(1.0 + amount, |v| v.max(1.0 + amount)));
        match () {
            _ if add && is(&m.attribute, "AV") => out.attack += amount,
            _ if add && is(&m.attribute, "AV_MELEE") => out.melee += amount,
            _ if add && is(&m.attribute, "AV_RANGED") => out.ranged += amount,
            _ if add && is(&m.attribute, "DV") => out.defense += amount,
            _ if add && is(&m.attribute, "PROJECTILE_ABILITY_RANGE") => {
                out.projectile_range += amount;
            }
            _ if add && is(&m.attribute, "BOMBARD_ABILITY_RANGE") => out.bombard_range += amount,
            _ if multiply && is(&m.attribute, "HEALTH") => out.health += amount,
            _ if multiply && is(&m.attribute, "RECHARGE_TIME") => out.recharge += amount,
            _ if multiply && is(&m.attribute, "ATTACK_EFFICIENCY_ACTUAL") => {
                out.efficiency.actual = most(out.efficiency.actual);
            }
            _ if multiply && is(&m.attribute, "ATTACK_EFFICIENCY_DESIRED") => {
                out.efficiency.desired = most(out.efficiency.desired);
            }
            _ => {}
        }
    }
    (out != CombatEffect::default()).then(|| CombatEffect {
        requires: listed(p.required_properties.as_deref()),
        ..out
    })
}

impl GameData {
    pub fn experience_gains(&self) -> ExperienceGains {
        let svars = self.svars();
        let d = ExperienceGains::default();
        let of = |get: fn(&defs::SVars) -> Option<f32>, or: f32| {
            svars.iter().find_map(get).unwrap_or(or)
        };
        ExperienceGains {
            gain_minimum: of(|v| v.experience_gain_minimum, d.gain_minimum),
            gain_maximum: of(|v| v.experience_gain_maximum, d.gain_maximum),
            kill_multiplier: of(|v| v.experience_kill_multiplier, d.kill_multiplier),
            capture_multiplier: of(|v| v.experience_capture_multiplier, d.capture_multiplier),
            rout_multiplier: of(|v| v.experience_rout_multiplier, d.rout_multiplier),
            engage_multiplier: of(|v| v.experience_engage_multiplier, d.engage_multiplier),
            default_value: of(|v| v.experience_default_value, d.default_value),
            faction_gain_multiplier: of(
                |v| v.experience_faction_gain_multiplier,
                d.faction_gain_multiplier,
            ),
        }
    }

    /// The levels of each template with an experience component, by
    /// template id.
    pub fn experience_levels(&self) -> HashMap<Id, Levels> {
        let mut out = HashMap::new();
        for (template, component) in self.experience_components().iter() {
            let levels = (component.experience_level.iter().enumerate())
                .map(|(n, l)| Level {
                    required: l.experience_required.unwrap_or(0.0),
                    property: level_property(template, n),
                })
                .collect();
            let fake_value = component.fake_cv_value.filter(|&v| v >= 0.0);
            out.insert(template.clone(), Levels { levels, fake_value });
        }
        out
    }

    /// The names of the experience levels that have one, by the ids
    /// [`level_property`] gives their properties.
    pub fn level_names(&self) -> HashMap<Id, String> {
        let strings = self.strings();
        let mut out = HashMap::new();
        for (template, component) in self.experience_components().iter() {
            for (n, level) in component.experience_level.iter().enumerate() {
                let name = level.property.as_ref().and_then(|p| p.name.as_deref());
                let name = crate::strings::localized(&strings, name).filter(|n| !n.is_empty());
                if let Some(name) = name {
                    out.insert(level_property(template, n), name);
                }
            }
        }
        out
    }

    /// Each template's experience component, read once.
    fn experience_components(&self) -> Arc<HashMap<Id, defs::ExperienceComponent>> {
        self.kept("experience_components", || {
            self.by_template(|t| t.experience_component.clone())
        })
    }

    /// The properties experience levels give, by the ids
    /// [`level_property`] gives them, for those that have one.
    pub(crate) fn level_properties(&self) -> Vec<(Id, defs::Property)> {
        let mut out = Vec::new();
        for (template, component) in self.experience_components().iter() {
            for (n, level) in component.experience_level.iter().enumerate() {
                if let Some(p) = &level.property {
                    out.push((level_property(template, n), p.clone()));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// What each property does to its holder's fighting, for those that do
    /// anything to it, by its id: the shared ones, those abilities give, and
    /// those experience levels give.
    pub fn combat_effects(&self) -> HashMap<Id, CombatEffect> {
        let mut out = self.property_effects(combat_effect);
        for (id, p) in self.level_properties() {
            if let Some(effect) = combat_effect(&p) {
                out.insert(id, effect);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(text: &str) -> defs::ExperienceComponent {
        let parsed = tgi::parse(text);
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("ExperienceComponent").next().unwrap();
        templates.deserialize(block).unwrap()
    }

    #[test]
    fn levels_say_what_they_do_in_a_fight() {
        let c = component(
            "[ExperienceComponent]\n{\n\t[ExperienceLevel]\n\t{\n\t\texperience_required = 0\n\t}\n\
             \t[ExperienceLevel]\n\t{\n\t\texperience_required = 150\n\t\t[Property]\n\t\t{\n\
             \t\t\t[Modifier]\n\t\t\t{\n\t\t\t\ttype = ADD\n\t\t\t\tattribute = AV\n\t\t\t\tamount = 4\n\t\t\t}\n\
             \t\t\t[Modifier]\n\t\t\t{\n\t\t\t\ttype = ADD\n\t\t\t\tattribute = DV\n\t\t\t\tamount = 1\n\t\t\t}\n\
             \t\t\t[Modifier]\n\t\t\t{\n\t\t\t\ttype = MULTIPLY\n\t\t\t\tattribute = HEALTH\n\t\t\t\tamount = 0.05\n\t\t\t}\n\
             \t\t}\n\t}\n}\n",
        );
        let required: Vec<_> = (c.experience_level.iter())
            .map(|l| l.experience_required)
            .collect();
        assert_eq!(required, [Some(0.0), Some(150.0)]);
        assert!(c.experience_level[0].property.is_none());
        let regular = c.experience_level[1].property.as_ref().unwrap();
        let effect = CombatEffect {
            attack: 4.0,
            defense: 1.0,
            health: 0.05,
            ..Default::default()
        };
        assert_eq!(combat_effect(regular), Some(effect));
    }

    #[test]
    fn properties_say_how_they_raise_attacks_and_set_efficiency() {
        let parsed = tgi::parse(
            "[Property]\n{\n\t[Modifier]\n\t{\n\t\ttype = ADD\n\t\tattribute = AV_MELEE\n\t\tamount = 2\n\t}\n\
             \t[Modifier]\n\t{\n\t\ttype = MULTIPLY\n\t\tattribute = RECHARGE_TIME\n\t\tamount = 0.5\n\t}\n\
             \t[Modifier]\n\t{\n\t\ttype = MULTIPLY\n\t\tattribute = ATTACK_EFFICIENCY_DESIRED\n\t\tamount = -.5\n\t}\n\
             \t[Modifier]\n\t{\n\t\ttype = MULTIPLY\n\t\tattribute = ATTACK_EFFICIENCY_DESIRED\n\t\tamount = -.25\n\t}\n}\n",
        );
        let block = parsed.root.blocks_of("Property").next().unwrap();
        let p: defs::Property = tgi::from_block(block).unwrap();
        let effect = combat_effect(&p).unwrap();
        assert_eq!(
            (effect.melee, effect.ranged, effect.recharge),
            (2.0, 0.0, 0.5)
        );
        // The largest counts, not their sum.
        assert_eq!(effect.efficiency.desired, Some(0.75));
        assert_eq!(effect.efficiency.value(), Some(0.75));
        let actual = Efficiency {
            actual: Some(1.0),
            desired: None,
        };
        assert_eq!(effect.efficiency.with(actual).value(), Some(1.0));
    }
}
