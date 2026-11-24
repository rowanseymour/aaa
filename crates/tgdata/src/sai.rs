//! How a computer player plays: its profile (`[SAI]` in `SAI/`) and the
//! moods (`[Ego]`) it names, resolved against the templates
//! `Templates/template_sai*.tgi`, whose comments say what each field does.
//! Each type's defaults are those of the engine-wide template, for list
//! entries, which inherit nothing.

use serde::Deserialize;

use crate::{GameData, Id, scoped};

/// A profile and its moods, the first the one it starts in.
#[derive(Debug, Clone)]
pub struct Profile {
    pub id: Id,
    /// The random amount up to which each mood's priority is raised when
    /// one is chosen.
    pub ego_fuzziness: f32,
    /// The most orders it gives in one plan, and in one reaction.
    pub max_ge_commands: usize,
    pub max_rai_commands: usize,
    pub moods: Vec<Ego>,
    /// Whether it sends air attacks, which the game decides for every
    /// profile.
    pub sends_air_attacks: bool,
}

/// A profile as its block holds it.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct SaiBlock {
    ids: Option<Id>,
    ego_fuzziness: f32,
    max_ge_commands: usize,
    max_rai_commands: usize,
    initial_ego: Option<Id>,
    ego: Vec<Named>,
}

impl Default for SaiBlock {
    fn default() -> Self {
        SaiBlock {
            ids: None,
            ego_fuzziness: 200.0,
            max_ge_commands: 100,
            max_rai_commands: 40,
            initial_ego: None,
            ego: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Named {
    ids: Option<Id>,
}

/// A mood: everything a computer player goes by while it's in it.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Ego {
    pub ids: Option<Id>,
    /// Which mood wins when several fit.
    pub priority: f32,
    pub filters: Filters,
    pub goal_engine: GoalEngine,
    #[serde(rename = "ReactiveAI")]
    pub reactive_ai: Reacting,
}

impl Default for Ego {
    fn default() -> Self {
        Ego {
            ids: None,
            priority: 500.0,
            filters: Filters::default(),
            goal_engine: GoalEngine::default(),
            reactive_ai: Reacting::default(),
        }
    }
}

/// The scoring statistics a mood needs to lie within.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Filters {
    pub item: Vec<Filter>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Filter {
    /// A scoring statistic, such as `money_rate`.
    pub stat: Option<Id>,
    pub min: f32,
    pub max: f32,
}

/// What the planning goes by.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GoalEngine {
    pub general_settings: GeneralSettings,
    pub goal_priority: Vec<GoalPriority>,
    pub resource_management: ResourceManagement,
    pub actor_value: ActorValue,
    #[serde(rename = "OffensiveCV")]
    pub offensive_cv: Strength,
    #[serde(rename = "DefensiveCV")]
    pub defensive_cv: Strength,
    pub travel_time: TravelTime,
    pub actor_priority: Vec<ActorPriority>,
    pub default_max_recruited_per_think: u32,
    /// The most of its own items it has queued at one building.
    pub build_queue_max_items: u32,
    pub combat: Combat,
    pub exploration: Exploration,
    pub construction: Construction,
    pub unpacking: Unpacking,
    pub packing: Packing,
    pub recover: Recover,
    pub recruiting: Recruiting,
    pub attach: Attach,
    pub research: Research,
}

impl Default for GoalEngine {
    fn default() -> Self {
        GoalEngine {
            general_settings: Default::default(),
            goal_priority: Vec::new(),
            resource_management: Default::default(),
            actor_value: Default::default(),
            offensive_cv: Strength::default(),
            defensive_cv: Strength {
                cv_advantage_target: 2.0,
                cv_advantage_max: 10.0,
                cv_exponent_under_target: 1.5,
                cv_max_mult_over_target: 4.0,
                min_cv_to_assign: 75.0,
                ..Strength::default()
            },
            travel_time: Default::default(),
            actor_priority: Vec::new(),
            default_max_recruited_per_think: 1,
            build_queue_max_items: 1,
            combat: Default::default(),
            exploration: Default::default(),
            construction: Default::default(),
            unpacking: Default::default(),
            packing: Default::default(),
            recover: Default::default(),
            recruiting: Default::default(),
            attach: Default::default(),
            research: Default::default(),
        }
    }
}

impl GoalEngine {
    /// The weights for goals of kind `goal`, such as `RECRUIT`: the last
    /// entry naming it, as moods list their own after their templates'.
    pub fn weights(&self, goal: &str) -> Option<&GoalPriority> {
        (self.goal_priority.iter().rev()).find(|g| {
            g.goal_ids
                .as_ref()
                .is_some_and(|i| i.as_str().eq_ignore_ascii_case(goal))
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    /// Seconds of play between plans.
    pub think_frequency: f32,
    pub fuzziness: f32,
    pub goal_inertia: f32,
    pub immediate_think_on_build_max_settlements: u32,
    pub immediate_think_on_build_max_time: f32,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        GeneralSettings {
            think_frequency: 30.0,
            fuzziness: 200.0,
            goal_inertia: 250.0,
            immediate_think_on_build_max_settlements: 2,
            immediate_think_on_build_max_time: 600.0,
        }
    }
}

/// How a mood changes one kind of goal's priority.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GoalPriority {
    pub goal_ids: Option<Id>,
    pub multiplier: f32,
    pub bonus: f32,
    pub minimum: f32,
    /// Below 0 for the mood's own.
    pub fuzziness: f32,
    pub inertia: f32,
    pub actor_preference: Vec<ActorPreference>,
}

impl Default for GoalPriority {
    fn default() -> Self {
        GoalPriority {
            goal_ids: None,
            multiplier: 1.0,
            bonus: 0.0,
            minimum: 0.0,
            fuzziness: -1.0,
            inertia: -1.0,
            actor_preference: Vec::new(),
        }
    }
}

/// How much a goal likes having some kind of thing on it.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ActorPreference {
    pub property_ids: Option<Id>,
    pub actor_ids: Option<Id>,
    pub value: f32,
    pub count_value_once_per_company: bool,
    pub multiplier: f32,
}

impl Default for ActorPreference {
    fn default() -> Self {
        ActorPreference {
            property_ids: None,
            actor_ids: None,
            value: 0.0,
            count_value_once_per_company: true,
            multiplier: 1.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ResourceManagement {
    /// Seconds of income a goal may count on.
    pub saving_time_limit: f32,
    pub stockpilable_short_need: f32,
    pub stockpilable_enough_need: f32,
    pub stockpilable_plenty_value: f32,
    pub stockpilable_plenty_need: f32,
    pub other_short_need: f32,
    pub other_enough_need: f32,
    pub other_plenty_value: f32,
    pub other_plenty_need: f32,
    pub resource_need: Vec<ResourceNeed>,
}

impl Default for ResourceManagement {
    fn default() -> Self {
        ResourceManagement {
            saving_time_limit: 180.0,
            stockpilable_short_need: 50.0,
            stockpilable_enough_need: 25.0,
            stockpilable_plenty_value: 0.0,
            stockpilable_plenty_need: 25.0,
            other_short_need: 0.0,
            other_enough_need: 10.0,
            other_plenty_value: 10.0,
            other_plenty_need: 1.0,
            resource_need: Vec::new(),
        }
    }
}

impl ResourceManagement {
    /// How much a point of `resource`'s income is needed: below 0, up to
    /// the plenty value, and above it, with that value.
    pub fn need(&self, resource: &str, stockpiled: bool) -> ResourceNeed {
        let named = (self.resource_need.iter()).find(|n| {
            (n.resource_ids.as_ref()).is_some_and(|r| r.as_str().eq_ignore_ascii_case(resource))
        });
        match (named, stockpiled) {
            (Some(n), _) => n.clone(),
            (None, true) => ResourceNeed {
                resource_ids: None,
                short_need: self.stockpilable_short_need,
                enough_need: self.stockpilable_enough_need,
                plenty_value: self.stockpilable_plenty_value,
                plenty_need: self.stockpilable_plenty_need,
            },
            (None, false) => ResourceNeed {
                resource_ids: None,
                short_need: self.other_short_need,
                enough_need: self.other_enough_need,
                plenty_value: self.other_plenty_value,
                plenty_need: self.other_plenty_need,
            },
        }
    }
}

/// How much a point of one resource's income is needed.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ResourceNeed {
    pub resource_ids: Option<Id>,
    pub short_need: f32,
    pub enough_need: f32,
    pub plenty_value: f32,
    pub plenty_need: f32,
}

impl ResourceNeed {
    /// What taking income from `from` to `to` a minute is worth: each
    /// point by the need where it falls.
    pub fn worth(&self, from: f32, to: f32) -> f32 {
        let (low, high, sign) = match from <= to {
            true => (from, to, 1.0),
            false => (to, from, -1.0),
        };
        let span = |a: f32, b: f32| (high.min(b) - low.max(a)).max(0.0);
        let plenty = match self.plenty_value > 0.0 {
            true => self.plenty_value,
            false => f32::INFINITY,
        };
        let short = span(f32::NEG_INFINITY, 0.0) * self.short_need;
        let enough = span(0.0, plenty) * self.enough_need;
        let more = match plenty.is_finite() {
            true => span(plenty, f32::INFINITY) * self.plenty_need,
            false => 0.0,
        };
        sign * (short + enough + more)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ActorValue {
    pub scaling_mult: f32,
    pub military_cv_mult: f32,
}

impl Default for ActorValue {
    fn default() -> Self {
        ActorValue {
            scaling_mult: 0.3,
            military_cv_mult: 5.0,
        }
    }
}

/// How much strength an attack or defence wants.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Strength {
    pub cv_advantage_target: f32,
    pub cv_advantage_max: f32,
    pub cv_exponent_under_target: f32,
    pub cv_max_mult_over_target: f32,
    pub min_cv_to_assign: f32,
    pub no_primary_supply_bonus: f32,
    pub count_static_cv: bool,
}

impl Default for Strength {
    fn default() -> Self {
        Strength {
            cv_advantage_target: 1.0,
            cv_advantage_max: 5.0,
            cv_exponent_under_target: 4.0,
            cv_max_mult_over_target: 15.0,
            min_cv_to_assign: 50.0,
            no_primary_supply_bonus: 500.0,
            count_static_cv: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TravelTime {
    pub max_travel_time: f32,
    pub travel_time_exponent: f32,
}

impl Default for TravelTime {
    fn default() -> Self {
        TravelTime {
            max_travel_time: 300.0,
            travel_time_exponent: 2.0,
        }
    }
}

/// How much it likes recruiting, building or attacking some kind of
/// thing.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ActorPriority {
    pub property_ids: Option<Id>,
    pub actor_ids: Option<Id>,
    pub count_all_property_actors: bool,
    pub value: f32,
    pub repeat_penalty: f32,
    /// 0 for the mood's default.
    pub max_recruited_per_think: u32,
    pub one_time_bonus: f32,
}

impl Default for ActorPriority {
    fn default() -> Self {
        ActorPriority {
            property_ids: None,
            actor_ids: None,
            count_all_property_actors: true,
            value: 0.0,
            repeat_penalty: 0.0,
            max_recruited_per_think: 0,
            one_time_bonus: 0.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Combat {
    pub penalty_to_builder_cv: f32,
    /// How many regions apart fights can be and still share their units'
    /// strength.
    pub linked_region_distance: u32,
    /// How many regions from its attack a unit is held to it.
    pub attack_commit_distance: u32,
    pub defend_priority: DefendPriority,
    pub air_attacks: AirAttacks,
    pub bombardement: Bombardement,
}

impl Default for Combat {
    fn default() -> Self {
        Combat {
            penalty_to_builder_cv: 1.0,
            linked_region_distance: 2,
            attack_commit_distance: 3,
            defend_priority: Default::default(),
            air_attacks: Default::default(),
            bombardement: Default::default(),
        }
    }
}

/// What bombarding a thing is worth.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Bombardement {
    /// What a target is multiplied by when it isn't fighting and isn't in
    /// a region being attacked.
    pub unengaged_target_multiplier: f32,
    /// What a bombarder's combat value is divided by in its worth.
    pub balancing_constant: f32,
}

impl Default for Bombardement {
    fn default() -> Self {
        Bombardement {
            unengaged_target_multiplier: 0.1,
            balancing_constant: 0.001,
        }
    }
}

/// What an air attack on a thing is worth, by how the thing stands.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AirAttacks {
    pub engaged_bonus: f32,
    /// The share of its health from which a thing counts as damaged, and
    /// up to which, not including it.
    pub target_damage_floor: f32,
    pub target_damage_ceiling: f32,
    pub target_damage_bonus: f32,
    pub support_ground_attack_bonus: f32,
    /// What it's multiplied by when no other factor applies.
    pub no_bonus_penalty: f32,
    /// The share of its health below which a thing is nearly dead.
    pub absolute_min_health: f32,
    pub absolute_min_health_engaged_mult: f32,
    pub absolute_min_health_unengaged_mult: f32,
    pub resupplying_mult: f32,
    /// Seconds before a thing attacked from the air can be again.
    pub min_time_between_attacks: f32,
    pub extra_planes_multiplier_max: f32,
    pub extra_planes_multiplier_per_plane: f32,
}

impl Default for AirAttacks {
    fn default() -> Self {
        AirAttacks {
            engaged_bonus: 2.0,
            target_damage_floor: 0.5,
            target_damage_ceiling: 0.8,
            target_damage_bonus: 2.0,
            support_ground_attack_bonus: 2.0,
            no_bonus_penalty: 0.0,
            absolute_min_health: 0.3,
            absolute_min_health_engaged_mult: 0.0,
            absolute_min_health_unengaged_mult: 5.0,
            resupplying_mult: 0.0,
            min_time_between_attacks: 120.0,
            extra_planes_multiplier_max: 2.0,
            extra_planes_multiplier_per_plane: 0.5,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DefendPriority {
    pub economic_value_exponent: f32,
    pub economic_value_multiplier: f32,
    pub enemy_influence_exponent: f32,
    pub enemy_influence_multiplier: f32,
    pub enemy_border_influence_exponent: f32,
    pub enemy_border_influence_multiplier: f32,
    pub enemy_forces_threat_distance: f32,
    pub enemy_forces_bonus: f32,
    pub capture_risk_ratio: f32,
    pub capture_risk_bonus: f32,
}

impl Default for DefendPriority {
    fn default() -> Self {
        DefendPriority {
            economic_value_exponent: 0.5,
            economic_value_multiplier: 2.0,
            enemy_influence_exponent: 1.0,
            enemy_influence_multiplier: 0.5,
            enemy_border_influence_exponent: 1.0,
            enemy_border_influence_multiplier: 0.2,
            enemy_forces_threat_distance: 80.0,
            enemy_forces_bonus: 3.0,
            capture_risk_ratio: 1.0,
            capture_risk_bonus: 2.0,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Exploration {
    pub close_goals: Explore,
    pub deep_goals: Explore,
    pub air_recon_goals: Explore,
}

/// How many explore goals of one kind, and how much each is worth.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Explore {
    pub num_goals: u32,
    pub base_priority: f32,
    pub repeat_penalty: f32,
    pub min_priority: f32,
}

impl Default for Explore {
    fn default() -> Self {
        Explore {
            num_goals: 0,
            base_priority: 200.0,
            repeat_penalty: 0.0,
            min_priority: 0.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Construction {
    pub construct_in_progress_bonus: f32,
    pub construct_city_defense_max_bonus: f32,
    pub construct_city_defense_exponent: f32,
    pub construct_supply_defense_bonus: f32,
    pub construct_in_supply_only: bool,
    pub construct_in_all_regions: bool,
    pub construct_to_defend_research: bool,
    pub construct_to_defend_recruiters: bool,
    pub construct_to_defend_cities: bool,
}

impl Default for Construction {
    fn default() -> Self {
        Construction {
            construct_in_progress_bonus: 2000.0,
            construct_city_defense_max_bonus: 200.0,
            construct_city_defense_exponent: 1.0,
            construct_supply_defense_bonus: 400.0,
            construct_in_supply_only: false,
            construct_in_all_regions: true,
            construct_to_defend_research: true,
            construct_to_defend_recruiters: true,
            construct_to_defend_cities: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Unpacking {
    pub adjacent_region_depth: u32,
    pub num_random_regions: u32,
    pub base_priority: f32,
    pub enemy_static_influence_mult: f32,
    pub enemy_dynamic_influence_mult: f32,
    pub max_friendly_building_bonus: f32,
    pub max_friendly_building_bonus_per_building: f32,
    pub max_distance: f32,
    pub distance_exponent: f32,
    pub travel_distance_mult: f32,
    pub travel_distance_exponent: f32,
    pub fuzzy_factor: f32,
    pub inertia: f32,
}

impl Default for Unpacking {
    fn default() -> Self {
        Unpacking {
            adjacent_region_depth: 1,
            num_random_regions: 20,
            base_priority: 500.0,
            enemy_static_influence_mult: 3.0,
            enemy_dynamic_influence_mult: 3.0,
            max_friendly_building_bonus: 500.0,
            max_friendly_building_bonus_per_building: 200.0,
            max_distance: 30.0,
            distance_exponent: 1.0,
            travel_distance_mult: 5.0,
            travel_distance_exponent: 1.0,
            fuzzy_factor: 150.0,
            inertia: 300.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Packing {
    pub pack_out_of_supply_priority: f32,
}

impl Default for Packing {
    fn default() -> Self {
        Packing {
            pack_out_of_supply_priority: 1000.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Recover {
    pub base_priority_hp: f32,
    pub exponent_hp: f32,
    pub max_hp: f32,
    pub base_priority_morale: f32,
    pub exponent_morale: f32,
    pub max_morale: f32,
    pub critical_morale: f32,
}

impl Default for Recover {
    fn default() -> Self {
        Recover {
            base_priority_hp: 500.0,
            exponent_hp: 3.0,
            max_hp: 0.4,
            base_priority_morale: 500.0,
            exponent_morale: 3.0,
            max_morale: 0.5,
            critical_morale: 0.3,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Recruiting {
    pub max_goals_per_think: u32,
    pub num_military_goals: u32,
    pub core_cv_multiplier: f32,
    pub core_cv_ratio_multiplier: f32,
    pub support_cv_multiplier: f32,
    pub support_cv_ratio_multiplier: f32,
    pub economic_value_multiplier: f32,
    pub decreased_income_penalty: f32,
    pub insufficient_income_level: f32,
    pub insufficient_income_penalty: f32,
    pub matching_builder_bonus: f32,
    pub mismatching_builder_penalty: f32,
    pub candidate_fuzziness: f32,
    pub base_value: f32,
    pub building_bonus: f32,
    pub location_fuzziness: f32,
    pub specific_recruit_request: Vec<RecruitRequest>,
}

impl Default for Recruiting {
    fn default() -> Self {
        Recruiting {
            max_goals_per_think: 1,
            num_military_goals: 1,
            core_cv_multiplier: 3.0,
            core_cv_ratio_multiplier: 0.0,
            support_cv_multiplier: 3.0,
            support_cv_ratio_multiplier: 0.0,
            economic_value_multiplier: 0.2,
            decreased_income_penalty: 20.0,
            insufficient_income_level: 40.0,
            insufficient_income_penalty: 350.0,
            matching_builder_bonus: 500.0,
            mismatching_builder_penalty: 500.0,
            candidate_fuzziness: 50.0,
            base_value: 100.0,
            building_bonus: 50.0,
            location_fuzziness: 100.0,
            specific_recruit_request: Vec::new(),
        }
    }
}

/// A mood's standing request for a kind of thing.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RecruitRequest {
    pub recruit_ids: Option<Id>,
    pub property_ids: Option<Id>,
    pub number_to_request: u32,
    pub base_priority: f32,
    pub economic_value_multiplier: f32,
    pub repeat_penalty: f32,
    pub one_time_bonus: f32,
    pub hq_type_not_available_bonus: f32,
}

impl Default for RecruitRequest {
    fn default() -> Self {
        RecruitRequest {
            recruit_ids: None,
            property_ids: None,
            number_to_request: 1,
            base_priority: 0.0,
            economic_value_multiplier: 1.0,
            repeat_penalty: 0.0,
            one_time_bonus: 0.0,
            hq_type_not_available_bonus: 1100.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Attach {
    pub attach_when_engaged: bool,
    pub cv_multiplier: f32,
}

impl Default for Attach {
    fn default() -> Self {
        Attach {
            attach_when_engaged: false,
            cv_multiplier: 5.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Research {
    pub base_priority: f32,
    pub tech_priority: Vec<TechPriority>,
}

impl Default for Research {
    fn default() -> Self {
        Research {
            base_priority: 100.0,
            tech_priority: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TechPriority {
    pub tech: Option<Id>,
    pub priority: f32,
}

impl Research {
    /// What researching `technology` is worth before the mood's weights.
    pub fn priority(&self, technology: &Id) -> f32 {
        (self.tech_priority.iter())
            .find(|t| t.tech.as_ref() == Some(technology))
            .map_or(self.base_priority, |t| t.priority)
    }
}

/// What the reacting goes by.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Reacting {
    /// Seconds of play between reactions.
    pub think_frequency: f32,
    pub unit_retreat_max_health: f32,
    pub unit_retreat_cv_ratio: f32,
    pub unit_retreat_probability: f32,
    pub attack_distance: u32,
    pub min_attack_advantage: f32,
    pub max_attack_advantage: f32,
}

impl Default for Reacting {
    fn default() -> Self {
        Reacting {
            think_frequency: 2.0,
            unit_retreat_max_health: 0.3,
            unit_retreat_cv_ratio: 0.9,
            unit_retreat_probability: 1.0,
            attack_distance: 2,
            min_attack_advantage: 1.0,
            max_attack_advantage: 1.5,
        }
    }
}

impl GameData {
    /// The computer player profile `id`, with its moods, if it's defined.
    pub fn profile(&self, id: &Id) -> Option<Profile> {
        self.profiles([id]).pop()
    }

    /// The computer player profiles of `ids` that are defined, with their
    /// moods.
    pub fn profiles<'a>(&self, ids: impl IntoIterator<Item = &'a Id>) -> Vec<Profile> {
        let mut files =
            self.parse_tgis(|n| n.starts_with("sai/") || n.starts_with("templates/template_sai"));
        // The templates first, so that every profile sees them.
        files.sort_by_key(|(n, _)| !n.to_ascii_lowercase().starts_with("templates/"));
        let files = scoped(&files);
        let sends_air_attacks = (self.svars().iter())
            .find_map(|v| v.use_air_attacks)
            .unwrap_or(true);
        ids.into_iter()
            .filter_map(|id| profile(&files, id))
            .map(|p| Profile {
                sends_air_attacks,
                ..p
            })
            .collect()
    }
}

fn profile(files: &[crate::Scoped], id: &Id) -> Option<Profile> {
    let block: SaiBlock = (files.iter())
        .flat_map(|f| f.read::<SaiBlock>("SAI"))
        .find(|s| s.ids.as_ref() == Some(id))?;
    let egos: Vec<Ego> = files.iter().flat_map(|f| f.read::<Ego>("Ego")).collect();
    let named = block
        .initial_ego
        .iter()
        .chain(block.ego.iter().filter_map(|e| e.ids.as_ref()));
    let mut moods: Vec<Ego> = Vec::new();
    for name in named {
        let found = egos.iter().find(|e| e.ids.as_ref() == Some(name));
        let fresh = !moods.iter().any(|m| m.ids.as_ref() == Some(name));
        match found {
            Some(ego) if fresh => moods.push(ego.clone()),
            Some(_) => {}
            None => log::warn!("{id}: no mood {name}"),
        }
    }
    Some(Profile {
        id: id.clone(),
        ego_fuzziness: block.ego_fuzziness,
        max_ge_commands: block.max_ge_commands,
        max_rai_commands: block.max_rai_commands,
        moods,
        sends_air_attacks: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A template, a mood built on it, and a profile in another file.
    fn files() -> Vec<(String, std::sync::Arc<tgi::Parsed>)> {
        // The schema, declaring which blocks are lists.
        let templates = "[template Ego]\n{\n\t[GoalEngine]\n\t{\n\t\t[list GoalPriority]\n\t\t{\n\t\t}\n\
            \t\t[Recruiting]\n\t\t{\n\t\t\t[list SpecificRecruitRequest]\n\t\t\t{\n\t\t\t}\n\t\t}\n\
            \t\t[Research]\n\t\t{\n\t\t\t[list TechPriority]\n\t\t\t{\n\t\t\t}\n\t\t}\n\t}\n}\n\
            [template Base template = Ego]\n{\n\tpriority = 2000\n\t[GoalEngine]\n\t{\n\
            \t\t[GoalPriority]\n\t\t{\n\t\t\tgoal_IDS = RECRUIT\n\t\t\tmultiplier = 0.4\n\t\t}\n\
            \t\t[Recruiting]\n\t\t{\n\t\t\tnum_military_goals = 2\n\
            \t\t\t[SpecificRecruitRequest]\n\t\t\t{\n\t\t\t\tproperty_ids = oil\n\t\t\t}\n\t\t}\n\
            \t}\n}\n\
            [template Low template = Base]\n{\n\t[Filters]\n\t{\n\t\t[Item]\n\t\tstat = money_rate\n\
            \t\tmin = -100\n\t\tmax = 40\n\t}\n\t[GoalEngine]\n\t{\n\
            \t\t[GoalPriority]\n\t\t{\n\t\t\tgoal_IDS = RESEARCH\n\t\t\tfixed multiplier = 1.5\n\t\t}\n\
            \t\t[Recruiting]\n\t\t{\n\t\t\t[SpecificRecruitRequest]\n\t\t\t{\n\
            \t\t\t\tproperty_ids = ammo\n\t\t\t\tbase_priority = -850\n\t\t\t}\n\t\t}\n\
            \t\t[Research]\n\t\t{\n\t\t\tbase_priority = 20\n\
            \t\t\t[TechPriority]\n\t\t\t{\n\t\t\t\ttech = tech_a\n\t\t\t\tpriority = 50\n\t\t\t}\n\t\t}\n\
            \t\t[Combat]\n\t\t{\n\t\t\t[AirAttacks]\n\t\t\t{\n\
            \t\t\t\tmin_time_between_attacks = 90\n\t\t\t}\n\t\t}\n\
            \t}\n}\n";
        let profile = "[Ego template = Low]\n{\n\tIDS = low_ego\n}\n\
            [Ego template = Base]\n{\n\tIDS = main_ego\n}\n\
            [SAI]\n{\n\tIDS = test_sai\n\tego_fuzziness = 10\n\tinitial_ego = low_ego\n\
            \t[Ego]\n\tIDS = main_ego\n}\n";
        vec![
            (
                "templates/template_sai.tgi".into(),
                tgi::parse(templates).into(),
            ),
            ("sai/test.tgi".into(), tgi::parse(profile).into()),
        ]
    }

    #[test]
    fn income_is_worth_most_where_its_short() {
        let need = ResourceNeed {
            resource_ids: None,
            short_need: 50.0,
            enough_need: 20.0,
            plenty_value: 10.0,
            plenty_need: 1.0,
        };
        // 5 points short, 10 enough and 5 plenty.
        assert_eq!(need.worth(-5.0, 15.0), 250.0 + 200.0 + 5.0);
        assert_eq!(need.worth(15.0, -5.0), -455.0);
        let never_plenty = ResourceNeed {
            plenty_value: 0.0,
            ..need
        };
        assert_eq!(never_plenty.worth(0.0, 30.0), 600.0);
    }

    #[test]
    fn a_profile_holds_its_moods_resolved_against_their_templates() {
        let files = files();
        let p = profile(&scoped(&files), &Id::from("test_sai")).unwrap();
        assert_eq!(p.ego_fuzziness, 10.0);
        let names: Vec<_> = p.moods.iter().map(|m| m.ids.clone().unwrap()).collect();
        assert_eq!(names, [Id::from("low_ego"), Id::from("main_ego")]);
        let low = &p.moods[0];
        assert_eq!(low.priority, 2000.0);
        assert_eq!(low.filters.item[0].max, 40.0);
        let engine = &low.goal_engine;
        assert_eq!(engine.weights("recruit").unwrap().multiplier, 0.4);
        assert_eq!(engine.weights("RESEARCH").unwrap().multiplier, 1.5);
        // Lists add to the template's, each entry with the template's
        // defaults.
        let requests = &engine.recruiting.specific_recruit_request;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].hq_type_not_available_bonus, 1100.0);
        assert_eq!(requests[1].base_priority, -850.0);
        assert_eq!(engine.recruiting.num_military_goals, 2);
        assert_eq!(engine.research.priority(&Id::from("tech_a")), 50.0);
        assert_eq!(engine.research.priority(&Id::from("tech_b")), 20.0);
        let air = &engine.combat.air_attacks;
        assert_eq!(air.min_time_between_attacks, 90.0);
        assert_eq!(air.engaged_bonus, 2.0);
        assert!(p.moods[1].filters.item.is_empty());
    }
}
