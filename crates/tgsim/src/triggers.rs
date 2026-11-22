//! A map's triggers: tested every tick in the map's order, each running
//! its effects when its events and conditions allow.
//!
//! Events aren't carried out, so a trigger waiting on one never goes
//! ahead; a condition or effect not handled here fails.

use std::collections::BTreeMap;
use std::sync::Arc;

use tgmap::Id;
use tgmap::triggers::{Element, Parameter, Trigger, Value};

use crate::targets::Thing;
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
    /// The things the map gives a name, with their ids, in its order.
    pub(crate) names: Arc<[(String, u32)]>,
    /// The changes of the sun's colours triggers have made, oldest first.
    pub suns: Vec<SunChange>,
}

/// A trigger turning the sun's colours to a sun definition's, moving from
/// whatever they show at `at` over `seconds` of game time.
#[derive(Debug, Clone, PartialEq)]
pub struct SunChange {
    /// The sun definition, by id, e.g. `sun_night`.
    pub sun: Id,
    /// The tick it was made.
    pub at: u64,
    pub seconds: f32,
}

crate::hash::digest!(SunChange { sun, at, seconds });

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
            names: Arc::default(),
            suns: Vec::new(),
        }
    }
}

impl crate::hash::Digest for Triggers {
    fn digest(&self, h: &mut crate::hash::Hasher) {
        // The list and names are fixed by the map.
        self.switches.digest(h);
        self.variables.digest(h);
        self.suns.digest(h);
    }
}

/// The conditions and effects carried out.
const HANDLED: &[&str] = &[
    "true",
    "activation_time",
    "value_compare_value",
    "kingdom_has_compare_value_primary_resource",
    "kingdom_has_technology",
    "kingdom_gains_technology",
    "kingdom_loses_technology",
    "actor_gains_technology",
    "actor_loses_technology",
    "kingdom_gains_hero",
    "kingdom_loses_hero",
    "kingdom_mission_won",
    "set_variable",
    "incr_variable",
    "decr_variable",
    "trigger_active",
    "trigger_deactive",
    "actor_modify_morale",
    "actor_gains_experience",
    "global_set_sun_colors",
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
            "kingdom_has_technology" => match (self.kingdom(p.first()), technology(p.get(1))) {
                (Some(k), Some(t)) => self.holds(k, &t),
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
            "kingdom_gains_technology" => {
                let Some(t) = technology(p.get(1)) else {
                    return false;
                };
                let all = p.first().is_some_and(|p| p.source == "mi_kingdom_all");
                let kingdoms: Vec<u32> = match all {
                    true => (self.sides.players.iter())
                        .filter(|k| !k.independent)
                        .map(|k| k.id)
                        .collect(),
                    false => self.kingdom(p.first()).into_iter().collect(),
                };
                for &k in &kingdoms {
                    self.gain_taken_back(k, t.clone());
                }
                !kingdoms.is_empty()
            }
            "kingdom_loses_technology" => match (self.kingdom(p.first()), technology(p.get(1))) {
                (Some(k), Some(t)) => {
                    self.lose_given(k, &t);
                    true
                }
                _ => false,
            },
            "actor_gains_technology" => match (self.thing(p.first()), technology(p.get(1))) {
                (Some(t), Some(tech)) => {
                    self.gain_own(t, tech);
                    true
                }
                _ => false,
            },
            "actor_loses_technology" => match (self.thing(p.first()), technology(p.get(1))) {
                (Some(t), Some(tech)) => {
                    if !self.lose_own(t, &tech) {
                        log::debug!("{t:?} has no copy of its own of {tech} to lose");
                    }
                    true
                }
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
            "actor_modify_morale" => match (self.thing(p.first()), self.number(p.get(1))) {
                (Some(t), Some(by)) => self.modify_morale(t, by),
                _ => false,
            },
            "actor_gains_experience" => match (self.thing(p.first()), self.number(p.get(1))) {
                (Some(t), Some(amount)) => {
                    // Only a regiment itself takes it, though naming
                    // anything is success.
                    if let Thing::Organization(r) = t {
                        self.gain_experience(r, amount);
                    }
                    true
                }
                _ => false,
            },
            "global_set_sun_colors" => match name(p.first()) {
                Some(sun) if !sun.is_empty() => {
                    self.triggers.suns.push(SunChange {
                        sun: Id::from(sun),
                        at: self.ticks,
                        seconds: self.number(p.get(1)).unwrap_or(0.0).max(0.0),
                    });
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Adds `by` to the morale of the regiment `t` is or is in.
    fn modify_morale(&mut self, t: Thing, by: f32) -> bool {
        let regiment = self.regiment_of(t);
        let has = |r: usize| self.organizations[r].morale_max > 0.0 && self.organization_on_map(r);
        let Some(r) = regiment.filter(|&r| has(r)) else {
            return false;
        };
        if by > 0.0 {
            self.gain_morale(r, by);
        } else if by < 0.0 {
            self.lose_morale(r, -by, false);
        }
        by != 0.0
    }

    /// The thing a parameter names, by its id or its name, while it's on
    /// the map.
    fn thing(&self, p: Option<&Parameter>) -> Option<Thing> {
        let p = p?;
        match (p.source.as_str(), &p.value) {
            ("mi_actor_id", Value::Int(id)) => self.by_id(*id),
            // The one that came into being last.
            ("mi_actor_named", Value::String(n)) => (self.triggers.names.iter().rev())
                .filter(|(name, _)| name.eq_ignore_ascii_case(n))
                .find_map(|&(_, id)| self.by_id(id)),
            _ => None,
        }
    }

    /// The thing on the map with the map id `id`.
    fn by_id(&self, id: u32) -> Option<Thing> {
        let organization = (0..self.organizations.len())
            .find(|&r| self.organizations[r].id == id && self.organization_on_map(r));
        let unit =
            || (0..self.units.len()).find(|&u| self.units[u].id == id && self.unit_on_map(u));
        let building = || {
            (0..self.buildings.len())
                .find(|&b| self.buildings[b].id == id && self.building_on_map(b))
        };
        (organization.map(Thing::Organization))
            .or_else(|| unit().map(Thing::Unit))
            .or_else(|| building().map(Thing::Building))
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

/// The technology a parameter names.
fn technology(p: Option<&Parameter>) -> Option<Id> {
    match p.map(|p| (p.source.as_str(), &p.value))? {
        ("mi_technology", Value::String(n)) if !n.is_empty() => Some(Id::from(n.as_str())),
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
    use crate::experience::regiment_tests::veterans;
    use crate::heroes::tests::pool;
    use crate::testing::run;
    use crate::{Notice, Outcome, World};
    use tgmap::Id;

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
    fn triggers_give_and_take_technologies_one_grant_at_a_time() {
        let radar = || {
            parameter(
                "ma_technology",
                "mi_technology",
                Value::String("radar".into()),
            )
        };
        let all = parameter("ma_kingdom", "mi_kingdom_all", Value::Int(0));
        let gains = |k: Parameter| element("kingdom_gains_technology", vec![k, radar()]);
        let loses = |k: Parameter| element("kingdom_loses_technology", vec![k, radar()]);
        let has = |k: u32| element("kingdom_has_technology", vec![kingdom(k), radar()]);
        let start = trigger(
            1,
            true,
            false,
            Vec::new(),
            vec![gains(kingdom(1)), gains(all)],
        );
        // Each loses one grant: player 1 keeps its other, and player 2's
        // start technology is never taken.
        let once = vec![
            loses(kingdom(1)),
            loses(kingdom(2)),
            loses(kingdom(2)),
            incr("n"),
        ];
        let lose = trigger(2, true, false, vec![has(1), has(2)], once);
        let last = trigger(3, true, false, vec![after(1)], vec![loses(kingdom(1))]);
        let mut w = with(pool(), vec![start, lose, last], &[("n", 0.0)]);
        w.grant(2, Id::from("radar"));
        w.sides.players[2].barred.push(Id::from("radar"));
        w.tick();
        assert_eq!(count(&w, "n"), 1.0);
        assert!(w.holds(1, &Id::from("radar")) && w.holds(2, &Id::from("radar")));
        assert!(!w.holds(3, &Id::from("radar")), "barred");
        let told = |w: &World, lost: bool| {
            (w.notices.iter())
                .filter(|n| match n {
                    Notice::Gained { player: 1, .. } => !lost,
                    Notice::TechnologyLost { player: 1, .. } => lost,
                    Notice::Gained { .. } | Notice::TechnologyLost { .. } => panic!("{n:?}"),
                    _ => false,
                })
                .count()
        };
        assert_eq!((told(&w, false), told(&w, true)), (1, 0));
        run(&mut w, 1.0);
        assert!(!w.holds(1, &Id::from("radar")));
        assert_eq!(told(&w, true), 1);
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
    fn a_trigger_turns_the_sun_to_a_definition_over_its_seconds() {
        let sun = parameter("ma_sun", "mi_sun", Value::String("sun_night".into()));
        let set = element("global_set_sun_colors", vec![sun, int(3600)]);
        let mut w = with(
            pool(),
            vec![trigger(1, true, false, vec![after(1)], vec![set])],
            &[],
        );
        run(&mut w, 0.5);
        assert!(w.triggers.suns.is_empty());
        run(&mut w, 1.0);
        let [change] = w.triggers.suns.as_slice() else {
            panic!("one sun change, not {:?}", w.triggers.suns);
        };
        assert_eq!(change.sun, "sun_night".into());
        assert_eq!(change.seconds, 3600.0);
        assert!(change.at > 0 && change.at <= w.ticks);
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

    fn actor_id(id: u32) -> Parameter {
        parameter("ma_actor", "mi_actor_id", Value::Int(id))
    }

    fn fixed(v: f32) -> Parameter {
        parameter("ma_value", "mi_fixed", Value::Float(v))
    }

    /// The veterans' world with a map init trigger running `effects`.
    fn running(effects: Vec<Element>) -> World {
        with(
            veterans(),
            vec![trigger(1, true, false, Vec::new(), effects)],
            &[],
        )
    }

    fn tech(id: &str) -> Parameter {
        parameter("ma_technology", "mi_technology", Value::String(id.into()))
    }

    #[test]
    fn things_given_a_technology_hold_their_own_copies_of_it() {
        let gains = |a: u32, t: &str| element("actor_gains_technology", vec![actor_id(a), tech(t)]);
        let loses = |a: u32, t: &str| element("actor_loses_technology", vec![actor_id(a), tech(t)]);
        let mut w = with(
            veterans(),
            vec![
                trigger(
                    1,
                    true,
                    false,
                    Vec::new(),
                    vec![gains(1, "radar"), gains(1, "radar"), gains(4, "armoured")],
                ),
                trigger(2, true, false, vec![after(1)], vec![loses(1, "radar")]),
            ],
            &[],
        );
        let templates: Vec<Id> = (w.organizations.iter().map(|o| o.template.clone()))
            .chain(w.units.iter().map(|u| u.template.clone()))
            .collect();
        for t in templates {
            w.rules.kinds.insert(t, tgmap::Kind::default());
        }
        (w.rules.property_requires).insert(Id::from("armoured"), vec![Id::from("tank")]);
        // Each property bears once on a member, its regiment's copies and
        // its side's grant together.
        let bears = |w: &World, u: usize| {
            let table: std::collections::HashMap<Id, Vec<Id>> = [
                (Id::from("radar"), Vec::new()),
                (Id::from("armoured"), Vec::new()),
            ]
            .into();
            (w.borne(&table, |e| e, crate::Target::Unit(u))).count()
        };
        let unit = w.organizations[0].units[0];
        w.gain_taken_back(1, Id::from("radar"));
        w.tick();
        let copies = |w: &World| {
            (w.organizations[0].properties.iter())
                .filter(|h| h.property.as_str() == "radar")
                .count()
        };
        assert_eq!(copies(&w), 2);
        assert_eq!(bears(&w, unit), 1);
        // What a thing's kind doesn't meet the needs of isn't given.
        assert!(w.organizations[1].properties.is_empty());
        assert!(w.units.iter().all(|u| u.properties.is_empty()));
        // Its side losing it leaves its own, and losing one of those the
        // other.
        w.lose_given(1, &Id::from("radar"));
        run(&mut w, 1.0);
        assert_eq!(copies(&w), 1);
        assert_eq!(bears(&w, unit), 1);
        // A member holds its own, and a building loses what it researched.
        let member = crate::targets::Thing::Unit(unit);
        w.gain_own(member, Id::from("radar"));
        assert_eq!(w.units[unit].properties.len(), 1);
        assert!(w.lose_own(member, &Id::from("radar")));
        assert!(!w.lose_own(member, &Id::from("radar")));
        // A property its template lists, its side holds and it has a copy
        // of fights once.
        let template = w.units[unit].template.clone();
        (w.rules.kinds.get_mut(&template).unwrap().properties).push(Id::from("radar"));
        let sharp = tgmap::CombatEffect {
            attack: 5.0,
            ..Default::default()
        };
        w.rules.combat_effects.insert(Id::from("radar"), sharp);
        w.gain_taken_back(1, Id::from("radar"));
        w.gain_own(member, Id::from("radar"));
        let held = w.timed_on(crate::Target::Unit(unit));
        assert_eq!(w.held_effect(1, &template, held, []).attack, 5.0);
        // Nothing's given to what has no properties.
        w.rules.kinds.clear();
        w.gain_own(crate::targets::Thing::Organization(0), Id::from("radar"));
        assert_eq!(copies(&w), 1);
    }

    #[test]
    fn morale_goes_to_the_regiment_named_or_one_of_its_units() {
        let morale = |w: &World, r: usize| w.organizations[r].morale;
        let mut w = running(vec![
            element("actor_modify_morale", vec![actor_id(1), fixed(-3.0)]),
            element("actor_modify_morale", vec![actor_id(4), fixed(-2.0)]),
        ]);
        let [a, b] = [morale(&w, 0), morale(&w, 1)];
        w.tick();
        assert!((morale(&w, 0) - (a - 3.0)).abs() < 1e-4);
        assert!((morale(&w, 1) - (b - 2.0)).abs() < 1e-4);
        // Never past its most.
        let mut w = running(vec![element(
            "actor_modify_morale",
            vec![actor_id(2), fixed(1000.0)],
        )]);
        w.organizations[0].morale = 1.0;
        w.tick();
        assert_eq!(morale(&w, 0), w.organizations[0].morale_max);
    }

    #[test]
    fn a_trigger_taking_morale_can_rout_a_regiment() {
        // To just above nothing, past its breakpoint, mid-fight or not.
        let morale = veterans().organizations[0].morale;
        let mut w = running(vec![element(
            "actor_modify_morale",
            vec![actor_id(1), fixed(0.01 - morale)],
        )]);
        w.rules.morale_breakpoints.insert("squad".into(), 1.0);
        w.tick();
        assert!(w.organizations[0].routing());
    }

    #[test]
    fn the_dead_are_found_until_their_remains_are_taken_off() {
        let mut w = running(vec![
            element("actor_gains_experience", vec![actor_id(1), fixed(12.0)]),
            element("actor_modify_morale", vec![actor_id(2), fixed(-3.0)]),
        ]);
        // Its unit's remains lie a second; the regiment has none.
        w.rules.remains.insert("soldier".into(), [0.5, 0.5]);
        w.units[0].died = Some(0);
        let was = w.organizations[0].morale;
        w.tick();
        assert_eq!(w.organizations[0].experience, 0.0);
        assert_eq!(w.organizations[0].morale, was);
        assert_eq!(
            w.thing(Some(&actor_id(2))),
            Some(crate::targets::Thing::Unit(0))
        );
        w.ticks = crate::SECOND;
        assert_eq!(w.thing(Some(&actor_id(2))), None);
    }

    #[test]
    fn experience_goes_only_to_a_regiment_itself() {
        let mut w = running(vec![
            element("actor_gains_experience", vec![actor_id(1), fixed(12.0)]),
            element("actor_gains_experience", vec![actor_id(4), fixed(12.0)]),
            element("actor_gains_experience", vec![actor_id(1), fixed(-5.0)]),
        ]);
        w.lead(1, 0.0, Some(5000.0), Vec::new());
        w.tick();
        assert!((w.organizations[0].experience - 12.0).abs() < 1e-4);
        assert_eq!(w.organizations[0].level, 1);
        assert_eq!(w.organizations[1].experience, 0.0);
        // The side takes its share.
        assert!((w.experience(1).unwrap() - 12.0).abs() < 1e-4);
    }

    #[test]
    fn a_named_thing_is_found_only_while_it_lives() {
        let named = parameter("ma_actor", "mi_actor_named", Value::String("Ivan".into()));
        let mut w = running(vec![element(
            "actor_gains_experience",
            vec![named, fixed(12.0)],
        )]);
        w.triggers.names = vec![("ivan".to_string(), 3)].into();
        let mut dead = w.clone();
        w.tick();
        assert!((w.organizations[1].experience - 12.0).abs() < 1e-4);
        dead.units[1].died = Some(0);
        dead.tick();
        assert_eq!(dead.organizations[1].experience, 0.0);
    }
}
