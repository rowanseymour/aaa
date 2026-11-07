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
    /// Path of a water terrain's animation frames, numbered by a `%04d`.
    pub anim_textures_path_prefix: Option<String>,
    pub anim_fps: Option<f32>,
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
    pub city_component: Option<CityComponent>,
    pub organization_component: Option<OrganizationComponent>,
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
/// its template sets its `type`. Declared in `Templates/template_fx.tgi`.
#[derive(Debug, Clone, Deserialize)]
pub struct Fxc {
    /// E.g. `PLAY_SFX` or `CREATE_VFX`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// The [`Audio`] a `PLAY_SFX` plays.
    pub audio_ids: Option<Id>,
    /// Where it happens, e.g. `SELF` or `TARGET_GROUND`.
    pub at: Option<String>,
    pub looping: Option<bool>,
    /// Seconds after the event.
    pub delay: Option<f32>,
    /// It runs on one in this many of the events, never for 0.
    pub frequency: Option<u32>,
    pub actor_state: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Placement {
    /// The group of templates a [`Nation`] fields, e.g. `group_nation_rus`.
    pub placement_group_ids: Option<Id>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BodyComponent {
    pub health: Option<f32>,
    pub defense: Option<f32>,
    pub captureable: Option<bool>,
    pub destructible: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CityComponent {
    pub zone_radius: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrganizationComponent {
    pub operational_range: Option<f32>,
    /// The [`OrganizationLayout`] its members are arranged by.
    pub layout_ids: Option<Id>,
    /// What fills each of the layout's slots, in the layout's order.
    #[serde(default)]
    pub slot: Vec<OrganizationSlot>,
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

/// A property objects can have, shared at the top level or an object's own.
#[derive(Debug, Clone, Deserialize)]
pub struct Property {
    pub ids: Option<Id>,
    /// Comma separated, the flags it gives its objects for abilities'
    /// `targets` to match.
    pub enabled_target_flags: Option<String>,
    #[serde(default)]
    pub modifier: Vec<Modifier>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Modifier {
    /// What it modifies, e.g. `RESIST_DAMAGE`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub damage_type: Option<Id>,
    pub amount: Option<f32>,
}

/// What an object can do to others.
#[derive(Debug, Clone, Deserialize)]
pub struct PerformerComponent {
    #[serde(default)]
    pub check_damage_resistance: bool,
    #[serde(default)]
    pub ability: Vec<Ability>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ability {
    /// The ids of the animation its user plays to use it.
    pub animation1: Option<String>,
    /// The bone of the turret that aims it, if it has one.
    pub turret_ids: Option<String>,
    pub ability_definition: Option<AbilityDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AbilityDefinition {
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
    /// Within this distance of the centre an area hit does full damage.
    pub attenuation_range_min: Option<f32>,
    /// The fraction of damage an area hit does at its edge.
    pub attenuation_factor_max: Option<f32>,
    /// Comma separated flags, each `+` to require it or `-` to rule it out.
    pub targets: Option<String>,
    #[serde(default)]
    pub effect: Vec<Effect>,
    /// Run each time it's used.
    #[serde(default)]
    pub when_do: Vec<Fxc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Effect {
    /// What it does, e.g. `MAIN_DAMAGE`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub damage_type_ids: Option<Id>,
    pub amount: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MoverComponent {
    /// World units a second.
    pub speed_max: Option<f32>,
    #[serde(default)]
    pub when_start_moving: Vec<Fxc>,
    #[serde(default)]
    pub when_stop_moving: Vec<Fxc>,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct Size {
    pub scale_min: Option<f32>,
    pub scale_max: Option<f32>,
    /// `CIRCLE`, `SQUARE`, `RECT` or `POINT`, sized by the fields below.
    pub blocking_shape: Option<String>,
    pub blocking_radius: Option<f32>,
    /// The outline it occupies the ground with, sized by the fields of the
    /// blocking shape but for a circle's `soft_radius`.
    pub soft_shape: Option<String>,
    pub soft_radius: Option<f32>,
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
    /// The image on an organization's banner.
    pub banner_icon: Option<String>,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct Aspect {
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
    /// milliseconds, or `SELECTED`.
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

/// A nation players can play.
#[derive(Debug, Clone, Deserialize)]
pub struct Nation {
    pub ids: Option<Id>,
    /// The group of the templates it fields.
    pub placement_group_ids: Option<Id>,
    /// What its divisions wear on their banners, each division one.
    #[serde(default)]
    pub badge: Vec<Badge>,
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

/// An order the player can give what they've selected, shown as a button.
#[derive(Debug, Clone, Deserialize)]
pub struct ActorCommand {
    pub ids: Option<Id>,
    /// A `#` key into the localized strings.
    pub name: Option<String>,
    pub icon: Option<String>,
    /// Which of the action buttons it takes, numbered across then down;
    /// negative for none.
    pub preferred_position: Option<i32>,
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

/// A visual effect; those with a model draw it, relative to their file's
/// folder.
#[derive(Debug, Clone, Deserialize)]
pub struct Vfx {
    pub ids: Option<Id>,
    pub model: Option<String>,
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
    pub volume: Option<String>,
    pub volume_adjust: Option<String>,
    pub pitch_adjust: Option<[String; 2]>,
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
