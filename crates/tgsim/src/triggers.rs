//! A map's triggers: tested every tick in the map's order, each running
//! its effects when its events and conditions allow.
//!
//! Events aren't carried out, so a trigger waiting on one never goes
//! ahead; a condition or effect not handled here fails.

use std::collections::BTreeMap;
use std::sync::Arc;

use tgmap::Id;
use tgmap::triggers::{Element, Parameter, Trigger, Value};

use crate::{SECOND, TICK, World};

/// Where a trigger stands in the match.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Switch {
    pub active: bool,
    /// The tick it was last switched on or, repeating, last fired.
    pub activated: u64,
}

crate::hash::digest!(Switch { active, activated });

/// The map's triggers and where each stands, with the trigger variables.
#[derive(Debug, Clone, Default)]
pub struct Triggers {
    /// As the map has them, in its order.
    pub(crate) list: Arc<[Trigger]>,
    /// One for each of `list`.
    pub switches: Vec<Switch>,
    pub variables: BTreeMap<String, f32>,
}

impl Triggers {
    pub fn new(triggers: Option<&tgmap::Triggers>) -> Triggers {
        let Some(t) = triggers else {
            return Triggers::default();
        };
        let switch = |t: &Trigger| Switch {
            active: t.active,
            activated: (t.activation_time.max(0.0) / TICK).round() as u64,
        };
        for trigger in &t.triggers {
            let elements = trigger.conditions.iter().chain(&trigger.effects);
            for e in elements.filter(|e| !HANDLED.contains(&e.name.as_str())) {
                log::debug!("trigger {}: {} isn't carried out", trigger.name, e.name);
            }
        }
        Triggers {
            list: t.triggers.clone().into(),
            switches: t.triggers.iter().map(switch).collect(),
            variables: t.variables.iter().cloned().collect(),
        }
    }
}

impl crate::hash::Digest for Triggers {
    fn digest(&self, h: &mut crate::hash::Hasher) {
        // The list is fixed by the map.
        self.switches.digest(h);
        self.variables.digest(h);
    }
}

/// The conditions and effects carried out.
const HANDLED: &[&str] = &[
    "true",
    "activation_time",
    "value_compare_value",
    "kingdom_has_compare_value_primary_resource",
    "kingdom_gains_hero",
    "kingdom_loses_hero",
    "kingdom_mission_won",
    "set_variable",
    "incr_variable",
    "decr_variable",
    "trigger_active",
    "trigger_deactive",
];

/// How two numbers are compared, by the game's numbering.
fn compare(how: u32, a: f32, b: f32) -> bool {
    match how {
        0 => a < b,
        1 => a <= b,
        3 => a >= b,
        4 => a > b,
        5 => a != b,
        _ => a == b,
    }
}

impl World {
    /// Tests each active trigger in the map's order, running those that go
    /// ahead.
    pub(crate) fn run_triggers(&mut self) {
        for t in 0..self.triggers.list.len() {
            if !self.triggers.switches[t].active {
                continue;
            }
            // Events aren't carried out: one waiting on any never goes ahead.
            if !self.triggers.list[t].events.is_empty() {
                continue;
            }
            let trigger = &self.triggers.list[t];
            let mut held = trigger
                .conditions
                .iter()
                .map(|c| self.condition_holds(t, c));
            let ahead = match trigger.all_conditions {
                true => held.all(|h| h),
                false => trigger.conditions.is_empty() || held.any(|h| h),
            };
            if !ahead {
                continue;
            }
            // Effects that fail do nothing, and the rest still run.
            let list = Arc::clone(&self.triggers.list);
            for effect in &list[t].effects {
                self.carry_out(effect);
            }
            let ticks = self.ticks;
            let repeats = self.triggers.list[t].repeats;
            let switch = &mut self.triggers.switches[t];
            match repeats {
                true => switch.activated = ticks,
                false => switch.active = false,
            }
        }
    }

    /// Whether the condition `c` of the trigger at `t` holds.
    fn condition_holds(&self, t: usize, c: &Element) -> bool {
        let p = &c.parameters;
        match c.name.as_str() {
            "true" => true,
            "activation_time" => {
                let since = self
                    .ticks
                    .saturating_sub(self.triggers.switches[t].activated);
                let since = since as f32 / SECOND as f32;
                self.number(p.get(1))
                    .is_some_and(|v| compare(comparison(p.first()), since, v))
            }
            "value_compare_value" => match (self.number(p.first()), self.number(p.get(2))) {
                (Some(a), Some(b)) => compare(comparison(p.get(1)), a, b),
                _ => false,
            },
            "kingdom_has_compare_value_primary_resource" => {
                match (self.kingdom(p.first()), self.number(p.get(2))) {
                    (Some(k), Some(v)) => {
                        compare(comparison(p.get(1)), self.stockpile(k).round(), v)
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Carries out the effect `e`, saying whether it did anything.
    fn carry_out(&mut self, e: &Element) -> bool {
        let p = &e.parameters;
        match e.name.as_str() {
            "kingdom_gains_hero" => match (self.kingdom(p.first()), hero(p.get(1))) {
                (Some(k), Some(Some(hero))) => self.gain_hero(k, &hero),
                (Some(k), Some(None)) => self.gain_random_hero(k),
                _ => false,
            },
            "kingdom_loses_hero" => match (self.kingdom(p.first()), hero(p.get(1))) {
                (Some(k), Some(Some(hero))) => self.lose_hero(k, &hero),
                (Some(k), Some(None)) => self.lose_random_hero(k),
                _ => false,
            },
            "kingdom_mission_won" => match self.kingdom(p.first()) {
                Some(k) => {
                    self.win(k);
                    true
                }
                None => false,
            },
            "set_variable" => match (name(p.first()), self.number(p.get(1))) {
                (Some(n), Some(v)) if !n.is_empty() => {
                    self.triggers.variables.insert(n.to_string(), v);
                    true
                }
                _ => false,
            },
            "incr_variable" | "decr_variable" => {
                let by = self.number(p.get(1));
                let sign = if e.name == "incr_variable" { 1.0 } else { -1.0 };
                match (name(p.first()), by) {
                    (Some(n), Some(by)) => match self.triggers.variables.get_mut(n) {
                        Some(v) => {
                            *v += sign * by;
                            true
                        }
                        None => false,
                    },
                    _ => false,
                }
            }
            "trigger_active" | "trigger_deactive" => {
                let on = e.name == "trigger_active";
                let Some(Value::Int(id)) = p.first().map(|p| &p.value) else {
                    return false;
                };
                let Some(t) = self.triggers.list.iter().position(|t| t.id == *id) else {
                    return false;
                };
                let ticks = self.ticks;
                let switch = &mut self.triggers.switches[t];
                if switch.active == on {
                    return false;
                }
                switch.active = on;
                if on {
                    switch.activated = ticks;
                }
                true
            }
            _ => false,
        }
    }

    /// The number a parameter gives: stored, or a variable's.
    fn number(&self, p: Option<&Parameter>) -> Option<f32> {
        let p = p?;
        Some(match (p.source.as_str(), &p.value) {
            ("mi_int", Value::Int(n)) => *n as i32 as f32,
            ("mi_fixed", Value::Float(f)) => *f,
            ("mi_variable", Value::String(n)) => {
                self.triggers.variables.get(n).copied().unwrap_or(0.0)
            }
            _ => 0.0,
        })
    }

    /// The side a parameter names, by its kingdom's name or id.
    fn kingdom(&self, p: Option<&Parameter>) -> Option<u32> {
        let p = p?;
        let players = &self.sides.players;
        match (p.source.as_str(), &p.value) {
            ("mi_kingdom_named", Value::String(n)) => players
                .iter()
                .find(|k| k.name.eq_ignore_ascii_case(n))
                .map(|k| k.id),
            ("mi_kingdom_id", Value::Int(id)) => players.iter().find(|k| k.id == *id).map(|k| k.id),
            _ => None,
        }
    }
}

/// The comparison a parameter gives, "equal" where it gives none.
fn comparison(p: Option<&Parameter>) -> u32 {
    match p.map(|p| (p.source.as_str(), &p.value)) {
        Some(("mi_compare", Value::Int(how))) => *how,
        _ => 2,
    }
}

/// The hero a parameter names, or none for one picked at random; nothing
/// where it does neither.
fn hero(p: Option<&Parameter>) -> Option<Option<Id>> {
    match p.map(|p| (p.source.as_str(), &p.value))? {
        ("mi_hero_named", Value::String(n)) => Some(Some(Id::from(n.as_str()))),
        ("mi_hero_random", _) => Some(None),
        _ => None,
    }
}

/// The name a parameter gives.
fn name(p: Option<&Parameter>) -> Option<&str> {
    match &p?.value {
        Value::String(n) => Some(n),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use tgmap::triggers::{Element, Parameter, Trigger, Value};

    use super::Triggers;
    use crate::heroes::tests::pool;
    use crate::testing::run;
    use crate::{Outcome, World};

    fn parameter(kind: &str, source: &str, value: Value) -> Parameter {
        Parameter {
            kind: kind.into(),
            source: source.into(),
            set: true,
            value,
        }
    }

    fn element(name: &str, parameters: Vec<Parameter>) -> Element {
        Element {
            version: 0,
            name: name.into(),
            id: 0,
            parameters,
        }
    }

    fn trigger(
        id: u32,
        active: bool,
        repeats: bool,
        conditions: Vec<Element>,
        effects: Vec<Element>,
    ) -> Trigger {
        Trigger {
            id,
            all_conditions: true,
            active,
            repeats,
            activation_time: 0.0,
            name: format!("trigger {id}"),
            comment: String::new(),
            next_element: 0,
            events: Vec::new(),
            conditions,
            effects,
        }
    }

    fn kingdom(id: u32) -> Parameter {
        parameter("ma_kingdom", "mi_kingdom_id", Value::Int(id))
    }

    fn int(n: i32) -> Parameter {
        parameter("ma_value", "mi_int", Value::Int(n as u32))
    }

    fn at_least() -> Parameter {
        parameter("ma_compare", "mi_compare", Value::Int(3))
    }

    fn variable(name: &str) -> Parameter {
        parameter("ma_variable", "mi_variable", Value::String(name.into()))
    }

    fn after(seconds: i32) -> Element {
        element("activation_time", vec![at_least(), int(seconds)])
    }

    fn incr(name: &str) -> Element {
        element("incr_variable", vec![variable(name), int(1)])
    }

    fn with(mut w: World, triggers: Vec<Trigger>, variables: &[(&str, f32)]) -> World {
        let section = tgmap::Triggers {
            version: 0,
            next_id: 0,
            triggers,
            variables: variables.iter().map(|&(n, v)| (n.to_string(), v)).collect(),
            legacy: None,
            unused_strings: None,
        };
        w.triggers = Triggers::new(Some(&section));
        w
    }

    fn count(w: &World, name: &str) -> f32 {
        w.triggers.variables.get(name).copied().unwrap_or(-1.0)
    }

    #[test]
    fn a_map_init_trigger_runs_once_on_the_first_tick() {
        let random = parameter("ma_hero", "mi_hero_random", Value::Int(0));
        let named = parameter("ma_kingdom", "mi_kingdom_named", Value::String("P2".into()));
        let gains = |k: Parameter| element("kingdom_gains_hero", vec![k, random.clone()]);
        let init = trigger(
            1,
            true,
            false,
            Vec::new(),
            vec![gains(kingdom(1)), gains(kingdom(1)), gains(named)],
        );
        let mut w = with(pool(), vec![init], &[]);
        w.sides.players[1].name = "P2".into();
        assert!(w.heroes(1).is_empty());
        w.tick();
        assert_eq!(w.heroes(1).len(), 2);
        assert_eq!(w.heroes(2).len(), 1);
        assert!(!w.triggers.switches[0].active);
        run(&mut w, 5.0);
        assert_eq!(w.heroes(1).len(), 2);
    }

    #[test]
    fn a_repeating_trigger_fires_every_so_often_from_its_last_firing() {
        let every = trigger(1, true, true, vec![after(5)], vec![incr("n")]);
        let mut w = with(pool(), vec![every], &[("n", 0.0)]);
        run(&mut w, 4.9);
        assert_eq!(count(&w, "n"), 0.0);
        run(&mut w, 0.1);
        assert_eq!(count(&w, "n"), 1.0);
        run(&mut w, 5.0);
        assert_eq!(count(&w, "n"), 2.0);
    }

    #[test]
    fn conditions_hold_all_or_any_and_compare_variables() {
        let small = element(
            "value_compare_value",
            vec![variable("n"), at_least(), int(2)],
        );
        let never = element(
            "value_compare_value",
            vec![variable("n"), at_least(), int(99)],
        );
        let mut all = trigger(
            1,
            true,
            true,
            vec![small.clone(), never.clone()],
            vec![incr("all")],
        );
        all.all_conditions = true;
        let mut any = trigger(2, true, true, vec![small, never], vec![incr("any")]);
        any.all_conditions = false;
        let mut w = with(
            pool(),
            vec![all, any],
            &[("n", 2.0), ("all", 0.0), ("any", 0.0)],
        );
        w.tick();
        assert_eq!((count(&w, "all"), count(&w, "any")), (0.0, 1.0));
        // Incrementing a variable there isn't does nothing.
        assert_eq!(count(&w, "missing"), -1.0);
    }

    #[test]
    fn a_trigger_switched_on_later_in_the_list_is_tested_that_tick() {
        let switch_on = element(
            "trigger_active",
            vec![parameter("ma_trigger", "mi_trigger", Value::Int(2))],
        );
        let first = trigger(1, true, false, Vec::new(), vec![switch_on.clone()]);
        let second = trigger(2, false, false, Vec::new(), vec![incr("n")]);
        let mut w = with(pool(), vec![first, second], &[("n", 0.0)]);
        w.tick();
        assert_eq!(count(&w, "n"), 1.0);
        assert!(!w.triggers.switches[1].active);
    }

    #[test]
    fn a_trigger_can_win_a_side_the_match() {
        let won = element("kingdom_mission_won", vec![kingdom(2)]);
        let mut w = with(
            pool(),
            vec![trigger(1, true, false, vec![after(1)], vec![won])],
            &[],
        );
        run(&mut w, 0.5);
        assert_eq!(w.outcome(2), Outcome::Playing);
        run(&mut w, 0.5);
        assert_eq!(w.outcome(2), Outcome::Won);
    }

    #[test]
    fn variables_go_down_and_need_a_name_to_be_set() {
        let set = |n: &str| element("set_variable", vec![variable(n), int(5)]);
        let decr = element("decr_variable", vec![variable("n"), int(2)]);
        let effects = vec![set("n"), set(""), decr];
        let mut w = with(
            pool(),
            vec![trigger(1, true, false, Vec::new(), effects)],
            &[],
        );
        w.tick();
        assert_eq!(count(&w, "n"), 3.0);
        assert_eq!(count(&w, ""), -1.0);
    }

    #[test]
    fn comparisons_go_by_the_games_numbering() {
        let cases = [
            (0, 1.0, 2.0, true),
            (1, 2.0, 2.0, true),
            (2, 2.0, 3.0, false),
            (4, 2.0, 2.0, false),
            (5, 1.0, 2.0, true),
            (9, 2.0, 2.0, true),
        ];
        for (how, a, b, want) in cases {
            assert_eq!(super::compare(how, a, b), want, "{how} {a} {b}");
        }
    }

    #[test]
    fn a_trigger_switched_off_is_no_longer_tested() {
        let id = |n| parameter("ma_trigger", "mi_trigger", Value::Int(n));
        let off = element("trigger_deactive", vec![id(2)]);
        let first = trigger(1, true, false, vec![after(1)], vec![off]);
        let second = trigger(2, true, true, Vec::new(), vec![incr("n")]);
        let mut w = with(pool(), vec![first, second], &[("n", 0.0)]);
        run(&mut w, 2.0);
        // On every tick until the one that switched it off.
        assert_eq!(count(&w, "n"), 9.0);
        assert!(!w.triggers.switches[1].active);
    }

    #[test]
    fn a_side_loses_a_named_or_random_hero_from_its_pool() {
        let named = parameter("ma_hero", "mi_hero_named", Value::String("andyr".into()));
        let random = parameter("ma_hero", "mi_hero_random", Value::Int(0));
        let loses = |h: Parameter| element("kingdom_loses_hero", vec![kingdom(1), h]);
        let effects = vec![loses(named.clone()), loses(random.clone()), loses(random)];
        let mut w = with(
            pool(),
            vec![trigger(1, true, false, Vec::new(), effects)],
            &[],
        );
        for h in ["andyr", "arossa", "cutscene"] {
            w.gain_hero(1, &tgmap::Id::from(h));
        }
        w.tick();
        // The hero for scenarios alone isn't taken at random.
        let left: Vec<String> = w.heroes(1).iter().map(|h| h.hero.to_string()).collect();
        assert_eq!(left, ["cutscene"]);
    }
}
