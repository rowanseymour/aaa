//! What a match is played by: the game's settings and what each template
//! does, read from its data once and fixed for the match.

use std::collections::{HashMap, HashSet};

use aaa_map::{Id, Packs, Requires};

/// The game's rules for one match. Each table left empty leaves what it
/// covers out of play, so a test sets only those it exercises.
#[derive(Debug, Clone)]
pub struct Rules {
    pub terrain_rules: aaa_map::TerrainRules,
    /// What each building template packs into, for those that can.
    pub packing: HashMap<String, Packs>,
    /// The buildings a player needs to commission a regiment with a member
    /// of each template, by template.
    pub requires: HashMap<Id, Requires>,
    /// Each template's own cost, production and upkeep, by template.
    pub economies: HashMap<Id, aaa_map::Economy>,
    /// The building templates whose buildings their owner can sell.
    pub sellable: HashSet<Id>,
    pub economy: aaa_map::EconomyRules,
    /// How each template gives supply, for those that do, by template.
    pub supplies: HashMap<Id, aaa_map::Supply>,
    /// The settings supply runs by.
    pub supply_rules: aaa_map::SupplyRules,
    pub guard_rules: aaa_map::GuardRules,
    /// Seconds an enemy's damage keeps what it hurt under siege.
    pub siege_seconds: f32,
    /// What each property does to supply, for those that do anything to
    /// it, by its id.
    pub supply_effects: HashMap<Id, aaa_map::SupplyEffect>,
    /// What each template's things are, by template, for the technologies
    /// and operations that work only on some.
    pub kinds: HashMap<Id, aaa_map::Kind>,
    /// Whether sides' objectives are kept in order of their text.
    pub sort_objectives: bool,
    /// The special operations that bear on supply, by id.
    pub operations: HashMap<Id, aaa_map::Operation>,
    /// What those operations drop, by template, each standing anywhere and
    /// owned by nobody.
    pub drops: HashMap<Id, aaa_map::Building>,
    /// The health each template's buildings lose a second from when
    /// they're put on the map, by template.
    pub decays: HashMap<Id, f32>,
    /// How each organization template stands with supply, by template.
    /// Regiments of those without it are never in or out of supply.
    pub logistics: HashMap<Id, aaa_map::Logistics>,
    /// The share of its health each template gets back a second in
    /// supply, by template.
    pub resupply_rates: HashMap<Id, f32>,
    /// The regiment templates each building template recruits, which are
    /// those that can attach to it as an HQ.
    pub recruit_lists: HashMap<Id, Vec<Id>>,
    /// What each template that researches lists, in its menu's order, by
    /// template id.
    pub research_lists: HashMap<Id, Vec<aaa_map::Research>>,
    /// The experience every side with a general gains a second.
    pub experience_per_second: f32,
    /// How far each template sees, by template.
    pub sights: HashMap<Id, aaa_map::Sight>,
    /// The templates whose regiments are numbered, as are the buildings
    /// standing in their organizations.
    pub numbered: HashSet<Id>,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            terrain_rules: Default::default(),
            packing: HashMap::new(),
            requires: HashMap::new(),
            economies: HashMap::new(),
            sellable: HashSet::new(),
            economy: Default::default(),
            supplies: HashMap::new(),
            supply_rules: Default::default(),
            guard_rules: Default::default(),
            siege_seconds: 7.0,
            supply_effects: HashMap::new(),
            kinds: HashMap::new(),
            sort_objectives: true,
            operations: HashMap::new(),
            drops: HashMap::new(),
            decays: HashMap::new(),
            logistics: HashMap::new(),
            resupply_rates: HashMap::new(),
            recruit_lists: HashMap::new(),
            research_lists: HashMap::new(),
            experience_per_second: 0.0,
            sights: HashMap::new(),
            numbered: HashSet::new(),
        }
    }
}
