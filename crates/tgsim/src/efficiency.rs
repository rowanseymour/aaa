//! Attack efficiency: how hard a regiment hits at the moment, as a share of
//! full strength, which multiplies its members' main damage.

use tgmap::{CombatEffect, Efficiency, Id};

use crate::supply::BEAT;
use crate::value::MULTIPLY_FLOOR;
use crate::{TICK, Target, World};

/// The efficiency `e` sets, full strength where it sets none.
fn strength(e: Efficiency) -> f32 {
    e.value().unwrap_or(1.0).max(MULTIPLY_FLOOR)
}

impl World {
    /// What the properties the organization at `r` holds itself add up to:
    /// its template's own, its side's technologies on its kind, those it
    /// holds for a time, its level's and its state's.
    fn organization_effect(&self, r: usize) -> CombatEffect {
        let organization = &self.organizations[r];
        let (owner, held) = (organization.owner, &organization.properties);
        let state = organization.mind.property().map(Id::from);
        let more = [self.level_property(r), state.as_ref()];
        self.held_effect(owner, &organization.template, held, &more)
    }

    /// The attack efficiency the organization at `r` eases towards: its
    /// state's if that sets one, else its formation's, or its broken
    /// formation's while it has none.
    pub fn efficiency_target(&self, r: usize) -> f32 {
        let organization = &self.organizations[r];
        let formation = match organization.broken() {
            true => self.rules.broken_efficiencies.get(&organization.template),
            false => (organization.formations.get(organization.formation)).and_then(|f| {
                self.rules
                    .formation_efficiencies
                    .get(&tgmap::Id::from(f.id.as_str()))
            }),
        };
        let held = self.organization_effect(r).efficiency;
        strength(held.with(formation.copied().unwrap_or_default()))
    }

    /// The attack efficiency a hit by `from` is dealt with: a regiment's
    /// for its members, a lone unit's or a building's own properties'
    /// otherwise, and full strength for what fires no weapons of its own.
    pub(crate) fn efficiency(&self, from: Option<Target>) -> f32 {
        match from {
            Some(Target::Unit(u)) => {
                let r = self.units[u].organization;
                match self.lone(r) {
                    true => strength(self.boost(u).efficiency),
                    false => self.organizations[r].efficiency,
                }
            }
            Some(Target::Building(b)) => {
                // One standing in a regiment keeps the one the map gives it.
                let building = &self.buildings[b];
                let regiment = building.organization.as_ref().and_then(|o| o.efficiency);
                regiment.unwrap_or_else(|| strength(self.building_boost(b).efficiency))
            }
            None => 1.0,
        }
    }

    /// Eases each regiment's attack efficiency towards its target on its
    /// beat, a second's worth at a time. A regiment has a leader for as
    /// long as it has members, so only one that's gone holds it.
    pub(crate) fn keep_efficiency(&mut self) {
        let [climbing, falling] = self.rules.efficiency_rates;
        let seconds = BEAT as f32 * TICK;
        for r in 0..self.organizations.len() {
            let beat = (self.ticks + r as u64).is_multiple_of(BEAT);
            if !beat || self.lone(r) || self.destroyed(r) {
                continue;
            }
            let target = self.efficiency_target(r);
            let now = self.organizations[r].efficiency;
            self.organizations[r].efficiency = match now < target {
                true => (now + climbing * seconds).min(target),
                false => (now - falling * seconds).max(target),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    use tgmap::Id;

    use crate::Mind;

    fn desired(amount: f32) -> Efficiency {
        Efficiency {
            actual: None,
            desired: Some(amount),
        }
    }

    /// Two riflemen's regiments, each led by its one member, the first in
    /// a formation that sets 0.5.
    fn column() -> crate::World {
        let mut w = battle(
            &[1, 2],
            vec![soldier(Vec::new()), soldier(Vec::new())],
            at_war(),
        );
        let id = Id::from(w.organizations[0].formations[0].id.as_str());
        w.rules.formation_efficiencies.insert(id, desired(0.5));
        w
    }

    #[test]
    fn efficiency_eases_towards_its_formations_by_the_second() {
        let mut w = column();
        assert_eq!(w.efficiency_target(0), 0.5);
        run(&mut w, 10.0);
        let e = w.organizations[0].efficiency;
        assert!((e - 0.8).abs() < 1e-4, "{e}");
        run(&mut w, 30.0);
        assert!((w.organizations[0].efficiency - 0.5).abs() < 1e-4);
        w.rules.formation_efficiencies.clear();
        run(&mut w, 10.0);
        let e = w.organizations[0].efficiency;
        assert!((e - 0.6).abs() < 1e-4, "{e}");
    }

    #[test]
    fn a_state_that_sets_efficiency_outranks_the_formation() {
        let mut w = column();
        let reforming = CombatEffect {
            efficiency: Efficiency {
                actual: Some(1.0),
                desired: None,
            },
            ..Default::default()
        };
        w.rules
            .combat_effects
            .insert(Id::from(tgmap::REFORMING), reforming);
        w.organizations[0].mind = Mind::Reforming { until: 1000 };
        assert_eq!(w.efficiency_target(0), 1.0);
    }

    #[test]
    fn a_broken_regiment_takes_its_broken_formations_efficiency() {
        let mut w = column();
        let template = w.organizations[0].template.clone();
        w.rules.broken_efficiencies.insert(template, desired(0.25));
        w.organizations[0].mind = Mind::Exhausted;
        assert_eq!(w.efficiency_target(0), 0.25);
    }

    #[test]
    fn efficiency_never_targets_below_the_floor() {
        let mut w = column();
        let id = Id::from(w.organizations[0].formations[0].id.as_str());
        w.rules.formation_efficiencies.insert(id, desired(0.0));
        assert_eq!(w.efficiency_target(0), MULTIPLY_FLOOR);
    }
}
