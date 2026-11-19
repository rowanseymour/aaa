//! The stances units take: the last of their template's stances whose
//! health and states they meet, which gives them its property while it's
//! held.

use tgmap::Id;

use crate::World;

/// The states a unit can be in, as stances and effects name them.
const ENGAGED: &str = "mind_engaged";
const UNDER_SIEGE: &str = "under_siege";
const FALLING: &str = "falling";
const DEAD: &str = "dead";

impl World {
    /// The states the unit at `u` is in: dead, falling under its
    /// parachute, attacking a target of its own other than by bombardment,
    /// and under siege.
    pub fn unit_states(&self, u: usize) -> Vec<&'static str> {
        [DEAD, FALLING, ENGAGED, UNDER_SIEGE]
            .into_iter()
            .filter(|s| self.in_state(u, s))
            .collect()
    }

    /// Whether the unit at `u` is in `state`, as [`World::unit_states`]
    /// has them; a dead one is in no other.
    fn in_state(&self, u: usize, state: &str) -> bool {
        let unit = &self.units[u];
        // Not one that left the map by unpacking, which is no death.
        let dead = unit.died.is_some();
        let is = |s: &str| state.eq_ignore_ascii_case(s);
        if is(DEAD) {
            return dead;
        }
        if dead {
            return false;
        }
        let bombarding = self.organizations[unit.organization].bombarding;
        (is(FALLING) && unit.descent.is_some())
            || (is(ENGAGED) && unit.alive() && unit.target.is_some() && !bombarding)
            || (is(UNDER_SIEGE) && unit.besieged.is_some())
    }

    /// The property the unit at `u`'s stance gives it, if it holds one that
    /// gives one it has what it needs for.
    pub(crate) fn stance_property(&self, u: usize) -> Option<&Id> {
        let unit = &self.units[u];
        let stances = self.rules.stances.get(&unit.template)?;
        let full = self.max_health(u);
        let share = match full > 0.0 {
            true => unit.health / full,
            false => return None,
        };
        let applies = |s: &&tgmap::Stance| {
            share <= s.health && s.states.iter().all(|n| self.in_state(u, n.as_str()))
        };
        let property = stances.iter().rfind(applies)?.property.as_ref()?;
        let needs = self.rules.combat_effects.get(property).map(|e| &e.requires);
        let kind = self.rules.kinds.get(&unit.template);
        let has = |p: &Id| kind.is_some_and(|k| k.properties.contains(p));
        needs.is_none_or(|n| n.iter().all(has)).then_some(property)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::*;
    use crate::{TICK, Target};

    use tgmap::{CombatEffect, Id, Stance};

    /// A soldier of player 1's that kneels while fighting back under fire,
    /// and goes prone at half health or less, slowed as AA's are.
    fn kneeling() -> crate::World {
        let mut w = battle(
            &[1, 2],
            vec![soldier(Vec::new()), soldier(Vec::new())],
            at_war(),
        );
        let stance = |ids: &str, health, property: &str| Stance {
            ids: ids.into(),
            health,
            states: vec!["mind_engaged".into(), "under_siege".into()],
            property: Some(property.into()),
        };
        let template = w.units[0].template.clone();
        (w.rules.stances).insert(
            template,
            vec![
                stance("kneel", 1.0, "infantry_kneel"),
                stance("prone", 0.5, "infantry_prone"),
            ],
        );
        for (p, speed) in [("infantry_kneel", -0.2), ("infantry_prone", -0.6)] {
            let effect = CombatEffect {
                speed,
                ..CombatEffect::default()
            };
            w.rules.combat_effects.insert(Id::from(p), effect);
        }
        w
    }

    #[test]
    fn infantry_kneel_only_while_fighting_back_under_fire_and_go_prone_when_hurt() {
        let mut w = kneeling();
        assert_eq!(w.stance_property(0), None);
        // Firing without being hit, or hit without firing, it stands.
        w.units[0].target = Some(Target::Unit(1));
        assert_eq!(w.stance_property(0), None);
        w.units[0].target = None;
        w.units[0].besieged = Some(w.ticks + (7.0 / TICK) as u64);
        assert_eq!(w.stance_property(0), None);
        w.units[0].target = Some(Target::Unit(1));
        assert_eq!(w.stance_property(0), Some(&Id::from("infantry_kneel")));
        assert!((w.boost(0).speed + 0.2).abs() < 1e-6);
        w.units[0].health = 0.5 * w.max_health(0);
        assert_eq!(w.stance_property(0), Some(&Id::from("infantry_prone")));
        assert!((w.boost(0).speed + 0.6).abs() < 1e-6);
        // Bombarding is no fight of its own.
        w.organizations[0].bombarding = true;
        assert_eq!(w.stance_property(0), None);
    }
}
