//! What a match is played by: the game's settings and what each template
//! does, read from its data once and fixed for the match.

use std::collections::{HashMap, HashSet};

use tgmap::{Id, Packs, Requires};

/// The game's rules for one match. Each table left empty leaves what it
/// covers out of play, so a test sets only those it exercises.
#[derive(Debug, Clone)]
pub struct Rules {
    /// What the match's chances are drawn from: every copy of a match is
    /// given the same one.
    pub seed: u64,
    pub terrain_rules: tgmap::TerrainRules,
    /// What each building template packs into, for those that can.
    pub packing: HashMap<Id, Packs>,
    /// The buildings a player needs to commission an organization with a member
    /// of each template, by template.
    pub requires: HashMap<Id, Requires>,
    /// Each template's own cost, production and upkeep, by template.
    pub economies: HashMap<Id, tgmap::Economy>,
    /// The building templates whose buildings their owner can sell.
    pub sellable: HashSet<Id>,
    pub economy: tgmap::EconomyRules,
    /// How each template gives supply, for those that do, by template.
    pub supplies: HashMap<Id, tgmap::Supply>,
    /// The settings supply runs by.
    pub supply_rules: tgmap::SupplyRules,
    pub guard_rules: tgmap::GuardRules,
    /// How far around them things that send out units of their own keep
    /// their guard zone, by template.
    pub guard_zones: HashMap<Id, f32>,
    /// Seconds the remains of each template's things lie rotting once
    /// dead, then sink, by template; none for one without a body.
    pub remains: HashMap<Id, [f32; 2]>,
    /// The stances each template's things take, in the order listed, by
    /// template.
    pub stances: HashMap<Id, Vec<tgmap::Stance>>,
    /// The animation sets each template's units play, their own first,
    /// by template.
    pub animation_sets: HashMap<Id, Vec<tgmap::AnimationSet>>,
    /// Each kind of damage's number for death animations, by its id.
    pub damage_filters: HashMap<Id, i32>,
    /// The settings morale and routing run by.
    pub morale_rules: tgmap::MoraleRules,
    /// The morale each unit template adds to its organization's maximum,
    /// by template. An organization whose maximum comes to nothing has no
    /// morale: it never loses, regains or routs.
    pub morale_values: HashMap<Id, f32>,
    /// Each organization template's breakpoint, by template.
    pub morale_breakpoints: HashMap<Id, f32>,
    /// What each property does to morale, for those that do anything to
    /// it, by its id.
    pub morale_effects: HashMap<Id, tgmap::MoraleEffect>,
    /// What entrenching takes and gives.
    pub entrenchment: tgmap::Entrenchment,
    /// The settings combat values are worked out by.
    pub combat_value_rules: tgmap::CombatValueRules,
    /// Each template's figures, which its things' combat values are worked
    /// out from, by template. A template without them is worth nothing.
    pub combat_profiles: HashMap<Id, tgmap::CombatProfile>,
    /// What combat values are measured against.
    pub combat_means: crate::CombatMeans,
    /// What each property does to its holder's fighting, for those that do
    /// anything to it, by its id.
    pub combat_effects: HashMap<Id, tgmap::CombatEffect>,
    /// The target types each property gives what holds it.
    pub property_types: HashMap<Id, tgmap::PropertyTypes>,
    /// What each formation's properties give the regiment in it, by
    /// formation id.
    pub formation_effects: HashMap<Id, tgmap::CombatEffect>,
    /// What each organization template's broken formation's property gives
    /// it while it has no formation, by template.
    pub broken_effects: HashMap<Id, tgmap::CombatEffect>,
    /// The target types each formation's properties give the regiment in
    /// it, by formation id.
    pub formation_types: HashMap<Id, Vec<Id>>,
    /// The target types each organization template's broken formation's
    /// property gives it while it has no formation, by template.
    pub broken_types: HashMap<Id, Vec<Id>>,
    /// Which target types share a slot.
    pub type_slots: crate::TypeSlots,
    /// Attack efficiency a second a regiment gains while below its target,
    /// and loses while above it.
    pub efficiency_rates: [f32; 2],
    /// How many times its own top speed a member catching up with its
    /// place in its regiment goes at most.
    pub catchup_speed: f32,
    /// How much experience organizations earn.
    pub experience_gains: tgmap::ExperienceGains,
    /// The levels each template's organizations earn with experience, by
    /// template. Those of templates without them earn none.
    pub experience_levels: HashMap<Id, tgmap::Levels>,
    /// Seconds an enemy's damage keeps what it hurt under siege.
    pub siege_seconds: f32,
    /// Seconds after an enemy's last hit on something that the enemy is
    /// still credited with killing it.
    pub kill_credit_seconds: f32,
    /// Added to a target's score for a unit it's attacking.
    pub attacker_bonus: f32,
    /// Whether an organization's attack target comes before all else its
    /// members could fire on.
    pub attack_target_first: bool,
    /// The longest a unit waits, once its organization engages, before it
    /// first decides, in seconds.
    pub engage_reaction: f32,
    /// What each property does to supply, for those that do anything to
    /// it, by its id.
    pub supply_effects: HashMap<Id, tgmap::SupplyEffect>,
    /// Each property's enchantment type, for those that have one.
    pub enchantment_types: HashMap<Id, Id>,
    /// What each template's things are, by template, for the technologies
    /// and operations that work only on some.
    pub kinds: HashMap<Id, tgmap::Kind>,
    /// Whether sides' objectives are kept in order of their text.
    pub sort_objectives: bool,
    /// The special operations that bear on supply, by id.
    pub operations: HashMap<Id, tgmap::Operation>,
    /// What those operations drop, by template, each standing anywhere and
    /// owned by nobody.
    pub drops: HashMap<Id, tgmap::Building>,
    /// What each template that can be built puts up, as its site starts.
    pub buildable: HashMap<Id, tgmap::Building>,
    /// The health each template's buildings lose a second from when
    /// they're put on the map, by template.
    pub decays: HashMap<Id, f32>,
    /// How each organization template stands with supply, by template.
    /// Organizations of those without it are never in or out of supply.
    pub logistics: HashMap<Id, tgmap::Logistics>,
    /// The share of its health each template gets back a second in
    /// supply, by template.
    pub resupply_rates: HashMap<Id, f32>,
    /// The organization templates each building template recruits, which are
    /// those that can attach to it as an HQ.
    pub recruit_lists: HashMap<Id, Vec<Id>>,
    /// Every hero a side could hold, in the order they're defined.
    pub heroes: Vec<tgmap::Hero>,
    /// The group of each faction's templates, by faction id, which tells
    /// which heroes are theirs.
    pub faction_groups: HashMap<Id, Id>,
    /// What those organization templates raise, by template, each standing
    /// anywhere and owned by nobody.
    pub raisable: HashMap<Id, tgmap::Organization>,
    /// The aircraft that flies in each organization template that's
    /// delivered, by template.
    pub airlifts: HashMap<Id, tgmap::Airlift>,
    /// How each template falls when it's dropped, by template.
    pub falls: HashMap<Id, tgmap::Fall>,
    /// World units a second each second that what falls gains.
    pub gravity: f32,
    /// Seconds commissioning an organization of each template takes, by
    /// template.
    pub recruit_times: HashMap<Id, f32>,
    /// What each template that researches lists, in its menu's order, by
    /// template id.
    pub research_lists: HashMap<Id, Vec<tgmap::Research>>,
    /// The experience every side with a faction gains a second.
    pub experience_per_second: f32,
    /// How far each template sees, by template.
    pub sights: HashMap<Id, tgmap::Sight>,
    /// The templates whose organizations are numbered, as are the buildings
    /// standing in their organizations.
    pub numbered: HashSet<Id>,
    /// The templates whose things keep their side in the match, as every
    /// organization does.
    pub sustaining: HashSet<Id>,
    /// Whether the sides of a team win and lose together.
    pub team_victory: bool,
    /// The percentage of the map's cities a team wins by holding; 0 for
    /// no such win.
    pub city_victory_ratio: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            seed: 0,
            terrain_rules: Default::default(),
            packing: HashMap::new(),
            requires: HashMap::new(),
            economies: HashMap::new(),
            sellable: HashSet::new(),
            economy: Default::default(),
            supplies: HashMap::new(),
            supply_rules: Default::default(),
            guard_rules: Default::default(),
            guard_zones: HashMap::new(),
            remains: HashMap::new(),
            stances: HashMap::new(),
            animation_sets: HashMap::new(),
            damage_filters: HashMap::new(),
            morale_rules: Default::default(),
            morale_values: HashMap::new(),
            morale_breakpoints: HashMap::new(),
            morale_effects: HashMap::new(),
            entrenchment: Default::default(),
            combat_value_rules: Default::default(),
            combat_profiles: HashMap::new(),
            combat_means: Default::default(),
            combat_effects: HashMap::new(),
            property_types: HashMap::new(),
            formation_effects: HashMap::new(),
            broken_effects: HashMap::new(),
            formation_types: HashMap::new(),
            broken_types: HashMap::new(),
            type_slots: Default::default(),
            efficiency_rates: [0.01, 0.02],
            catchup_speed: 1.75,
            experience_gains: Default::default(),
            experience_levels: HashMap::new(),
            siege_seconds: 7.0,
            kill_credit_seconds: 2.0,
            attacker_bonus: 0.5,
            attack_target_first: true,
            engage_reaction: 0.3,
            supply_effects: HashMap::new(),
            enchantment_types: HashMap::new(),
            kinds: HashMap::new(),
            sort_objectives: true,
            operations: HashMap::new(),
            drops: HashMap::new(),
            buildable: HashMap::new(),
            decays: HashMap::new(),
            logistics: HashMap::new(),
            resupply_rates: HashMap::new(),
            recruit_lists: HashMap::new(),
            heroes: Vec::new(),
            faction_groups: HashMap::new(),
            raisable: HashMap::new(),
            airlifts: HashMap::new(),
            falls: HashMap::new(),
            gravity: 49.05,
            recruit_times: HashMap::new(),
            research_lists: HashMap::new(),
            experience_per_second: 0.0,
            sights: HashMap::new(),
            numbered: HashSet::new(),
            sustaining: HashSet::new(),
            team_victory: true,
            city_victory_ratio: 0,
        }
    }
}
