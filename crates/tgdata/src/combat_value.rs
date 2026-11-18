//! The figures a thing's combat value is worked out from, and the settings
//! it's worked out by.

use std::collections::HashMap;

use crate::supply::is;
use crate::{GameData, defs};

/// The settings combat values are worked out by, by their `SVars` names
/// less the `CombatValue` they start with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombatValueRules {
    /// `CombatDamageMinimum`: the least an attack is taken to do.
    pub damage_minimum: f32,
    pub range_power: f32,
    pub area_power: f32,
    pub offense_weight: f32,
    pub defense_weight: f32,
    /// The defense weight for an ability of at least `ranged_minimum`'s
    /// range on something shorter-lived than the average.
    pub defense_weight_ranged: f32,
    pub ranged_minimum: f32,
    pub movement_weight: f32,
    pub movement_default: f32,
    /// `CombatValueMeanValuesDiscardPercent`: the share of the lowest and
    /// of the highest left out of each mean.
    pub mean_values_discard_percent: f32,
}

impl Default for CombatValueRules {
    /// AA's.
    fn default() -> Self {
        CombatValueRules {
            damage_minimum: 1.0,
            range_power: 0.15,
            area_power: 1.0,
            offense_weight: 0.5,
            defense_weight: 0.5,
            defense_weight_ranged: 0.25,
            ranged_minimum: 12.0,
            movement_weight: 0.2,
            movement_default: 1.0,
            mean_values_discard_percent: 0.2,
        }
    }
}

/// A template's own figures, as its combat value is worked out from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CombatProfile {
    pub health: f32,
    pub defense: f32,
    /// Top speed; `None` if it can't move.
    pub speed: Option<f32>,
    /// Its abilities and its defender's.
    pub abilities: Vec<CombatAbility>,
}

/// One of a template's abilities, as its combat value sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CombatAbility {
    /// Its main damage; 0 for one that does none.
    pub damage: f32,
    pub range: f32,
    pub area: f32,
    /// Its animation's length plus its `reload_time`.
    pub played: f32,
    /// Its `recharge_time`; the longer of the two is the seconds from one
    /// use to the next.
    pub recharge: f32,
    /// It's left out of its user's value, though not out of the means.
    pub ignored: bool,
    /// It fires a projectile, so `AV_RANGED` raises it rather than
    /// `AV_MELEE`.
    pub projectile: bool,
    /// It's a `BOMBARD` ability.
    pub bombard: bool,
}

/// `a`, used by something animated by `animations`.
fn ability(a: &defs::Ability, animations: &[defs::Animation]) -> Option<CombatAbility> {
    let d = a.ability_definition.as_ref()?;
    let main = d.effect.iter().filter(|e| is(&e.kind, "MAIN_DAMAGE"));
    let played = (a.animation1.as_deref())
        .and_then(|ids| animations.iter().find(|n| is(&n.ids, ids)))
        .and_then(|n| n.length)
        .unwrap_or(0.0);
    let reload = a.reload_time.unwrap_or(0.0);
    Some(CombatAbility {
        damage: main.filter_map(|e| e.amount).sum(),
        range: d.range.unwrap_or(0.0),
        area: d.area,
        played: played + reload,
        recharge: d.recharge_time,
        ignored: d.flags.iter().any(|f| f.ignore_for_cv),
        projectile: crate::not_null(d.projectile_ids.clone()).is_some(),
        bombard: d.flags.iter().any(|f| f.bombard),
    })
}

fn profile(t: &defs::Thing) -> Option<CombatProfile> {
    let body = t.body_component.as_ref()?;
    let health = body.health.filter(|&h| h > 0.0)?;
    let performs = t.performer_component.iter().flat_map(|p| &p.ability);
    let defends = t.defender_component.iter().flat_map(|d| &d.ability);
    let speed = (t.mover_component.as_ref())
        .and_then(|m| m.speed_max)
        .filter(|&s| s > 0.0);
    let animations = t
        .character_component
        .as_ref()
        .map_or(&[][..], |c| &c.animation);
    let abilities = performs
        .chain(defends)
        .filter_map(|a| ability(a, animations));
    Some(CombatProfile {
        health,
        defense: body.defense.unwrap_or(0.0),
        speed,
        abilities: abilities.collect(),
    })
}

impl GameData {
    pub fn combat_value_rules(&self) -> CombatValueRules {
        let svars = self.svars();
        let d = CombatValueRules::default();
        let of = |get: fn(&defs::SVars) -> Option<f32>, or: f32| {
            svars.iter().find_map(get).unwrap_or(or)
        };
        CombatValueRules {
            damage_minimum: of(|v| v.combat_damage_minimum, d.damage_minimum),
            range_power: of(|v| v.combat_value_range_power, d.range_power),
            area_power: of(|v| v.combat_value_area_power, d.area_power),
            offense_weight: of(|v| v.combat_value_offense_weight, d.offense_weight),
            defense_weight: of(|v| v.combat_value_defense_weight, d.defense_weight),
            defense_weight_ranged: of(
                |v| v.combat_value_defense_weight_ranged,
                d.defense_weight_ranged,
            ),
            ranged_minimum: of(|v| v.combat_value_ranged_minimum, d.ranged_minimum),
            movement_weight: of(|v| v.combat_value_movement_weight, d.movement_weight),
            movement_default: of(|v| v.combat_value_movement_default, d.movement_default),
            mean_values_discard_percent: of(
                |v| v.combat_value_mean_values_discard_percent,
                d.mean_values_discard_percent,
            ),
        }
    }

    /// The figures of each template that can be hurt, by template id.
    pub fn combat_profiles(&self) -> HashMap<crate::Id, CombatProfile> {
        self.by_template(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thing(text: &str) -> defs::Thing {
        let parsed = tgi::parse(text);
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        templates.deserialize(block).unwrap()
    }

    #[test]
    fn profiles_take_main_damage_and_the_longer_wait() {
        let t = thing(
            "[Thing]\n{\n\t[BodyComponent]\n\t{\n\t\thealth = 100\n\t\tdefense = 2\n\t}\n\
             \t[MoverComponent]\n\t{\n\t\tspeed_max = 5\n\t}\n\
             \t[PerformerComponent]\n\t{\n\
             \t\t[Ability]\n\t\t{\n\t\t\tanimation1 = fire\n\t\t\treload_time = 0.5\n\t\t\t[AbilityDefinition]\n\t\t\t{\n\
             \t\t\t\tability_type = attack\n\t\t\t\trange = 20\n\t\t\t\trecharge_time = 2\n\
             \t\t\t\t[Effect]\n\t\t\t\t{\n\t\t\t\t\ttype = MAIN_DAMAGE\n\t\t\t\t\tdamage_type_IDS = gunfire\n\t\t\t\t\tamount = 10\n\t\t\t\t}\n\
             \t\t\t}\n\t\t}\n\
             \t\t[Ability]\n\t\t{\n\t\t\t[AbilityDefinition]\n\t\t\t{\n\t\t\t\trange = 30\n\
             \t\t\t\t[Flags]\n\t\t\t\t{\n\t\t\t\t\tignore_for_cv = true\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\
             \t}\n}\n",
        );
        let gun = CombatAbility {
            damage: 10.0,
            range: 20.0,
            area: 0.0,
            played: 0.5,
            recharge: 2.0,
            ignored: false,
            projectile: false,
            bombard: false,
        };
        let flak = CombatAbility {
            range: 30.0,
            ignored: true,
            ..CombatAbility::default()
        };
        let expected = CombatProfile {
            health: 100.0,
            defense: 2.0,
            speed: Some(5.0),
            abilities: vec![gun, flak],
        };
        assert_eq!(profile(&t), Some(expected));
    }

    #[test]
    fn a_long_attack_animation_lengthens_the_cycle() {
        let t = thing(
            "[Thing]\n{\n\t[BodyComponent]\n\t{\n\t\thealth = 100\n\t}\n\
             \t[CharacterComponent]\n\t{\n\t\t[Animation]\n\t\t{\n\t\t\tIDS = fire\n\t\t\tlength = 3\n\t\t}\n\t}\n\
             \t[PerformerComponent]\n\t{\n\
             \t\t[Ability]\n\t\t{\n\t\t\tanimation1 = fire\n\t\t\treload_time = 0.5\n\t\t\t[AbilityDefinition]\n\t\t\t{\n\
             \t\t\t\trange = 20\n\t\t\t\trecharge_time = 2\n\t\t\t}\n\t\t}\n\
             \t}\n}\n",
        );
        let a = profile(&t).unwrap().abilities[0];
        assert_eq!((a.played, a.recharge), (3.5, 2.0));
    }
}
