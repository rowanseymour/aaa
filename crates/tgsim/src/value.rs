//! Combat value: a single number for how strong a thing is in a fight,
//! measured against the average of the templates loaded.

use std::collections::{HashMap, HashSet};

use tgmap::{CombatAbility, CombatEffect, CombatProfile, CombatValueRules, Id};

use crate::{Piece, TICK, Target, World};

/// What combat values are measured against: trimmed means over every
/// template that can be hurt, can move and has an ability.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CombatMeans {
    /// The mean base damage of an attack.
    pub attack: f32,
    pub defense: f32,
    pub speed: f32,
    pub health: f32,
}

impl CombatMeans {
    /// The means of `profiles`, heroes aside.
    pub fn of(
        profiles: &HashMap<Id, CombatProfile>,
        rules: &CombatValueRules,
        heroes: &HashSet<Id>,
    ) -> CombatMeans {
        let counted: Vec<&CombatProfile> = (profiles.iter())
            .filter(|(id, p)| !heroes.contains(id) && p.speed.is_some() && !p.abilities.is_empty())
            .map(|(_, p)| p)
            .collect();
        let discard = rules.mean_values_discard_percent;
        let mean = |of: fn(&CombatProfile) -> f32| {
            trimmed(counted.iter().map(|p| of(p)).collect(), discard)
        };
        // Each attack is its own entry, those left out of values among them.
        let attacks = counted.iter().flat_map(|p| &p.abilities);
        let damages = attacks.map(|a| a.damage).filter(|&d| d > 0.0);
        CombatMeans {
            attack: trimmed(damages.collect(), discard),
            defense: mean(|p| p.defense),
            speed: mean(|p| p.speed.unwrap_or(0.0)),
            health: mean(|p| p.health),
        }
    }

    /// Mean attack less mean defense.
    pub fn offense(&self) -> f32 {
        self.attack - self.defense
    }

    /// How long the average thing lasts against the average attack.
    pub fn lifetime(&self) -> f32 {
        self.health / self.offense()
    }
}

/// The mean of `values` less the lowest and highest `discard` of them,
/// rounded down to whole values; 0 for none.
fn trimmed(mut values: Vec<f32>, discard: f32) -> f32 {
    values.sort_by(f32::total_cmp);
    let cut = (values.len() as f32 * discard) as usize;
    let kept = values
        .get(cut..values.len().saturating_sub(cut))
        .unwrap_or(&[]);
    match kept.is_empty() {
        true => 0.0,
        false => kept.iter().sum::<f32>() / kept.len() as f32,
    }
}

/// The combat value of something with `profile`'s figures and what
/// `boost` adds to them.
pub(crate) fn combat_value(
    profile: &CombatProfile,
    boost: &CombatEffect,
    rules: &CombatValueRules,
    means: &CombatMeans,
) -> f32 {
    let health = profile.health * (1.0 + boost.health).max(MULTIPLY_FLOOR);
    let defense = (profile.defense + boost.defense).max(0.0);
    let movement = match profile.speed {
        None => FIXED_MOVEMENT,
        Some(speed) => {
            let m = 1.0 + (speed / means.speed - 1.0) * rules.movement_weight;
            match m > 0.0 {
                true => m,
                false => rules.movement_default,
            }
        }
    };
    let rate = (1.0 + boost.recharge).max(MULTIPLY_FLOOR);
    let value = |a: &CombatAbility| {
        let range = a.range
            + if a.projectile {
                boost.projectile_range
            } else {
                0.0
            }
            + if a.bombard { boost.bombard_range } else { 0.0 };
        let cycle = a.played.max(a.recharge / rate);
        let offense = match a.damage > 0.0 {
            true => {
                let reach = if a.projectile {
                    boost.ranged
                } else {
                    boost.melee
                };
                let damage = a.damage + boost.attack + reach;
                let damage = (damage - means.defense).max(rules.damage_minimum);
                damage / cycle.max(TICK)
                    * libm::powf(range.max(0.0), rules.range_power)
                    * libm::powf(1.0 + a.area / 2.0, rules.area_power)
            }
            false => 0.0,
        };
        let mean_offense = means.offense();
        let offense_factor = mean_offense + (offense - mean_offense) * rules.offense_weight;
        let taken = (means.attack - defense).max(rules.damage_minimum);
        let ratio = match health > 0.0 {
            true => health / taken / means.lifetime(),
            false => 0.0,
        };
        let weight = match range >= rules.ranged_minimum && ratio < 1.0 {
            true => rules.defense_weight_ranged,
            false => rules.defense_weight,
        };
        let defense_factor = 1.0 + (ratio - 1.0) * weight;
        defense_factor * offense_factor * movement
    };
    let counted = profile.abilities.iter().filter(|a| !a.ignored);
    let best = counted.map(value).fold(0.0, f32::max);
    best.max(0.0)
}

/// The least a multiplier leaves of what it multiplies.
pub(crate) const MULTIPLY_FLOOR: f32 = 0.05;

/// The movement factor of something that can't move.
const FIXED_MOVEMENT: f32 = 0.8;

impl World {
    fn value_of(&self, template: &Id, boost: &CombatEffect) -> f32 {
        let rules = &self.rules;
        match rules.combat_profiles.get(template) {
            Some(p) => combat_value(p, boost, &rules.combat_value_rules, &rules.combat_means),
            None => 0.0,
        }
    }

    /// A template's own combat value, with nothing added to it.
    pub fn template_value(&self, template: &Id) -> f32 {
        self.value_of(template, &CombatEffect::default())
    }

    /// The unit at `u`'s combat value, with what its properties add.
    pub fn unit_value(&self, u: usize) -> f32 {
        let boost = self.boost(u);
        self.value_of(&self.units[u].template, &boost)
    }

    /// A target's own combat value.
    pub fn target_value(&self, t: Target) -> f32 {
        match t {
            Target::Unit(u) => self.unit_value(u),
            Target::Building(b) => {
                let boost = self.building_boost(b);
                self.value_of(&self.buildings[b].template, &boost)
            }
        }
    }

    /// The organization at `r`'s full combat value: its present members'
    /// and, for each empty place, its template's.
    pub fn full_value(&self, r: usize) -> f32 {
        let present = |u: usize| self.units[u].alive() && !self.units[u].awaited;
        (self.organizations[r].units.iter())
            .map(|&u| match present(u) {
                true => self.unit_value(u),
                false => self.template_value(&self.units[u].template),
            })
            .sum()
    }

    /// A piece's combat value, as experience compares it: an
    /// organization's full value, a building's own.
    pub(crate) fn piece_value(&self, p: Piece) -> f32 {
        match p {
            Piece::Organization(r) => self.full_value(r),
            Piece::Building(b) => self.template_value(&self.buildings[b].template),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> CombatValueRules {
        CombatValueRules::default()
    }

    fn rifleman(damage: f32) -> CombatProfile {
        CombatProfile {
            health: 100.0,
            defense: 1.0,
            speed: Some(2.0),
            abilities: vec![CombatAbility {
                damage,
                range: 12.0,
                area: 0.0,
                played: 0.0,
                recharge: 2.0,
                ignored: false,
                projectile: false,
                bombard: false,
            }],
        }
    }

    #[test]
    fn means_leave_out_the_weakest_and_strongest_fifth() {
        let mut profiles = HashMap::new();
        for (i, damage) in [1.0, 10.0, 10.0, 100.0].into_iter().enumerate() {
            profiles.insert(Id::from(format!("t{i}")), rifleman(damage));
        }
        // An unarmed one counts for all but the attack, and a second
        // attack is an entry of its own.
        profiles.insert("unarmed".into(), rifleman(0.0));
        let mut twice = rifleman(10.0);
        twice.abilities.push(twice.abilities[0]);
        twice.abilities[1].ignored = true;
        profiles.insert("twice".into(), twice);
        // Unmoving ones, those with no abilities and heroes count for
        // nothing.
        let mut idle = rifleman(1000.0);
        idle.abilities.clear();
        profiles.insert("idle".into(), idle);
        let mut fixed = rifleman(1000.0);
        fixed.speed = None;
        profiles.insert("fixed".into(), fixed);
        profiles.insert("hero".into(), rifleman(1000.0));
        let heroes = HashSet::from([Id::from("hero")]);
        let means = CombatMeans::of(&profiles, &rules(), &heroes);
        assert_eq!(means.attack, 10.0);
        assert_eq!(
            (means.defense, means.speed, means.health),
            (1.0, 2.0, 100.0)
        );
        assert_eq!(trimmed(Vec::new(), 0.2), 0.0);
    }

    #[test]
    fn faster_fire_and_longer_range_count_only_for_their_attacks() {
        let means = CombatMeans {
            attack: 10.0,
            defense: 1.0,
            speed: 2.0,
            health: 100.0,
        };
        let rules = rules();
        let value = |profile: &CombatProfile, boost: &CombatEffect| {
            combat_value(profile, boost, &rules, &means)
        };
        let plain = rifleman(10.0);
        let none = CombatEffect::default();
        let faster = CombatEffect {
            recharge: 1.0,
            ..Default::default()
        };
        assert!(value(&plain, &faster) > value(&plain, &none));
        let longer = CombatEffect {
            projectile_range: 8.0,
            melee: 3.0,
            ..Default::default()
        };
        // A rifle that fires no projectile gains the melee bonus only.
        let melee = CombatEffect {
            melee: 3.0,
            ..Default::default()
        };
        assert_eq!(value(&plain, &longer), value(&plain, &melee));
        let mut shooter = rifleman(10.0);
        shooter.abilities[0].projectile = true;
        let ranged = CombatEffect {
            projectile_range: 8.0,
            ..Default::default()
        };
        assert!(value(&shooter, &ranged) > value(&shooter, &none));
    }

    #[test]
    fn the_average_thing_is_worth_the_mean_offense() {
        let means = CombatMeans {
            attack: 10.0,
            defense: 1.0,
            speed: 2.0,
            health: 100.0,
        };
        let rules = rules();
        let none = CombatEffect::default();
        // An attack doing 9 more than defense every 2 seconds at range 12.
        let offense = 9.0 / 2.0 * libm::powf(12.0, 0.15);
        let factor = 9.0 + (offense - 9.0) * 0.5;
        let average = combat_value(&rifleman(10.0), &none, &rules, &means);
        assert!((average - factor).abs() < 1e-4, "{average}");
        // More attack and defense and health are worth more.
        let boost = CombatEffect {
            attack: 4.0,
            defense: 1.0,
            health: 0.05,
            ..Default::default()
        };
        let regular = combat_value(&rifleman(10.0), &boost, &rules, &means);
        assert!(regular > average);
        // Standing still is worth 0.8 of moving at the mean speed.
        let mut fixed = rifleman(10.0);
        fixed.speed = None;
        let fixed = combat_value(&fixed, &none, &rules, &means);
        assert!((fixed - average * 0.8).abs() < 1e-4);
        // Nothing to do is worth nothing.
        let mut idle = rifleman(10.0);
        idle.abilities[0].ignored = true;
        assert_eq!(combat_value(&idle, &none, &rules, &means), 0.0);
    }
}
