//! Typed forms of the game's `.tgi` blocks, named after the block kinds they
//! read. Each holds only the keys something uses so far.

use serde::Deserialize;

use crate::Id;

#[derive(Debug, Clone, Deserialize)]
pub struct Terrain {
    pub ids: Option<Id>,
    pub terrain_color: Option<[f32; 3]>,
    #[serde(default)]
    pub base_priority: i32,
    /// Land units can cross it.
    #[serde(default = "yes")]
    pub land: bool,
    /// Ships can cross it.
    #[serde(default)]
    pub water: bool,
    /// `LAND`, `MOUNTAIN` or `WATER`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// What effects take it for, e.g. `dirt`, `snow` or `water`.
    pub terrain_group: Option<Id>,
    /// The range a mountain's tiles are raised by, each its own amount.
    pub noise_min: Option<f32>,
    pub noise_max: Option<f32>,
    /// Path of a water terrain's animation frames, numbered by a `%04d`.
    pub anim_textures_path_prefix: Option<String>,
    pub anim_fps: Option<f32>,
    /// The shared [`Property`]s it gives those standing on it.
    #[serde(default)]
    pub property: Vec<PropertyRef>,
    #[serde(default)]
    pub variation: Vec<Variation>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct Variation {
    pub ids: Option<Id>,
    pub texture: Option<String>,
    pub texture_width: Option<f32>,
    pub texture_height: Option<f32>,
    /// Negative where it takes its terrain's.
    pub terrain_color: Option<[f32; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Biome {
    #[serde(default)]
    pub water_info: Vec<WaterInfo>,
}

/// How a biome's water terrain is coloured.
#[derive(Debug, Clone, Deserialize)]
pub struct WaterInfo {
    pub terrain_ids: Option<Id>,
    #[serde(default)]
    pub depth_info: Vec<DepthInfo>,
}

/// One stop of a water's colour by depth.
#[derive(Debug, Clone, Deserialize)]
pub struct DepthInfo {
    pub depth: f32,
    pub color: [f32; 3],
    pub alpha: f32,
}

/// An object template: units, buildings, doodads, sound emitters, ...
#[derive(Debug, Clone, Deserialize)]
pub struct Thing {
    pub ids: Option<Id>,
    /// A `#` key into the localized strings.
    pub name: Option<String>,
    /// Whether a side numbers those it has, as "1st Infantry Regiment".
    pub numbered: Option<bool>,
    /// On what's recruited, the transport that carries it to where it's
    /// sent, which puts it in a recruiter's Deploy to Location menu.
    pub transport_actor: Option<Id>,
    /// Whether a recruiter hides it, rather than greys it, while the
    /// properties it needs are missing.
    pub hide_if_missing_properties: Option<bool>,
    #[serde(default)]
    pub flags: Vec<Flags>,
    pub view: Option<View>,
    #[serde(default)]
    pub size: Vec<Size>,
    pub character_component: Option<CharacterComponent>,
    pub mover_component: Option<MoverComponent>,
    pub body_component: Option<BodyComponent>,
    pub properties_component: Option<PropertiesComponent>,
    pub performer_component: Option<PerformerComponent>,
    pub defender_component: Option<DefenderComponent>,
    pub city_component: Option<CityComponent>,
    pub organization_component: Option<OrganizationComponent>,
    pub projectile_component: Option<ProjectileComponent>,
    pub recruit_component: Option<RecruitComponent>,
    pub research_component: Option<ResearchComponent>,
    pub mobile_deployment_component: Option<MobileDeploymentComponent>,
    pub economy_component: Option<EconomyComponent>,
    pub supply_component: Option<SupplyComponent>,
    #[serde(rename = "HQComponent")]
    pub hq_component: Option<HqComponent>,
    pub denizen_component: Option<DenizenComponent>,
    pub element_component: Option<ElementComponent>,
    pub detection_component: Option<DetectionComponent>,
    pub placement: Option<Placement>,
    /// On entering and leaving the states named by each command's
    /// `actor_state`.
    #[serde(default)]
    pub when_enter: Vec<Fxc>,
    #[serde(default)]
    pub when_leave: Vec<Fxc>,
    #[serde(default)]
    pub component: Vec<Component>,
}

/// A component named by its template, e.g. a doodad's
/// `DisturbableDynamicDoodadComponent`.
#[derive(Debug, Clone, Deserialize)]
pub struct Component {
    /// What it does when set off.
    #[serde(default)]
    pub action: Vec<Fxc>,
}

/// An effects command, run when the event its block is named after happens;
/// its template sets its `type`. Declared in `Templates/template_fx.tgi`,
/// which documents the fields.
#[derive(Debug, Clone, Deserialize)]
pub struct Fxc {
    /// E.g. `PLAY_SFX` or `CREATE_VFX`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// The [`Audio`] a `PLAY_SFX` plays.
    pub audio_ids: Option<Id>,
    /// The [`Vfx`] a `CREATE_VFX` creates or a `STOP_VFX` stops.
    pub vfx_ids: Option<Id>,
    /// Where it happens, e.g. `SELF` or `TARGET_GROUND`.
    pub at: Option<String>,
    /// The point of the model it happens at, e.g. `cannon_ae_01`.
    pub location: Option<String>,
    pub attached: Option<bool>,
    pub translation: Option<[f32; 3]>,
    pub orientation: Option<[f32; 3]>,
    /// Seconds the effect lasts, 0 for the effect's own.
    pub duration: Option<f32>,
    pub looping: Option<bool>,
    /// Seconds after the event.
    pub delay: Option<f32>,
    /// It runs on one in this many of the events, never for 0.
    pub frequency: Option<u32>,
    pub actor_state: Option<String>,
    /// The terrain group it's limited to.
    pub terrain_group_filter: Option<String>,
    /// Flags the event's target must have or lack, written like an
    /// ability's `targets`.
    pub target_flags: Option<String>,
    /// For `WhenHealth`, the share of its health whose crossing runs it,
    /// and which way.
    pub health: Option<f32>,
    pub health_trigger_type: Option<String>,
    pub stop_on_destructed: Option<bool>,
    /// Run when this one's effect ends, and alongside it.
    #[serde(default)]
    pub chain: Vec<Fxc>,
    #[serde(default)]
    pub chain_immediate: Vec<Fxc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Placement {
    /// The group of templates a [`Nation`] fields, e.g. `group_nation_rus`.
    pub placement_group_ids: Option<Id>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BodyComponent {
    pub health: Option<f32>,
    /// Run as its health crosses each command's `health`.
    #[serde(default)]
    pub when_health: Vec<Fxc>,
    pub defense: Option<f32>,
    pub captureable: Option<bool>,
    pub destructible: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CityComponent {
    pub zone_radius: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectileComponent {
    pub projectile_speed: Option<f32>,
    /// `ANTIGRAV`, `LOB` or `BOMB`.
    pub projectile_type: Option<String>,
    /// Seconds it flies, if above 0.
    pub projectile_life: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrganizationComponent {
    pub operational_range: Option<f32>,
    pub control_range: Option<f32>,
    /// Seconds recruiting one takes.
    pub recruit_time: Option<f32>,
    /// The [`OrganizationLayout`] its members are arranged by.
    pub layout_ids: Option<Id>,
    /// What fills each of the layout's slots, in the layout's order.
    #[serde(default)]
    pub slot: Vec<OrganizationSlot>,
    /// Whether it's in or out of supply at all.
    pub supply_used: Option<bool>,
    /// Whether it can be in supply without an HQ.
    pub in_supply_detached: Option<bool>,
    /// The share of their health its members lose a second out of supply
    /// while their side can't pay its upkeep.
    pub shortfall_damage_rate: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HqComponent {
    /// How many regiments it takes.
    pub max_attachments: Option<u32>,
}

/// What an object can recruit, which for a Corps HQ is what it commissions.
#[derive(Debug, Clone, Deserialize)]
pub struct RecruitComponent {
    #[serde(default)]
    pub recruit_actor: Vec<RecruitActor>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecruitActor {
    pub actor_ids: Option<Id>,
}

/// The technologies an object can research.
#[derive(Debug, Clone, Deserialize)]
pub struct ResearchComponent {
    #[serde(default)]
    pub research_item: Vec<ResearchItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResearchItem {
    pub property_ids: Option<Id>,
    /// Who gets the property: `GLOBAL`, `LOCAL` or `SUPPORT`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub research_time: Option<f32>,
    pub research_cost: Option<Resources>,
    /// Comma separated templates of the buildings its owner needs.
    pub required_structures: Option<String>,
    #[serde(default)]
    pub required_structures_any: bool,
    /// Comma separated properties the researcher or its side needs.
    pub required_properties: Option<String>,
    #[serde(default)]
    pub required_properties_any: bool,
}

/// Packing a building into a vehicle and unpacking it again.
#[derive(Debug, Clone, Deserialize)]
pub struct MobileDeploymentComponent {
    /// On a packed building, the template it unpacks into.
    pub unpack_to_actor_ids: Option<Id>,
    /// Where the vehicle stands from the building it packs from, and the
    /// way it faces there.
    pub packed_position_offset: Option<[f32; 2]>,
    pub packed_orientation: Option<[f32; 2]>,
    /// On a building, the share of its health at or below which it can't
    /// pack, and the seconds packing and unpacking take.
    pub pack_health_min: Option<f32>,
    pub packing_length: Option<f32>,
    pub unpacking_length: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EconomyComponent {
    pub cost: Option<Resources>,
    /// What it takes to keep, for as long as it's kept.
    pub upkeep: Option<Resources>,
    /// What it earns its owner, per minute.
    pub production: Option<Resources>,
    /// Whether it earns nothing while out of supply.
    pub supply_required: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SupplyComponent {
    pub supply_range: Option<f32>,
    /// Whether it has to be supplied itself to supply others.
    pub is_relay: Option<bool>,
    /// Seconds a relay keeps going once cut off.
    pub stockpile_max: Option<f32>,
}

/// Amounts of the resources the data's costs, upkeeps and production name, each
/// called `resources_<field>_name` in the localized strings.
#[derive(Debug, Clone, Deserialize)]
pub struct Resources {
    pub money: Option<f32>,
    pub ammo: Option<f32>,
    pub oil: Option<f32>,
    pub unit_limit_consumed: Option<f32>,
    pub building_limit_consumed: Option<f32>,
    pub unit_limit_provided: Option<f32>,
    pub building_limit_provided: Option<f32>,
}

impl Resources {
    /// Each resource given, by field name.
    pub fn amounts(&self) -> impl Iterator<Item = (&'static str, f32)> {
        [
            ("money", self.money),
            ("ammo", self.ammo),
            ("oil", self.oil),
            ("unit_limit_consumed", self.unit_limit_consumed),
            ("building_limit_consumed", self.building_limit_consumed),
            ("unit_limit_provided", self.unit_limit_provided),
            ("building_limit_provided", self.building_limit_provided),
        ]
        .into_iter()
        .filter_map(|(k, v)| Some((k, v?)))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ElementComponent {
    /// Health it gets back a second in supply, as a share of its own;
    /// recruiting one alone takes its inverse in seconds.
    pub resupply_rate: Option<f32>,
    /// Comma separated templates of the buildings its owner needs to
    /// recruit it, read into a [`Requires`](crate::Requires).
    pub required_structures: Option<String>,
    #[serde(default)]
    pub required_structures_any: bool,
    /// Comma separated properties the recruiter, or for a technology its
    /// side, needs to recruit it.
    pub required_properties: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DenizenComponent {
    pub deployment_info: Option<DeploymentInfo>,
    pub guard_range: Option<f32>,
    pub guard_operational_range: Option<f32>,
}

/// How far a thing sees, in world units.
#[derive(Debug, Clone, Deserialize)]
pub struct DetectionComponent {
    pub detection_range: Option<f32>,
    /// How far round it its enemies see while they fight it.
    pub remote_detection_range: Option<f32>,
}

/// Where what an object recruits appears, from its root node: x across its
/// facing and y along it. Declared in `Templates/template_deployment.tgi`.
#[derive(Debug, Clone, Deserialize)]
pub struct DeploymentInfo {
    pub creation_offset: Option<[f32; 2]>,
    /// Where it then moves to, in a straight line.
    pub deployment_offset: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrganizationSlot {
    /// The template of the slot's members; none for a slot left empty.
    pub element_ids: Option<Id>,
}

/// The properties an object has.
#[derive(Debug, Clone, Deserialize)]
pub struct PropertiesComponent {
    /// References to shared [`Property`]s.
    #[serde(default)]
    pub property: Vec<PropertyRef>,
    /// Properties of this template's own.
    #[serde(default)]
    pub intrinsic_property: Vec<Property>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PropertyRef {
    pub property_ids: Option<Id>,
}

/// The game's word for a range of some figure, such as a speed.
#[derive(Debug, Clone, Deserialize)]
pub struct Description {
    /// What the figure is, e.g. `SPEED`.
    pub category: Option<String>,
    /// A `#` key into the localized strings.
    pub description: Option<String>,
    /// The least it's the word for, and the least it isn't.
    pub range: Option<[f32; 2]>,
}

/// A property objects can have, shared at the top level or an object's own.
#[derive(Debug, Clone, Deserialize)]
pub struct Property {
    pub ids: Option<Id>,
    /// A `#` key into the localized strings.
    pub name: Option<String>,
    pub description: Option<String>,
    /// Whether a side can hold it, rather than only the objects that carry
    /// it.
    #[serde(default)]
    pub is_technology: bool,
    /// The icon it shows by beside what holds it, relative to the defining
    /// file; `%s` stands for the folder for the screen's size.
    pub icon: Option<String>,
    /// Its research button's icon, relative to the defining file.
    pub action_button_icon: Option<String>,
    /// Comma separated, the flags it gives its objects for abilities'
    /// `targets` to match.
    pub enabled_target_flags: Option<String>,
    /// Comma separated, the properties an object needs for a technology
    /// to be on it.
    pub required_properties: Option<String>,
    /// Seconds it lasts, for one an ability gives.
    pub duration: Option<f32>,
    #[serde(default)]
    pub modifier: Vec<Modifier>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Modifier {
    /// What it modifies, e.g. `RESIST_DAMAGE`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub damage_type: Option<Id>,
    /// What an `ADD` or `MULTIPLY` one changes, e.g. `SUPPLY_RANGE`.
    pub attribute: Option<String>,
    /// What a `FLAG` one sets, e.g. `SUPPLIED`.
    pub modifier_flag: Option<String>,
    pub amount: Option<f32>,
}

/// What an object can do to others.
#[derive(Debug, Clone, Deserialize)]
pub struct PerformerComponent {
    #[serde(default)]
    pub check_damage_resistance: bool,
    #[serde(default)]
    pub ability: Vec<Ability>,
    /// How high above its user an attack starts, unless it says.
    pub attack_height: Option<f32>,
    #[serde(default)]
    pub turret: Vec<Turret>,
}

/// A part of an object that turns to aim its abilities. Left-out fields
/// take the `Turret` template's values.
#[derive(Debug, Clone, Deserialize)]
pub struct Turret {
    pub turret_ids: Option<Id>,
    /// Degrees a second.
    #[serde(default = "full_turn")]
    pub yaw_rotation_per_sec_degrees: f32,
    /// The whole sweep it turns within, centred on where it starts.
    #[serde(default = "full_turn")]
    pub yaw_rotation_range_degrees: f32,
    /// Where it points from its object's facing, counter-clockwise.
    #[serde(default)]
    pub yaw_start_degrees: f32,
    /// Seconds without turning to anything before it turns back.
    #[serde(default = "reset_time")]
    pub reset_time: f32,
    #[serde(default = "yes")]
    pub can_reset: bool,
    /// Where it stands on its object, as [`crate::Turret::offset`] reads it.
    pub offset: Option<[f32; 2]>,
}

fn full_turn() -> f32 {
    360.0
}

fn reset_time() -> f32 {
    10.0
}

/// What a building fires on its own, at whatever comes near.
#[derive(Debug, Clone, Deserialize)]
pub struct DefenderComponent {
    /// How near something must come for it to notice it.
    pub control_range: Option<f32>,
    #[serde(default)]
    pub ability: Vec<Ability>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ability {
    /// The ids of the animation its user plays to use it.
    pub animation1: Option<String>,
    /// The bone of the turret that aims it, if it has one.
    pub turret_ids: Option<String>,
    /// Where its projectiles appear, from its user, forward along -y.
    pub spawn_position: Option<[f32; 3]>,
    pub ability_definition: Option<AbilityDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AbilityDefinition {
    pub ids: Option<Id>,
    #[serde(default)]
    pub flags: Vec<AbilityFlags>,
    /// E.g. `attack`.
    pub ability_type: Option<String>,
    pub range: Option<f32>,
    #[serde(default)]
    pub min_range: f32,
    /// Seconds between uses.
    #[serde(default)]
    pub recharge_time: f32,
    /// Radius of the area it hits, 0 for only its target.
    #[serde(default)]
    pub area: f32,
    /// The most an area hit lands on, 0 for all it reaches.
    pub max_targets: Option<u32>,
    /// Within this distance of the centre an area hit does full damage.
    pub attenuation_range_min: Option<f32>,
    /// The fraction of damage an area hit does at its edge.
    pub attenuation_factor_max: Option<f32>,
    /// Comma separated flags, each `+` to require it or `-` to rule it out.
    pub targets: Option<String>,
    #[serde(default)]
    pub effect: Vec<Effect>,
    /// The projectile template a shot fires.
    pub projectile_ids: Option<Id>,
    /// Run as a use starts, each time it's used, once for each target a use
    /// lands on, and as a use ends.
    #[serde(default)]
    pub when_start: Vec<Fxc>,
    #[serde(default)]
    pub when_do: Vec<Fxc>,
    #[serde(default)]
    pub when_do_per_target: Vec<Fxc>,
    #[serde(default)]
    pub when_end: Vec<Fxc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Effect {
    /// What it does, e.g. `MAIN_DAMAGE`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub damage_type_ids: Option<Id>,
    pub amount: Option<f32>,
    /// The property it gives what it lands on, for a time.
    pub enchantment_property: Option<Property>,
    /// The template of what it puts on the map.
    pub object_ids: Option<Id>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AbilityFlags {
    /// An order to bombard can use it.
    #[serde(default)]
    pub bombard: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MoverComponent {
    /// World units a second.
    pub speed_max: Option<f32>,
    /// World units a second gained each second; 0 to reach top speed at
    /// once.
    pub acceleration: Option<f32>,
    /// The radius of the circle it drives round when turning, for turn
    /// types other than `INSTANT`.
    pub turn_radius: Option<f32>,
    /// `INSTANT`, `IN_PLACE` or `WHILE_MOVING`, in any case.
    pub turn_type: Option<String>,
    /// The template of the trailer it tows.
    pub trailer_actor: Option<Id>,
    /// Where it's coupled to what tows it or what it tows.
    pub trailer_hitch: Option<TrailerHitch>,
    #[serde(default)]
    pub when_start_moving: Vec<Fxc>,
    #[serde(default)]
    pub when_stop_moving: Vec<Fxc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrailerHitch {
    /// From the object's middle: x to its right, y ahead and z up.
    pub node_offset: Option<[f32; 3]>,
}

/// How a unit's model is animated.
#[derive(Debug, Clone, Deserialize)]
pub struct CharacterComponent {
    #[serde(default)]
    pub animation: Vec<Animation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Animation {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub ids: Option<String>,
    /// The `.kf` file, with `%s`s for the view's resource prefix and the ids.
    pub kf: Option<String>,
    pub length: Option<f32>,
    /// For moving animations, how far one play carries the unit.
    pub walk_distance: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Flags {
    #[serde(default)]
    pub invisible: bool,
    pub blocking: Option<bool>,
    /// Its model tilts to follow the ground under its soft shape.
    pub conform_to_terrain: Option<bool>,
    /// It floats on water rather than standing on the bed under it.
    pub water_based: Option<bool>,
    /// It keeps its own height rather than standing on the ground.
    pub flying: Option<bool>,
    /// It stands on the half-tile grid.
    pub tile_locked: Option<bool>,
    /// Nothing gets in its way.
    pub ethereal: Option<bool>,
    /// Every side has it explored from the start, when the match's fog
    /// starts explored.
    pub revealed: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Size {
    pub scale_min: Option<f32>,
    pub scale_max: Option<f32>,
    /// `CIRCLE`, `SQUARE`, `RECT` or `POINT`, sized by the fields below.
    pub blocking_shape: Option<String>,
    pub blocking_radius: Option<f32>,
    /// The outline it occupies the ground with, sized by the fields of the
    /// blocking shape but for a circle's `soft_radius`; `null` for the
    /// blocking shape's kind.
    pub soft_shape: Option<String>,
    pub soft_radius: Option<f32>,
    /// The outline weapons' ranges run to, sized as the soft shape but for
    /// a circle's `target_radius`; `null` for the soft shape itself.
    pub target_shape: Option<String>,
    pub target_radius: Option<f32>,
    pub square_size: Option<f32>,
    pub rect_width: Option<f32>,
    pub rect_height: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct View {
    /// Substituted for `%s` in model paths.
    pub resource_prefix: Option<String>,
    #[serde(default)]
    pub aspect: Vec<Aspect>,
    #[serde(default)]
    pub aspect_normal: Vec<Aspect>,
    #[serde(default)]
    pub aspects_as_variations: bool,
    #[serde(default)]
    pub texture_search_path: Vec<TextureSearchPath>,
    /// The image on the button that commissions or recruits it.
    pub icon: Option<String>,
    /// The image on an organization's banner.
    pub banner_icon: Option<String>,
    /// The image marking it on the minimap, white to take its owner's
    /// colour, drawn square to the map's sides.
    pub minimap_icon: Option<String>,
    #[serde(default)]
    pub when_selected: Vec<Fxc>,
    /// Given an order, or an order to attack.
    #[serde(default)]
    pub when_commanded: Vec<Fxc>,
    #[serde(default)]
    pub when_commanded_aggressive: Vec<Fxc>,
    #[serde(default)]
    pub when_created: Vec<Fxc>,
    #[serde(default)]
    pub when_destroyed: Vec<Fxc>,
    /// When a building is finished.
    #[serde(default)]
    pub when_constructed: Vec<Fxc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Aspect {
    /// E.g. `aspect_unpacking`, for the state it's shown in.
    pub ids: Option<Id>,
    pub model: Option<String>,
    #[serde(default)]
    pub texture_rename: Vec<TextureRename>,
    #[serde(default)]
    pub animation: Vec<ObjectAnimation>,
}

/// An animation of an aspect's model, as opposed to a unit's
/// [`Animation`]s. Declared in `Templates/template_animation.tgi`.
#[derive(Debug, Clone, Deserialize)]
pub struct ObjectAnimation {
    pub ids: Option<String>,
    /// E.g. `RANDOM`, which plays every `start_min` to `start_max`
    /// seconds, or `SELECTED`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub start_min: Option<f32>,
    pub start_max: Option<f32>,
    /// Played as it starts.
    #[serde(default)]
    pub play_sfx: Vec<PlaySfx>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaySfx {
    pub sfx_ids: Option<Id>,
    #[serde(default)]
    pub looping: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextureSearchPath {
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextureRename {
    pub substring_from: Option<String>,
    pub substring_to: Option<String>,
}

/// How an organization's members are arranged.
#[derive(Debug, Clone, Deserialize)]
pub struct OrganizationLayout {
    pub ids: Option<Id>,
    /// The first formation's when not set.
    pub default_formation_ids: Option<Id>,
    /// The one it fights in once engaged.
    pub combat_formation_ids: Option<Id>,
    #[serde(default)]
    pub slot: Vec<LayoutSlot>,
    #[serde(default)]
    pub formation: Vec<LayoutFormation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LayoutSlot {
    /// The property a member needs to fill it.
    pub property_ids: Option<String>,
    #[serde(default)]
    pub primary: bool,
    /// How many times the slot repeats.
    pub number: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LayoutFormation {
    pub formation_ids: Option<Id>,
    #[serde(default)]
    pub position: Vec<FormationPosition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FormationPosition {
    /// The property of the slots that take it.
    pub property_ids: Option<String>,
    /// From the primary slot's position, with 0,1 straight ahead.
    pub offset: Option<[f32; 2]>,
}

/// A renamed id, which maps made before the rename still use.
#[derive(Debug, Clone, Deserialize)]
pub struct Translate {
    /// The kind of block renamed, e.g. `Thing`.
    pub node: Option<String>,
    pub ids_old: Option<Id>,
    pub ids_new: Option<Id>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionSet {
    /// Start of the mask file names, relative to the transition set's file.
    pub path_prefix: Option<String>,
}

/// A lighting preset the player can pick, with the colours a map's sun takes.
#[derive(Debug, Clone, Deserialize)]
pub struct Sun {
    pub ids: Option<Id>,
    pub diffuse: [f32; 3],
    pub ambient: [f32; 3],
    pub specular: [f32; 3],
}

/// How a nation's banners look.
#[derive(Debug, Clone, Deserialize)]
pub struct Banner {
    pub ids: Option<Id>,
    /// What its own images' paths start with, under the banners' folder.
    pub resource_prefix: Option<String>,
}

/// A nation players can play.
#[derive(Debug, Clone, Deserialize)]
pub struct Nation {
    pub ids: Option<Id>,
    pub name: Option<String>,
    /// The group of the templates it fields.
    pub placement_group_ids: Option<Id>,
    /// What its divisions wear on their banners, each division one.
    #[serde(default)]
    pub badge: Vec<Badge>,
    #[serde(default)]
    pub placeholder: Vec<Placeholder>,
    /// The ids of the generals who lead it, comma-separated.
    pub factions: Option<String>,
    /// The lobby's image of its flag.
    pub flag_icon: Option<String>,
    /// The music its player hears in a match.
    pub game_music: Option<Id>,
}

/// A bonus or penalty the lobby can give a side the computer plays.
#[derive(Debug, Clone, Deserialize)]
pub struct Handicap {
    pub ids: Option<Id>,
    pub name: Option<String>,
    pub description: Option<String>,
    /// The technology the side starts with, or `null`.
    pub property_ids: Option<String>,
}

/// A general a side can be led by.
#[derive(Debug, Clone, Deserialize)]
pub struct General {
    pub ids: Option<Id>,
    pub name: Option<String>,
    /// What the lobby calls them.
    pub staging_name: Option<String>,
    /// False for generals only scenarios use.
    pub public: Option<bool>,
    #[serde(default)]
    pub ability: Vec<GeneralAbility>,
}

/// A special operation a general gives their side.
#[derive(Debug, Clone, Deserialize)]
pub struct GeneralAbility {
    pub ability_ids: Option<Id>,
}

/// A template of the nation's that a map's placeholder stands for, such as
/// its Corps HQ.
#[derive(Debug, Clone, Deserialize)]
pub struct Placeholder {
    pub placeholder_ids: Option<Id>,
    pub thing_ids: Option<Id>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Badge {
    /// Its image, relative to `Nations/`.
    pub badge_texture: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KingdomColor {
    pub ids: Option<Id>,
    pub color: [f32; 3],
}

/// The minimap's colours: static for buildings, mobile for units, by how
/// their owner stands toward the player.
#[derive(Debug, Clone, Deserialize)]
pub struct Minimap {
    pub color_object_border: [f32; 3],
    pub color_owned_static: [f32; 3],
    pub color_owned_mobile: [f32; 3],
    pub color_allied_static: [f32; 3],
    pub color_allied_mobile: [f32; 3],
    pub color_enemy_static: [f32; 3],
    pub color_enemy_mobile: [f32; 3],
    pub color_neutral_static: [f32; 3],
    pub color_neutral_mobile: [f32; 3],
    /// What the player's own things blink in while they're being fought,
    /// and while they're out of the supply they need.
    pub color_under_siege: Option<[f32; 3]>,
    pub color_requires_supply: Option<[f32; 3]>,
}

/// An order the player can give what they've selected, shown as a button.
#[derive(Debug, Clone, Deserialize)]
pub struct ActorCommand {
    pub ids: Option<Id>,
    /// A `#` key into the localized strings.
    pub name: Option<String>,
    /// `#` keys into the localized strings: what it does, and why it
    /// can't be given when it can't.
    pub tooltip: Option<String>,
    pub description_disabled: Option<String>,
    pub icon: Option<String>,
    /// Which of the action buttons it takes, numbered across then down;
    /// negative for none.
    pub preferred_position: Option<i32>,
    /// Whether, its button taken, it takes the last free one rather than
    /// the first.
    #[serde(default)]
    pub high_importance: bool,
    /// The cursor shown while the order waits for a place or target.
    pub cursor_ids: Option<Id>,
    /// The [`Vfx`] marking where the order was given.
    pub command_vfx_ids_location: Option<Id>,
    /// Whether each of a group keeps its place relative to the rest, rather
    /// than all going to the one place; true unless set.
    pub apply_group_offset: Option<bool>,
}

/// A mouse cursor, drawn from numbered frames that loop.
#[derive(Debug, Clone, Deserialize)]
pub struct Cursor {
    pub ids: Option<Id>,
    pub loop_length_ms: Option<f32>,
    /// The pixel that points, from the top left.
    pub hotspot_offset: Option<[f32; 2]>,
    /// Relative to its file's folder, with `%d` for the frame number.
    pub frame_filename: Option<String>,
}

/// A character quoted text names by id.
#[derive(Debug, Clone, Deserialize)]
pub struct Glyph {
    pub ids: Option<Id>,
    /// The character's code.
    pub index: Option<u32>,
    /// What the font's image of it is called, e.g. `Rank1`.
    pub display_name: Option<String>,
}

/// The game's tuning settings.
#[derive(Debug, Clone, Deserialize)]
pub struct SVars {
    #[serde(rename = "BuildingPaddingMeters")]
    pub building_padding_meters: Option<f32>,
    /// How far up its target a projectile flying straight aims.
    #[serde(rename = "ProjectileTargetHeightRatio")]
    pub projectile_target_height_ratio: Option<f32>,
    /// How far up its target a lobbed or dropped projectile aims.
    #[serde(rename = "ProjectileArcingTargetHeightRatio")]
    pub projectile_arcing_target_height_ratio: Option<f32>,
    /// The most a resource that limits another provides, in total.
    #[serde(rename = "EconomyLimitedResourceMax")]
    pub economy_limited_resource_max: Option<f32>,
    /// The most a stockpile holds.
    #[serde(rename = "EconomyStockpileResourceMax")]
    pub economy_stockpile_resource_max: Option<f32>,
    /// What stockpiles start at on new and random maps.
    #[serde(rename = "ResourceStockpileDefault")]
    pub resource_stockpile_default: Option<f32>,
    /// Seconds a connected relay's stockpile gains each second.
    #[serde(rename = "SupplyStockpileRechargeRate")]
    pub supply_stockpile_recharge_rate: Option<f32>,
    /// How much nearer the intruder a guard is already after counts.
    #[serde(rename = "GuardOldTargetBias")]
    pub guard_old_target_bias: Option<f32>,
    /// The lowest a terrain's speed limit comes to.
    #[serde(rename = "TerrainMinimumMoveSpeed")]
    pub terrain_minimum_move_speed: Option<f32>,
    /// Seconds a bombardment's hit keeps a regiment out of supply.
    #[serde(rename = "BombardTargetTimeout")]
    pub bombard_target_timeout: Option<f32>,
    /// It's night while the sun's diffuse red, green and blue sum to less
    /// than this.
    #[serde(rename = "SunNightThreshold")]
    pub sun_night_threshold: Option<f32>,
    /// Seconds an enemy's damage keeps what it hurt under siege.
    #[serde(rename = "UnderSiegeRecentlyDamagedSeconds")]
    pub under_siege_recently_damaged_seconds: Option<f32>,
    /// The least a multiplier that modifiers add up to comes to.
    #[serde(rename = "ModifierAttributeClampMultiply")]
    pub modifier_attribute_clamp_multiply: Option<f32>,
    /// Morale a second a regiment regaining it gains more in supply.
    #[serde(rename = "MoraleSupplyBonus")]
    pub morale_supply_bonus: Option<f32>,
    /// 0, 1 or 2 for no fog, explored or full; the world rule overrides it.
    #[serde(rename = "FogOfWar")]
    pub fog_of_war: Option<u8>,
}

/// The interface's settings.
#[derive(Debug, Clone, Deserialize)]
pub struct UVars {
    /// What marks a figure or option that's no good, such as a shortfall.
    #[serde(rename = "UIColorOptionInvalid")]
    pub ui_color_option_invalid: Option<[f32; 3]>,
    /// What marks the key for something in its name.
    #[serde(rename = "UIColorHighlight")]
    pub ui_color_highlight: Option<[f32; 3]>,
    /// The id of the [`HotkeyConfiguration`] in use.
    #[serde(rename = "HotkeyConfiguration")]
    pub hotkey_configuration: Option<Id>,
    /// The options' volumes, as they start.
    #[serde(rename = "AudioMainVolume")]
    pub audio_main_volume: Option<f32>,
    #[serde(rename = "AudioSpeechVolume")]
    pub audio_speech_volume: Option<f32>,
    #[serde(rename = "AudioMusicVolume")]
    pub audio_music_volume: Option<f32>,
}

/// A set of hotkeys players can choose.
#[derive(Debug, Clone, Deserialize)]
pub struct HotkeyConfiguration {
    pub ids: Option<Id>,
    /// What the options call it, or the key of the string that does.
    pub name: Option<String>,
    /// The archive path of the file binding its keys.
    pub hotkey_file_location: Option<String>,
}

/// The settings for how the game looks.
#[derive(Debug, Clone, Deserialize)]
pub struct AVars {
    /// The supply border's width in world units, its colour, and the image
    /// drawn along it, by its archive path.
    #[serde(rename = "ViewZoneFriendlySupplyWidth")]
    pub view_zone_friendly_supply_width: Option<f32>,
    #[serde(rename = "ViewZoneFriendlySupplyColor")]
    pub view_zone_friendly_supply_color: Option<[f32; 3]>,
    #[serde(rename = "ViewZoneFriendlySupplyTexture")]
    pub view_zone_friendly_supply_texture: Option<String>,
    /// The side, in world units, of the squares the terrain is drawn in.
    #[serde(rename = "TerrainBlockSize")]
    pub terrain_block_size: Option<f32>,
    /// What streamed audio plays in place of a file the archives don't have.
    #[serde(rename = "DefaultAudioStreamingResource")]
    pub default_audio_streaming_resource: Option<String>,
    /// The music that plays nothing, for stopping music.
    #[serde(rename = "AudioMusicSilent")]
    pub audio_music_silent: Option<Id>,
    /// What other audio falls to while audio that fades it plays, over how
    /// many seconds, and how long after the last such it starts to recover.
    #[serde(rename = "AudioFadeVolume")]
    pub audio_fade_volume: Option<f32>,
    #[serde(rename = "AudioFadeTime")]
    pub audio_fade_time: Option<f32>,
    #[serde(rename = "AudioRecoverFromFadeTime")]
    pub audio_recover_from_fade_time: Option<f32>,
    /// The match music of someone watching rather than playing.
    #[serde(rename = "ObserverGameMusic")]
    pub observer_game_music: Option<Id>,
}

/// A screen of the interface.
#[derive(Debug, Clone, Deserialize)]
pub struct Interface {
    /// The music it asks for when shown; none leaves what's playing.
    pub interface_music: Option<Id>,
}

/// One of the resources sides earn and spend.
#[derive(Debug, Clone, Deserialize)]
pub struct Resource {
    pub ids: Option<Id>,
    /// The resource that caps how much of it can be used.
    pub limited_by: Option<Id>,
    /// What a unit of it costs, for those that can be bought.
    pub purchase_cost: Option<Resources>,
}

/// A setting a match is played under, which the lobby can change.
#[derive(Debug, Clone, Deserialize)]
pub struct WorldRule {
    pub ids: Option<Id>,
    /// What it's set to unless changed, a number or a choice's id.
    pub default: Option<String>,
}

/// A visual effect. Its base templates are declared in
/// `Templates/template_vfx.tgi`, which documents the fields.
#[derive(Debug, Clone, Deserialize)]
pub struct Vfx {
    pub ids: Option<Id>,
    /// What kind it is, e.g. `VFXMODEL` or `VFXTRACER`.
    pub category: Option<String>,
    /// Relative to its file's folder.
    pub model: Option<String>,
    #[serde(default)]
    pub texture_search_path: Vec<TextureSearchPath>,
    pub permanent: Option<bool>,
    pub critical: Option<bool>,
    /// Seconds, 0 for its model's own length.
    pub duration: Option<f32>,
    pub attached_scale: Option<f32>,
    pub never_discard_temporary: Option<bool>,
    pub nightonly: Option<bool>,
    pub constant_screen_size: Option<bool>,
    pub force_real_time: Option<bool>,
    pub gyroscope: Option<bool>,
    /// What a tint multiplies its object's colours by.
    pub color: Option<[f32; 3]>,
    pub tint_max: Option<f32>,
    /// Seconds a tint takes to rise from none to full.
    pub enhance_duration: Option<f32>,
    /// Seconds a tint takes to fall from full to none.
    pub fade_duration: Option<f32>,
    pub repeating: Option<bool>,
    #[serde(default)]
    pub flags: Vec<VfxFlags>,
    /// The picture one laid on the ground shows, relative to its file's
    /// folder.
    pub texture: Option<String>,
    /// World units across that picture is.
    pub texture_size: Option<f32>,
    /// One laid on the ground is lit as the ground is.
    pub respect_lighting: Option<bool>,
    /// It's added to what's under it rather than laid over it.
    pub additive: Option<bool>,
    /// Which others laid on the ground it's drawn over, e.g. `scorch`.
    pub decal_type: Option<String>,
    /// Seconds one laid on the ground takes to show, and to go.
    pub decal_fade_time: Option<[f32; 2]>,
    /// One laid on the ground is turned a random way.
    pub random_orientation: Option<bool>,
    /// One laid on the ground is stretched to an oval round what it's
    /// attached to.
    pub oval_shape: Option<bool>,
    /// One laid on the ground takes the shape of what it's attached to.
    pub attached_shape: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VfxFlags {
    pub random_orientation: Option<bool>,
}

/// A well-known point of models, which effects name by id.
#[derive(Debug, Clone, Deserialize)]
pub struct Bone {
    pub ids: Option<Id>,
    /// The node it is in a model.
    pub bone_name: Option<String>,
}

/// A way of arranging an organization, which its layout places its slots
/// in.
#[derive(Debug, Clone, Deserialize)]
pub struct Formation {
    pub ids: Option<Id>,
    /// A `#` key into the localized strings.
    pub name: Option<String>,
    /// Relative to its file's folder unless it starts with `/`.
    pub icon: Option<String>,
    /// Whether a regiment standing or moving in it takes on enemies that
    /// come within its operational range, or runs into.
    #[serde(default)]
    pub aggressive: bool,
}

/// A sound, speech or music the game plays by id. Its base templates are
/// declared in `Templates/template_audio.tgi`, which documents the fields.
/// Percentages and `NULL`s are left as written for [`crate::Audio`] to
/// read.
#[derive(Debug, Clone, Deserialize)]
pub struct Audio {
    pub ids: Option<Id>,
    /// `SOUND`, `SPEECH` or `MUSIC`.
    pub category: Option<String>,
    /// Comma separated file lists: one of the lead-ins plays first, then the
    /// main files, then one of the fall-outs.
    pub leadin: Option<String>,
    pub files: Option<String>,
    pub fallout: Option<String>,
    pub priority: Option<String>,
    pub control_flags: Option<String>,
    pub world_flags: Option<String>,
    pub who_flags: Option<String>,
    /// How steeply it falls off, and the distance it is silent beyond.
    pub range: Option<[f32; 2]>,
    pub simultaneous_limit: Option<u32>,
    pub volume: Option<f32>,
    pub volume_adjust: Option<f32>,
    pub pitch_adjust: Option<[f32; 2]>,
    pub delay_ms: Option<[f32; 2]>,
    /// Seconds to fade in and out.
    pub fade_time_s: Option<[f32; 2]>,
    pub subtitle: Option<Subtitle>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Subtitle {
    /// Seconds, 0 for the length of the audio.
    pub duration: Option<f32>,
    /// A localisation key, e.g. `#sounds_speech_tutorial_Subtitle_text`.
    pub text: Option<String>,
}
