//! How a computer player plays: its profile (`[SAI]` in `SAI/`) and the
//! moods (`[Ego]`) it names, resolved against the templates
//! `Templates/template_sai*.tgi`, whose comments say what each field does.
//! Each type's defaults are those of the engine-wide template, for list
//! entries, which inherit nothing.

use serde::Deserialize;
use smart_default::SmartDefault;

pub use crate::defs::Named;
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
    /// The share of a region its side has to see for it to count as just
    /// explored.
    pub explored_share: f32,
}

/// A profile as its block holds it.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct SaiBlock {
    ids: Option<Id>,
    #[default(200.0)]
    ego_fuzziness: f32,
    #[default(100)]
    max_ge_commands: usize,
    #[default(40)]
    max_rai_commands: usize,
    initial_ego: Option<Id>,
    ego: Vec<Named>,
}

/// A mood: everything a computer player goes by while it's in it.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Ego {
    pub ids: Option<Id>,
    /// Which mood wins when several fit.
    #[default(500.0)]
    pub priority: f32,
    pub filters: Filters,
    pub goal_engine: GoalEngine,
    pub reactive_ai: Reacting,
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
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GoalEngine {
    pub general_settings: GeneralSettings,
    pub goal_priority: Vec<GoalPriority>,
    pub resource_management: ResourceManagement,
    pub actor_value: ActorValue,
    pub offensive_cv: Strength,
    #[default(Strength {
        cv_advantage_target: 2.0,
        cv_advantage_max: 10.0,
        cv_exponent_under_target: 1.5,
        cv_max_mult_over_target: 4.0,
        min_cv_to_assign: 75.0,
        ..Strength::default()
    })]
    pub defensive_cv: Strength,
    pub travel_time: TravelTime,
    pub actor_priority: Vec<ActorPriority>,
    #[default(1)]
    pub default_max_recruited_per_think: u32,
    /// The most of its own items it has queued at one building.
    #[default(1)]
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
    pub general_powers: GeneralPowers,
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

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    /// Seconds of play between plans.
    #[default(30.0)]
    pub think_frequency: f32,
    #[default(200.0)]
    pub fuzziness: f32,
    #[default(250.0)]
    pub goal_inertia: f32,
    #[default(2)]
    pub immediate_think_on_build_max_settlements: u32,
    #[default(600.0)]
    pub immediate_think_on_build_max_time: f32,
}

/// How a mood changes one kind of goal's priority.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GoalPriority {
    pub goal_ids: Option<Id>,
    #[default(1.0)]
    pub multiplier: f32,
    pub bonus: f32,
    pub minimum: f32,
    /// Below 0 for the mood's own.
    #[default(-1.0)]
    pub fuzziness: f32,
    #[default(-1.0)]
    pub inertia: f32,
    pub actor_preference: Vec<ActorPreference>,
}

/// How much a goal likes having some kind of thing on it.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ActorPreference {
    pub property_ids: Option<Id>,
    pub actor_ids: Option<Id>,
    pub value: f32,
    #[default(true)]
    pub count_value_once_per_company: bool,
    #[default(1.0)]
    pub multiplier: f32,
}

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ResourceManagement {
    /// Seconds of income a goal may count on.
    #[default(180.0)]
    pub saving_time_limit: f32,
    #[default(50.0)]
    pub stockpilable_short_need: f32,
    #[default(25.0)]
    pub stockpilable_enough_need: f32,
    pub stockpilable_plenty_value: f32,
    #[default(25.0)]
    pub stockpilable_plenty_need: f32,
    pub other_short_need: f32,
    #[default(10.0)]
    pub other_enough_need: f32,
    #[default(10.0)]
    pub other_plenty_value: f32,
    #[default(1.0)]
    pub other_plenty_need: f32,
    pub resource_need: Vec<ResourceNeed>,
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

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ActorValue {
    #[default(0.3)]
    pub scaling_mult: f32,
    #[default(5.0)]
    pub military_cv_mult: f32,
}

/// How much strength an attack or defence wants.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Strength {
    #[default(1.0)]
    pub cv_advantage_target: f32,
    #[default(5.0)]
    pub cv_advantage_max: f32,
    #[default(4.0)]
    pub cv_exponent_under_target: f32,
    #[default(15.0)]
    pub cv_max_mult_over_target: f32,
    #[default(50.0)]
    pub min_cv_to_assign: f32,
    #[default(500.0)]
    pub no_primary_supply_bonus: f32,
    #[default(true)]
    pub count_static_cv: bool,
}

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TravelTime {
    #[default(300.0)]
    pub max_travel_time: f32,
    #[default(2.0)]
    pub travel_time_exponent: f32,
}

/// How much it likes recruiting, building or attacking some kind of
/// thing.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ActorPriority {
    pub property_ids: Option<Id>,
    pub actor_ids: Option<Id>,
    #[default(true)]
    pub count_all_property_actors: bool,
    pub value: f32,
    pub repeat_penalty: f32,
    /// 0 for the mood's default.
    pub max_recruited_per_think: u32,
    pub one_time_bonus: f32,
}

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Combat {
    #[default(1.0)]
    pub penalty_to_builder_cv: f32,
    /// How many regions apart fights can be and still share their units'
    /// strength.
    #[default(2)]
    pub linked_region_distance: u32,
    /// How many regions from its attack a unit is held to it.
    #[default(3)]
    pub attack_commit_distance: u32,
    pub defend_priority: DefendPriority,
    pub air_attacks: AirAttacks,
    pub bombardement: Bombardement,
}

/// What bombarding a thing is worth.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Bombardement {
    /// What a target is multiplied by when it isn't fighting and isn't in
    /// a region being attacked.
    #[default(0.1)]
    pub unengaged_target_multiplier: f32,
    /// What a bombarder's combat value is divided by in its worth.
    #[default(0.001)]
    pub balancing_constant: f32,
}

/// What an air attack on a thing is worth, by how the thing stands.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AirAttacks {
    #[default(2.0)]
    pub engaged_bonus: f32,
    /// The share of its health from which a thing counts as damaged, and
    /// up to which, not including it.
    #[default(0.5)]
    pub target_damage_floor: f32,
    #[default(0.8)]
    pub target_damage_ceiling: f32,
    #[default(2.0)]
    pub target_damage_bonus: f32,
    #[default(2.0)]
    pub support_ground_attack_bonus: f32,
    /// What it's multiplied by when no other factor applies.
    pub no_bonus_penalty: f32,
    /// The share of its health below which a thing is nearly dead.
    #[default(0.3)]
    pub absolute_min_health: f32,
    pub absolute_min_health_engaged_mult: f32,
    #[default(5.0)]
    pub absolute_min_health_unengaged_mult: f32,
    pub resupplying_mult: f32,
    /// Seconds before a thing attacked from the air can be again.
    #[default(120.0)]
    pub min_time_between_attacks: f32,
    #[default(2.0)]
    pub extra_planes_multiplier_max: f32,
    #[default(0.5)]
    pub extra_planes_multiplier_per_plane: f32,
}

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DefendPriority {
    #[default(0.5)]
    pub economic_value_exponent: f32,
    #[default(2.0)]
    pub economic_value_multiplier: f32,
    #[default(1.0)]
    pub enemy_influence_exponent: f32,
    #[default(0.5)]
    pub enemy_influence_multiplier: f32,
    #[default(1.0)]
    pub enemy_border_influence_exponent: f32,
    #[default(0.2)]
    pub enemy_border_influence_multiplier: f32,
    #[default(80.0)]
    pub enemy_forces_threat_distance: f32,
    #[default(3.0)]
    pub enemy_forces_bonus: f32,
    #[default(1.0)]
    pub capture_risk_ratio: f32,
    #[default(2.0)]
    pub capture_risk_bonus: f32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Exploration {
    pub close_goals: Explore,
    pub deep_goals: Explore,
    pub air_recon_goals: Explore,
}

/// How many explore goals of one kind, and how much each is worth.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Explore {
    pub num_goals: u32,
    #[default(200.0)]
    pub base_priority: f32,
    pub repeat_penalty: f32,
    pub min_priority: f32,
}

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Construction {
    #[default(2000.0)]
    pub construct_in_progress_bonus: f32,
    #[default(200.0)]
    pub construct_city_defense_max_bonus: f32,
    #[default(1.0)]
    pub construct_city_defense_exponent: f32,
    #[default(400.0)]
    pub construct_supply_defense_bonus: f32,
    pub construct_in_supply_only: bool,
    #[default(true)]
    pub construct_in_all_regions: bool,
    #[default(true)]
    pub construct_to_defend_research: bool,
    #[default(true)]
    pub construct_to_defend_recruiters: bool,
    #[default(true)]
    pub construct_to_defend_cities: bool,
}

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Unpacking {
    #[default(1)]
    pub adjacent_region_depth: u32,
    #[default(20)]
    pub num_random_regions: u32,
    #[default(500.0)]
    pub base_priority: f32,
    #[default(3.0)]
    pub enemy_static_influence_mult: f32,
    #[default(3.0)]
    pub enemy_dynamic_influence_mult: f32,
    #[default(500.0)]
    pub max_friendly_building_bonus: f32,
    #[default(200.0)]
    pub max_friendly_building_bonus_per_building: f32,
    #[default(30.0)]
    pub max_distance: f32,
    #[default(1.0)]
    pub distance_exponent: f32,
    #[default(5.0)]
    pub travel_distance_mult: f32,
    #[default(1.0)]
    pub travel_distance_exponent: f32,
    #[default(150.0)]
    pub fuzzy_factor: f32,
    #[default(300.0)]
    pub inertia: f32,
}

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Packing {
    #[default(1000.0)]
    pub pack_out_of_supply_priority: f32,
}

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Recover {
    #[default(500.0)]
    pub base_priority_hp: f32,
    #[default(3.0)]
    pub exponent_hp: f32,
    #[default(0.4)]
    pub max_hp: f32,
    #[default(500.0)]
    pub base_priority_morale: f32,
    #[default(3.0)]
    pub exponent_morale: f32,
    #[default(0.5)]
    pub max_morale: f32,
    #[default(0.3)]
    pub critical_morale: f32,
}

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Recruiting {
    #[default(1)]
    pub max_goals_per_think: u32,
    #[default(1)]
    pub num_military_goals: u32,
    #[default(3.0)]
    pub core_cv_multiplier: f32,
    pub core_cv_ratio_multiplier: f32,
    #[default(3.0)]
    pub support_cv_multiplier: f32,
    pub support_cv_ratio_multiplier: f32,
    #[default(0.2)]
    pub economic_value_multiplier: f32,
    #[default(20.0)]
    pub decreased_income_penalty: f32,
    #[default(40.0)]
    pub insufficient_income_level: f32,
    #[default(350.0)]
    pub insufficient_income_penalty: f32,
    #[default(500.0)]
    pub matching_builder_bonus: f32,
    #[default(500.0)]
    pub mismatching_builder_penalty: f32,
    #[default(50.0)]
    pub candidate_fuzziness: f32,
    #[default(100.0)]
    pub base_value: f32,
    #[default(50.0)]
    pub building_bonus: f32,
    #[default(100.0)]
    pub location_fuzziness: f32,
    pub specific_recruit_request: Vec<RecruitRequest>,
}

/// A mood's standing request for a kind of thing.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RecruitRequest {
    pub recruit_ids: Option<Id>,
    pub property_ids: Option<Id>,
    #[default(1)]
    pub number_to_request: u32,
    pub base_priority: f32,
    #[default(1.0)]
    pub economic_value_multiplier: f32,
    pub repeat_penalty: f32,
    pub one_time_bonus: f32,
    #[default(1100.0)]
    pub hq_type_not_available_bonus: f32,
}

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Attach {
    pub attach_when_engaged: bool,
    #[default(5.0)]
    pub cv_multiplier: f32,
}

/// The general's special operations a mood uses, and how it picks where.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct GeneralPowers {
    pub power: Vec<GeneralPower>,
}

/// One special operation a mood uses.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct GeneralPower {
    /// The operation, by its ability's id.
    pub ability_ids: Option<Id>,
    pub location: PowerLocation,
    pub priority: PowerPriority,
}

/// Where an operation is tried.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PowerLocation {
    /// `NEAR_ACTOR`, `ON_ACTOR` or `FOG`.
    #[serde(rename = "type")]
    pub kind: Option<Id>,
    /// What it's found from.
    pub actor_requirements: ActorRequirements,
    pub placement: Placement,
}

/// What a thing has to be to count.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ActorRequirements {
    /// The most of its full health it has, as a share.
    #[default(1.0)]
    pub max_percent_health: f32,
    pub must_be_engaged: bool,
    pub must_not_be_engaged: bool,
    /// Its side is the computer player's own or an ally.
    pub must_be_friendly: bool,
    pub must_be_enemy: bool,
    pub must_be_owned: bool,
    /// Properties it has to have one of, if any.
    pub required_property: Vec<Named>,
    /// What a thing matching it, as a category, counts for.
    #[default(1.0)]
    pub multiplier: f32,
}

/// How spots near a thing are drawn.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Placement {
    #[default(-1.0)]
    pub radius: f32,
    #[default(3)]
    pub num_candidates: u32,
}

/// How a spot for an operation is scored.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PowerPriority {
    /// What weighs its place in the order, and starts every spot's score.
    pub base: f32,
    pub minimum: f32,
    #[default(100.0)]
    pub fuzzy_factor: f32,
    pub cv: Tally,
    pub actor_value: Tally,
    pub enemy_influence: EnemyInfluence,
    pub influence_ratio: InfluenceRatio,
    pub time_since_explored: Factor,
    pub economy_generated: EconomyGenerated,
}

/// A factor that adds up things in reach, each by the category it
/// matches.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Tally {
    pub minimum: f32,
    /// For things matching no category.
    pub multiplier: f32,
    /// A cap when 0 or more.
    #[default(-1.0)]
    pub maximum: f32,
    pub consider_friendly: bool,
    pub consider_enemy: bool,
    pub consider_owned: bool,
    pub category: Vec<ActorRequirements>,
}

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct EnemyInfluence {
    pub minimum: f32,
    pub multiplier: f32,
    #[default(-1.0)]
    pub maximum: f32,
    #[default(1.0)]
    pub exponent: f32,
}

/// Friendly over enemy influence.
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct InfluenceRatio {
    pub minimum: f32,
    pub multiplier: f32,
    #[default(-1.0)]
    pub maximum: f32,
    /// Counting its own side's alone, not its allies'.
    pub count_owned_only: bool,
    #[default(1.0)]
    pub exponent_over_one: f32,
    #[default(1.0)]
    pub exponent_under_one: f32,
}

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Factor {
    pub minimum: f32,
    pub multiplier: f32,
    #[default(-1.0)]
    pub maximum: f32,
}

/// What an operation that gives resources makes.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct EconomyGenerated {
    pub minimum: f32,
    pub multiplier: f32,
    pub resource: Vec<Generated>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Generated {
    pub ids: Option<Id>,
    pub amount: f32,
}

#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Research {
    #[default(100.0)]
    pub base_priority: f32,
    pub tech_priority: Vec<TechPriority>,
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
#[derive(SmartDefault, Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Reacting {
    /// Seconds of play between reactions.
    #[default(2.0)]
    pub think_frequency: f32,
    #[default(0.3)]
    pub unit_retreat_max_health: f32,
    #[default(0.9)]
    pub unit_retreat_cv_ratio: f32,
    #[default(1.0)]
    pub unit_retreat_probability: f32,
    #[default(2)]
    pub attack_distance: u32,
    #[default(1.0)]
    pub min_attack_advantage: f32,
    #[default(1.5)]
    pub max_attack_advantage: f32,
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
        let sends_air_attacks = self.svar(|v| v.use_air_attacks).unwrap_or(true);
        let explored_share = self
            .svar(|v| v.reset_white_fog_timer_percent)
            .unwrap_or(0.1);
        let blocks: Vec<SaiBlock> = files.iter().flat_map(|f| f.read("SAI")).collect();
        let egos: Vec<Ego> = files.iter().flat_map(|f| f.read("Ego")).collect();
        ids.into_iter()
            .filter_map(|id| {
                let block = blocks.iter().find(|s| s.ids.as_ref() == Some(id))?;
                Some(Profile {
                    id: id.clone(),
                    ego_fuzziness: block.ego_fuzziness,
                    max_ge_commands: block.max_ge_commands,
                    max_rai_commands: block.max_rai_commands,
                    moods: moods(block, &egos, id),
                    sends_air_attacks,
                    explored_share,
                })
            })
            .collect()
    }
}

/// The moods `block` names that `egos` define, its initial one first, each
/// once.
fn moods(block: &SaiBlock, egos: &[Ego], id: &Id) -> Vec<Ego> {
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
    moods
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
        let files = scoped(&files);
        let id = Id::from("test_sai");
        let blocks: Vec<SaiBlock> = files.iter().flat_map(|f| f.read("SAI")).collect();
        let egos: Vec<Ego> = files.iter().flat_map(|f| f.read("Ego")).collect();
        let block = blocks.iter().find(|s| s.ids.as_ref() == Some(&id)).unwrap();
        assert_eq!(block.ego_fuzziness, 10.0);
        let moods = moods(block, &egos, &id);
        let names: Vec<_> = moods.iter().map(|m| m.ids.clone().unwrap()).collect();
        assert_eq!(names, [Id::from("low_ego"), Id::from("main_ego")]);
        let low = &moods[0];
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
        assert!(moods[1].filters.item.is_empty());
    }
}
