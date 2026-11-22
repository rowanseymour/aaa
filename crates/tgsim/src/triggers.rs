//! A map's triggers: tested every tick in the map's order, each running
//! its effects when its events and conditions allow.
//!
//! An event, condition or effect not handled here never holds, or fails.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use tgmap::Id;
use tgmap::triggers::{Element, Parameter, Trigger, Value};

use crate::targets::Thing;
use crate::{Notice, Piece, Refused, SECOND, Standing, State, TICK, World};

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
    /// The countdown a trigger has put on screen, for every side to see.
    pub counter: Option<Counter>,
    /// The objectives the map defines, for its triggers to give.
    pub(crate) objectives: Arc<[tgmap::Objective]>,
    /// The kingdoms an event's parameter and a condition's last picked,
    /// which later ones can name as `mi_kingdom_event` and
    /// `mi_kingdom_condition`, kept from one trigger and tick to the next.
    pub event_kingdom: Option<u32>,
    pub condition_kingdom: Option<u32>,
    /// What's happened since the last pass that the world doesn't keep.
    pub(crate) happened: Vec<Happening>,
    /// The interface commands the map listens for, in lower case.
    pub listening: BTreeSet<String>,
    /// What each side's player has selected, as their orders last said.
    pub selected: BTreeMap<u32, Vec<Piece>>,
    /// Whether any trigger tests what's selected, which players need only
    /// say then.
    pub watches_selection: bool,
    /// The thing the trigger being run is run for, and the one a condition
    /// last picked, while it's tested.
    event_thing: Option<Thing>,
    condition_thing: Option<Thing>,
}

/// Something events hear of that the world keeps nothing of.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Happening {
    /// A side placed a building site of the template.
    Begun { player: u32, template: Id },
    /// A thing of the template was finished for a side: a site completed
    /// or a packed building unpacked.
    Finished { player: u32, template: Id },
    /// A side recruited something of the template.
    Recruited { player: u32, template: Id },
    /// A side took a building, by index into [`World::buildings`]: by
    /// damage, or a city by holding its zone.
    Captured { building: usize, player: u32 },
    /// A player ran an interface command the map listens for, in lower
    /// case.
    Heard { command: String },
}

/// A thing that died since the last pass, with what last damaged it.
struct Death {
    dead: Thing,
    killer: Option<Thing>,
}

/// A countdown from `seconds` of game time, shown at `at`. Reaching 0
/// does nothing but stop it there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Counter {
    /// The tick it was shown.
    pub at: u64,
    pub seconds: f32,
}

crate::hash::digest!(Counter { at, seconds });

impl Counter {
    /// The seconds left at `ticks` into the match, which may fall between
    /// two. Counted in ticks, so whole seconds don't round short.
    pub fn left(&self, ticks: f32) -> f32 {
        (self.seconds - (ticks - self.at as f32) * TICK).max(0.0)
    }
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
            let elements = (trigger.events.iter())
                .chain(&trigger.conditions)
                .chain(&trigger.effects);
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
            counter: None,
            objectives: Arc::default(),
            event_kingdom: None,
            condition_kingdom: None,
            happened: Vec::new(),
            listening: BTreeSet::new(),
            selected: BTreeMap::new(),
            watches_selection: (t.triggers.iter())
                .flat_map(|t| &t.conditions)
                .any(|c| SELECTION.contains(&c.name.as_str())),
            event_thing: None,
            condition_thing: None,
        }
    }
}

impl crate::hash::Digest for Triggers {
    fn digest(&self, h: &mut crate::hash::Hasher) {
        // The list and names are fixed by the map.
        self.switches.digest(h);
        self.variables.digest(h);
        self.suns.digest(h);
        self.counter.digest(h);
        self.event_kingdom.digest(h);
        self.condition_kingdom.digest(h);
        self.listening.digest(h);
        self.selected.digest(h);
    }
}

/// The conditions that test what a player has selected.
const SELECTION: &[&str] = &[
    "actor_selected_by_kingdom",
    "actor_data_selected_by_kingdom",
];

/// The events, conditions and effects carried out.
const HANDLED: &[&str] = &[
    "event_script_heard",
    "effect_ui_script_listen",
    "actor_selected_by_kingdom",
    "actor_data_selected_by_kingdom",
    "actor_captured_by_kingdom",
    "actor_destroys_actor",
    "kingdom_begins_construction_on_actor",
    "kingdom_finishes_construction_on_actor",
    "kingdom_recruits_actor",
    "actor_is_dead",
    "actor_owned_by_kingdom",
    "actor_comparison_life",
    "actor_comparison_morale",
    "actor_has_state",
    "actor_does_not_have_state",
    "actor_visible_to_kingdom",
    "difficulty_compare_difficulty",
    "objective_succeeded",
    "objective_failed",
    "kingdom_default_lose",
    "kingdom_has_no_settlements",
    "kingdom_mission_lost",
    "kingdom_enable_default_victory_condition",
    "kingdom_gains_value_primary_resource",
    "kingdom_gains_objective",
    "kingdom_loses_objective",
    "objective_complete_for_kingdom",
    "objective_failed_for_kingdom",
    "objective_incomplete_for_kingdom",
    "object_destroyed",
    "object_removed",
    "actor_transfer_control_to_kingdom",
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
    "game_counter_show",
    "game_counter_hide",
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
    /// Refused for an interface command the map doesn't listen for, so
    /// nothing else is kept.
    pub(crate) fn may_hear(&self, command: &str) -> Result<(), Refused> {
        match self.triggers.listening.contains(&command.to_lowercase()) {
            true => Ok(()),
            false => Err(Refused::Unable),
        }
    }

    /// Has the triggers hear a player ran an interface command the map
    /// listens for.
    pub(crate) fn hear(&mut self, command: &str) -> Result<(), Refused> {
        self.may_hear(command)?;
        let command = command.to_lowercase();
        self.happen(Happening::Heard { command });
        Ok(())
    }

    /// Refused for a selection naming what the match doesn't have.
    pub(crate) fn may_select(&self, pieces: &[Piece]) -> Result<(), Refused> {
        let there = |p: &Piece| match *p {
            Piece::Organization(r) => r < self.organizations.len(),
            Piece::Building(b) => b < self.buildings.len(),
        };
        match pieces.iter().all(there) {
            true => Ok(()),
            false => Err(Refused::Unable),
        }
    }

    /// Keeps something that's happened for the triggers' next pass.
    pub(crate) fn happen(&mut self, happening: Happening) {
        self.triggers.happened.push(happening);
    }

    /// Tests each active trigger in the map's order, running those that go
    /// ahead, then forgets what its events heard of.
    pub(crate) fn run_triggers(&mut self) {
        let deaths = self.deaths();
        for t in 0..self.triggers.list.len() {
            if self.triggers.switches[t].active {
                self.test(t, &deaths);
            }
        }
        self.triggers.happened.clear();
    }

    /// Tests the trigger at `t`, running it once for each of its event
    /// things, or once if it has none, until a run goes ahead if it doesn't
    /// repeat.
    fn test(&mut self, t: usize, deaths: &[Death]) {
        let list = Arc::clone(&self.triggers.list);
        let trigger = &list[t];
        let mut runs = vec![None];
        if !trigger.events.is_empty() {
            // Every event is heard, to gather all their things.
            let heard: Vec<_> = (trigger.events.iter())
                .map(|e| self.heard(e, deaths))
                .collect();
            if heard.iter().all(Option::is_none) {
                return;
            }
            let things: Vec<Thing> = heard.into_iter().flatten().flatten().collect();
            if !things.is_empty() {
                runs = things.into_iter().map(Some).collect();
            }
        }
        let mut ran = false;
        for thing in runs {
            self.triggers.event_thing = thing;
            self.triggers.condition_thing = None;
            let held = trigger
                .conditions
                .iter()
                .map(|c| self.condition_holds(t, c));
            let ahead = match trigger.all_conditions {
                // Every condition is tested, for the things they pick.
                true => held.fold(true, |all, h| all & h),
                false => trigger.conditions.is_empty() | held.fold(false, |any, h| any | h),
            };
            if !ahead {
                continue;
            }
            // Effects that fail do nothing, and the rest still run.
            for effect in &trigger.effects {
                self.carry_out(effect);
            }
            log::debug!("trigger {} ran", trigger.name);
            ran = true;
            if !trigger.repeats {
                break;
            }
        }
        self.triggers.event_thing = None;
        self.triggers.condition_thing = None;
        if !ran {
            return;
        }
        let ticks = self.ticks;
        let switch = &mut self.triggers.switches[t];
        match trigger.repeats {
            true => switch.activated = ticks,
            false => switch.active = false,
        }
    }

    /// The things that died since the last pass: on the tick before it, or
    /// on that pass itself, by a trigger.
    fn deaths(&self) -> Vec<Death> {
        let Some(at) = self.ticks.checked_sub(1) else {
            return Vec::new();
        };
        let from = |struck: Option<crate::Struck>| struck?.from.map(Thing::from);
        let units = (0..self.units.len())
            .filter(|&u| self.units[u].died == Some(at))
            .map(|u| Death {
                dead: Thing::Unit(u),
                killer: from(self.units[u].struck),
            });
        let organizations = (0..self.organizations.len())
            .filter(|&r| {
                let members = self.organizations[r].units.iter();
                let last = members.filter_map(|&u| self.units[u].died).max();
                last == Some(at) && self.destroyed(r)
            })
            .map(|r| Death {
                dead: Thing::Organization(r),
                killer: from(self.organizations[r].struck),
            });
        let buildings = (0..self.buildings.len())
            .filter(|&b| self.buildings[b].state == State::Destroyed(at))
            .map(|b| Death {
                dead: Thing::Building(b),
                killer: from(self.buildings[b].struck),
            });
        units.chain(organizations).chain(buildings).collect()
    }

    /// Whether the event `e` happened since the last pass and, if so, the
    /// things it was run for.
    fn heard(&mut self, e: &Element, deaths: &[Death]) -> Option<Vec<Thing>> {
        let p = &e.parameters;
        let made = |made: fn(&Happening) -> Option<(u32, &Id)>| {
            move |w: &mut World| {
                let k = w.pick_kingdom(p.first(), Role::Event)?;
                let template = template(p.get(1))?;
                let matched = |h: &Happening| {
                    made(h).is_some_and(|(player, t)| {
                        player == k && template.as_ref().is_none_or(|n| n == t)
                    })
                };
                w.triggers.happened.iter().any(matched).then(Vec::new)
            }
        };
        match e.name.as_str() {
            "actor_captured_by_kingdom" => {
                let thing = self.thing(p.first());
                let k = self.pick_kingdom(p.get(1), Role::Event)?;
                let Some(Thing::Building(b)) = thing else {
                    return None;
                };
                // Only its latest capture counts.
                let latest = (self.triggers.happened.iter().rev()).find_map(|h| match *h {
                    Happening::Captured { building, player } if building == b => Some(player),
                    _ => None,
                });
                (latest == Some(k)).then(|| vec![Thing::Building(b)])
            }
            "actor_destroys_actor" => self.destroys(p, deaths),
            "event_script_heard" => {
                let command = name(p.first())?.to_lowercase();
                let heard =
                    |h: &Happening| matches!(h, Happening::Heard { command: c } if *c == command);
                self.triggers.happened.iter().any(heard).then(Vec::new)
            }
            "kingdom_begins_construction_on_actor" => made(|h| match h {
                Happening::Begun { player, template } => Some((*player, template)),
                _ => None,
            })(self),
            "kingdom_finishes_construction_on_actor" => made(|h| match h {
                Happening::Finished { player, template } => Some((*player, template)),
                _ => None,
            })(self),
            "kingdom_recruits_actor" => made(|h| match h {
                Happening::Recruited { player, template } => Some((*player, template)),
                _ => None,
            })(self),
            _ => None,
        }
    }

    /// Whether `actor_destroys_actor`, with the killer and the destroyed
    /// as `p` names them, happened since the last pass, and its things.
    fn destroys(&self, p: &[Parameter], deaths: &[Death]) -> Option<Vec<Thing>> {
        let any = |p: Option<&Parameter>| p.is_some_and(|p| p.source == "mi_actor_any");
        let (killer, destroyed) = (p.first(), p.get(1));
        let things: Vec<Thing> = match (any(killer), any(destroyed)) {
            (true, true) => (deaths.iter())
                .flat_map(|d| d.killer.into_iter().chain([d.dead]))
                .collect(),
            (true, false) => {
                let named = self.thing(destroyed)?;
                let death = deaths.iter().find(|d| d.dead == named)?;
                vec![death.killer?]
            }
            (false, true) => {
                // Credited to exactly that thing.
                let named = self.thing(killer)?;
                let killed = deaths.iter().any(|d| d.killer == Some(named));
                killed.then(|| vec![named])?
            }
            (false, false) => {
                let (named, by) = (self.thing(destroyed)?, self.thing(killer)?);
                let death = deaths.iter().find(|d| d.dead == named)?;
                let unit_of = |k: Thing| match (by, k) {
                    (Thing::Organization(r), Thing::Unit(u)) => self.units[u].organization == r,
                    _ => false,
                };
                let credited = death.killer.is_some_and(|k| k == by || unit_of(k));
                credited.then(|| vec![by])?
            }
        };
        (!things.is_empty()).then_some(things)
    }

    /// Whether the condition `c` of the trigger at `t` holds.
    fn condition_holds(&mut self, t: usize, c: &Element) -> bool {
        let p = &c.parameters;
        let thing = |w: &mut World| w.pick_thing(p.first());
        let kingdom = |w: &mut World, i: usize| w.pick_kingdom(p.get(i), Role::Condition);
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
            "kingdom_has_technology" => match (kingdom(self, 0), technology(p.get(1))) {
                (Some(k), Some(t)) => self.holds(k, &t),
                _ => false,
            },
            "kingdom_has_compare_value_primary_resource" => {
                match (kingdom(self, 0), self.number(p.get(2))) {
                    (Some(k), Some(v)) => {
                        compare(comparison(p.get(1)), self.stockpile(k).round(), v)
                    }
                    _ => false,
                }
            }
            // Naming nothing counts as dead.
            "actor_is_dead" => thing(self).is_none_or(|t| self.dying(t)),
            "actor_owned_by_kingdom" => match (thing(self), kingdom(self, 1)) {
                (Some(t), Some(k)) => self.thing_owner(t) == k,
                _ => false,
            },
            "actor_comparison_life" | "actor_comparison_morale" => {
                let life = c.name == "actor_comparison_life";
                let Some(t) = thing(self) else {
                    return false;
                };
                let share = match life {
                    true => self.life(t),
                    false => self.morale_share(t),
                };
                match (share, self.number(p.get(2))) {
                    (Some(share), Some(v)) => compare(comparison(p.get(1)), share * 100.0, v),
                    _ => false,
                }
            }
            "actor_has_state" | "actor_does_not_have_state" => {
                let has = c.name == "actor_has_state";
                let state = match p.get(1).map(|p| &p.value) {
                    Some(Value::Int(n)) => *n,
                    _ => return false,
                };
                // Neither holds of a thing that isn't there.
                thing(self).is_some_and(|t| self.in_trigger_state(t, state) == has)
            }
            "actor_visible_to_kingdom" => match (thing(self), kingdom(self, 1)) {
                (Some(t), Some(k)) => self.sees_thing(k, t),
                _ => false,
            },
            "difficulty_compare_difficulty" => {
                match (self.difficulty(p.first()), self.difficulty(p.get(2))) {
                    (Some(a), Some(b)) => compare(comparison(p.get(1)), a as f32, b as f32),
                    _ => false,
                }
            }
            "objective_succeeded" | "objective_failed" => {
                let standing = match c.name == "objective_succeeded" {
                    true => Standing::Completed,
                    false => Standing::Failed,
                };
                let k = kingdom(self, 1);
                match (name(p.first()), k) {
                    (Some(n), Some(k)) => {
                        (self.objectives.get(&k)).is_some_and(|o| o.stands(n, standing))
                    }
                    _ => false,
                }
            }
            "actor_selected_by_kingdom" => match (thing(self), kingdom(self, 1)) {
                (Some(t), Some(k)) => {
                    let piece = match t {
                        Thing::Organization(r) => Piece::Organization(r),
                        Thing::Building(b) => Piece::Building(b),
                        Thing::Unit(_) => return false,
                    };
                    self.selection(k).contains(&piece)
                }
                _ => false,
            },
            "actor_data_selected_by_kingdom" => match (template(p.first()), kingdom(self, 1)) {
                (Some(Some(template)), Some(k)) => self.selection(k).iter().any(|&p| {
                    let of = match p {
                        Piece::Organization(r) => &self.organizations[r].template,
                        Piece::Building(b) => &self.buildings[b].template,
                    };
                    *of == template
                }),
                _ => false,
            },
            "kingdom_default_lose" => kingdom(self, 0).is_some_and(|k| self.has_nothing_left(k)),
            "kingdom_has_no_settlements" => {
                kingdom(self, 0).is_some_and(|k| !self.settlements.iter().any(|s| s.owner == k))
            }
            _ => false,
        }
    }

    /// What the player of side `k` has selected.
    fn selection(&self, k: u32) -> &[Piece] {
        self.triggers.selected.get(&k).map_or(&[], Vec::as_slice)
    }

    /// Whether `t` is dying or has died.
    fn dying(&self, t: Thing) -> bool {
        match t {
            Thing::Unit(u) => self.units[u].died.is_some(),
            Thing::Organization(r) => self.destroyed(r),
            Thing::Building(b) => matches!(self.buildings[b].state, State::Destroyed(_)),
        }
    }

    /// The share of its most health `t` has; none for one with no health
    /// of its own, as a regiment has.
    fn life(&self, t: Thing) -> Option<f32> {
        let (health, most) = match t {
            Thing::Unit(u) => (self.units[u].health, self.max_health(u)),
            Thing::Building(b) => (self.buildings[b].health, self.building_max_health(b)),
            Thing::Organization(_) => return None,
        };
        (most > 0.0).then(|| health.max(0.0) / most)
    }

    /// The share of its most morale a regiment has; none for anything else.
    fn morale_share(&self, t: Thing) -> Option<f32> {
        let Thing::Organization(r) = t else {
            return None;
        };
        let organization = &self.organizations[r];
        (organization.morale_max > 0.0).then(|| organization.morale / organization.morale_max)
    }

    /// Whether `t` is itself in the state the game numbers `state`.
    fn in_trigger_state(&self, t: Thing, state: u32) -> bool {
        let Some(&named) = STATES.get(state as usize) else {
            return false;
        };
        match (t, named) {
            (_, "dead") => self.dying(t),
            (Thing::Organization(r), _) if named.starts_with("mind_") => {
                !self.destroyed(r) && self.activity(r) == named
            }
            (Thing::Organization(r), "out_of_supply") => !self.organizations[r].in_supply,
            (Thing::Organization(r), "hq_attached") => self.organizations[r].hq.is_some(),
            (Thing::Organization(r), "organization_entrenched") => {
                self.organizations[r].entrenched.is_some()
            }
            (Thing::Organization(r), "unpacked") => self.organizations[r].unpacked_into.is_some(),
            (Thing::Building(b), "construction") => self.buildings[b].constructing,
            (Thing::Building(b), "structure_normal") => {
                let building = &self.buildings[b];
                building.standing() && !building.constructing
            }
            _ => {
                log::debug!("trigger state {named} isn't told of {t:?}");
                false
            }
        }
    }

    /// Whether `player` sees `t`: a regiment, or one of its units, when it
    /// sees any of the regiment.
    fn sees_thing(&self, player: u32, t: Thing) -> bool {
        match t {
            Thing::Building(b) => self.sees_building(player, b),
            Thing::Unit(_) | Thing::Organization(_) => self.regiment_of(t).is_some_and(|r| {
                let units = &self.organizations[r].units;
                units.iter().any(|&u| self.sees_unit(player, u))
            }),
        }
    }

    /// The place among the difficulty levels of the one a parameter gives:
    /// the match's, or one it names.
    fn difficulty(&self, p: Option<&Parameter>) -> Option<usize> {
        let p = p?;
        let level = match (p.source.as_str(), &p.value) {
            ("mi_difficulty_level_map", _) => self.rules.difficulty.clone()?,
            ("mi_difficulty_level_specified", Value::String(n)) => Id::from(n.as_str()),
            _ => return None,
        };
        self.rules.difficulties.iter().position(|d| *d == level)
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
            "game_counter_show" => match self.number(p.first()) {
                Some(seconds) if seconds > 0.0 => {
                    let at = self.ticks;
                    self.triggers.counter = Some(Counter { at, seconds });
                    true
                }
                _ => false,
            },
            "game_counter_hide" => {
                self.triggers.counter = None;
                true
            }
            "kingdom_mission_lost" => match self.kingdom(p.first()) {
                Some(k) => {
                    self.lose(k);
                    true
                }
                None => false,
            },
            "kingdom_enable_default_victory_condition" => {
                match (self.kingdom(p.first()), p.get(1).map(|p| &p.value)) {
                    (Some(k), Some(Value::Int(on))) => {
                        self.set_default_defeat(k, *on != 0);
                        true
                    }
                    _ => false,
                }
            }
            "kingdom_gains_value_primary_resource" => {
                match (self.kingdom(p.first()), self.number(p.get(1))) {
                    (Some(k), Some(v)) => {
                        let had = self.stockpile(k);
                        self.set_stockpile(k, had + v);
                        true
                    }
                    _ => false,
                }
            }
            "kingdom_gains_objective" => match (self.kingdom(p.first()), name(p.get(1))) {
                (Some(k), Some(n)) => self.give_objective(k, n),
                _ => false,
            },
            "kingdom_loses_objective" => match (self.kingdom(p.first()), name(p.get(1))) {
                (Some(k), Some(n)) => self.take_objective(k, n),
                _ => false,
            },
            "objective_complete_for_kingdom"
            | "objective_failed_for_kingdom"
            | "objective_incomplete_for_kingdom" => {
                let standing = match e.name.as_str() {
                    "objective_complete_for_kingdom" => Standing::Completed,
                    "objective_failed_for_kingdom" => Standing::Failed,
                    _ => Standing::Active,
                };
                match (name(p.first()), self.kingdom(p.get(1))) {
                    (Some(n), Some(k)) => self.settle_objective(k, n, standing),
                    _ => false,
                }
            }
            "effect_ui_script_listen" => match (name(p.first()), p.get(1).map(|p| &p.value)) {
                (Some(command), Some(Value::Int(on))) if !command.is_empty() => {
                    let command = command.to_lowercase();
                    match *on != 0 {
                        true => self.triggers.listening.insert(command),
                        false => self.triggers.listening.remove(&command),
                    };
                    true
                }
                _ => false,
            },
            "actor_transfer_control_to_kingdom" => {
                match (self.thing(p.first()), self.kingdom(p.get(1))) {
                    (Some(t), Some(k)) => self.transfer(t, k),
                    _ => false,
                }
            }
            "object_destroyed" => match self.thing(p.first()) {
                Some(t) => self.destroy_thing(t),
                None => false,
            },
            "object_removed" => match self.thing(p.first()) {
                Some(t) => self.remove_thing(t),
                None => false,
            },
            _ => false,
        }
    }

    /// Gives a side the map's objective `name`, active.
    fn give_objective(&mut self, player: u32, name: &str) -> bool {
        let defined = self.triggers.objectives.iter().find(|o| o.name == name);
        let Some(objective) = defined.cloned() else {
            return false;
        };
        let sorted = self.rules.sort_objectives;
        let held = self.objectives.entry(player).or_default();
        held.give(&objective.name, &objective.text, objective.primary, sorted);
        self.notices.push(Notice::Objective {
            player,
            text: objective.text,
            primary: objective.primary,
            standing: Some(Standing::Active),
        });
        true
    }

    /// Takes the objective `name` from a side that holds it.
    fn take_objective(&mut self, player: u32, name: &str) -> bool {
        let taken = self.objectives.get_mut(&player).and_then(|o| o.take(name));
        let Some(objective) = taken else {
            return false;
        };
        self.notices.push(Notice::Objective {
            player,
            text: objective.text,
            primary: objective.primary,
            standing: None,
        });
        true
    }

    /// Sets where an objective a side holds stands.
    fn settle_objective(&mut self, player: u32, name: &str, standing: Standing) -> bool {
        let held = self.objectives.get_mut(&player);
        let Some(objective) = held.and_then(|o| o.settle(name, standing)).cloned() else {
            return false;
        };
        self.notices.push(Notice::Objective {
            player,
            text: objective.text,
            primary: objective.primary,
            standing: Some(standing),
        });
        true
    }

    /// Hands `t` to side `k` as a capture would, though it isn't heard as
    /// one: a unit with its regiment.
    fn transfer(&mut self, t: Thing, k: u32) -> bool {
        match t {
            Thing::Building(b) => self.change_hands(b, k),
            Thing::Organization(_) | Thing::Unit(_) => match self.regiment_of(t) {
                Some(r) => self.change_side(r, k),
                None => return false,
            },
        }
        true
    }

    /// Has `t` die as if killed, crediting no one; false for one already
    /// dead.
    fn destroy_thing(&mut self, t: Thing) -> bool {
        if self.dying(t) {
            return false;
        }
        let ticks = self.ticks;
        match t {
            Thing::Unit(u) => {
                self.units[u].struck = None;
                self.units[u].die(ticks);
                self.mourn(u);
                self.lost_member(u);
            }
            Thing::Organization(r) => {
                self.organizations[r].struck = None;
                for u in self.organizations[r].units.clone() {
                    self.units[u].struck = None;
                }
                self.destroy(r);
            }
            // Brought down as in a fight, with no one to take it.
            Thing::Building(b) => {
                self.buildings[b].struck = None;
                self.fall(b, None);
            }
        }
        true
    }

    /// Takes `t` off the map at once, without its dying; false for one
    /// already gone.
    fn remove_thing(&mut self, t: Thing) -> bool {
        let ticks = self.ticks;
        let units = match t {
            Thing::Unit(u) => vec![u],
            Thing::Organization(r) => {
                if let Some(hq) = self.packed_hq(r) {
                    self.detach_all(hq);
                }
                let organization = &mut self.organizations[r];
                organization.engaged.clear();
                organization.guard = None;
                organization.units.clone()
            }
            Thing::Building(b) => {
                if !self.buildings[b].standing() && self.buildings[b].state != State::Disabled {
                    return false;
                }
                self.remove(b, State::Sold(ticks));
                return true;
            }
        };
        let mut removed = false;
        for u in units {
            let unit = &mut self.units[u];
            if unit.alive() {
                unit.left = Some(crate::Left::Disbanded(ticks));
                unit.besieged = None;
                unit.target = None;
                unit.halt();
                unit.speed = 0.0;
                removed = true;
            }
        }
        removed
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

    /// The thing a parameter names, by its id or its name while it's on
    /// the map, or as the trigger's event thing or condition thing.
    fn thing(&self, p: Option<&Parameter>) -> Option<Thing> {
        let p = p?;
        match (p.source.as_str(), &p.value) {
            ("mi_actor_event", _) => self.triggers.event_thing,
            ("mi_actor_condition", _) => self.triggers.condition_thing,
            ("mi_actor_id", Value::Int(id)) => self.by_id(*id),
            // The one that came into being last.
            ("mi_actor_named", Value::String(n)) => (self.triggers.names.iter().rev())
                .filter(|(name, _)| name.eq_ignore_ascii_case(n))
                .find_map(|&(_, id)| self.by_id(id)),
            _ => None,
        }
    }

    /// The thing a condition's parameter names, which becomes the
    /// condition thing.
    fn pick_thing(&mut self, p: Option<&Parameter>) -> Option<Thing> {
        let thing = self.thing(p);
        let named =
            p.is_some_and(|p| matches!(p.source.as_str(), "mi_actor_id" | "mi_actor_named"));
        if named && thing.is_some() {
            self.triggers.condition_thing = thing;
        }
        thing
    }

    /// The side a parameter names, which becomes the event or the condition
    /// kingdom when an event's or a condition's names it.
    fn pick_kingdom(&mut self, p: Option<&Parameter>, role: Role) -> Option<u32> {
        let k = self.kingdom(p);
        let named =
            p.is_some_and(|p| matches!(p.source.as_str(), "mi_kingdom_named" | "mi_kingdom_id"));
        if let (true, Some(k)) = (named, k) {
            match role {
                Role::Event => self.triggers.event_kingdom = Some(k),
                Role::Condition => self.triggers.condition_kingdom = Some(k),
            }
        }
        k
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
            ("mi_kingdom_event", _) => self.triggers.event_kingdom,
            ("mi_kingdom_condition", _) => self.triggers.condition_kingdom,
            _ => None,
        }
    }
}

/// What picked a parameter's kingdom, which decides what remembers it.
#[derive(Clone, Copy)]
enum Role {
    Event,
    Condition,
}

/// The states a trigger can test, by the game's numbering.
const STATES: &[&str] = &[
    "mind_attacking",
    "mind_bombard",
    "mind_building",
    "mind_chasing",
    "mind_engaged",
    "mind_exhausted",
    "mind_guarding",
    "mind_joining",
    "mind_moving",
    "mind_reattaching",
    "mind_regrouping",
    "mind_repairing",
    "mind_resting",
    "mind_retreating",
    "mind_routing",
    "mind_reforming",
    "mind_exploring",
    "mind_rampaging",
    "mind_razing",
    "out_of_supply",
    "under_siege",
    "construction",
    "organization_entrenched",
    "dead",
    "falling",
    "structure_packing",
    "structure_unpacking",
    "structure_normal",
    "structure_researching",
    "structure_upgrading",
    "structure_razing",
    "structure_recruiting",
    "active_construction",
    "summoning",
    "unpacked",
    "mind_packing",
    "mind_unpacking",
    "missing_primary",
    "unloading",
    "disabled",
    "targeted",
    "hq_attached",
];

/// The template a parameter names, or none for any.
fn template(p: Option<&Parameter>) -> Option<Option<Id>> {
    match p.map(|p| (p.source.as_str(), &p.value))? {
        ("mi_actor_data_named", Value::String(n)) => Some(Some(Id::from(n.as_str()))),
        ("mi_actor_data_any", _) => Some(None),
        _ => None,
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

    use super::{Happening, Triggers};
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
    fn a_trigger_shows_a_countdown_that_stops_at_zero_until_hidden() {
        let show = |seconds| element("game_counter_show", vec![int(seconds)]);
        let hide = element("game_counter_hide", vec![]);
        let mut w = with(
            pool(),
            vec![
                trigger(1, true, false, vec![], vec![show(0)]),
                trigger(2, true, false, vec![after(1)], vec![show(30)]),
                trigger(3, true, false, vec![after(60)], vec![hide]),
            ],
            &[],
        );
        run(&mut w, 0.5);
        assert_eq!(w.triggers.counter, None, "0 seconds fails");
        run(&mut w, 1.0);
        let counter = w.triggers.counter.expect("shown after a second");
        assert_eq!(counter.seconds, 30.0);
        let shown = counter.at as f32;
        assert_eq!(counter.left(shown), 30.0);
        assert_eq!(counter.left(shown + 100.0), 20.0);
        assert_eq!(counter.left(shown + 1000.0), 0.0);
        run(&mut w, 60.0);
        assert_eq!(w.triggers.counter, None);
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

    fn heard(id: u32, repeats: bool, events: Vec<Element>, effects: Vec<Element>) -> Trigger {
        Trigger {
            events,
            ..trigger(id, true, repeats, Vec::new(), effects)
        }
    }

    fn any() -> Parameter {
        parameter("ma_actor", "mi_actor_any", Value::Int(0))
    }

    fn event_thing() -> Parameter {
        parameter("ma_actor", "mi_actor_event", Value::Int(0))
    }

    fn template(id: &str) -> Parameter {
        parameter(
            "ma_actor_data",
            "mi_actor_data_named",
            Value::String(id.into()),
        )
    }

    fn objective(name: &str) -> Parameter {
        parameter("ma_objective", "mi_objective", Value::String(name.into()))
    }

    #[test]
    fn a_death_is_heard_once_and_runs_the_trigger_for_its_killer() {
        // Player 2's rifleman (id 4) killed by player 1's (unit 0).
        let killed = element("actor_destroys_actor", vec![any(), actor_id(4)]);
        let by_ours = element("actor_owned_by_kingdom", vec![event_thing(), kingdom(1)]);
        let trigger = Trigger {
            conditions: vec![by_ours],
            ..heard(1, true, vec![killed], vec![incr("kills")])
        };
        let mut w = with(veterans(), vec![trigger], &[("kills", 0.0)]);
        // Named while its remains lie.
        w.rules.remains.insert("soldier".into(), [0.5, 0.5]);
        w.tick();
        assert_eq!(count(&w, "kills"), 0.0);
        crate::testing::hit_by(&mut w, 1, 0);
        let ticks = w.ticks;
        w.units[1].die(ticks);
        w.tick();
        assert_eq!(count(&w, "kills"), 1.0);
        w.tick();
        assert_eq!(count(&w, "kills"), 1.0);
    }

    #[test]
    fn a_death_no_one_dealt_has_no_killer_to_hear_of() {
        let killed = element("actor_destroys_actor", vec![any(), actor_id(4)]);
        let all = element("actor_destroys_actor", vec![any(), any()]);
        let mut w = with(
            veterans(),
            vec![
                heard(1, true, vec![killed], vec![incr("named")]),
                heard(2, true, vec![all], vec![incr("any")]),
            ],
            &[("named", 0.0), ("any", 0.0)],
        );
        let ticks = w.ticks;
        w.units[1].die(ticks);
        w.tick();
        assert_eq!(count(&w, "named"), 0.0);
        // The unit and its regiment, each run for.
        assert_eq!(count(&w, "any"), 2.0);
    }

    #[test]
    fn a_named_killer_is_heard_of_for_its_kills() {
        let killer = element("actor_destroys_actor", vec![actor_id(2), any()]);
        let regiment = element("actor_destroys_actor", vec![actor_id(1), actor_id(4)]);
        let mut w = with(
            veterans(),
            vec![
                heard(1, true, vec![killer], vec![incr("unit")]),
                heard(2, true, vec![regiment], vec![incr("regiment")]),
            ],
            &[("unit", 0.0), ("regiment", 0.0)],
        );
        w.rules.remains.insert("soldier".into(), [0.5, 0.5]);
        crate::testing::hit_by(&mut w, 1, 0);
        let ticks = w.ticks;
        w.units[1].die(ticks);
        w.tick();
        assert_eq!(count(&w, "unit"), 1.0);
        // Named both ways, a regiment stands for its units.
        assert_eq!(count(&w, "regiment"), 1.0);
    }

    #[test]
    fn a_capture_is_heard_by_the_side_that_took_it() {
        let mut house = crate::testing::building(50, 2, [30.0, 30.0]);
        house.fighter.capturable = true;
        let w = crate::testing::with_buildings(veterans(), &[house]);
        let taken = |k| element("actor_captured_by_kingdom", vec![actor_id(50), kingdom(k)]);
        let mut w = with(
            w,
            vec![
                heard(1, true, vec![taken(1)], vec![incr("ours")]),
                heard(2, true, vec![taken(2)], vec![incr("theirs")]),
            ],
            &[("ours", 0.0), ("theirs", 0.0)],
        );
        w.fall(0, Some(1));
        w.tick();
        w.tick();
        assert_eq!((count(&w, "ours"), count(&w, "theirs")), (1.0, 0.0));
        // Handed over by a trigger, it isn't heard as captured.
        let back = element(
            "actor_transfer_control_to_kingdom",
            vec![actor_id(50), kingdom(2)],
        );
        w.triggers.list = vec![
            heard(1, true, vec![taken(2)], vec![incr("theirs")]),
            trigger(2, true, false, Vec::new(), vec![back]),
        ]
        .into();
        w.triggers.switches = vec![
            super::Switch {
                active: true,
                activated: 0
            };
            2
        ];
        w.tick();
        w.tick();
        assert_eq!(w.buildings[0].owner, 2);
        assert_eq!(count(&w, "theirs"), 0.0);
    }

    #[test]
    fn recruits_and_buildings_are_heard_by_template_and_side() {
        let recruits = |k, t: Parameter| element("kingdom_recruits_actor", vec![kingdom(k), t]);
        let any_template = parameter("ma_actor_data", "mi_actor_data_any", Value::Int(0));
        let finished = element(
            "kingdom_finishes_construction_on_actor",
            vec![kingdom(1), template("rus_depot_ammo")],
        );
        let mut w = with(
            veterans(),
            vec![
                heard(
                    1,
                    true,
                    vec![recruits(1, template("regiment_rus_infantry"))],
                    vec![incr("infantry")],
                ),
                heard(2, true, vec![recruits(1, any_template)], vec![incr("any")]),
                heard(
                    3,
                    true,
                    vec![recruits(2, template("regiment_rus_infantry"))],
                    vec![incr("theirs")],
                ),
                heard(4, true, vec![finished], vec![incr("depots")]),
            ],
            &[
                ("infantry", 0.0),
                ("any", 0.0),
                ("theirs", 0.0),
                ("depots", 0.0),
            ],
        );
        w.happen(Happening::Recruited {
            player: 1,
            template: Id::from("regiment_rus_infantry"),
        });
        w.happen(Happening::Finished {
            player: 1,
            template: Id::from("rus_depot_ammo"),
        });
        w.tick();
        w.tick();
        let counts = ["infantry", "any", "theirs", "depots"].map(|n| count(&w, n));
        assert_eq!(counts, [1.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn the_event_kingdom_is_the_last_an_event_named() {
        let recruits = element(
            "kingdom_recruits_actor",
            vec![kingdom(2), template("regiment_rus_infantry")],
        );
        let since = parameter("ma_kingdom", "mi_kingdom_event", Value::Int(0));
        let won = element("kingdom_mission_won", vec![since]);
        let mut w = with(
            veterans(),
            vec![heard(1, false, vec![recruits], vec![won])],
            &[],
        );
        w.happen(Happening::Recruited {
            player: 2,
            template: Id::from("regiment_rus_infantry"),
        });
        w.tick();
        assert_eq!(w.outcome(2), Outcome::Won);
        assert_eq!(w.triggers.event_kingdom, Some(2));
    }

    #[test]
    fn naming_nothing_is_dead_and_states_are_the_things_own() {
        let nobody = parameter("ma_actor", "mi_actor_named", Value::String("nobody".into()));
        let dead = element("actor_is_dead", vec![nobody.clone()]);
        let state = |name: &str, n| {
            element(
                name,
                vec![
                    actor_id(1),
                    parameter("ma_state", "mi_state", Value::Int(n)),
                ],
            )
        };
        let w = veterans();
        let mut w = with(
            w,
            vec![
                trigger(1, true, false, vec![dead], vec![incr("dead")]),
                trigger(
                    3,
                    true,
                    false,
                    vec![state("actor_does_not_have_state", 41)],
                    vec![incr("free")],
                ),
                trigger(
                    4,
                    true,
                    false,
                    vec![state("actor_has_state", 41)],
                    vec![incr("attached")],
                ),
                // Neither of a thing that isn't there.
                trigger(
                    5,
                    true,
                    false,
                    vec![element(
                        "actor_does_not_have_state",
                        vec![nobody, parameter("ma_state", "mi_state", Value::Int(41))],
                    )],
                    vec![incr("missing")],
                ),
            ],
            &[
                ("dead", 0.0),
                ("free", 0.0),
                ("attached", 0.0),
                ("missing", 0.0),
            ],
        );
        w.tick();
        let counts = ["dead", "free", "attached", "missing"].map(|n| count(&w, n));
        assert_eq!(counts, [1.0, 1.0, 0.0, 0.0]);
        // What it's doing is one state, by the game's numbering.
        let doing = super::STATES.iter().position(|&s| s == w.activity(0));
        let regiment = crate::targets::Thing::Organization(0);
        assert!(w.in_trigger_state(regiment, doing.unwrap() as u32));
        assert!(!w.in_trigger_state(regiment, 17));
    }

    #[test]
    fn health_is_compared_as_a_share_and_a_regiment_has_none() {
        let life = |id, n| {
            element(
                "actor_comparison_life",
                vec![actor_id(id), at_least(), int(n)],
            )
        };
        let mut w = with(
            veterans(),
            vec![
                trigger(1, true, false, vec![life(2, 50)], vec![incr("half")]),
                trigger(2, true, false, vec![life(2, 99)], vec![incr("whole")]),
                trigger(3, true, false, vec![life(1, 0)], vec![incr("regiment")]),
            ],
            &[("half", 0.0), ("whole", 0.0), ("regiment", 0.0)],
        );
        let most = w.max_health(0);
        w.units[0].health = most * 0.6;
        w.tick();
        let counts = ["half", "whole", "regiment"].map(|n| count(&w, n));
        assert_eq!(counts, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn difficulties_compare_by_their_place() {
        let map = parameter(
            "ma_difficulty_level",
            "mi_difficulty_level_map",
            Value::Int(0),
        );
        let level = |n: &str| {
            parameter(
                "ma_difficulty_level",
                "mi_difficulty_level_specified",
                Value::String(n.into()),
            )
        };
        let compared = |n| {
            element(
                "difficulty_compare_difficulty",
                vec![map.clone(), at_least(), level(n)],
            )
        };
        let mut w = with(
            veterans(),
            vec![
                trigger(1, true, false, vec![compared("easy")], vec![incr("easy")]),
                trigger(2, true, false, vec![compared("hard")], vec![incr("hard")]),
            ],
            &[("easy", 0.0), ("hard", 0.0)],
        );
        w.rules.difficulties = ["easy", "medium", "hard"].map(Id::from).to_vec();
        w.rules.difficulty = Some(Id::from("MEDIUM"));
        w.tick();
        assert_eq!((count(&w, "easy"), count(&w, "hard")), (1.0, 0.0));
    }

    #[test]
    fn objectives_are_given_settled_and_tested() {
        let gains = element(
            "kingdom_gains_objective",
            vec![kingdom(1), objective("take_it")],
        );
        let done = element(
            "objective_complete_for_kingdom",
            vec![objective("take_it"), kingdom(1)],
        );
        let unknown = element(
            "kingdom_gains_objective",
            vec![kingdom(1), objective("nothing")],
        );
        let succeeded = element(
            "objective_succeeded",
            vec![objective("take_it"), kingdom(1)],
        );
        let won = element("kingdom_mission_won", vec![kingdom(1)]);
        let mut w = with(
            veterans(),
            vec![
                trigger(1, true, false, Vec::new(), vec![gains, unknown]),
                trigger(2, true, false, vec![after(1)], vec![done]),
                trigger(3, true, false, vec![succeeded], vec![won]),
            ],
            &[],
        );
        w.triggers.objectives = vec![tgmap::Objective {
            name: "take_it".into(),
            text: "1. Take it".into(),
            primary: true,
        }]
        .into();
        w.tick();
        let held = &w.objectives[&1].0;
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].standing, crate::Standing::Active);
        assert!(w.notices.contains(&Notice::Objective {
            player: 1,
            text: "1. Take it".into(),
            primary: true,
            standing: Some(crate::Standing::Active),
        }));
        run(&mut w, 1.1);
        assert_eq!(w.objectives[&1].0[0].standing, crate::Standing::Completed);
        assert_eq!(w.outcome(1), Outcome::Won);
    }

    #[test]
    fn a_side_can_lose_or_be_kept_in_with_nothing() {
        let lost = element("kingdom_mission_lost", vec![kingdom(2)]);
        let off = element(
            "kingdom_enable_default_victory_condition",
            vec![
                kingdom(1),
                parameter("ma_boolean", "mi_boolean", Value::Int(0)),
            ],
        );
        let nothing = element("kingdom_default_lose", vec![kingdom(1)]);
        let w = with(
            veterans(),
            vec![
                trigger(1, true, false, Vec::new(), vec![lost, off]),
                trigger(2, true, false, vec![nothing], vec![incr("nothing")]),
            ],
            &[("nothing", 0.0)],
        );
        let mut w = w;
        w.tick();
        assert_eq!(w.outcome(2), Outcome::Lost);
        assert!(!w.fates[&1].default_defeat);
        assert_eq!(count(&w, "nothing"), 0.0);
    }

    #[test]
    fn a_thing_a_trigger_destroys_is_heard_of_next_tick_and_one_removed_never() {
        let destroyed = element("object_destroyed", vec![actor_id(1)]);
        let removed = element("object_removed", vec![actor_id(3)]);
        let killed = element("actor_destroys_actor", vec![any(), any()]);
        let mut w = with(
            veterans(),
            vec![
                heard(1, true, vec![killed], vec![incr("deaths")]),
                trigger(2, true, false, Vec::new(), vec![destroyed, removed]),
            ],
            &[("deaths", 0.0)],
        );
        w.tick();
        assert!(w.destroyed(0) && w.destroyed(1));
        assert_eq!(count(&w, "deaths"), 0.0);
        w.tick();
        // Its unit and the regiment, on the tick after.
        assert_eq!(count(&w, "deaths"), 2.0);
        assert!(w.units[1].died.is_none() && w.units[1].left.is_some());
    }

    #[test]
    fn money_is_given_to_a_side() {
        let gives = element(
            "kingdom_gains_value_primary_resource",
            vec![kingdom(1), int(1000)],
        );
        let mut w = running(vec![gives]);
        let had = w.stockpile(1);
        w.tick();
        assert!((w.stockpile(1) - had - 1000.0).abs() < 1e-3);
    }

    fn command(text: &str) -> Parameter {
        parameter(
            "ma_script_command",
            "mi_script_command",
            Value::String(text.into()),
        )
    }

    #[test]
    fn a_command_is_heard_only_while_the_map_listens_for_it() {
        let on = |n| parameter("ma_boolean", "mi_boolean", Value::Int(n));
        let listen = element(
            "effect_ui_script_listen",
            vec![command("ActorCommand move"), on(1)],
        );
        let stop = element(
            "effect_ui_script_listen",
            vec![command("ActorCommand move"), on(0)],
        );
        let heard_move = element("event_script_heard", vec![command("ActorCommand move")]);
        let mut w = with(
            veterans(),
            vec![
                trigger(1, true, false, Vec::new(), vec![listen]),
                heard(2, true, vec![heard_move], vec![incr("moves")]),
                trigger(3, false, false, Vec::new(), vec![stop]),
            ],
            &[("moves", 0.0)],
        );
        // Not yet listened for when it ran.
        let early = w.apply(
            1,
            crate::Command::Heard {
                command: "ActorCommand move".into(),
            },
        );
        assert!(early.is_err());
        w.tick();
        assert_eq!(count(&w, "moves"), 0.0);
        assert!(w.triggers.listening.contains("actorcommand move"));
        w.apply(
            1,
            crate::Command::Heard {
                command: "actorcommand MOVE".into(),
            },
        )
        .unwrap();
        w.tick();
        assert_eq!(count(&w, "moves"), 1.0);
        w.tick();
        assert_eq!(count(&w, "moves"), 1.0);
        w.triggers.switches[2].active = true;
        w.tick();
        let late = w.apply(
            1,
            crate::Command::Heard {
                command: "ActorCommand move".into(),
            },
        );
        assert!(late.is_err());
        w.tick();
        assert_eq!(count(&w, "moves"), 1.0);
    }

    #[test]
    fn a_selection_is_tested_by_thing_and_by_template() {
        let by_thing = element("actor_selected_by_kingdom", vec![actor_id(1), kingdom(1)]);
        let unit = element("actor_selected_by_kingdom", vec![actor_id(2), kingdom(1)]);
        let theirs = element("actor_selected_by_kingdom", vec![actor_id(1), kingdom(2)]);
        let by_template = |k| {
            element(
                "actor_data_selected_by_kingdom",
                vec![template("squad"), kingdom(k)],
            )
        };
        let w = with(
            veterans(),
            vec![
                heard(1, true, Vec::new(), Vec::new()),
                trigger(2, true, true, vec![by_thing], vec![incr("regiment")]),
                trigger(3, true, true, vec![unit], vec![incr("unit")]),
                trigger(4, true, true, vec![theirs], vec![incr("theirs")]),
                trigger(5, true, true, vec![by_template(1)], vec![incr("template")]),
            ],
            &[
                ("regiment", 0.0),
                ("unit", 0.0),
                ("theirs", 0.0),
                ("template", 0.0),
            ],
        );
        let mut w = w;
        assert!(w.triggers.watches_selection);
        w.organizations[0].template = Id::from("squad");
        let select = crate::Command::Select {
            pieces: vec![crate::Piece::Organization(0)],
        };
        w.apply(1, select).unwrap();
        w.tick();
        let counts = ["regiment", "unit", "theirs", "template"].map(|n| count(&w, n));
        assert_eq!(counts, [1.0, 0.0, 0.0, 1.0]);
        let nowhere = crate::Command::Select {
            pieces: vec![crate::Piece::Building(9)],
        };
        assert!(w.apply(1, nowhere).is_err());
    }
}
