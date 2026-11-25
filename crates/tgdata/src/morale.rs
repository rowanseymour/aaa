//! The settings regiments' morale and routing run by, and what templates
//! and properties add to them.

use std::collections::HashMap;

use crate::supply::{is, listed};
use crate::{GameData, Id, defs};

/// The property a routing regiment has.
pub const ROUTING: &str = "actor_state_routing";
/// The property a regiment reforming after a rout has.
pub const REFORMING: &str = "actor_state_reforming";
/// The property a regiment out of morale has.
pub const EXHAUSTED: &str = "actor_state_exhausted";
/// The property a regiment standing idle, with nothing else to do, has.
pub const RESTING: &str = "actor_state_resting";
/// The property a regiment running after its attack target, out of
/// formation, has.
pub const CHASING: &str = "actor_state_chasing";
/// The property a regiment that's engaged has.
pub const ENGAGED: &str = "actor_state_engaged";
/// The property a regiment that's engaged by another without engaging
/// back has.
pub const TARGETED: &str = "actor_state_targeted";

/// The settings morale and routing run by, by their `SVars` names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoraleRules {
    /// `MoraleEngagementLoss`: lost a second while engaged...
    pub engagement_loss: f32,
    /// `MoraleEngagementLossFactorMax`: ...times up to this with every
    /// slot but one empty.
    pub engagement_loss_factor_max: f32,
    /// `MoraleDeathFactor`: of a dying member's morale value, lost.
    pub death_factor: f32,
    /// `MoraleRetreatPenalty`: lost running while engaged, and added to
    /// the breakpoint below which running is withdrawn.
    pub retreat_penalty: f32,
    /// `MoraleSlayerBase`, `MoraleSlayerFactorMin` and `Max`: won by a
    /// kill.
    pub slayer_base: f32,
    pub slayer_factor: [f32; 2],
    /// `MoraleBreakerBase`, `MoraleBreakerFactorMin` and `Max`: won by
    /// breaking an enemy.
    pub breaker_base: f32,
    pub breaker_factor: [f32; 2],
    /// `MoraleReplenish`: regained a second resting.
    pub replenish: f32,
    /// `RoutingTime` and `ReformTime`, in seconds.
    pub routing_time: f32,
    pub reform_time: f32,
    pub rout: RoutRules,
}

/// How a routing regiment picks where to run, by their `SVars` names
/// less the `Rout` they start with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoutRules {
    pub random_candidates: u32,
    pub max_distance_delta: f32,
    pub supply_zone_candidates: u32,
    /// In degrees.
    pub supply_zone_max_angle_delta: f32,
    pub base_value: f32,
    pub factor_enemy_dist: f32,
    pub factor_enemy_dist_cap: f32,
    pub factor_enemy_angle: f32,
    pub factor_moved_dist: f32,
    pub factor_moved_dist_cap: f32,
    pub factor_supply_dist: f32,
    pub factor_supply_dist_cap: f32,
    pub penalty_enemy_zoc: f32,
    pub penalty_enemy_zog: f32,
    pub bonus_friendly_zos: f32,
    pub bonus_supply_in_range: f32,
    pub fuzziness: f32,
}

impl Default for MoraleRules {
    /// AA's.
    fn default() -> Self {
        MoraleRules {
            engagement_loss: 0.5,
            engagement_loss_factor_max: 3.0,
            death_factor: 0.25,
            retreat_penalty: 0.0,
            slayer_base: 0.5,
            slayer_factor: [0.5, 3.0],
            breaker_base: 3.0,
            breaker_factor: [0.5, 2.0],
            replenish: 2.0,
            routing_time: 10.0,
            reform_time: 10.0,
            rout: RoutRules {
                random_candidates: 20,
                max_distance_delta: 24.0,
                supply_zone_candidates: 2,
                supply_zone_max_angle_delta: 50.0,
                base_value: 0.0,
                factor_enemy_dist: 10.0,
                factor_enemy_dist_cap: 2000.0,
                factor_enemy_angle: 0.25,
                factor_moved_dist: 8.0,
                factor_moved_dist_cap: 2000.0,
                factor_supply_dist: 1.0,
                factor_supply_dist_cap: 2000.0,
                penalty_enemy_zoc: 50.0,
                penalty_enemy_zog: 75.0,
                bonus_friendly_zos: 50.0,
                bonus_supply_in_range: 50.0,
                fuzziness: 20.0,
            },
        }
    }
}

/// What a property does to the morale of what has it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MoraleEffect {
    /// `MORALE`: added to a regiment's maximum.
    pub morale: f32,
    /// `MORALE_BREAKPOINT`: added to its breakpoint.
    pub breakpoint: f32,
    /// `MORALE_LOSS_RATE`: added to the 1 what it loses is divided by.
    pub loss_rate: f32,
    /// `MORALE_RECOVERY_RATE`: added to the 1 what it regains is
    /// multiplied by.
    pub recovery_rate: f32,
    /// `BERSERK`: it loses none from fighting, running or weapons.
    pub berserk: bool,
    /// For a technology, the properties a thing of the side needs for it
    /// to be on it.
    pub requires: Vec<Id>,
}

fn effect(p: &defs::Property) -> Option<MoraleEffect> {
    let mut out = MoraleEffect::default();
    for m in &p.modifier {
        let amount = m.amount.unwrap_or(0.0);
        let field = match () {
            _ if is(&m.attribute, "MORALE") => &mut out.morale,
            _ if is(&m.attribute, "MORALE_BREAKPOINT") => &mut out.breakpoint,
            _ if is(&m.attribute, "MORALE_LOSS_RATE") => &mut out.loss_rate,
            _ if is(&m.attribute, "MORALE_RECOVERY_RATE") => &mut out.recovery_rate,
            _ => {
                out.berserk |= is(&m.modifier_flag, "BERSERK");
                continue;
            }
        };
        *field += amount;
    }
    (out != MoraleEffect::default()).then(|| MoraleEffect {
        requires: listed(p.required_properties.as_deref()),
        ..out
    })
}

impl GameData {
    pub fn morale_rules(&self) -> MoraleRules {
        let d = MoraleRules::default();
        let of = |get: fn(&defs::SVars) -> Option<f32>, or: f32| self.svar(get).unwrap_or(or);
        let count = |get: fn(&defs::SVars) -> Option<u32>, or: u32| self.svar(get).unwrap_or(or);
        let r = d.rout;
        MoraleRules {
            engagement_loss: of(|v| v.morale_engagement_loss, d.engagement_loss),
            engagement_loss_factor_max: of(
                |v| v.morale_engagement_loss_factor_max,
                d.engagement_loss_factor_max,
            ),
            death_factor: of(|v| v.morale_death_factor, d.death_factor),
            retreat_penalty: of(|v| v.morale_retreat_penalty, d.retreat_penalty),
            slayer_base: of(|v| v.morale_slayer_base, d.slayer_base),
            slayer_factor: [
                of(|v| v.morale_slayer_factor_min, d.slayer_factor[0]),
                of(|v| v.morale_slayer_factor_max, d.slayer_factor[1]),
            ],
            breaker_base: of(|v| v.morale_breaker_base, d.breaker_base),
            breaker_factor: [
                of(|v| v.morale_breaker_factor_min, d.breaker_factor[0]),
                of(|v| v.morale_breaker_factor_max, d.breaker_factor[1]),
            ],
            replenish: of(|v| v.morale_replenish, d.replenish),
            routing_time: of(|v| v.routing_time, d.routing_time),
            reform_time: of(|v| v.reform_time, d.reform_time),
            rout: RoutRules {
                random_candidates: count(|v| v.rout_random_candidates, r.random_candidates),
                max_distance_delta: of(|v| v.rout_max_distance_delta, r.max_distance_delta),
                supply_zone_candidates: count(
                    |v| v.rout_supply_zone_candidates,
                    r.supply_zone_candidates,
                ),
                supply_zone_max_angle_delta: of(
                    |v| v.rout_supply_zone_max_angle_delta,
                    r.supply_zone_max_angle_delta,
                ),
                base_value: of(|v| v.rout_base_value, r.base_value),
                factor_enemy_dist: of(|v| v.rout_factor_enemy_dist, r.factor_enemy_dist),
                factor_enemy_dist_cap: of(
                    |v| v.rout_factor_enemy_dist_cap,
                    r.factor_enemy_dist_cap,
                ),
                factor_enemy_angle: of(|v| v.rout_factor_enemy_angle, r.factor_enemy_angle),
                factor_moved_dist: of(|v| v.rout_factor_moved_dist, r.factor_moved_dist),
                factor_moved_dist_cap: of(
                    |v| v.rout_factor_moved_dist_cap,
                    r.factor_moved_dist_cap,
                ),
                factor_supply_dist: of(|v| v.rout_factor_supply_dist, r.factor_supply_dist),
                factor_supply_dist_cap: of(
                    |v| v.rout_factor_supply_dist_cap,
                    r.factor_supply_dist_cap,
                ),
                penalty_enemy_zoc: of(|v| v.rout_penalty_enemy_zoc, r.penalty_enemy_zoc),
                penalty_enemy_zog: of(|v| v.rout_penalty_enemy_zog, r.penalty_enemy_zog),
                bonus_friendly_zos: of(|v| v.rout_bonus_friendly_zos, r.bonus_friendly_zos),
                bonus_supply_in_range: of(
                    |v| v.rout_bonus_supply_in_range,
                    r.bonus_supply_in_range,
                ),
                fuzziness: of(|v| v.rout_fuzziness, r.fuzziness),
            },
        }
    }

    /// The morale each unit template adds to its organization's maximum,
    /// for those that add any, by template id.
    pub fn morale_values(&self) -> HashMap<Id, f32> {
        self.by_template(|t| (t.element_component.as_ref()?.morale_value).filter(|&v| v != 0.0))
    }

    /// Each organization template's `morale_breakpoint`, by template id.
    pub fn morale_breakpoints(&self) -> HashMap<Id, f32> {
        self.by_template(|t| t.organization_component.as_ref()?.morale_breakpoint)
    }

    /// What each property does to morale, for those that do anything to
    /// it, by its id: the shared ones, technologies among them, those
    /// abilities give, and those experience levels give.
    pub fn morale_effects(&self) -> HashMap<Id, MoraleEffect> {
        self.effects_with_levels(effect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn property(text: &str) -> defs::Property {
        let parsed = tgi::parse(text);
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Property").next().unwrap();
        templates.deserialize(block).unwrap()
    }

    #[test]
    fn properties_say_what_they_do_to_morale() {
        let entrenched = property(
            "[Property]\n{\n\tIDS = entrench\n\t[Modifier]\n\t{\n\t\ttype = ADD\n\
             \t\tattribute = DV\n\t\tamount = 4\n\t}\n\t[Modifier]\n\t{\n\t\ttype = MULTIPLY\n\
             \t\tattribute = MORALE_LOSS_RATE\n\t\tamount = 0.5\n\t}\n}\n",
        );
        let loss = MoraleEffect {
            loss_rate: 0.5,
            ..Default::default()
        };
        assert_eq!(effect(&entrenched), Some(loss));
        let unbreakable = property(
            "[Property]\n{\n\tIDS = tech\n\t[Modifier]\n\t{\n\t\ttype = FLAG\n\
             \t\tmodifier_flag = BERSERK\n\t}\n}\n",
        );
        assert!(effect(&unbreakable).unwrap().berserk);
        let nothing = property("[Property]\n{\n\tIDS = x\n}\n");
        assert_eq!(effect(&nothing), None);
    }
}
