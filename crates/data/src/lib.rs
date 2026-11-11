//! The game's data as a whole: access to the installed archives, and the
//! definitions built from their files (terrain, object views, ...).

pub mod defs;
mod economy;
mod fog;
mod hotkeys;
mod id;
mod messages;
mod supply;

pub use economy::{Amounts, Economy, EconomyRules, Logistics, RESOURCES, Supply};
pub use fog::{FogSetting, Sight};
pub use hotkeys::Hotkey;
pub use id::Id;
pub use messages::{GameEvent, InterfaceVars, MessageKind};
pub use supply::{Kind, Operation, SupplyEffect, SupplyRules};

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tgck::{Archive, Map};

/// A nation a side can play as.
#[derive(Debug, Clone)]
pub struct Nation {
    /// E.g. `nation_usa`.
    pub id: Id,
    /// E.g. `United States`.
    pub name: String,
    /// The template it puts in place of each of a map's placeholders, by
    /// placeholder id: its Corps HQ for `placeholder_corpsunpacked`.
    pub placeholders: HashMap<Id, Id>,
    /// Those who may lead its sides.
    pub generals: Vec<General>,
}

/// A general who leads a side.
#[derive(Debug, Clone, PartialEq)]
pub struct General {
    /// E.g. `patton`.
    pub id: Id,
    /// E.g. `General Patton`.
    pub name: String,
    /// What the lobby calls them, e.g. `Patton`.
    pub short_name: String,
    /// The special operations they give their side, by id.
    pub operations: Vec<Id>,
    /// Whether the lobby offers them, as it doesn't those only scenarios
    /// use.
    pub public: bool,
}

pub struct GameData {
    archives: Vec<Archive<BufReader<File>>>,
    /// Lower-case path to entry; where archives share a path the first wins.
    paths: HashMap<String, EntryRef>,
    /// The archive paths of the `.tgi` files the game loads, in the order
    /// it loads them.
    tgis: Vec<String>,
    /// See [`GameData::definitions`].
    definitions: Option<Arc<Definitions>>,
}

/// The object templates and shared properties the `.tgi` files define,
/// deserialized.
struct Definitions {
    /// In the order the game loads them. Defining an id twice is an error
    /// in the data, and only the first definition is kept.
    things: Vec<Template>,
    properties: HashMap<Id, defs::Property>,
    /// The abilities defined on their own, such as special operations,
    /// in the order they load, the first kept of those defined twice.
    abilities: Vec<(Id, defs::AbilityDefinition)>,
    /// The generals, in the order they load, the first kept of those
    /// defined twice.
    generals: Vec<defs::General>,
}

/// The names a comma separated list holds, leaving out `NULL`.
fn listed(list: Option<&str>) -> impl Iterator<Item = &str> {
    let names = list.unwrap_or("").split(',').map(str::trim);
    names.filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case("null"))
}

/// An object template as some `.tgi` file defines it.
struct Template {
    /// The defining file's archive path.
    file: String,
    id: Id,
    thing: defs::Thing,
}

impl Definitions {
    /// Defining a Thing id again is an error that ends the reading of its
    /// file there, the blocks before it kept.
    fn new(files: &[(String, tgi::Parsed)]) -> Self {
        let mut ids = HashSet::new();
        let mut things = Vec::new();
        let mut properties = HashMap::new();
        let mut abilities: Vec<(Id, defs::AbilityDefinition)> = Vec::new();
        let mut generals: Vec<defs::General> = Vec::new();
        for Scoped {
            file,
            root,
            templates,
        } in scoped(files)
        {
            for block in root.blocks() {
                let kind = block.kind().unwrap_or("");
                if kind.eq_ignore_ascii_case("Thing") {
                    let Some(id) = block.value("ids").map(Id::from) else {
                        continue;
                    };
                    if ids.contains(&id) {
                        break;
                    }
                    if let Ok(thing) = templates.deserialize(block) {
                        ids.insert(id.clone());
                        things.push(Template {
                            file: file.to_string(),
                            id,
                            thing,
                        });
                    }
                } else if kind.eq_ignore_ascii_case("Property")
                    && let Ok(p) = templates.deserialize::<defs::Property>(block)
                    && let Some(id) = p.ids.clone()
                {
                    properties.insert(id, p);
                } else if kind.eq_ignore_ascii_case("AbilityDefinition")
                    && let Ok(a) = templates.deserialize::<defs::AbilityDefinition>(block)
                    && let Some(id) = a.ids.clone()
                    && !abilities.iter().any(|(known, _)| *known == id)
                {
                    abilities.push((id, a));
                } else if kind.eq_ignore_ascii_case("Faction")
                    && let Ok(g) = templates.deserialize::<defs::General>(block)
                    && let Some(id) = &g.ids
                    && !generals.iter().any(|known| known.ids.as_ref() == Some(id))
                {
                    generals.push(g);
                }
            }
        }
        Definitions {
            things,
            properties,
            abilities,
            generals,
        }
    }
}

/// A `.tgi` file that loads, with the templates its blocks see.
struct Scoped<'a> {
    file: &'a str,
    root: &'a tgi::Block,
    templates: tgi::Templates<'a>,
}

impl Scoped<'_> {
    /// Its blocks of `kind` that read as `T`.
    fn read<'s, T: serde::de::DeserializeOwned>(
        &'s self,
        kind: &'s str,
    ) -> impl Iterator<Item = T> + 's {
        (self.root.blocks_of(kind)).filter_map(|b| self.templates.deserialize(b).ok())
    }
}

/// The files, in the order they load, that don't fail to, with the
/// templates each sees.
fn scoped(files: &[(String, tgi::Parsed)]) -> Vec<Scoped<'_>> {
    let scopes = tgi::Templates::scoped(files.iter().map(|(_, p)| &p.root));
    (files.iter().zip(scopes))
        .filter_map(|((file, parsed), templates)| {
            Some(Scoped {
                file,
                root: &parsed.root,
                templates: templates?,
            })
        })
        .collect()
}

/// An entry of one of [`GameData`]'s archives: `(archive, entry)` indices.
type EntryRef = (usize, usize);

/// The archives besides `Data.rwd` that are opened when present, in the
/// order their paths are looked up.
const EXTRA_ARCHIVES: [&str; 2] = ["Music.rwd", "Speech.rwd"];

/// How a template is drawn: its models and where their textures are.
#[derive(Debug, Clone)]
pub struct View {
    /// Archive paths of the model variations; each placed object picks one.
    pub models: Vec<String>,
    /// Archive folders searched for the models' textures.
    pub texture_dirs: Vec<String>,
    /// Substitutions made in the paths of the textures each model names,
    /// `(from, to)` in the order declared, by model.
    pub texture_renames: HashMap<String, Vec<(String, String)>>,
    /// Size multiplier: the middle of the template's random size range, which
    /// each placed object scales again by its own factor.
    pub scale: f32,
    /// The animations the models can play; units have them.
    pub animations: Vec<Animation>,
    /// The outline under which the ground is sampled to tilt the model to
    /// follow it, as vehicles do; `None` for models that stay upright.
    pub conform: Option<Shape>,
    /// What height its objects stand at.
    pub footing: Footing,
    /// Archive path of the model a building plays once as it unpacks.
    pub unpacking: Option<String>,
    /// Archive path of the model a building plays once as it packs.
    pub packing: Option<String>,
    /// Archive path of its picture in the interface, such as a unit's
    /// portrait in its regiment's panel or a building's button.
    pub icon: Option<String>,
}

/// What height a template's objects stand at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Footing {
    /// On the ground, under water on its bed.
    #[default]
    Ground,
    /// On the water where it's above the ground, as ships do.
    Water,
    /// At its own height, as aircraft and shells do.
    Air,
}

/// An animation a template's models can play.
#[derive(Debug, Clone)]
pub struct Animation {
    /// What it is for: `IDLE`, `MOVE`, `DIE`, ...
    pub kind: String,
    /// Which one of its kind, e.g. `idle`, `fidget1`, `walk`.
    pub ids: String,
    /// Archive path of its `.kf` file.
    pub kf: String,
    /// Seconds one play takes; the file's keys are stretched to fit.
    pub length: f32,
    /// For moving animations, how far one play carries the unit.
    pub walk_distance: Option<f32>,
}

/// How a template's objects take part in movement.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Body {
    /// The ground it keeps movers off; `None` if it doesn't block.
    pub blocking: Option<Shape>,
    /// Its blocking shape, which it takes up the ground with whether or
    /// not it blocks.
    pub shape: Shape,
    /// Its soft shape, which holds its blocking shape.
    pub soft: Shape,
    /// It floats, so it stands on water rather than land.
    pub water_based: bool,
    /// It stands on the half-tile grid.
    pub tile_locked: bool,
    /// Nothing gets in its way, so it can stand anywhere on the map.
    pub ethereal: bool,
    /// It never moves: it has no mover with a top speed, and is neither an
    /// organization nor a projectile. Only such things get in the way of
    /// what's placed as a map loads or a building unpacks.
    pub fixed: bool,
    /// Top speed in world units a second; `None` if it can't move.
    pub speed: Option<f32>,
    /// World units a second it gains each second, speeding up; 0 for
    /// reaching top speed at once. It slows down at once.
    pub acceleration: f32,
    /// Radians a second it turns while moving; 0 for facing its way at
    /// once.
    pub turn_rate: f32,
    /// Where it's coupled to what tows it or what it tows: x to its right
    /// and y ahead.
    pub hitch: Option<[f32; 2]>,
    /// The template of the trailer it tows.
    pub trailer: Option<Id>,
}

/// How a template's objects fight: what they can take and what they deal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fighter {
    pub health: f32,
    pub defense: f32,
    /// Whoever brings its health to nothing can take it over.
    pub capturable: bool,
    /// It's destroyed when its health runs out; otherwise it's disabled.
    pub destructible: bool,
    /// The fraction of each damage type's damage its properties turn
    /// aside, by damage type; negative where they add to it.
    pub resists: HashMap<Id, f32>,
    /// What its properties make it for weapons' targets to match, e.g.
    /// `infantry`.
    pub flags: Vec<Id>,
    pub weapons: Vec<Weapon>,
    /// How high above it its attacks start, where they don't say.
    pub attack_height: f32,
    /// For one that defends itself, how near something must come for it to
    /// notice it, and so ever fire on it.
    pub notice: Option<f32>,
    /// The parts that turn to aim its weapons.
    pub turrets: Vec<Turret>,
    /// Its soft shape: area shots reach as far as the circle round it.
    pub soft: Shape,
    /// The outline weapons' ranges run to and from.
    pub target: Shape,
    /// Whether that's a square, which gives way to a rectangle where
    /// ranges keep only one of two as it is.
    pub target_square: bool,
}

/// A part of a fighter that turns to aim its weapons, without turning the
/// fighter. Angles are radians from the fighter's facing, counter-clockwise
/// seen from above.
#[derive(Debug, Clone, PartialEq)]
pub struct Turret {
    /// Radians a second.
    pub turn_rate: f32,
    /// The whole sweep it aims within, centred on `start`; a full turn or
    /// more aims anywhere.
    pub sweep: f32,
    /// Where it points as the fighter appears, and goes back to.
    pub start: f32,
    /// Seconds without turning to anything before it goes back, if it does.
    pub reset: Option<f32>,
    /// Where it stands from its fighter: ahead, then to the left.
    pub offset: [f32; 2],
}

/// An attack a fighter makes by itself on what comes in range.
#[derive(Debug, Clone, PartialEq)]
pub struct Weapon {
    pub range: f32,
    pub min_range: f32,
    /// Seconds between shots.
    pub recharge: f32,
    /// Radius of the area a shot hits, 0 for only its target.
    pub area: f32,
    /// The most an area shot hits, the nearest where it lands; 0 for all
    /// it reaches.
    pub max_targets: u32,
    /// Within this distance of where a shot lands it does full damage...
    pub full_within: f32,
    /// ...falling to this fraction of it at the area's edge.
    pub edge_damage: f32,
    pub targets: Targets,
    pub damage: Vec<Damage>,
    /// The ids of the animation its user plays to fire it.
    pub animation: Option<String>,
    /// It's on a turret, which aims it without turning its user.
    pub turret: bool,
    /// The id of the model point of that turret.
    pub turret_point: Option<Id>,
    /// The fighter's turret that must point at a target before it fires,
    /// by index into [`Fighter::turrets`].
    pub aimed_by: Option<usize>,
    /// Where its projectiles appear from its user, in the user's model's
    /// axes, forward along -y.
    pub spawn_position: [f32; 3],
    /// Played each shot.
    pub sounds: Vec<Cue>,
    pub effects: WeaponEffects,
    /// The template of the projectile a shot fires, if it fires one.
    pub projectile: Option<Id>,
    /// How that projectile flies, if its template says.
    pub flight: Option<Flight>,
    /// An order to bombard can use it, and its hits are bombardment when
    /// it fires other than by its user's own choice of shot.
    pub bombard: bool,
}

/// How a projectile flies to what it's fired at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flight {
    pub kind: FlightKind,
    /// World units a second.
    pub speed: f32,
    /// Seconds it flies, whatever the distance, if above 0.
    pub life: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlightKind {
    /// Straight at its target.
    #[default]
    Straight,
    /// In an arc.
    Lob,
    /// Dropped from above.
    Bomb,
}

/// How far up their targets projectiles aim, as shares of the targets'
/// heights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimHeights {
    pub straight: f32,
    /// Lobbed and dropped.
    pub arcing: f32,
}

/// Which flags a target must have, and must not, for a weapon to fire on
/// it. Besides a fighter's own [`Fighter::flags`], it has `enemy`, `friend`
/// or `neutral` by how its side stands toward the firer's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Targets {
    pub required: Vec<Id>,
    pub excluded: Vec<Id>,
}

impl Targets {
    fn parse(text: &str) -> Targets {
        let mut out = Targets::default();
        for word in text.split(|c: char| c == ',' || c.is_whitespace()) {
            let (list, flag) = match word.strip_prefix(['-', '!']) {
                Some(flag) => (&mut out.excluded, flag),
                None => (&mut out.required, word.strip_prefix('+').unwrap_or(word)),
            };
            if !flag.is_empty() {
                list.push(Id::from(flag));
            }
        }
        out
    }

    pub fn allow<'a>(&self, mut flags: impl Iterator<Item = &'a Id> + Clone) -> bool {
        self.required.iter().all(|r| flags.clone().any(|f| f == r))
            && !flags.any(|f| self.excluded.contains(f))
    }
}

/// Damage a shot does, of one type.
#[derive(Debug, Clone, PartialEq)]
pub struct Damage {
    /// E.g. `gunfire`.
    pub kind: Id,
    pub amount: f32,
}

/// How far around it an organization reaches.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ranges {
    /// How far from where it stands it takes on enemies that fire on it.
    pub operational: Option<f32>,
    /// Its zone of control's radius.
    pub control: Option<f32>,
}

/// What an organization template, such as a regiment, is made of.
#[derive(Debug, Clone, PartialEq)]
pub struct Organization {
    /// The [`Layout`] its members are arranged by.
    pub layout: Id,
    /// The template filling each of the layout's slots; `None` for one left
    /// empty.
    pub slots: Vec<Option<Id>>,
}

/// How an organization's members can be arranged.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The formations it can take, in the order the layout lists them.
    pub formations: Vec<Formation>,
    /// Index into `formations` of the one it starts in.
    pub default: usize,
    /// Index into `formations` of the one it fights in once engaged.
    pub combat: Option<usize>,
    /// The slot whose member the organization's position follows.
    pub primary: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Formation {
    /// E.g. `regiment_defensive`.
    pub id: Id,
    /// The positions of each slot's members, as many as the slot holds:
    /// offsets from the primary slot's position, with 0,1 straight ahead.
    pub slots: Vec<Vec<[f32; 2]>>,
    /// Whether a regiment standing or moving in it takes on enemies that
    /// come within its operational range, or runs into.
    pub aggressive: bool,
}

/// An outline centred on an object, in world units: an object's own scale
/// doesn't change it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Shape {
    #[default]
    Point,
    Circle {
        radius: f32,
    },
    /// Width across the object's facing and height along it.
    Rect {
        width: f32,
        height: f32,
    },
}

impl Shape {
    /// The shape grown by `by` on every side; a point grows into a circle.
    pub fn padded(self, by: f32) -> Shape {
        match self {
            Shape::Point => Shape::Circle { radius: by },
            Shape::Circle { radius } => Shape::Circle {
                radius: radius + by,
            },
            Shape::Rect { width, height } => Shape::Rect {
                width: width + 2.0 * by,
                height: height + 2.0 * by,
            },
        }
    }
}

/// How a template shows on the button that commissions or recruits it.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub name: String,
    /// Archive path of its button's icon.
    pub icon: Option<String>,
    /// What it costs, earns and takes to keep, by resource.
    pub cost: Vec<Amount>,
    pub production: Vec<Amount>,
    pub upkeep: Vec<Amount>,
}

/// So much of a resource.
#[derive(Debug, Clone, PartialEq)]
pub struct Amount {
    /// The resource's field name in the data, such as `money`.
    pub kind: &'static str,
    /// Its localized name, such as Money.
    pub resource: String,
    pub amount: f32,
}

/// How a template packs and unpacks.
#[derive(Debug, Clone, PartialEq)]
pub struct Deployment {
    /// For a packed building, such as a supply truck, the template it
    /// unpacks into.
    pub unpacks_into: Option<Id>,
    /// For a packed building, where it stands from the building it unpacks
    /// into, and the way it faces when the building packs into it, in world
    /// space whichever way the building faces.
    pub packed_offset: [f32; 2],
    pub packed_facing: [f32; 2],
    /// For a building, the seconds packing and unpacking take.
    pub packing: f32,
    pub unpacking: f32,
    /// For a building, the share of its health at or below which it can't
    /// pack.
    pub pack_health_min: f32,
}

/// What recruiting a template needs: buildings its owner has standing, and
/// properties the recruiter or its side has.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Requires {
    /// Each building, by every id it may go by on a map: its current one
    /// first, then any it was renamed from.
    pub structures: Vec<Vec<Id>>,
    /// Whether one of them is enough, rather than all.
    pub any: bool,
    pub properties: Vec<Id>,
    /// Whether missing a property leaves it out of a recruiter's menu,
    /// rather than greying it there.
    pub hide: bool,
}

/// Where what a building recruits appears and goes, from where it stands:
/// x across its facing and y along it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeployPoints {
    pub creation: [f32; 2],
    pub deployment: [f32; 2],
}

/// An order the player can give, shown as one of the action buttons.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    /// E.g. `move`, `kill`, `stop`.
    pub ids: Id,
    pub name: String,
    /// What it does, and why it can't be given when it can't.
    pub tooltip: Option<String>,
    pub disabled: Option<String>,
    /// Archive path of its icon.
    pub icon: Option<String>,
    /// Which button it takes, numbered across then down.
    pub slot: Option<usize>,
    /// The [`CursorLook`] shown while it waits for a place or target.
    pub cursor: Option<Id>,
    /// The effect marking where it was given, from [`GameData::vfx_models`].
    pub marker: Option<Id>,
    /// Whether each of a group goes to its own place, keeping its place
    /// relative to the rest, rather than all to the one place.
    pub group_offset: bool,
}

/// A mouse cursor's frames, which loop.
#[derive(Debug, Clone, PartialEq)]
pub struct CursorLook {
    /// Archive paths, in order.
    pub frames: Vec<String>,
    /// The pixel that points, from the top left.
    pub hotspot: [u16; 2],
    /// Seconds one loop through the frames takes.
    pub period: f32,
}

/// How a formation shows on its button.
#[derive(Debug, Clone, PartialEq)]
pub struct FormationLook {
    pub name: String,
    /// Archive path of its icon.
    pub icon: Option<String>,
}

/// One texture variation of a terrain type, e.g. `temperate_grass01`.
#[derive(Debug, Clone)]
pub struct Variation {
    /// Archive path of the texture.
    pub texture: String,
    /// Size of one texture repeat, in world units.
    pub repeat: [f32; 2],
    /// Average colour, used where the texture is missing.
    pub color: [f32; 3],
    /// How water terrains colour the surface by depth, shallowest first.
    pub water: Option<Vec<DepthColor>>,
    /// The animated texture of a water terrain's surface.
    pub animation: Option<TextureAnimation>,
    /// Higher priority terrain is drawn over lower where they meet. Each
    /// variation ranks one above the one before it in its terrain.
    pub priority: i32,
    /// Land units can cross it.
    pub land: bool,
    /// Ships can cross it.
    pub naval: bool,
    pub kind: TerrainKind,
    /// What effects take it for, e.g. `dirt`, `snow` or `water`.
    pub group: Option<Id>,
}

/// What a terrain is, which decides how the ground stands on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerrainKind {
    Land,
    /// Its tiles each stand raised by an amount from `noise`, lowest and
    /// highest.
    Mountain {
        noise: [f32; 2],
    },
    Water,
}

/// A looping sequence of textures.
#[derive(Debug, Clone)]
pub struct TextureAnimation {
    /// Archive path of each frame, with `%04d` where its number goes.
    pub frames: String,
    pub fps: f32,
}

impl TextureAnimation {
    /// Archive path of frame `i`; frames count up from 0 to the first
    /// missing one.
    pub fn frame(&self, i: usize) -> String {
        self.frames.replace("%04d", &format!("{i:04}"))
    }
}

/// A water surface's colour and opacity at a depth below it.
#[derive(Debug, Clone, Copy)]
pub struct DepthColor {
    pub depth: f32,
    pub color: [f32; 3],
    pub alpha: f32,
}

/// An [`Audio`] an object plays when something happens.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub audio: Id,
    /// Where it plays, e.g. `SELF`, or `TARGET_GROUND` for a shot's landing.
    pub at: String,
    /// Seconds after what sets it off.
    pub delay: f32,
    pub looping: bool,
    /// It plays on one in this many of the times it's set off.
    pub every: u32,
}

/// The audio an object template plays, by what sets it off; its weapons'
/// are on its [`Weapon`]s.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjectAudio {
    pub selected: Vec<Cue>,
    /// Given an order, or an order to attack.
    pub commanded: Vec<Cue>,
    pub commanded_aggressive: Vec<Cue>,
    pub created: Vec<Cue>,
    pub destroyed: Vec<Cue>,
    pub start_moving: Vec<Cue>,
    pub stop_moving: Vec<Cue>,
    /// On entering and leaving states, by state, e.g. `construction`.
    pub entered: Vec<(Id, Cue)>,
    pub left: Vec<(Id, Cue)>,
    /// When something disturbs it, as a vehicle crushing a doodad does.
    pub disturbed: Vec<Cue>,
    /// Played as its model's animations start.
    pub animations: Vec<AnimationAudio>,
}

impl ObjectAudio {
    fn is_empty(&self) -> bool {
        *self == ObjectAudio::default()
    }
}

/// An [`Audio`] played as one of a model's animations starts.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationAudio {
    /// The animation's kind, e.g. `RANDOM` or `SELECTED`.
    pub kind: String,
    pub ids: Option<String>,
    /// For `RANDOM` animations, the range of seconds it waits, from being
    /// created and from the end of each play, before it starts again.
    pub interval: Option<[f32; 2]>,
    pub audio: Id,
    pub looping: bool,
}

/// What nations' banners are drawn with, by nation id, e.g. `nation_usa`.
#[derive(Debug, Clone, Default)]
pub struct NationBanners {
    /// The nation each organization template is of, by template id.
    pub of_template: HashMap<Id, Id>,
    /// The archive path of the frame round a nation's banners.
    pub frames: HashMap<Id, String>,
    /// The archive paths of the badges a nation's divisions wear, in the
    /// order HQs number them.
    pub badges: HashMap<Id, Vec<String>>,
}

/// How the supply border is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct SupplyBorder {
    pub colour: [f32; 3],
    /// How wide it is, in world units.
    pub width: f32,
    /// The archive path of the image drawn along it.
    pub texture: Option<String>,
    /// The side, in world units, of the squares the map is cut into,
    /// which decide where each of its loops starts.
    pub block: f32,
}

/// A visual effect the game shows by id.
#[derive(Debug, Clone, PartialEq)]
pub struct Vfx {
    /// What kind it is, e.g. `VFXMODEL`, `VFXPROJECTEDTEXTURE` or
    /// `VFXTRACER`.
    pub category: String,
    /// Archive path of its model, if it has one that's there.
    pub model: Option<String>,
    /// Archive folders searched for the model's textures.
    pub texture_dirs: Vec<String>,
    /// It lasts until stopped, rather than for a time.
    pub permanent: bool,
    /// It shows whatever the effects detail setting.
    pub critical: bool,
    /// Seconds a temporary one lasts; `None` for its model's own length.
    pub duration: Option<f32>,
    /// Attached, its size as a multiple of what it's attached to; 0 for
    /// its own size.
    pub attached_scale: f32,
    /// A temporary one is created off screen too.
    pub never_discard: bool,
    pub night_only: bool,
    /// The same size on screen however far the camera is.
    pub constant_screen_size: bool,
    /// It runs in real time rather than game time.
    pub real_time: bool,
    /// Each one is turned a random way about the vertical.
    pub random_orientation: bool,
    /// Attached, it keeps its facing as what it's attached to turns.
    pub gyroscope: bool,
    /// For one that tints what it's on, how.
    pub tint: Option<VfxTint>,
}

/// How a [`Vfx`] tints what it's on, which pulses from untinted up to a
/// strength and back down, each way at a steady pace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VfxTint {
    /// What it multiplies colours by at full strength.
    pub colour: [f32; 3],
    /// The strength it rises to, where 1 is full.
    pub max: f32,
    /// Seconds it takes to rise from untinted to full; 0 to start at its
    /// top.
    pub rise: f32,
    /// Seconds it takes to fall from full to untinted; 0 to stay at its
    /// top.
    pub fall: f32,
    /// It rises again once it's fallen to untinted, rather than staying so.
    pub repeating: bool,
}

impl VfxTint {
    /// Its strength this many seconds after it started.
    pub fn level(&self, secs: f32) -> f32 {
        let (up, down) = (self.max * self.rise, self.max * self.fall);
        let t = match self.repeating && down > 0.0 {
            true => secs % (up + down),
            false => secs,
        };
        if t < up {
            t / self.rise
        } else if down <= 0.0 {
            self.max
        } else if t < up + down {
            self.max - (t - up) / self.fall
        } else {
            0.0
        }
    }

    /// What it multiplies colours by at a strength.
    pub fn colour(&self, level: f32) -> [f32; 3] {
        self.colour.map(|c| 1.0 - (1.0 - c) * level)
    }
}

/// A [`Vfx`] an object creates, or stops, when something happens.
#[derive(Debug, Clone, PartialEq)]
pub struct VfxCue {
    pub vfx: Id,
    /// It stops the effect of that id that the object's cues created,
    /// rather than creating one.
    pub stop: bool,
    /// Where it appears, e.g. `SELF`, `TARGET_GROUND` or
    /// `TARGET_HIT_POSITION`.
    pub at: String,
    /// The point of the model it appears at, e.g. `cannon_ae_01`; `None`
    /// for the model's origin.
    pub location: Option<String>,
    /// It follows what it's at as that moves.
    pub attached: bool,
    /// Offset from its spot, and the way it faces there.
    pub translation: [f32; 3],
    pub orientation: [f32; 3],
    /// Seconds it lasts in place of the effect's own.
    pub duration: Option<f32>,
    /// Seconds after what sets it off.
    pub delay: f32,
    /// It plays on one in this many of the times it's set off.
    pub every: u32,
    /// The terrain group it plays on, if only one.
    pub terrain: Option<Id>,
    /// What the event's target must be for it to play.
    pub targets: Targets,
    /// For a building being finished, it stops when the building's
    /// destroyed.
    pub stop_on_destructed: bool,
    /// Run when its effect ends, and alongside it.
    pub chain: Vec<VfxCue>,
    pub chain_immediate: Vec<VfxCue>,
}

/// A [`VfxCue`] run as an object's health crosses a share of its maximum.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthCue {
    pub health: f32,
    pub trigger: HealthTrigger,
    pub cue: VfxCue,
}

/// Which crossing of a [`HealthCue`]'s health runs it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HealthTrigger {
    #[default]
    Below,
    Above,
    /// Relative to the health it had, falling or rising by that share.
    Decrease,
    Increase,
}

/// The effects an object template shows, by what sets them off; its
/// weapons' are on its [`Weapon`]s.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjectEffects {
    pub created: Vec<VfxCue>,
    /// As a projectile, on reaching its target.
    pub destroyed: Vec<VfxCue>,
    /// When a building's finished.
    pub constructed: Vec<VfxCue>,
    pub start_moving: Vec<VfxCue>,
    pub stop_moving: Vec<VfxCue>,
    /// On entering and leaving states, by state, e.g. `construction`.
    pub entered: Vec<(Id, VfxCue)>,
    pub left: Vec<(Id, VfxCue)>,
    pub health: Vec<HealthCue>,
    /// When something disturbs it, as a vehicle crushing a doodad does.
    pub disturbed: Vec<VfxCue>,
}

impl ObjectEffects {
    fn is_empty(&self) -> bool {
        *self == ObjectEffects::default()
    }
}

/// The effects a [`Weapon`] shows each time it fires.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeaponEffects {
    /// As a shot begins, e.g. a tank gun's muzzle flash.
    pub start: Vec<VfxCue>,
    pub shot: Vec<VfxCue>,
    /// For each target a shot lands on.
    pub per_target: Vec<VfxCue>,
    pub end: Vec<VfxCue>,
}

/// A sound, speech or music, as the game asks for it by id.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    pub category: AudioCategory,
    /// Archive paths: one of the lead-ins plays first, then the main files
    /// (one, or all where [`AudioControl::all`]), then one of the fall-outs.
    pub leadin: Vec<String>,
    pub files: Vec<String>,
    pub fallout: Vec<String>,
    /// Files it names that aren't in the archives, as written.
    pub missing: Vec<String>,
    pub priority: AudioPriority,
    pub control: AudioControl,
    pub world: AudioWorld,
    pub who: AudioWho,
    /// How steeply it falls off with distance; lower is steeper.
    pub falloff: f32,
    /// The distance beyond which it can't be heard.
    pub range: f32,
    /// How many can play at once.
    pub limit: u32,
    /// Its loudness, 1 for full.
    pub volume: f32,
    /// Up to how much quieter each play is picked, as a fraction of
    /// `volume`, 0 or less.
    pub volume_adjust: f32,
    /// The range each play's pitch is picked from, as deltas from 1.
    pub pitch_adjust: [f32; 2],
    /// The range each play's delay is picked from, in seconds.
    pub delay: [f32; 2],
    /// Seconds to fade in and out.
    pub fade: [f32; 2],
    /// The localisation key of the text shown while it plays.
    pub subtitle: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCategory {
    Sound,
    Speech,
    Music,
}

/// Which plays win when there are more than channels to play them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AudioPriority {
    Lowest,
    Low,
    Medium,
    High,
    Critical,
}

/// How an [`Audio`] is played.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioControl {
    /// A new play takes over an old one when out of channels or at its limit.
    pub interrupt: bool,
    /// Its files are played in order instead of picked at random.
    pub sequential: bool,
    /// Each play plays all its files.
    pub all: bool,
    /// It is spoken by an object, which says one such at a time.
    pub voice: bool,
    /// It is heard from a place in the world.
    pub world: bool,
    /// It fades other kinds of audio down while it plays.
    pub cause_fade: bool,
    /// It is the sample played by the options' volume sliders.
    pub test: bool,
}

/// How an [`Audio`] played in the world is heard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioWorld {
    /// From anywhere on the map, however far.
    pub global: bool,
    /// Not where the listener's fog covers it.
    pub fogged: bool,
}

/// Who hears an [`Audio`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioWho {
    /// The local player, whom it is meant for.
    pub local: bool,
    /// Its owner, when they caused it.
    pub player: bool,
    pub allies: bool,
    pub enemies: bool,
}

impl GameData {
    /// Opens the archives in the game's install directory: `Data.rwd`, which
    /// must be there, and the music and speech archives if they are.
    pub fn open(dir: &Path) -> Result<Self, String> {
        let open =
            |path: PathBuf| Archive::open(&path).map_err(|e| format!("{}: {e}", path.display()));
        let data = find_case_insensitive(dir, "Data.rwd")
            .ok_or_else(|| format!("no Data.rwd in {}", dir.display()))?;
        let mut archives = vec![open(data)?];
        for name in EXTRA_ARCHIVES {
            if let Some(path) = find_case_insensitive(dir, name) {
                archives.push(open(path)?);
            }
        }
        let mut paths = HashMap::new();
        for (a, archive) in archives.iter().enumerate() {
            for (i, e) in archive.entries.iter().enumerate() {
                paths.entry(e.name.to_ascii_lowercase()).or_insert((a, i));
            }
        }
        let mut game = GameData {
            archives,
            paths,
            tgis: Vec::new(),
            definitions: None,
        };
        let names: Vec<String> = game.entries().map(str::to_string).collect();
        let tgis = listed_tgis(&names, |name| game.read(name).map(|b| tgi::decode(&b)));
        game.tgis = tgis;
        Ok(game)
    }

    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        let (a, i) = *self.paths.get(&name.to_ascii_lowercase())?;
        let archive = &mut self.archives[a];
        let entry = archive.entries[i].clone();
        archive.read(&entry).ok()
    }

    /// Every entry's path, in archive order.
    fn entries(&self) -> impl Iterator<Item = &str> {
        self.archives
            .iter()
            .flat_map(|a| &a.entries)
            .map(|e| e.name.as_str())
    }

    fn entry_name(&self, (a, i): EntryRef) -> &str {
        &self.archives[a].entries[i].name
    }

    pub fn exists(&self, name: &str) -> bool {
        self.paths.contains_key(&name.to_ascii_lowercase())
    }

    /// Archive paths whose lower-case form passes `filter`.
    fn entry_names(&self, filter: impl Fn(&str) -> bool) -> Vec<String> {
        self.entries()
            .filter(|n| filter(&n.to_ascii_lowercase()))
            .map(str::to_string)
            .collect()
    }

    /// Every `.tgi` file the game loads whose lower-case path passes
    /// `filter`, parsed, in the order the game loads them.
    fn parse_tgis(&mut self, filter: impl Fn(&str) -> bool) -> Vec<(String, tgi::Parsed)> {
        let names: Vec<String> = (self.tgis.iter())
            .filter(|n| filter(&n.to_ascii_lowercase()))
            .cloned()
            .collect();
        names
            .into_iter()
            .filter_map(|n| {
                let parsed = tgi::parse_bytes(&self.read(&n)?);
                Some((n, parsed))
            })
            .collect()
    }

    /// The archive path of a texture that `model`, one of `view`'s, names
    /// as `file`: beside the model, under the name the model's renames make
    /// of it, else that file name in each of the view's search paths in
    /// turn. `None` where none of them exists, as the game then draws the
    /// default texture.
    pub fn find_texture(&self, view: &View, model: &str, file: &str) -> Option<String> {
        texture_paths(view, model, file)
            .into_iter()
            .find(|p| self.exists(p))
    }

    /// Archive paths of all maps, sorted.
    pub fn map_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .entries()
            .filter(|n| n.to_ascii_lowercase().ends_with(".rmp"))
            .collect();
        names.sort_by_key(|n| n.to_ascii_lowercase());
        names
    }

    /// Finds a map by archive path or by file name without extension.
    pub fn find_map(&self, name: &str) -> Option<String> {
        self.map_names()
            .into_iter()
            .find(|n| n.eq_ignore_ascii_case(name) || map_title(n).eq_ignore_ascii_case(name))
            .map(str::to_string)
    }

    pub fn open_map(&mut self, name: &str) -> Result<Map<Cursor<Vec<u8>>>, String> {
        let bytes = self
            .read(name)
            .ok_or_else(|| format!("{name}: not found"))?;
        Map::new(Cursor::new(bytes)).map_err(|e| format!("{name}: {e}"))
    }

    /// Reads every terrain definition and indexes the variations by IDS,
    /// which is how maps refer to them.
    pub fn terrain_variations(&mut self) -> HashMap<Id, Variation> {
        let files = self.parse_tgis(|n| n.starts_with("terrain/") || n.starts_with("templates/"));
        let files = scoped(&files);
        let water = water_colors(&files);
        let mut out = HashMap::new();
        for Scoped {
            file,
            root,
            templates,
        } in &files
        {
            if !file.to_ascii_lowercase().starts_with("terrain/") {
                continue;
            }
            let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
            for block in root.blocks_of("Terrain") {
                let Ok(terrain) = templates.deserialize::<defs::Terrain>(block) else {
                    continue;
                };
                let water = terrain.ids.as_ref().and_then(|id| water.get(id));
                let kind = terrain_kind(&terrain);
                let animation =
                    terrain
                        .anim_textures_path_prefix
                        .as_ref()
                        .map(|p| TextureAnimation {
                            frames: format!("{dir}/{}", p.trim_start_matches('/')),
                            // The schema's default.
                            fps: terrain.anim_fps.unwrap_or(30.0),
                        });
                for (i, v) in terrain.variation.into_iter().enumerate() {
                    let (Some(ids), Some(texture)) = (v.ids, v.texture) else {
                        continue;
                    };
                    out.insert(
                        ids,
                        Variation {
                            texture: format!("{dir}/{}", texture.trim_start_matches('/')),
                            repeat: [
                                v.texture_width.unwrap_or(8.0),
                                v.texture_height.unwrap_or(8.0),
                            ],
                            color: v
                                .terrain_color
                                .filter(|c| c.iter().all(|&c| c >= 0.0))
                                .or(terrain.terrain_color)
                                .unwrap_or([0.5; 3]),
                            water: water.cloned(),
                            animation: animation.clone(),
                            priority: terrain.base_priority + i as i32,
                            land: terrain.land,
                            naval: terrain.water,
                            kind,
                            group: not_null(terrain.terrain_group.clone()),
                        },
                    );
                }
            }
        }
        if let Some(bytes) = self.read("translate.tgi") {
            add_renames(&mut out, &tgi::decode(&bytes), "Variation");
        }
        out
    }

    /// The views of all object templates, by template id,
    /// including the ids the game has since renamed: `None` for templates
    /// that name no model, such as sound emitters, or are invisible.
    /// Visible templates without a view, of their own or inherited, are left
    /// out.
    pub fn views(&mut self) -> HashMap<Id, Option<View>> {
        let mut out = HashMap::new();
        for Template { file, id, thing } in &self.definitions().things {
            let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
            let invisible = thing.flags.iter().any(|f| f.invisible);
            let view = match &thing.view {
                _ if invisible => None,
                Some(view) => self.view(dir, thing, view),
                None => continue,
            };
            out.insert(id.clone(), view);
        }
        if let Some(bytes) = self.read("translate.tgi") {
            add_renames(&mut out, &tgi::decode(&bytes), "Thing");
        }
        out
    }

    /// How the objects of every template block and move, by template id,
    /// including the ids the game has since renamed.
    pub fn bodies(&mut self) -> HashMap<Id, Body> {
        self.by_template(|t| Some(body(t)))
    }

    /// How the objects of every template that can be hurt fight, by
    /// template id, including the ids the game has since renamed.
    pub fn fighters(&mut self) -> HashMap<Id, Fighter> {
        let definitions = self.definitions();
        let flights = self.by_template(|t| flight(t.projectile_component.as_ref()?));
        let mut out = self.by_template(|t| fighter(t, &definitions.properties));
        for w in out.values_mut().flat_map(|f| &mut f.weapons) {
            w.flight = w.projectile.as_ref().and_then(|p| flights.get(p)).copied();
        }
        out
    }

    /// How far up their targets projectiles aim.
    pub fn aim_heights(&mut self) -> AimHeights {
        let svars = self.svars();
        AimHeights {
            straight: (svars.iter())
                .find_map(|v| v.projectile_target_height_ratio)
                .unwrap_or(0.65),
            arcing: (svars.iter())
                .find_map(|v| v.projectile_arcing_target_height_ratio)
                .unwrap_or(0.35),
        }
    }

    /// Seconds an enemy's damage keeps what it hurt under siege.
    pub fn siege_seconds(&mut self) -> Option<f32> {
        let svars = self.svars();
        svars
            .iter()
            .find_map(|v| v.under_siege_recently_damaged_seconds)
    }

    /// How far around them city centres hold sway, by template id, including
    /// the ids the game has since renamed.
    pub fn city_zones(&mut self) -> HashMap<Id, f32> {
        // A city's other buildings have the component with no zone.
        self.by_template(|t| {
            let zone = t.city_component.as_ref()?.zone_radius;
            zone.filter(|&r| r > 0.0)
        })
    }

    /// The archive path of the image on each organization template's
    /// banner, by template id.
    pub fn banner_icons(&mut self) -> HashMap<Id, String> {
        self.by_template(|t| ui_path(t.view.as_ref()?.banner_icon.as_deref()?))
    }

    /// The organization templates that fly no banner, such as animal herds:
    /// part of the scenery, which players can't pick.
    pub fn scenery(&mut self) -> HashSet<Id> {
        self.by_template(|t| {
            let icon = t.view.as_ref().and_then(|v| v.banner_icon.as_deref());
            (t.organization_component.is_some() && icon.and_then(ui_path).is_none()).then_some(())
        })
        .into_keys()
        .collect()
    }

    /// The archive path of the image marking each template's objects on
    /// the minimap, by template id.
    pub fn minimap_icons(&mut self) -> HashMap<Id, String> {
        self.by_template(|t| ui_path(t.view.as_ref()?.minimap_icon.as_deref()?))
    }

    /// The minimap's colours.
    pub fn minimap(&mut self) -> Option<defs::Minimap> {
        let files = self.parse_tgis(|n| n == "minimap/minimap.tgi");
        let files = scoped(&files);
        files.iter().find_map(|f| f.read("Minimap").next())
    }

    /// What each nation's banners are drawn with, and which nation each
    /// organization template is of.
    pub fn nation_banners(&mut self) -> NationBanners {
        let files = self.parse_tgis(|n| {
            ["nations/", "templates/", "organizations/banners/"]
                .iter()
                .any(|d| n.starts_with(d))
        });
        let files = scoped(&files);
        let prefixes: HashMap<Id, String> = files
            .iter()
            .flat_map(|f| f.read::<defs::Banner>("Banner"))
            .filter_map(|b| Some((b.ids?, b.resource_prefix?)))
            .collect();
        let mut out = NationBanners::default();
        let mut by_group = HashMap::new();
        for nation in files.iter().flat_map(|f| f.read::<defs::Nation>("Nation")) {
            let Some(ids) = nation.ids else {
                continue;
            };
            let badges = nation.badge.iter();
            let paths = badges.map(|b| format!("Nations/{}", b.badge_texture));
            out.badges.insert(ids.clone(), paths.collect());
            // The banner named for the nation, `ger_banner` for
            // `nation_ger`.
            let lower = ids.as_str().to_ascii_lowercase();
            let banner = lower.strip_prefix("nation_").map(|n| format!("{n}_banner"));
            if let Some(prefix) = banner.and_then(|b| prefixes.get(&Id::from(b.as_str()))) {
                let path = format!("Organizations/Banners/{prefix}BannerBase.png");
                out.frames.insert(ids.clone(), path);
            }
            if let Some(group) = nation.placement_group_ids {
                by_group.insert(group, ids);
            }
        }
        out.of_template = self.by_template(|t| {
            let group = t.placement.as_ref()?.placement_group_ids.as_ref()?;
            by_group.get(group).cloned()
        });
        out
    }

    /// Organizations' ranges, by template id, including the ids the game
    /// has since renamed.
    pub fn organization_ranges(&mut self) -> HashMap<Id, Ranges> {
        self.by_template(|t| {
            let o = t.organization_component.as_ref()?;
            Some(Ranges {
                operational: o.operational_range,
                control: o.control_range,
            })
        })
    }

    /// What every organization template is made of, by template id,
    /// including the ids the game has since renamed.
    pub fn organizations(&mut self) -> HashMap<Id, Organization> {
        self.by_template(|t| {
            let o = t.organization_component.as_ref()?;
            Some(Organization {
                layout: o.layout_ids.clone()?,
                // Templates name an empty slot's element `null`.
                slots: o
                    .slot
                    .iter()
                    .map(|s| s.element_ids.clone().filter(|e| *e != Id::from("null")))
                    .collect(),
            })
            .filter(|o| !o.slots.is_empty())
        })
    }

    /// What each template that recruits lists, in the order its buttons
    /// take, by template id: for a Corps HQ, what it commissions.
    pub fn recruit_lists(&mut self) -> HashMap<Id, Vec<Id>> {
        self.by_template(|t| {
            let list = &t.recruit_component.as_ref()?.recruit_actor;
            let ids: Vec<Id> = list.iter().filter_map(|r| r.actor_ids.clone()).collect();
            (!ids.is_empty()).then_some(ids)
        })
    }

    /// How each template that packs or unpacks does, by template id.
    pub fn deployments(&mut self) -> HashMap<Id, Deployment> {
        self.by_template(|t| {
            let m = t.mobile_deployment_component.as_ref()?;
            Some(Deployment {
                unpacks_into: m
                    .unpack_to_actor_ids
                    .clone()
                    .filter(|i| *i != Id::from("null")),
                packed_offset: m.packed_position_offset.unwrap_or([0.0, 0.0]),
                packed_facing: m.packed_orientation.unwrap_or([0.0, -1.0]),
                packing: m.packing_length.unwrap_or(7.0),
                unpacking: m.unpacking_length.unwrap_or(7.0),
                pack_health_min: m.pack_health_min.unwrap_or(0.5),
            })
        })
    }

    /// The packed building, such as a truck, that each building packs into,
    /// by template id: of those that unpack into it, the one defined first.
    pub fn packs_into(&mut self) -> HashMap<Id, Id> {
        let renames = self.thing_renames();
        let mut out = HashMap::new();
        for t in &self.definitions().things {
            let into = (t.thing.mobile_deployment_component.as_ref())
                .and_then(|m| m.unpack_to_actor_ids.clone())
                .filter(|i| *i != Id::from("null"));
            if let Some(into) = into {
                let into = renamed(&renames, &into);
                out.entry(into).or_insert_with(|| t.id.clone());
            }
        }
        if let Some(bytes) = self.read("translate.tgi") {
            add_renames(&mut out, &tgi::decode(&bytes), "Thing");
        }
        out
    }

    /// Seconds recruiting or commissioning one of each template takes, by
    /// template id: an organization's recruit time, else the inverse of
    /// how fast it resupplies.
    pub fn recruit_times(&mut self) -> HashMap<Id, f32> {
        self.by_template(|t| match &t.organization_component {
            Some(o) => o.recruit_time,
            None => {
                let rate = t.element_component.as_ref()?.resupply_rate?;
                // The game takes half a tick for what never resupplies.
                Some(if rate > 0.0 { 1.0 / rate } else { 0.05 })
            }
        })
    }

    /// What buildings recruiting each template needs, by template id, for
    /// those that need any.
    pub fn requirements(&mut self) -> HashMap<Id, Requires> {
        // Maps may still place a building under any id it once had.
        let renames = self.thing_renames();
        let mut aliases: HashMap<Id, Vec<Id>> = HashMap::new();
        for (old, new) in &renames {
            aliases.entry(new.clone()).or_default().push(old.clone());
        }
        for olds in aliases.values_mut() {
            olds.sort_unstable();
        }
        self.by_template(|t| {
            let e = t.element_component.as_ref()?;
            let structures: Vec<Vec<Id>> = listed(e.required_structures.as_deref())
                .map(|s| {
                    let id = renamed(&renames, &Id::from(s));
                    let olds = aliases.get(&id).cloned().unwrap_or_default();
                    std::iter::once(id).chain(olds).collect()
                })
                .collect();
            let properties: Vec<Id> = listed(e.required_properties.as_deref())
                .map(Id::from)
                .collect();
            (!structures.is_empty() || !properties.is_empty()).then_some(Requires {
                structures,
                any: e.required_structures_any,
                properties,
                hide: t.hide_if_missing_properties.unwrap_or(true),
            })
        })
    }

    /// The transport each template that's carried to where it's recruited
    /// names, by template id.
    pub fn transports(&mut self) -> HashMap<Id, Id> {
        self.by_template(|t| (t.transport_actor.clone()).filter(|i| *i != Id::from("null")))
    }

    /// Each renamed object template's old id to its new one.
    fn thing_renames(&mut self) -> HashMap<Id, Id> {
        let Some(bytes) = self.read("translate.tgi") else {
            return HashMap::new();
        };
        renames(&tgi::decode(&bytes), "Thing")
    }

    /// Where what each template recruits appears, by template id.
    pub fn deploy_points(&mut self) -> HashMap<Id, DeployPoints> {
        self.by_template(|t| {
            let d = t.denizen_component.as_ref()?.deployment_info.as_ref()?;
            let creation = d.creation_offset.unwrap_or([0.0, 0.0]);
            Some(DeployPoints {
                creation,
                deployment: d.deployment_offset.unwrap_or(creation),
            })
        })
    }

    /// How each template that has a name shows on the button that
    /// commissions or recruits it, by template id. An organization costs
    /// what its captain, the one member it's raised with, does as well as
    /// its own, which for a Division HQ is all of it, and takes to keep its
    /// own upkeep and each member's.
    pub fn listings(&mut self) -> HashMap<Id, Listing> {
        let strings = self.strings();
        let layouts = self.layouts();
        let definitions = self.definitions();
        let mut things: HashMap<&Id, &defs::Thing> = definitions
            .things
            .iter()
            .map(|t| (&t.id, &t.thing))
            .collect();
        // Templates may name their members by ids since renamed.
        let renames = self.thing_renames();
        let renamed: Vec<_> = (renames.iter())
            .map(|(old, new)| (old, things.get(new).copied()))
            .collect();
        for (old, thing) in renamed {
            match thing {
                Some(t) => things.insert(old, t),
                None => things.remove(old),
            };
        }
        self.by_template(|t| {
            let view = t.view.as_ref();
            let captain = t
                .organization_component
                .as_ref()
                .and_then(|o| o.slot.last()?.element_ids.as_ref())
                .and_then(|id| things.get(id));
            let economies: Vec<&defs::EconomyComponent> = [Some(t), captain.copied()]
                .into_iter()
                .flatten()
                .filter_map(|t| t.economy_component.as_ref())
                .collect();
            let economy = |of: fn(&defs::EconomyComponent) -> Option<&defs::Resources>| {
                amounts(&strings, economies.iter().filter_map(|&e| of(e)))
            };
            let members = members(t, &things, &layouts);
            Some(Listing {
                name: localized(&strings, t.name.as_deref())?,
                icon: view
                    .and_then(|v| v.icon.as_deref())
                    .filter(|i| i.to_ascii_lowercase().starts_with("/ui/"))
                    .and_then(ui_path),
                cost: economy(|e| e.cost.as_ref()),
                production: amounts(&strings, production(t, &members)),
                upkeep: amounts(&strings, upkeep(t, &members)),
            })
        })
    }

    /// What `of` finds in each template that has it, by template id,
    /// including the ids the game has since renamed.
    fn by_template<T: Clone>(&mut self, of: impl Fn(&defs::Thing) -> Option<T>) -> HashMap<Id, T> {
        let mut out = HashMap::new();
        for t in &self.definitions().things {
            if let Some(v) = of(&t.thing) {
                out.insert(t.id.clone(), v);
            }
        }
        if let Some(bytes) = self.read("translate.tgi") {
            add_renames(&mut out, &tgi::decode(&bytes), "Thing");
        }
        out
    }

    /// Every `.tgi` file's [`Definitions`]. Deserialized once and kept, as
    /// there are thousands of templates and much of the game asks after them.
    fn definitions(&mut self) -> Arc<Definitions> {
        if let Some(definitions) = &self.definitions {
            return definitions.clone();
        }
        let definitions = Arc::new(Definitions::new(&self.parse_tgis(|_| true)));
        self.definitions = Some(definitions.clone());
        definitions
    }

    /// The organization layouts, by id.
    pub fn layouts(&mut self) -> HashMap<Id, Layout> {
        let files =
            self.parse_tgis(|n| n.starts_with("organizations/") || n.starts_with("templates/"));
        let files = scoped(&files);
        let aggressive: HashSet<Id> = files
            .iter()
            .flat_map(|f| f.read::<defs::Formation>("Formation"))
            .filter(|f| f.aggressive)
            .filter_map(|f| f.ids)
            .collect();
        files
            .iter()
            .flat_map(|f| f.read::<defs::OrganizationLayout>("OrganizationLayout"))
            .filter_map(|l| Some((l.ids.clone()?, layout(&l, &aggressive)?)))
            .collect()
    }

    /// A template's models: its aspects when they are variations, else its
    /// normal model, else whatever aspects it lists. Models and animations
    /// missing from the archive are left out, so a view may have none.
    fn view(&self, dir: &str, thing: &defs::Thing, view: &defs::View) -> Option<View> {
        let prefix = view.resource_prefix.as_deref().unwrap_or("");
        let path = |m: &str| {
            let m = m.replace("%s", prefix).replace('\\', "/");
            match m.strip_prefix('/') {
                Some(abs) => abs.to_string(),
                None => format!("{dir}/{m}"),
            }
        };
        // Aspects take their model, if they don't name one, from templates
        // such as the schema's `%s.nif`.
        let aspects: Vec<String> = view
            .aspect
            .iter()
            .filter_map(|a| a.model.as_deref().map(path))
            .collect();
        let normal = view
            .aspect_normal
            .iter()
            .find_map(|a| a.model.as_deref())
            .map(path)
            .or_else(|| (!prefix.is_empty()).then(|| path("%s.nif")));
        if aspects.is_empty() && normal.is_none() {
            return None;
        }
        let state_model = |ids: &str| {
            view.aspect
                .iter()
                .filter(|a| a.ids == Some(Id::from(ids)))
                .find_map(|a| a.model.as_deref().map(path))
                .filter(|p| self.exists(p))
        };
        // Interface images name their folder for a screen size `%s`;
        // others take the resource prefix, a unit's `%sIcon.tga` say.
        let icon = view
            .icon
            .as_deref()
            .and_then(|i| match i.to_ascii_lowercase().starts_with("/ui/") {
                true => ui_path(i),
                false => Some(path(i)),
            })
            .or_else(|| (!prefix.is_empty()).then(|| path("%sIcon.tga")))
            .filter(|p| self.exists(p));
        let unpacking = state_model("aspect_unpacking");
        let packing = state_model("aspect_packing");
        let aspects: Vec<String> = aspects
            .into_iter()
            .filter(|p| {
                self.exists(p) && Some(p) != unpacking.as_ref() && Some(p) != packing.as_ref()
            })
            .collect();
        let normal = normal.filter(|p| self.exists(p));
        let texture_renames = texture_renames(view, normal.as_deref(), path);
        let models = match normal {
            Some(n) if !view.aspects_as_variations => vec![n],
            _ => aspects,
        };
        let texture_dirs = texture_dirs(&view.texture_search_path);
        let scale_min = thing.size.iter().find_map(|s| s.scale_min);
        let scale_max = thing.size.iter().find_map(|s| s.scale_max);
        let scale = match (scale_min, scale_max) {
            (Some(a), Some(b)) => (a + b) / 2.0,
            (Some(a), None) | (None, Some(a)) => a,
            _ => 1.0,
        };
        // A `.kf` path's second `%s` is the animation's ids.
        let animations = thing
            .character_component
            .iter()
            .flat_map(|c| &c.animation)
            .filter_map(|a| {
                let ids = a.ids.clone()?;
                let kf = path(
                    &a.kf
                        .as_deref()?
                        .replacen("%s", prefix, 1)
                        .replacen("%s", &ids, 1),
                );
                Some(Animation {
                    kind: a.kind.clone().unwrap_or_default(),
                    ids,
                    kf,
                    length: a.length.unwrap_or(1.0),
                    walk_distance: a.walk_distance,
                })
            })
            .filter(|a| self.exists(&a.kf))
            .collect();
        Some(View {
            models,
            texture_dirs,
            texture_renames,
            scale,
            animations,
            conform: conform(thing),
            footing: footing(thing),
            unpacking,
            packing,
            icon,
        })
    }

    /// How much of the `.tgi` data the definitions read: each top-level
    /// block is read as its definition, the way the methods above read it,
    /// or counted as unread where there is none. Templates count through the
    /// blocks that inherit from them.
    pub fn coverage(&mut self) -> tgi::de::Coverage {
        let files = self.parse_tgis(|_| true);
        let files = scoped(&files);
        let mut coverage = tgi::de::Coverage::default();
        for (block, t) in
            (files.iter()).flat_map(|f| f.root.blocks().map(|b| (b, Some(&f.templates))))
        {
            // Blocks that don't deserialize still count what they read.
            let _ = match block.kind().unwrap_or("").to_ascii_lowercase().as_str() {
                "template" => continue,
                "thing" => coverage.read::<defs::Thing>(block, t).map(drop),
                "terrain" => coverage.read::<defs::Terrain>(block, t).map(drop),
                "biome" => coverage.read::<defs::Biome>(block, t).map(drop),
                "translate" => coverage.read::<defs::Translate>(block, None).map(drop),
                "transitionset" => coverage.read::<defs::TransitionSet>(block, None).map(drop),
                "sun" => coverage.read::<defs::Sun>(block, t).map(drop),
                "property" => coverage.read::<defs::Property>(block, t).map(drop),
                "organizationlayout" => coverage
                    .read::<defs::OrganizationLayout>(block, t)
                    .map(drop),
                "nation" => coverage.read::<defs::Nation>(block, t).map(drop),
                "kingdomcolor" => coverage.read::<defs::KingdomColor>(block, t).map(drop),
                "actorcommand" => coverage.read::<defs::ActorCommand>(block, t).map(drop),
                "formation" => coverage.read::<defs::Formation>(block, t).map(drop),
                "audio" => coverage.read::<defs::Audio>(block, t).map(drop),
                "vfx" => coverage.read::<defs::Vfx>(block, t).map(drop),
                "cursor" => coverage.read::<defs::Cursor>(block, t).map(drop),
                "glyph" => coverage.read::<defs::Glyph>(block, t).map(drop),
                "svars" => coverage.read::<defs::SVars>(block, t).map(drop),
                "resource" => coverage.read::<defs::Resource>(block, t).map(drop),
                // A string table, read whole.
                "text" => coverage
                    .read::<HashMap<String, String>>(block, None)
                    .map(drop),
                _ => {
                    coverage.skip(block, t);
                    continue;
                }
            };
        }
        coverage
    }

    /// The lighting presets the player can pick from, by id.
    pub fn sun_presets(&mut self) -> Vec<defs::Sun> {
        let files = self.parse_tgis(|n| n.starts_with("sun/"));
        let files = scoped(&files);
        let mut out: Vec<defs::Sun> = files.iter().flat_map(|f| f.read("Sun")).collect();
        out.sort_by(|a, b| a.ids.cmp(&b.ids));
        out
    }

    /// The nations sides can play as, by name.
    pub fn nations(&mut self) -> Vec<Nation> {
        let strings = self.strings();
        let generals = self.generals(&strings);
        let files = self.parse_tgis(|n| n.starts_with("nations/"));
        let files = scoped(&files);
        let mut nations: Vec<Nation> = files
            .iter()
            .flat_map(|f| f.read::<defs::Nation>("Nation"))
            .filter_map(|n| {
                let id = n.ids?;
                let placeholders = n.placeholder.into_iter();
                Some(Nation {
                    name: localized(&strings, n.name.as_deref()).unwrap_or_else(|| id.to_string()),
                    id,
                    placeholders: placeholders
                        .filter_map(|p| Some((p.placeholder_ids?, p.thing_ids?)))
                        .collect(),
                    generals: (n.factions.as_deref().unwrap_or("").split(','))
                        .filter_map(|id| generals.get(&Id::from(id.trim())).cloned())
                        .collect(),
                })
            })
            .collect();
        nations.sort_by(|a, b| a.name.cmp(&b.name));
        nations
    }

    /// The generals who may lead a side, by id.
    fn generals(&mut self, strings: &HashMap<String, String>) -> HashMap<Id, General> {
        let definitions = self.definitions();
        (definitions.generals.iter())
            .filter_map(|g| {
                let id = g.ids.clone()?;
                let name = localized(strings, g.name.as_deref()).unwrap_or_else(|| id.to_string());
                let general = General {
                    short_name: localized(strings, g.staging_name.as_deref())
                        .unwrap_or_else(|| name.clone()),
                    name,
                    operations: (g.ability.iter())
                        .filter_map(|a| a.ability_ids.clone())
                        .collect(),
                    public: g.public != Some(false),
                    id: id.clone(),
                };
                Some((id, general))
            })
            .collect()
    }

    /// The colours players can take, by id, e.g. `blue`.
    pub fn kingdom_colors(&mut self) -> HashMap<Id, [f32; 3]> {
        let files = self.parse_tgis(|n| n == "game/kingdom_colors.tgi");
        let files = scoped(&files);
        files
            .iter()
            .flat_map(|f| f.read::<defs::KingdomColor>("KingdomColor"))
            .filter_map(|c| Some((c.ids?, c.color)))
            .collect()
    }

    /// How far beyond a blocking building's footprint the ground must be
    /// clear for it to be put there, in world units.
    pub fn building_padding(&mut self) -> Option<f32> {
        self.svars().iter().find_map(|v| v.building_padding_meters)
    }

    /// How the supply border is drawn; green and thin where the settings
    /// don't say.
    pub fn supply_border(&mut self) -> SupplyBorder {
        // Not in the load lists; the game's own settings take precedence
        // over the engine's.
        let files: Vec<(String, tgi::Parsed)> = ["AVars_AA.tgi", "AVars.tgi"]
            .into_iter()
            .filter_map(|n| Some((n.to_string(), tgi::parse_bytes(&self.read(n)?))))
            .collect();
        let files = scoped(&files);
        let vars: Vec<defs::AVars> = files.iter().flat_map(|f| f.read("Vars")).collect();
        SupplyBorder {
            colour: (vars.iter())
                .find_map(|v| v.view_zone_friendly_supply_color)
                .unwrap_or([0.0, 1.0, 0.0]),
            width: (vars.iter())
                .find_map(|v| v.view_zone_friendly_supply_width)
                .unwrap_or(0.15),
            texture: (vars.iter()).find_map(|v| v.view_zone_friendly_supply_texture.clone()),
            block: (vars.iter())
                .find_map(|v| v.terrain_block_size)
                .unwrap_or(8.0),
        }
    }

    /// What colours a figure or option that's no good, such as a shortfall.
    pub fn invalid_colour(&mut self) -> Option<[f32; 3]> {
        self.interface_setting(|v| v.ui_color_option_invalid)
    }

    /// What colours the key for something where its name is shown.
    pub fn highlight_colour(&mut self) -> Option<[f32; 3]> {
        self.interface_setting(|v| v.ui_color_highlight)
    }

    /// One of the interface's settings.
    fn interface_setting<T>(&mut self, of: impl Fn(defs::UVars) -> Option<T>) -> Option<T> {
        // Not in the load lists; the game's own settings take precedence
        // over the engine's.
        let files: Vec<(String, tgi::Parsed)> = ["UVars_AA.tgi", "UVars.tgi"]
            .into_iter()
            .filter_map(|n| Some((n.to_string(), tgi::parse_bytes(&self.read(n)?))))
            .collect();
        let files = scoped(&files);
        files
            .iter()
            .flat_map(|f| f.read::<defs::UVars>("Vars"))
            .find_map(of)
    }

    /// The orders the player can give, in the order the files list them.
    pub fn commands(&mut self) -> Vec<Command> {
        let strings = self.strings();
        let files = self.parse_tgis(|n| {
            n.starts_with("game/actor_commands") || n.starts_with("game/actorcommands/")
        });
        let files = scoped(&files);
        files
            .iter()
            .flat_map(|f| f.read::<defs::ActorCommand>("ActorCommand"))
            .filter_map(|c| {
                let ids = c.ids?;
                Some(Command {
                    name: localized(&strings, c.name.as_deref()).unwrap_or(ids.to_string()),
                    ids,
                    tooltip: localized(&strings, c.tooltip.as_deref()).filter(|t| !t.is_empty()),
                    disabled: localized(&strings, c.description_disabled.as_deref())
                        .filter(|t| !t.is_empty()),
                    icon: c.icon.as_deref().and_then(ui_path),
                    slot: c.preferred_position.and_then(|p| usize::try_from(p).ok()),
                    cursor: not_null(c.cursor_ids),
                    marker: c.command_vfx_ids_location,
                    group_offset: c.apply_group_offset.unwrap_or(true),
                })
            })
            .collect()
    }

    /// The mouse cursors whose frames are all there, by id.
    pub fn cursors(&mut self) -> HashMap<Id, CursorLook> {
        let files = self.parse_tgis(|n| n.starts_with("cursors/"));
        let files = scoped(&files);
        let mut out = HashMap::new();
        for Scoped {
            file,
            root,
            templates,
        } in &files
        {
            for block in root.blocks_of("Cursor") {
                let Ok(c) = templates.deserialize::<defs::Cursor>(block) else {
                    continue;
                };
                let (Some(ids), Some(pattern)) = (c.ids, c.frame_filename) else {
                    continue;
                };
                let frames = frames(&beside(file, &pattern), |f| self.exists(f));
                if frames.is_empty() {
                    continue;
                }
                let [x, y] = c.hotspot_offset.unwrap_or_default();
                out.insert(
                    ids,
                    CursorLook {
                        frames,
                        hotspot: [x as u16, y as u16],
                        period: c.loop_length_ms.unwrap_or(1000.0) / 1000.0,
                    },
                );
            }
        }
        out
    }

    /// Every visual effect, by id.
    pub fn vfx(&mut self) -> HashMap<Id, Vfx> {
        // Effects live beside what uses them as well as in their own folder.
        let files = self.parse_tgis(|_| true);
        let files = scoped(&files);
        let mut out = HashMap::new();
        for Scoped {
            file,
            root,
            templates,
        } in &files
        {
            // Ground decals are declared as things of a VFX category.
            let blocks = root.blocks_of("VFX").chain(root.blocks_of("Thing"));
            for block in blocks {
                let Ok(v) = templates.deserialize::<defs::Vfx>(block) else {
                    continue;
                };
                let is_vfx = v
                    .category
                    .as_deref()
                    .is_some_and(|c| c.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("VFX")));
                let (true, Some(ids)) = (is_vfx, v.ids.clone()) else {
                    continue;
                };
                let model = v
                    .model
                    .as_deref()
                    .map(|m| beside(file, m))
                    .filter(|m| self.exists(m));
                out.insert(ids, vfx(v, model));
            }
        }
        out
    }

    /// How each formation shows on its button, by formation id.
    pub fn formation_looks(&mut self) -> HashMap<Id, FormationLook> {
        let strings = self.strings();
        let files = self.parse_tgis(|n| n.starts_with("organizations/"));
        let files = scoped(&files);
        let mut out = HashMap::new();
        for Scoped {
            file,
            root,
            templates,
        } in &files
        {
            let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
            for block in root.blocks_of("Formation") {
                let Ok(f) = templates.deserialize::<defs::Formation>(block) else {
                    continue;
                };
                let Some(ids) = f.ids else {
                    continue;
                };
                let icon = f.icon.as_deref().map(|i| match i.strip_prefix('/') {
                    Some(abs) => abs.to_string(),
                    None => format!("{dir}/{i}"),
                });
                let icon = icon.filter(|i| self.exists(i));
                let name = localized(&strings, f.name.as_deref()).unwrap_or(ids.to_string());
                out.insert(ids, FormationLook { name, icon });
            }
        }
        out
    }

    /// The templates whose things each side numbers.
    pub fn numbered(&mut self) -> HashSet<Id> {
        let numbered = self.by_template(|t| t.numbered.filter(|&n| n));
        numbered.into_keys().collect()
    }

    /// The game's text in its default language, by key.
    pub fn strings(&mut self) -> HashMap<String, String> {
        let glyphs = self.glyphs();
        let files = self.parse_tgis(|n| n.starts_with("localization/strings_"));
        let mut out = HashMap::new();
        let blocks = files.iter().flat_map(|(_, p)| p.root.blocks_of("Text"));
        for (key, text) in blocks.flat_map(text_strings) {
            // A key defined again keeps its first text.
            out.entry(key).or_insert_with(|| unescape(&text, &glyphs));
        }
        out
    }

    /// The characters quoted text names by id, such as `n` for a newline
    /// or `ch_gem` for one the font draws as an icon.
    fn glyphs(&mut self) -> HashMap<Id, char> {
        let files = self.parse_tgis(|n| n.starts_with("fonts/special_glyphs"));
        let files = scoped(&files);
        files
            .iter()
            .flat_map(|f| f.read::<defs::Glyph>("Glyph"))
            .filter_map(|g| Some((g.ids?, char::from_u32(g.index?)?)))
            .collect()
    }

    /// The audio of each object template that plays any, by template id.
    pub fn object_audio(&mut self) -> HashMap<Id, ObjectAudio> {
        let mut out = self.by_template(|t| Some(object_audio(t)));
        out.retain(|_, a| !a.is_empty());
        out
    }

    /// The effects of each object template that shows any, by template id.
    pub fn object_effects(&mut self) -> HashMap<Id, ObjectEffects> {
        let mut out = self.by_template(|t| Some(object_effects(t)));
        out.retain(|_, e| !e.is_empty());
        out
    }

    /// The node names of the model points effects name by id, such as
    /// `gun_ae_01`.
    pub fn bones(&mut self) -> HashMap<Id, String> {
        let files = self.parse_tgis(|n| n.starts_with("game/bones"));
        let files = scoped(&files);
        files
            .iter()
            .flat_map(|f| f.read::<defs::Bone>("Bone"))
            .filter_map(|b| Some((b.ids?, b.bone_name?.trim().to_string())))
            .collect()
    }

    /// Every sound, speech and music, by id.
    pub fn audio(&mut self) -> HashMap<Id, Audio> {
        let files = self.parse_tgis(|n| n.starts_with("audio/") || n.starts_with("templates/"));
        let files = scoped(&files);
        files
            .iter()
            .flat_map(|f| f.read::<defs::Audio>("Audio"))
            .filter_map(|a| Some((a.ids.clone()?, audio(&a, |p| self.path(p))?)))
            .collect()
    }

    /// An archive path as the archives spell it.
    fn path(&self, path: &str) -> Option<String> {
        let &entry = self.paths.get(&path.to_ascii_lowercase())?;
        Some(self.entry_name(entry).to_string())
    }

    /// The transition masks of the first transition set, keyed by the
    /// digits in their file names.
    pub fn transition_masks(&mut self) -> Vec<(String, Vec<u8>)> {
        const DIR: &str = "Terrain/Transitions/";
        let Some(prefix) = self
            .read(&format!("{DIR}transition_sets.tgi"))
            .and_then(|b| {
                let parsed = tgi::parse_bytes(&b);
                let set = parsed.root.blocks_of("TransitionSet").next()?;
                let set: defs::TransitionSet = tgi::from_block(set).ok()?;
                set.path_prefix.map(|p| format!("{DIR}{p}"))
            })
        else {
            return Vec::new();
        };
        let lower = prefix.to_ascii_lowercase();
        self.entry_names(|n| n.len() > lower.len() && n.starts_with(&lower))
            .into_iter()
            .filter_map(|n| {
                let digits = n[prefix.len()..].split('.').next()?.to_string();
                Some((digits, self.read(&n)?))
            })
            .collect()
    }
}

/// A layout's formations, each with its positions given to the slots in
/// turn: a slot takes the next position for its property.
fn layout(l: &defs::OrganizationLayout, aggressive: &HashSet<Id>) -> Option<Layout> {
    let formations: Vec<Formation> = l
        .formation
        .iter()
        .filter_map(|f| {
            let id = f.formation_ids.clone()?;
            Some(Formation {
                aggressive: aggressive.contains(&id),
                slots: slot_positions(&l.slot, f)?,
                id,
            })
        })
        .collect();
    let index = |id: &Option<Id>| {
        let id = id.as_ref()?;
        formations.iter().position(|f| &f.id == id)
    };
    // Layouts may inherit a default they don't define.
    let default = index(&l.default_formation_ids).unwrap_or(0);
    let combat = index(&l.combat_formation_ids);
    (!formations.is_empty()).then(|| Layout {
        formations,
        default,
        combat,
        primary: l.slot.iter().position(|s| s.primary),
    })
}

fn slot_positions(
    slots: &[defs::LayoutSlot],
    formation: &defs::LayoutFormation,
) -> Option<Vec<Vec<[f32; 2]>>> {
    let mut taken = vec![false; formation.position.len()];
    slots
        .iter()
        .map(|slot| {
            let property = slot.property_ids.as_deref()?;
            (0..slot.number.unwrap_or(1))
                .map(|_| {
                    let i = formation.position.iter().enumerate().position(|(i, p)| {
                        !taken[i]
                            && p.property_ids
                                .as_deref()
                                .is_some_and(|q| q.eq_ignore_ascii_case(property))
                    })?;
                    taken[i] = true;
                    Some(formation.position[i].offset.unwrap_or([0.0, 0.0]))
                })
                .collect()
        })
        .collect()
}

fn body(thing: &defs::Thing) -> Body {
    let flag =
        |f: fn(&defs::Flags) -> Option<bool>| thing.flags.iter().rev().find_map(f) == Some(true);
    let shape = blocking_shape(thing);
    let mover = thing.mover_component.as_ref();
    let speed = mover.and_then(|m| m.speed_max).filter(|&s| s > 0.0);
    // Turning takes time only for turn types that name a radius to turn
    // round, at top speed.
    let turns = mover
        .and_then(|m| m.turn_type.as_deref())
        .is_some_and(|t| !t.eq_ignore_ascii_case("INSTANT"));
    let radius = mover.and_then(|m| m.turn_radius).filter(|&r| r > 0.0);
    let turn_rate = match (turns, speed, radius) {
        (true, Some(s), Some(r)) => s / r,
        _ => 0.0,
    };
    let hitch = mover.and_then(|m| m.trailer_hitch.as_ref()?.node_offset);
    Body {
        blocking: flag(|f| f.blocking).then_some(shape),
        shape,
        soft: soft_shape(thing),
        water_based: flag(|f| f.water_based),
        tile_locked: flag(|f| f.tile_locked),
        ethereal: flag(|f| f.ethereal),
        fixed: speed.is_none()
            && thing.organization_component.is_none()
            && thing.projectile_component.is_none(),
        speed,
        acceleration: mover.and_then(|m| m.acceleration).unwrap_or(0.0).max(0.0),
        turn_rate,
        hitch: hitch.map(|[x, y, _]| [x, y]),
        trailer: mover
            .and_then(|m| m.trailer_actor.clone())
            .filter(|t| !t.as_str().eq_ignore_ascii_case("null")),
    }
}

fn conform(thing: &defs::Thing) -> Option<Shape> {
    let conforms = thing.flags.iter().rev().find_map(|f| f.conform_to_terrain) == Some(true);
    conforms.then(|| soft_shape(thing))
}

fn blocking_shape(thing: &defs::Thing) -> Shape {
    let kind = thing
        .size
        .iter()
        .rev()
        .find_map(|s| s.blocking_shape.as_deref());
    shape(thing, kind, |s| s.blocking_radius, 0.2)
}

/// The soft shape, which where the template leaves it `null` takes the
/// blocking shape's kind, but still its own radius as a circle.
fn soft_shape(thing: &defs::Thing) -> Shape {
    let kind = thing
        .size
        .iter()
        .rev()
        .find_map(|s| s.soft_shape.as_deref())
        .filter(|k| !k.trim().is_empty() && !k.trim().eq_ignore_ascii_case("null"));
    let kind = kind.or_else(|| {
        thing
            .size
            .iter()
            .rev()
            .find_map(|s| s.blocking_shape.as_deref())
    });
    shape(thing, kind, |s| s.soft_radius, 0.5)
}

/// The target shape, which where the template leaves it `null` is the
/// soft shape.
fn target_shape(thing: &defs::Thing) -> Shape {
    let size = |f: fn(&defs::Size) -> Option<&str>| thing.size.iter().rev().find_map(f);
    match size(|s| s.target_shape.as_deref()).and_then(named) {
        Some(kind) => shape(thing, Some(kind), |s| s.target_radius, 0.5),
        None => soft_shape(thing),
    }
}

/// The kind of shape the target shape ends up as.
fn target_kind(thing: &defs::Thing) -> Option<&str> {
    let size = |f: fn(&defs::Size) -> Option<&str>| thing.size.iter().rev().find_map(f);
    (size(|s| s.target_shape.as_deref()).and_then(named))
        .or_else(|| size(|s| s.soft_shape.as_deref()).and_then(named))
        .or_else(|| size(|s| s.blocking_shape.as_deref()))
        .map(str::trim)
}

/// A shape's kind, unless it's left `null`.
fn named(kind: &str) -> Option<&str> {
    let kind = kind.trim();
    (!kind.is_empty() && !kind.eq_ignore_ascii_case("null")).then_some(kind)
}

fn footing(thing: &defs::Thing) -> Footing {
    let flag =
        |f: fn(&defs::Flags) -> Option<bool>| thing.flags.iter().rev().find_map(f) == Some(true);
    if flag(|f| f.water_based) {
        Footing::Water
    } else if flag(|f| f.flying) {
        Footing::Air
    } else {
        Footing::Ground
    }
}

/// A shape of the template's `[Size]`, which its shapes share all but the
/// radius of, `default` where it sets none.
fn shape(
    thing: &defs::Thing,
    kind: Option<&str>,
    radius: fn(&defs::Size) -> Option<f32>,
    default: f32,
) -> Shape {
    let size = |f: fn(&defs::Size) -> Option<f32>| thing.size.iter().rev().find_map(f);
    match kind.unwrap_or("CIRCLE").to_ascii_uppercase().as_str() {
        "POINT" => Shape::Point,
        "SQUARE" => {
            let side = size(|s| s.square_size).unwrap_or(4.0);
            Shape::Rect {
                width: side,
                height: side,
            }
        }
        "RECT" => Shape::Rect {
            width: size(|s| s.rect_width).unwrap_or(4.0),
            height: size(|s| s.rect_height).unwrap_or(4.0),
        },
        _ => Shape::Circle {
            radius: size(radius).unwrap_or(default),
        },
    }
}

fn fighter(thing: &defs::Thing, shared: &HashMap<Id, defs::Property>) -> Option<Fighter> {
    let body = thing.body_component.as_ref()?;
    let health = body.health.filter(|&h| h > 0.0)?;
    let mut fighter = Fighter {
        health,
        defense: body.defense.unwrap_or(0.0),
        capturable: body.captureable.unwrap_or(false),
        destructible: body.destructible.unwrap_or(true),
        notice: (thing.defender_component.as_ref())
            .map(|d| d.control_range.unwrap_or(0.0).max(0.0)),
        soft: soft_shape(thing),
        target: target_shape(thing),
        target_square: target_kind(thing).is_some_and(|k| k.eq_ignore_ascii_case("SQUARE")),
        ..Fighter::default()
    };
    let properties = thing.properties_component.iter().flat_map(|c| {
        let refs = c
            .property
            .iter()
            .filter_map(|r| shared.get(r.property_ids.as_ref()?));
        refs.chain(&c.intrinsic_property)
    });
    for p in properties {
        let flags = p.enabled_target_flags.as_deref().unwrap_or("");
        for flag in flags.split(',').map(str::trim) {
            let flag = Id::from(flag);
            if !flag.as_str().is_empty()
                && flag != Id::from("null")
                && !fighter.flags.contains(&flag)
            {
                fighter.flags.push(flag);
            }
        }
        for m in &p.modifier {
            let resist = m
                .kind
                .as_deref()
                .is_some_and(|k| k.eq_ignore_ascii_case("RESIST_DAMAGE"));
            if let (true, Some(kind), Some(amount)) = (resist, &m.damage_type, m.amount) {
                *fighter.resists.entry(kind.clone()).or_default() += amount;
            }
        }
    }
    fighter.attack_height = (thing.performer_component.iter())
        .find_map(|p| p.attack_height)
        .unwrap_or(1.45);
    let turrets = thing.performer_component.iter().flat_map(|p| &p.turret);
    let name = thing.ids.as_ref().map_or("?", Id::as_str);
    let turrets: Vec<(Option<&Id>, Turret)> = turrets
        .map(|t| (t.turret_ids.as_ref(), turret(name, t)))
        .collect();
    fighter.turrets = turrets.iter().map(|(_, t)| t.clone()).collect();
    let performs = thing.performer_component.iter().flat_map(|p| &p.ability);
    let defends = thing.defender_component.iter().flat_map(|d| &d.ability);
    let abilities = performs.chain(defends);
    for (a, d) in abilities.filter_map(|a| Some((a, a.ability_definition.as_ref()?))) {
        let attack = d
            .ability_type
            .as_deref()
            .is_some_and(|t| t.eq_ignore_ascii_case("attack"));
        let damage: Vec<Damage> = d
            .effect
            .iter()
            .filter(|e| {
                e.kind
                    .as_deref()
                    .is_some_and(|k| k.eq_ignore_ascii_case("MAIN_DAMAGE"))
            })
            .filter_map(|e| {
                Some(Damage {
                    kind: e.damage_type_ids.clone()?,
                    amount: e.amount?,
                })
            })
            .collect();
        let Some(range) = d.range.filter(|_| attack && !damage.is_empty()) else {
            continue;
        };
        fighter.weapons.push(Weapon {
            range,
            min_range: d.min_range,
            recharge: d.recharge_time,
            area: d.area,
            max_targets: d.max_targets.unwrap_or(0),
            full_within: d.attenuation_range_min.unwrap_or(1.0),
            edge_damage: d.attenuation_factor_max.unwrap_or(1.0),
            targets: Targets::parse(d.targets.as_deref().unwrap_or("")),
            damage,
            animation: a
                .animation1
                .clone()
                .filter(|ids| !ids.eq_ignore_ascii_case("null")),
            turret: a
                .turret_ids
                .as_deref()
                .is_some_and(|ids| !ids.eq_ignore_ascii_case("null")),
            turret_point: not_null(a.turret_ids.as_deref().map(Id::from)),
            aimed_by: not_null(a.turret_ids.as_deref().map(Id::from)).and_then(|ids| {
                let found = turrets.iter().position(|(t, _)| *t == Some(&ids));
                if found.is_none() {
                    log::warn!("{name}: no turret {ids} to aim its ability");
                }
                found
            }),
            spawn_position: a.spawn_position.unwrap_or_default(),
            sounds: cues(&d.when_do),
            effects: WeaponEffects {
                start: vfx_cues(&d.when_start),
                shot: vfx_cues(&d.when_do),
                per_target: vfx_cues(&d.when_do_per_target),
                end: vfx_cues(&d.when_end),
            },
            projectile: not_null(d.projectile_ids.clone()),
            flight: None,
            bombard: d.flags.iter().any(|f| f.bombard),
        });
    }
    Some(fighter)
}

/// A turret's yaw fields must each be 0 to 360 degrees; those outside are
/// taken to the nearer end.
fn turret(thing: &str, t: &defs::Turret) -> Turret {
    let degrees = |d: f32| {
        if !(0.0..=360.0).contains(&d) {
            log::warn!("{thing}: turret yaw field {d} isn't within 0 to 360 degrees");
        }
        d.clamp(0.0, 360.0).to_radians()
    };
    Turret {
        turn_rate: degrees(t.yaw_rotation_per_sec_degrees),
        sweep: degrees(t.yaw_rotation_range_degrees),
        start: degrees(t.yaw_start_degrees),
        reset: t.can_reset.then_some(t.reset_time),
        offset: t.offset.unwrap_or_default(),
    }
}

fn flight(p: &defs::ProjectileComponent) -> Option<Flight> {
    let kind = match p.projectile_type.as_deref().map(str::to_ascii_uppercase) {
        Some(k) if k == "LOB" => FlightKind::Lob,
        Some(k) if k == "BOMB" => FlightKind::Bomb,
        _ => FlightKind::Straight,
    };
    Some(Flight {
        kind,
        speed: p.projectile_speed.unwrap_or(0.0),
        life: p.projectile_life.unwrap_or(0.0),
    })
}

/// The depth colours of each water terrain, by IDS. Biomes name a
/// `WaterInfo` template for each of their water terrains.
fn water_colors(files: &[Scoped]) -> HashMap<Id, Vec<DepthColor>> {
    files
        .iter()
        .flat_map(|f| f.read::<defs::Biome>("Biome"))
        .flat_map(|b| b.water_info)
        .filter_map(|w| {
            let mut stops: Vec<DepthColor> = w
                .depth_info
                .into_iter()
                .map(|d| DepthColor {
                    depth: d.depth,
                    color: d.color,
                    alpha: d.alpha,
                })
                .collect();
            stops.sort_by(|a, b| a.depth.total_cmp(&b.depth));
            Some((w.terrain_ids?, stops))
        })
        .collect()
}

/// Gives the old ids that maps may still use the entries of their
/// replacements, as `translate.tgi` lists them for `node`, e.g. `Thing`:
/// one step on, so an id renamed to one that's itself only a rename has
/// none, and in place of any entry still made under the old id.
fn add_renames<T: Clone>(entries: &mut HashMap<Id, T>, translate: &str, node: &str) {
    let renamed: Vec<(Id, Option<T>)> = renames(translate, node)
        .into_iter()
        .map(|(old, new)| {
            let entry = entries.get(&new).cloned();
            (old, entry)
        })
        .collect();
    for (old, entry) in renamed {
        match entry {
            Some(e) => entries.insert(old, e),
            None => entries.remove(&old),
        };
    }
}

/// Old ids of `node`s that maps may still use, to their replacements, from
/// `translate.tgi`. Where an id is renamed more than once the first counts.
fn renames(translate: &str, node: &str) -> HashMap<Id, Id> {
    let mut out = HashMap::new();
    for t in tgi::parse(translate).root.blocks_of("Translate") {
        let Ok(t) = tgi::from_block::<defs::Translate>(t) else {
            continue;
        };
        if !t.node.is_some_and(|n| n.trim().eq_ignore_ascii_case(node)) {
            continue;
        }
        if let (Some(old), Some(new)) = (t.ids_old, t.ids_new) {
            out.entry(old).or_insert(new);
        }
    }
    out
}

/// The id the game looks `id` up by.
fn renamed(renames: &HashMap<Id, Id>, id: &Id) -> Id {
    renames.get(id).unwrap_or(id).clone()
}

fn object_audio(thing: &defs::Thing) -> ObjectAudio {
    let view = thing.view.as_ref();
    let mover = thing.mover_component.as_ref();
    let of_view =
        |f: fn(&defs::View) -> &Vec<defs::Fxc>| view.map(|v| cues(f(v))).unwrap_or_default();
    let of_mover = |f: fn(&defs::MoverComponent) -> &Vec<defs::Fxc>| {
        mover.map(|m| cues(f(m))).unwrap_or_default()
    };
    let by_state = |fxcs: &[defs::Fxc]| -> Vec<(Id, Cue)> {
        fxcs.iter()
            .filter_map(|f| Some((Id::from(f.actor_state.as_deref()?), cue(f)?)))
            .filter(|(state, _)| *state != Id::from("null"))
            .collect()
    };
    let aspects = view
        .into_iter()
        .flat_map(|v| v.aspect.iter().chain(&v.aspect_normal));
    let animations = aspects
        .flat_map(|a| &a.animation)
        .flat_map(|anim| {
            let kind = anim.kind.clone().unwrap_or_default().to_ascii_uppercase();
            let interval = match (kind.as_str(), anim.start_min, anim.start_max) {
                ("RANDOM", Some(min), Some(max)) => Some([min, max]),
                _ => None,
            };
            anim.play_sfx.iter().filter_map(move |p| {
                Some(AnimationAudio {
                    kind: kind.clone(),
                    ids: anim.ids.clone(),
                    interval,
                    audio: p.sfx_ids.clone()?,
                    looping: p.looping,
                })
            })
        })
        .collect();
    ObjectAudio {
        selected: of_view(|v| &v.when_selected),
        commanded: of_view(|v| &v.when_commanded),
        commanded_aggressive: of_view(|v| &v.when_commanded_aggressive),
        created: of_view(|v| &v.when_created),
        destroyed: of_view(|v| &v.when_destroyed),
        start_moving: of_mover(|m| &m.when_start_moving),
        stop_moving: of_mover(|m| &m.when_stop_moving),
        entered: by_state(&thing.when_enter),
        left: by_state(&thing.when_leave),
        disturbed: thing
            .component
            .iter()
            .flat_map(|c| cues(&c.action))
            .collect(),
        animations,
    }
}

fn vfx(v: defs::Vfx, model: Option<String>) -> Vfx {
    Vfx {
        category: v.category.unwrap_or_default().to_ascii_uppercase(),
        model,
        texture_dirs: texture_dirs(&v.texture_search_path),
        permanent: v.permanent.unwrap_or(false),
        critical: v.critical.unwrap_or(true),
        duration: v.duration.filter(|d| *d > 0.0),
        attached_scale: v.attached_scale.unwrap_or(0.0),
        never_discard: v.never_discard_temporary.unwrap_or(false),
        night_only: v.nightonly.unwrap_or(false),
        constant_screen_size: v.constant_screen_size.unwrap_or(false),
        real_time: v.force_real_time.unwrap_or(false),
        random_orientation: v.flags.iter().rev().find_map(|f| f.random_orientation) == Some(true),
        gyroscope: v.gyroscope.unwrap_or(true),
        tint: v.color.map(|colour| VfxTint {
            colour,
            max: v.tint_max.unwrap_or(1.0),
            rise: v.enhance_duration.unwrap_or(0.0),
            fall: v.fade_duration.unwrap_or(0.0),
            repeating: v.repeating.unwrap_or(false),
        }),
    }
}

fn object_effects(thing: &defs::Thing) -> ObjectEffects {
    let view = thing.view.as_ref();
    let mover = thing.mover_component.as_ref();
    let of_view =
        |f: fn(&defs::View) -> &Vec<defs::Fxc>| view.map(|v| vfx_cues(f(v))).unwrap_or_default();
    let of_mover = |f: fn(&defs::MoverComponent) -> &Vec<defs::Fxc>| {
        mover.map(|m| vfx_cues(f(m))).unwrap_or_default()
    };
    let by_state = |fxcs: &[defs::Fxc]| -> Vec<(Id, VfxCue)> {
        fxcs.iter()
            .filter_map(|f| {
                Some((
                    not_null(f.actor_state.as_deref().map(Id::from))?,
                    vfx_cue(f)?,
                ))
            })
            .collect()
    };
    let health = thing
        .body_component
        .iter()
        .flat_map(|b| &b.when_health)
        .filter_map(|f| {
            let trigger = match f
                .health_trigger_type
                .as_deref()?
                .to_ascii_uppercase()
                .as_str()
            {
                "ABSOLUTE_BELOW" => HealthTrigger::Below,
                "ABSOLUTE_ABOVE" => HealthTrigger::Above,
                "RELATIVE_DECREASE" => HealthTrigger::Decrease,
                "RELATIVE_INCREASE" => HealthTrigger::Increase,
                _ => return None,
            };
            Some(HealthCue {
                health: f.health.filter(|h| *h >= 0.0)?,
                trigger,
                cue: vfx_cue(f)?,
            })
        })
        .collect();
    ObjectEffects {
        created: of_view(|v| &v.when_created),
        destroyed: of_view(|v| &v.when_destroyed),
        constructed: of_view(|v| &v.when_constructed),
        start_moving: of_mover(|m| &m.when_start_moving),
        stop_moving: of_mover(|m| &m.when_stop_moving),
        entered: by_state(&thing.when_enter),
        left: by_state(&thing.when_leave),
        health,
        disturbed: thing
            .component
            .iter()
            .flat_map(|c| vfx_cues(&c.action))
            .collect(),
    }
}

/// The effects the commands create or stop.
fn vfx_cues(fxcs: &[defs::Fxc]) -> Vec<VfxCue> {
    fxcs.iter().filter_map(vfx_cue).collect()
}

/// The effect a command creates or stops, if it's one that does.
fn vfx_cue(f: &defs::Fxc) -> Option<VfxCue> {
    let stop = match f.kind.as_deref()?.to_ascii_uppercase().as_str() {
        "CREATE_VFX" => false,
        "STOP_VFX" => true,
        _ => return None,
    };
    let every = f.frequency.unwrap_or(1);
    if every == 0 {
        return None;
    }
    Some(VfxCue {
        vfx: not_null(f.vfx_ids.clone())?,
        stop,
        at: f.at.as_deref().unwrap_or("SELF").to_ascii_uppercase(),
        location: f
            .location
            .clone()
            .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("null")),
        attached: f.attached.unwrap_or(true),
        translation: f.translation.unwrap_or_default(),
        orientation: f.orientation.unwrap_or([0.0, -1.0, 0.0]),
        duration: f.duration.filter(|d| *d > 0.0),
        delay: f.delay.unwrap_or(0.0),
        every,
        terrain: not_null(f.terrain_group_filter.as_deref().map(Id::from)),
        targets: f
            .target_flags
            .as_deref()
            .filter(|t| !t.eq_ignore_ascii_case("null"))
            .map(Targets::parse)
            .unwrap_or_default(),
        stop_on_destructed: f.stop_on_destructed.unwrap_or(true),
        chain: vfx_cues(&f.chain),
        chain_immediate: vfx_cues(&f.chain_immediate),
    })
}

/// The audio the commands play.
fn cues(fxcs: &[defs::Fxc]) -> Vec<Cue> {
    fxcs.iter().filter_map(cue).collect()
}

/// The audio a command plays, if it's one that plays audio.
fn cue(f: &defs::Fxc) -> Option<Cue> {
    let plays = f
        .kind
        .as_deref()
        .is_some_and(|k| k.eq_ignore_ascii_case("PLAY_SFX"));
    let every = f.frequency.unwrap_or(1);
    if !plays || every == 0 {
        return None;
    }
    Some(Cue {
        audio: f.audio_ids.clone()?,
        at: f.at.as_deref().unwrap_or("SELF").to_ascii_uppercase(),
        delay: f.delay.unwrap_or(0.0),
        looping: f.looping.unwrap_or(false),
        every,
    })
}

/// An audio definition with its files found by `find`, which gives an
/// archive path as the archives spell it.
fn audio(a: &defs::Audio, find: impl Fn(&str) -> Option<String>) -> Option<Audio> {
    let category = match a.category.as_deref()?.to_ascii_uppercase().as_str() {
        "SOUND" => AudioCategory::Sound,
        "SPEECH" => AudioCategory::Speech,
        "MUSIC" => AudioCategory::Music,
        _ => return None,
    };
    let mut missing = Vec::new();
    let mut paths = |list: &Option<String>| -> Vec<String> {
        let mut found = Vec::new();
        for file in audio_files(list.as_deref()) {
            match find(&audio_path(file)) {
                Some(p) => found.push(p),
                None => missing.push(file.to_string()),
            }
        }
        found
    };
    let (leadin, files, fallout) = (paths(&a.leadin), paths(&a.files), paths(&a.fallout));
    let priority = match a
        .priority
        .as_deref()
        .map(str::to_ascii_uppercase)
        .as_deref()
    {
        Some("LOWEST") => AudioPriority::Lowest,
        Some("LOW") => AudioPriority::Low,
        Some("HIGH") => AudioPriority::High,
        Some("CRITICAL") => AudioPriority::Critical,
        _ => AudioPriority::Medium,
    };
    let mut control = AudioControl::default();
    for flag in flags(&a.control_flags) {
        match flag.as_str() {
            "INTERRUPT" => control.interrupt = true,
            "SEQUENTIAL" => control.sequential = true,
            "ALL" => control.all = true,
            "VOICE" => control.voice = true,
            "WORLD" => control.world = true,
            "CAUSEFADE" => control.cause_fade = true,
            "TESTSOUND" => control.test = true,
            _ => {}
        }
    }
    let mut world = AudioWorld::default();
    for flag in flags(&a.world_flags) {
        match flag.as_str() {
            "GLOBAL" => world.global = true,
            "FOGGED" => world.fogged = true,
            _ => {}
        }
    }
    let mut who = AudioWho::default();
    for flag in flags(&a.who_flags) {
        match flag.as_str() {
            "LOCAL" => who.local = true,
            "PLAYER" => who.player = true,
            "ALLIES" => who.allies = true,
            "ENEMIES" => who.enemies = true,
            "EVERYONE" => (who.player, who.allies, who.enemies) = (true, true, true),
            _ => {}
        }
    }
    let [falloff, range] = a.range.unwrap_or([4.0, 64.0]);
    Some(Audio {
        category,
        leadin,
        files,
        fallout,
        missing,
        priority,
        control,
        world,
        who,
        falloff,
        range,
        limit: a.simultaneous_limit.unwrap_or(2),
        volume: a.volume.unwrap_or(0.8),
        volume_adjust: a.volume_adjust.unwrap_or(0.0),
        pitch_adjust: a.pitch_adjust.unwrap_or([0.0; 2]),
        delay: a.delay_ms.unwrap_or_default().map(|ms| ms / 1000.0),
        fade: a.fade_time_s.unwrap_or_default(),
        subtitle: a.subtitle.as_ref().and_then(|s| s.text.clone()),
    })
}

/// The archive path of a file an [`Audio`] names: from the root where it
/// starts with a `/`, else from `Audio/`.
fn audio_path(file: &str) -> String {
    let file = file.replace('\\', "/");
    match file.strip_prefix('/') {
        Some(rooted) => rooted.to_string(),
        None => format!("Audio/{file}"),
    }
}

/// The files of an audio file list, which `NULL` leaves empty.
fn audio_files(list: Option<&str>) -> impl Iterator<Item = &str> {
    list.into_iter()
        .flat_map(|l| l.split(','))
        .map(str::trim)
        .filter(|f| !f.is_empty() && !f.eq_ignore_ascii_case("null"))
}

/// The upper-case flags of a comma separated list.
fn flags(list: &Option<String>) -> Vec<String> {
    audio_files(list.as_deref())
        .map(str::to_ascii_uppercase)
        .collect()
}

/// The texture renames of each of a view's models, by archive path, which
/// `path` makes of the paths the aspects give; `normal` is the normal
/// model's. Each model takes only its own aspect's renames, the first
/// aspect to name a model keeping it.
fn texture_renames(
    view: &defs::View,
    normal: Option<&str>,
    path: impl Fn(&str) -> String,
) -> HashMap<String, Vec<(String, String)>> {
    let renames = |a: &defs::Aspect| -> Vec<(String, String)> {
        (a.texture_rename.iter())
            .filter_map(|r| Some((r.substring_from.clone()?, r.substring_to.clone()?)))
            .collect()
    };
    let mut out = HashMap::new();
    let named = view.aspect_normal.iter().find(|a| a.model.is_some());
    if let (Some(n), Some(a)) = (normal, named.or(view.aspect_normal.first())) {
        out.insert(n.to_string(), renames(a));
    }
    for a in &view.aspect {
        if let Some(m) = a.model.as_deref() {
            out.entry(path(m)).or_insert_with(|| renames(a));
        }
    }
    out.retain(|_, r| !r.is_empty());
    out
}

/// Where a texture that `model`, one of `view`'s, names as `file` may be,
/// in the order it's looked for.
fn texture_paths(view: &View, model: &str, file: &str) -> Vec<String> {
    let dir = model.rsplit_once('/').map_or("", |(d, _)| d);
    let mut path = join(dir, &file.replace('\\', "/"));
    let renames = view
        .texture_renames
        .get(model)
        .map_or(&[][..], Vec::as_slice);
    for (from, to) in renames.iter().rev() {
        path = path.replace(from.as_str(), to);
    }
    let (folder, name) = path.rsplit_once('/').unwrap_or(("", &path));
    let searched = view.texture_dirs.iter().map(|d| match d.starts_with('.') {
        true => join(folder, d),
        false => d.trim_matches('/').to_string(),
    });
    let searched: Vec<String> = searched.map(|d| join(&d, name)).collect();
    std::iter::once(path).chain(searched).collect()
}

/// `path` taken from the folder `dir`, with its `.` and `..` folders
/// resolved.
fn join(dir: &str, path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in dir.split('/').chain(path.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// The `.tgi` files the game loads, by archive path, in its order: those
/// the `index*.lst` lists at the archive's root name, in turn, where a line
/// naming another list takes in its files there and then. `entries` are the
/// archive paths, `read` gives a list's text.
fn listed_tgis(entries: &[String], mut read: impl FnMut(&str) -> Option<String>) -> Vec<String> {
    let mut roots: Vec<&str> = entries
        .iter()
        .map(String::as_str)
        .filter(|n| {
            let lower = n.to_ascii_lowercase();
            !lower.contains('/') && lower.starts_with("index") && lower.ends_with(".lst")
        })
        .collect();
    roots.sort_unstable();
    roots.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        take_list(
            root,
            entries,
            &mut read,
            &mut Vec::new(),
            &mut seen,
            &mut out,
        );
    }
    out
}

/// Adds the `.tgi` files a list names to `out`, `open` holding the lists
/// being read so that one naming itself, directly or not, stops there.
fn take_list(
    list: &str,
    entries: &[String],
    read: &mut impl FnMut(&str) -> Option<String>,
    open: &mut Vec<String>,
    seen: &mut HashSet<String>,
    out: &mut Vec<String>,
) {
    let lower = list.to_ascii_lowercase();
    if open.contains(&lower) {
        return;
    }
    let Some(text) = read(list) else {
        return;
    };
    open.push(lower);
    for line in text.lines() {
        let spec = line
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .replace('\\', "/");
        if spec.is_empty() {
            continue;
        }
        for path in matching(&spec, entries) {
            if path.to_ascii_lowercase().ends_with(".lst") {
                take_list(path, entries, read, open, seen, out);
            } else if path.to_ascii_lowercase().ends_with(".tgi")
                && seen.insert(path.to_ascii_lowercase())
            {
                out.push(path.to_string());
            }
        }
    }
    open.pop();
}

/// The archive paths a list's line names: the one path, or where its file
/// name has a `*`, the files in that one folder it matches, in plain
/// character order.
fn matching<'a>(spec: &str, entries: &'a [String]) -> Vec<&'a str> {
    let (dir, file) = spec.rsplit_once('/').unwrap_or(("", spec));
    let pattern = file.to_ascii_lowercase();
    let mut out: Vec<&str> = entries
        .iter()
        .map(String::as_str)
        .filter(|n| {
            let (d, f) = n.rsplit_once('/').unwrap_or(("", n));
            d.eq_ignore_ascii_case(dir) && wildcard(&pattern, &f.to_ascii_lowercase())
        })
        .collect();
    out.sort_unstable_by_key(|n| n.rsplit('/').next().unwrap_or(n));
    out
}

/// Whether `name` matches `pattern`, where each `*` stands for any run of
/// characters.
fn wildcard(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or("");
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// A map's file name without its `.rmp` extension.
pub fn map_title(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    let ext = file
        .len()
        .checked_sub(4)
        .filter(|&i| file.is_char_boundary(i));
    match ext {
        Some(i) if file[i..].eq_ignore_ascii_case(".rmp") => &file[..i],
        _ => file,
    }
}

fn find_case_insensitive(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

/// A `Text` block's strings by lower-case key, which keeps any `|` and
/// format after it. The block's `ids` and `name` describe it and aren't
/// strings.
fn text_strings(block: &tgi::Block) -> impl Iterator<Item = (String, String)> + '_ {
    block.fields().filter(|f| f.quoted).filter_map(|f| {
        let key = f.key.to_ascii_lowercase();
        (key != "ids" && key != "name").then_some((key, f.value.clone()?))
    })
}

/// Quoted text with its escapes read: `\c` and `&name.` are the glyphs
/// called `c` and `name`, and `\\`, `\&` and `&&` a plain `\` and `&`.
/// What names no glyph is left as written.
fn unescape(text: &str, glyphs: &HashMap<Id, char>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(['\\', '&']) {
        out.push_str(&rest[..i]);
        let escape = rest[i..].chars().next().unwrap_or('&');
        let after = &rest[i + 1..];
        let next = after.chars().next();
        let (read, used) = match (escape, next) {
            ('\\', Some(c @ ('\\' | '&'))) | ('&', Some(c @ '&')) => (Some(c), 1),
            ('\\', Some(c)) => {
                let name = &after[..c.len_utf8()];
                (glyphs.get(&Id::from(name)).copied(), name.len())
            }
            ('&', Some(_)) => match after.split_once('.') {
                Some((name, _)) => (glyphs.get(&Id::from(name)).copied(), name.len() + 1),
                None => (None, 0),
            },
            _ => (None, 0),
        };
        match read {
            Some(c) => {
                out.push(c);
                rest = &after[used..];
            }
            None => {
                out.push(escape);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text a `#` key names, or a name written out as is.
fn localized(strings: &HashMap<String, String>, name: Option<&str>) -> Option<String> {
    let name = name?;
    match name.strip_prefix('#') {
        Some(key) => strings.get(&key.to_ascii_lowercase()).cloned(),
        None => Some(name.to_string()),
    }
}

/// The resources given, totalled, with their localized names; those the
/// strings don't name keep their field name.
fn amounts<'a>(
    strings: &HashMap<String, String>,
    of: impl Iterator<Item = &'a defs::Resources>,
) -> Vec<Amount> {
    let mut totals: Vec<(&str, f32)> = Vec::new();
    for (field, amount) in of.flat_map(defs::Resources::amounts) {
        match totals.iter_mut().find(|(f, _)| *f == field) {
            Some((_, total)) => *total += amount,
            None => totals.push((field, amount)),
        }
    }
    totals
        .into_iter()
        .filter(|&(_, amount)| amount != 0.0)
        .map(|(field, amount)| Amount {
            kind: field,
            resource: strings
                .get(&format!("resources_{field}_name"))
                .cloned()
                .unwrap_or_else(|| field.to_string()),
            amount,
        })
        .collect()
}

/// An organization template's members when whole: each slot's, once for
/// every place its layout gives the slot in its starting formation, or
/// once where the layout isn't known.
fn members<'a>(
    t: &defs::Thing,
    things: &HashMap<&Id, &'a defs::Thing>,
    layouts: &HashMap<Id, Layout>,
) -> Vec<&'a defs::Thing> {
    let Some(o) = &t.organization_component else {
        return Vec::new();
    };
    let layout = o.layout_ids.as_ref().and_then(|l| layouts.get(l));
    let places = |slot: usize| {
        let start = layout.and_then(|l| l.formations.get(l.default));
        start.and_then(|f| f.slots.get(slot)).map_or(1, Vec::len)
    };
    (o.slot.iter().enumerate())
        .filter_map(|(i, s)| Some((i, *things.get(s.element_ids.as_ref()?)?)))
        .flat_map(|(i, member)| std::iter::repeat_n(member, places(i)))
        .collect()
}

/// What a template earns as it's shown: its own `[Production]`, or for a
/// regiment the sum of its `members`'.
fn production<'a>(
    t: &'a defs::Thing,
    members: &[&'a defs::Thing],
) -> impl Iterator<Item = &'a defs::Resources> {
    let earners = if members.is_empty() {
        vec![t]
    } else {
        members.to_vec()
    };
    earners
        .into_iter()
        .filter_map(|t| t.economy_component.as_ref()?.production.as_ref())
}

/// What a template takes to keep as it's shown: its own `[Upkeep]` and,
/// for a regiment, each of its `members`'.
fn upkeep<'a>(
    t: &'a defs::Thing,
    members: &[&'a defs::Thing],
) -> impl Iterator<Item = &'a defs::Resources> {
    std::iter::once(t)
        .chain(members.iter().copied())
        .filter_map(|t| t.economy_component.as_ref()?.upkeep.as_ref())
}

fn terrain_kind(t: &defs::Terrain) -> TerrainKind {
    match t.kind.as_deref().map(str::to_ascii_uppercase).as_deref() {
        Some("MOUNTAIN") => TerrainKind::Mountain {
            noise: [t.noise_min.unwrap_or(0.0), t.noise_max.unwrap_or(0.0)],
        },
        Some("WATER") => TerrainKind::Water,
        _ => TerrainKind::Land,
    }
}

/// The id, unless it's the data's word for none.
/// The archive folders a model's textures are searched for in.
fn texture_dirs(paths: &[defs::TextureSearchPath]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|b| b.path.as_deref())
        .map(|p| p.trim_matches('/').to_string())
        .collect()
}

fn not_null(id: Option<Id>) -> Option<Id> {
    id.filter(|id| !id.as_str().eq_ignore_ascii_case("null"))
}

/// A path a file gives relative to its own folder, unless it starts with `/`.
fn beside(file: &str, path: &str) -> String {
    match path.strip_prefix('/') {
        Some(abs) => abs.to_string(),
        None => match file.rsplit_once('/') {
            Some((dir, _)) => format!("{dir}/{path}"),
            None => path.to_string(),
        },
    }
}

/// The numbered files a `%d` pattern names, counting up from 0 or 1 for as
/// long as they're there; a pattern without one names a single file.
fn frames(pattern: &str, exists: impl Fn(&str) -> bool) -> Vec<String> {
    if !pattern.contains("%d") {
        return exists(pattern)
            .then(|| pattern.to_string())
            .into_iter()
            .collect();
    }
    let frame = |n: usize| pattern.replace("%d", &n.to_string());
    let first = usize::from(!exists(&frame(0)));
    (first..).map(frame).take_while(|f| exists(f)).collect()
}

/// The archive path of an interface image, whose `%s` folder, for a
/// screen size, is empty for the default images.
fn ui_path(path: &str) -> Option<String> {
    let path = path.split_whitespace().next()?;
    (!path.eq_ignore_ascii_case("null"))
        .then(|| path.replace("%s/", "").trim_start_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_take_names_and_icons_from_the_interface() {
        let parsed = tgi::parse(
            "[Text language = default]\n{\n\tname = \"@strings.tgi\"\n\tmove_name = \"Move\"\n\
             \tcount|d = \"%d left\"\n}\n",
        );
        let block = parsed.root.blocks_of("Text").next().unwrap();
        let strings: HashMap<String, String> = text_strings(block).collect();
        assert_eq!(
            localized(&strings, Some("#Move_name")).as_deref(),
            Some("Move")
        );
        assert_eq!(localized(&strings, Some("Plain")).as_deref(), Some("Plain"));
        // A key's format is part of it, and the block's name isn't a string.
        assert_eq!(strings.get("count|d").map(String::as_str), Some("%d left"));
        assert_eq!(strings.get("count"), None);
        assert_eq!(strings.get("name"), None);
        assert_eq!(
            ui_path("/UI/%s/Game/ActionButtons/IconMove.png").as_deref(),
            Some("UI/Game/ActionButtons/IconMove.png")
        );
        assert_eq!(ui_path("NULL"), None);
    }

    #[test]
    fn target_filters_require_and_exclude_flags() {
        let ids = |flags: &[&str]| flags.iter().copied().map(Id::from).collect::<Vec<_>>();
        let t = Targets::parse("+enemy, vehicle -air\t!mine,+ - é");
        assert_eq!(t.required, ids(&["enemy", "vehicle", "é"]));
        assert_eq!(t.excluded, ids(&["air", "mine"]));
        assert_eq!(Targets::parse(""), Targets::default());
    }

    #[test]
    fn quoted_text_reads_its_escapes() {
        let glyphs = HashMap::from([(Id::from("n"), '\n'), (Id::from("ch_gem"), '\u{84}')]);
        let read = |text: &str| unescape(text, &glyphs);
        assert_eq!(read(r"a\nb &CH_Gem. 5"), "a\nb \u{84} 5");
        assert_eq!(read(r"50%% \\ \& && R&D"), r"50%% \ & & R&D");
        // What names no glyph stays as written, a trailing escape too.
        assert_eq!(read(r"\q &nope. é\"), r"\q &nope. é\");
    }

    #[test]
    fn production_is_the_templates_own_or_its_members() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\tIDS = truck\n\t[EconomyComponent]\n\t{\n\
             \t\t[Production]\n\t\t{\n\t\t\tmoney = 12\n\t\t\tunit_limit_provided = 5\n\t\t}\n\
             \t}\n}\n\
             [Thing]\n{\n\tIDS = regiment\n\t[EconomyComponent]\n\t{\n\
             \t\t[Production]\n\t\t{\n\t\t\tmoney = 99\n\t\t}\n\t}\n\
             \t[OrganizationComponent]\n\t{\n\
             \t\t[Slot]\n\t\t{\n\t\t\telement_ids = truck\n\t\t}\n\
             \t\t[Slot]\n\t\t{\n\t\t\telement_ids = truck\n\t\t}\n\t}\n}\n",
        );
        let things: Vec<(Id, defs::Thing)> = parsed
            .root
            .blocks_of("Thing")
            .zip(["truck", "regiment"])
            .map(|(b, id)| (Id::from(id), tgi::from_block(b).unwrap()))
            .collect();
        let by_id: HashMap<&Id, &defs::Thing> = things.iter().map(|(i, t)| (i, t)).collect();
        let earned = |id: &str| -> Vec<(&str, f32)> {
            let t = by_id[&Id::from(id)];
            let members = members(t, &by_id, &HashMap::new());
            amounts(&HashMap::new(), production(t, &members))
                .into_iter()
                .map(|a| (a.kind, a.amount))
                .collect()
        };
        assert_eq!(
            earned("truck"),
            [("money", 12.0), ("unit_limit_provided", 5.0)]
        );
        assert_eq!(
            earned("regiment"),
            [("money", 24.0), ("unit_limit_provided", 10.0)]
        );
    }

    #[test]
    fn upkeep_is_the_templates_own_and_each_members() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\tIDS = man\n\t[EconomyComponent]\n\t{\n\
             \t\t[Upkeep]\n\t\t{\n\t\t\tammo = 1\n\t\t}\n\t}\n}\n\
             [Thing]\n{\n\tIDS = regiment\n\t[EconomyComponent]\n\t{\n\
             \t\t[Upkeep]\n\t\t{\n\t\t\tunit_limit_consumed = 1\n\t\t}\n\t}\n\
             \t[OrganizationComponent]\n\t{\n\t\tlayout_ids = line\n\
             \t\t[Slot]\n\t\t{\n\t\t\telement_ids = man\n\t\t}\n\
             \t\t[Slot]\n\t\t{\n\t\t\telement_ids = man\n\t\t}\n\
             \t\t[Slot]\n\t\t{\n\t\t\telement_ids = man\n\t\t}\n\t}\n}\n",
        );
        let things: Vec<(Id, defs::Thing)> = parsed
            .root
            .blocks_of("Thing")
            .zip(["man", "regiment"])
            .map(|(b, id)| (Id::from(id), tgi::from_block(b).unwrap()))
            .collect();
        let by_id: HashMap<&Id, &defs::Thing> = things.iter().map(|(i, t)| (i, t)).collect();
        let regiment = by_id[&Id::from("regiment")];
        let kept = |layouts: &HashMap<Id, Layout>| -> Vec<(&str, f32)> {
            let members = members(regiment, &by_id, layouts);
            amounts(&HashMap::new(), upkeep(regiment, &members))
                .into_iter()
                .map(|a| (a.kind, a.amount))
                .collect()
        };
        let whole = [("unit_limit_consumed", 1.0), ("ammo", 3.0)];
        assert_eq!(kept(&HashMap::new()), whole);
        // A slot's member counts for each place its layout gives the slot.
        let line = Layout {
            formations: vec![Formation {
                id: Id::from("column"),
                slots: vec![vec![[0.0; 2]; 3], vec![[0.0; 2]], vec![[0.0; 2]]],
                aggressive: false,
            }],
            default: 0,
            combat: None,
            primary: None,
        };
        assert_eq!(
            kept(&HashMap::from([(Id::from("line"), line)])),
            [("unit_limit_consumed", 1.0), ("ammo", 5.0)]
        );
    }

    #[test]
    fn cursor_frames_count_up_beside_their_file() {
        let pattern = beside("cursors/cursors_aa.tgi", "Cursors/movement_%d.tga");
        assert_eq!(pattern, "cursors/Cursors/movement_%d.tga");
        assert_eq!(beside("effects/w.tgi", "/UI/x.png"), "UI/x.png");
        let there = |n: &str| ["a_1.tga", "a_2.tga", "a_4.tga"].contains(&n);
        assert_eq!(frames("a_%d.tga", there), ["a_1.tga", "a_2.tga"]);
        let from_zero = |n: &str| ["b_0.tga", "b_1.tga"].contains(&n);
        assert_eq!(frames("b_%d.tga", from_zero), ["b_0.tga", "b_1.tga"]);
        assert!(frames("c_%d.tga", |_| false).is_empty());
        // A cursor of one frame names its file whole.
        assert_eq!(frames("d.tga", |n| n == "d.tga"), ["d.tga"]);
        assert!(frames("e.tga", |_| false).is_empty());
    }

    #[test]
    fn terrains_take_their_kind_and_noise_from_their_templates() {
        let parsed = tgi::parse(
            "[template Terrain]\n{\n\tstring IDS\n\tstring type = LAND\n}\n\
             [template MountainTerrain inherit=Terrain]\n{\n\ttype = MOUNTAIN\n\
             \tfixed noise_min = 1.0\n\tfixed noise_max = 2.0\n}\n\
             [Terrain template=MountainTerrain]\n{\n\tIDS = rock\n\tfixed noise_min = 0.25\n}\n\
             [Terrain template=Terrain]\n{\n\tIDS = grass\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let kinds: Vec<TerrainKind> = parsed
            .root
            .blocks_of("Terrain")
            .map(|b| terrain_kind(&templates.deserialize::<defs::Terrain>(b).unwrap()))
            .collect();
        assert_eq!(
            kinds,
            [
                TerrainKind::Mountain { noise: [0.25, 2.0] },
                TerrainKind::Land
            ]
        );
    }

    #[test]
    fn map_titles_drop_only_the_extension() {
        assert_eq!(map_title("Maps/Campaign/axis01_crete.RMP"), "axis01_crete");
        assert_eq!(map_title("(2v2)Berlin.rmp"), "(2v2)Berlin");
        assert_eq!(map_title("a"), "a");
        assert_eq!(map_title("notes.txt"), "notes.txt");
        assert_eq!(map_title("écu"), "écu");
    }

    #[test]
    fn null_ids_are_none() {
        assert_eq!(not_null(Some(Id::from("NULL"))), None);
        assert_eq!(not_null(Some(Id::from("move"))), Some(Id::from("move")));
    }

    #[test]
    fn settings_read_the_building_padding() {
        let parsed = tgi::parse("[SVars]\n{\n\tBuildingPaddingMeters = 3\n\tOther = 1\n}\n");
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("SVars").next().unwrap();
        let svars: defs::SVars = templates.deserialize(block).unwrap();
        assert_eq!(svars.building_padding_meters, Some(3.0));
    }

    #[test]
    fn renames_go_one_step_and_win_over_the_old_ids() {
        let mut entries: HashMap<Id, &str> = ["b", "c", "old", "gone", "kept"]
            .into_iter()
            .map(|id| (Id::from(id), id))
            .collect();
        let rename = |node, old, new| {
            format!("[Translate]\nnode = {node}\nids_old = {old}\nids_new = {new}\n")
        };
        let translate = [
            rename("Thing", "A", "b"),
            rename("Thing", "b", "C"),
            // The first rename of an id counts.
            rename("Thing", "a", "c"),
            // Even where the old id is still defined.
            rename("Thing", "old", "b"),
            // To an id that's only a rename, or nothing, finds nothing.
            rename("Thing", "gone", "a"),
            rename("Thing", "lost", "nowhere"),
            rename("Variation", "kept", "c"),
            format!("/*\n{}*/\n", rename("Thing", "hidden", "c")),
        ]
        .concat();
        add_renames(&mut entries, &translate, "Thing");
        let get = |id: &str| entries.get(&Id::from(id)).copied();
        assert_eq!(get("a"), Some("b"));
        assert_eq!(get("b"), Some("c"));
        assert_eq!(get("old"), Some("b"));
        assert_eq!(get("gone"), None);
        assert_eq!(get("lost"), None);
        assert_eq!(get("kept"), Some("kept"));
        assert_eq!(get("hidden"), None);
    }

    #[test]
    fn only_listed_tgis_load_in_list_order() {
        let entries: Vec<String> = [
            "index_b.lst",
            "Index_A.lst",
            "Units/b.tgi",
            "Units/A.tgi",
            "Units/x.TGI",
            "Units/Vulture.tgi",
            "Units/Old/c.tgi",
            "units/notes.txt",
            "more.lst",
            "first.tgi",
            "translate.tgi",
            "Sub/index_c.lst",
        ]
        .map(String::from)
        .into();
        let lists = HashMap::from([
            (
                "Index_A.lst",
                "first.tgi ; comment\n;; units/a.tgi\nmore.lst\n",
            ),
            ("more.lst", "units/*.tgi\n\tunits\\b.tgi\n"),
            ("index_b.lst", "translate.tgi\nindex_b.lst\n"),
            ("Sub/index_c.lst", "first.tgi"),
        ]);
        let tgis = listed_tgis(&entries, |n| lists.get(n).map(|t| t.to_string()));
        assert_eq!(
            tgis,
            [
                "first.tgi",
                "Units/A.tgi",
                "Units/Vulture.tgi",
                "Units/b.tgi",
                "Units/x.TGI",
                "translate.tgi"
            ]
        );
        assert!(wildcard("special_glyphs*.tgi", "special_glyphs_x.tgi"));
        assert!(wildcard("a*b*c", "abc") && !wildcard("a*b*c", "acb"));
        assert!(!wildcard("*.tgi", "x.tgi.bak"));
    }

    #[test]
    fn layouts_give_positions_to_slots_by_property() {
        let parsed = tgi::parse(
            "[OrganizationLayout]\n{\n\tIDS = squad\n\tdefault_formation_ids = missing\n\
             \tcombat_formation_ids = line\n\
             \t[Slot]\n\tproperty_IDS = front\n\tnumber = 2\n\
             \t[Slot]\n\tproperty_IDS = captain\n\tprimary = true\n\
             \t[Formation]\n\t{\n\t\tformation_IDS = line\n\
             \t\t[Position]\n\t\tproperty_IDS = captain\n\t\toffset = 0,0\n\
             \t\t[Position]\n\t\tproperty_IDS = front\n\t\toffset = -1,1\n\
             \t\t[Position]\n\t\tproperty_IDS = front\n\t\toffset = 1,1\n\t}\n}\n",
        );
        let block = parsed.root.blocks_of("OrganizationLayout").next().unwrap();
        let l: defs::OrganizationLayout = tgi::from_block(block).unwrap();
        let layout = layout(&l, &HashSet::new()).unwrap();
        assert_eq!(layout.default, 0);
        assert_eq!(layout.combat, Some(0));
        assert_eq!(layout.primary, Some(1));
        assert_eq!(layout.formations[0].id, Id::from("line"));
        assert_eq!(
            layout.formations[0].slots,
            [vec![[-1.0, 1.0], [1.0, 1.0]], vec![[0.0, 0.0]]]
        );
    }

    #[test]
    fn audio_takes_schema_defaults_and_finds_its_files() {
        // Shaped like the game's schema: typed declarations with defaults.
        let parsed = tgi::parse(
            "[Template Audio]\n{\n\tstring IDS\n\tstring category\n\tstring files = NULL\n\
             \tstring priority = MEDIUM\n\tstring control_flags = NULL\n\
             \tstring who_flags = NULL\n\tpoint2 range = 4, 64\n\
             \tint simultaneous_limit = 2\n\tfloat volume = 80%\n\
             \tpoint2 pitch_adjust = 0%, 0%\n\tpoint2 delay_ms = 0, 0\n\
             \t[optional Subtitle]\n\t{\n\t\tfixed duration = 0\n\t\tlocalized text\n\t}\n}\n\
             [Template AudioSound inherit=Audio]\n{\n\tcategory = SOUND\n}\n\
             [Audio Template=AudioSound]\n{\n\tIDS = tank_select\n\
             \tfiles = Sounds/a.wav, sounds/B.wav,/Audio/Default/c.wav, Sounds/gone.wav\n\
             \tpriority = high\n\tcontrol_flags = VOICE, SEQUENTIAL\n\
             \twho_flags = LOCAL, EVERYONE\n\tvolume = 125%\n\
             \tpitch_adjust = -5%, 5%\n\tdelay_ms = 0, 250\n}\n\
             [Audio Template=AudioSound]\n{\n\tIDS = quiet\n\
             \t[Subtitle]\n\ttext = \"#quiet_text\"\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let archive = [
            "Audio/Sounds/a.wav",
            "Audio/Sounds/b.wav",
            "Audio/Default/c.wav",
        ];
        let find = |p: &str| {
            archive
                .iter()
                .find(|a| a.eq_ignore_ascii_case(p))
                .map(|a| a.to_string())
        };
        let mut blocks = parsed.root.blocks_of("Audio");
        let mut next = || {
            audio(
                &templates.deserialize(blocks.next().unwrap()).unwrap(),
                find,
            )
        };

        let select = next().unwrap();
        assert_eq!(select.category, AudioCategory::Sound);
        assert_eq!(select.files, archive);
        assert_eq!(select.missing, ["Sounds/gone.wav"]);
        assert_eq!(select.priority, AudioPriority::High);
        assert!(select.control.voice && select.control.sequential && !select.control.world);
        let everyone = AudioWho {
            local: true,
            player: true,
            allies: true,
            enemies: true,
        };
        assert_eq!(select.who, everyone);
        assert_eq!(select.volume, 1.25);
        assert_eq!(select.pitch_adjust, [-0.05, 0.05]);
        assert_eq!(select.delay, [0.0, 0.25]);

        let quiet = next().unwrap();
        assert!(quiet.files.is_empty() && quiet.missing.is_empty());
        assert_eq!(quiet.priority, AudioPriority::Medium);
        assert_eq!(quiet.control, AudioControl::default());
        assert_eq!((quiet.falloff, quiet.range, quiet.limit), (4.0, 64.0, 2));
        assert_eq!(quiet.volume, 0.8);
        assert_eq!(quiet.subtitle.as_deref(), Some("#quiet_text"));
    }

    #[test]
    fn objects_play_audio_from_their_sound_commands() {
        let parsed = tgi::parse(
            "[Template FXC]\n{\n\tstring type\n\tint frequency = 1\n\tfloat delay = 0\n\
             \tstring actor_state = NULL\n}\n\
             [Template FXC_PlaySFX inherit=FXC]\n{\n\ttype = PLAY_SFX\n\tstring audio_ids\n\
             \tstring at = SELF\n\tflag looping = false\n}\n\
             [Template FXC_CreateVFX inherit=FXC]\n{\n\ttype = CREATE_VFX\n\tstring vfx_ids\n}\n\
             [Thing]\n{\n\tIDS = bunker\n\
             \t[View]\n\t{\n\
             \t\t[WhenSelected Template=FXC_CreateVFX]\n\t\tvfx_ids = glow\n\
             \t\t[WhenSelected Template=FXC_PlaySFX]\n\t\taudio_ids = bunker_select\n\
             \t\t[WhenDestroyed Template=FXC_PlaySFX]\n\t\taudio_ids = rare\n\t\tfrequency = 0\n\
             \t\t[AspectNormal]\n\t\t{\n\t\t\t[Animation]\n\t\t\t{\n\
             \t\t\t\ttype = RANDOM\n\t\t\t\tstart_min = 20\n\t\t\t\tstart_max = 60\n\
             \t\t\t\t[PlaySFX]\n\t\t\t\tsfx_ids = crickets\n\t\t\t}\n\t\t}\n\t}\n\
             \t[WhenLeave Template=FXC_PlaySFX]\n\taudio_ids = built\n\
             \tactor_state = construction\n\
             \t[PerformerComponent]\n\t{\n\t\t[Ability]\n\t\t{\n\
             \t\t\t[AbilityDefinition]\n\t\t\t{\n\t\t\t\tability_type = attack\n\
             \t\t\t\trange = 10\n\
             \t\t\t\t[Effect]\n\t\t\t\ttype = MAIN_DAMAGE\n\
             \t\t\t\tdamage_type_IDS = gunfire\n\t\t\t\tamount = 5\n\
             \t\t\t\t[WhenDo Template=FXC_PlaySFX]\n\t\t\t\taudio_ids = shot\n\
             \t\t\t\tat = target_ground\n\t\t\t\tdelay = 0.5\n\
             \t\t\t}\n\t\t}\n\t}\n\
             \t[BodyComponent]\n\thealth = 100\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        let cue = |audio: &str, at: &str, delay| Cue {
            audio: Id::from(audio),
            at: at.to_string(),
            delay,
            looping: false,
            every: 1,
        };

        let audio = object_audio(&thing);
        assert_eq!(audio.selected, [cue("bunker_select", "SELF", 0.0)]);
        assert!(audio.destroyed.is_empty());
        assert_eq!(
            audio.left,
            [(Id::from("construction"), cue("built", "SELF", 0.0))]
        );
        let crickets = AnimationAudio {
            kind: "RANDOM".to_string(),
            ids: None,
            interval: Some([20.0, 60.0]),
            audio: Id::from("crickets"),
            looping: false,
        };
        assert_eq!(audio.animations, [crickets]);

        let fighter = fighter(&thing, &HashMap::new()).unwrap();
        assert_eq!(
            fighter.weapons[0].sounds,
            [cue("shot", "TARGET_GROUND", 0.5)]
        );
    }

    #[test]
    fn objects_show_effects_from_their_vfx_commands() {
        let parsed = tgi::parse(
            "[Template FXC]\n{\n\tstring type\n\tint frequency = 1\n\tfloat delay = 0\n\
             \tstring target_flags = null\n\tstring terrain_group_filter = null\n\
             \tfloat health = -1\n\tflag stop_on_destructed = true\n\
             \tstring health_trigger_type = ABSOLUTE_BELOW\n\tstring actor_state = NULL\n\
             \t[list Chain]\n\t[list ChainImmediate]\n}\n\
             [Template FXC_CreateVFX inherit=FXC]\n{\n\ttype = CREATE_VFX\n\tstring VFX_IDS\n\
             \tfloat duration = 0\n\tstring at = SELF\n\tflag attached = true\n\
             \tstring location = null\n\tpoint3 translation = 0,0,0\n\
             \tpoint3 orientation = 0,-1,0\n}\n\
             [Template FXC_StopVFX inherit=FXC]\n{\n\ttype = STOP_VFX\n\tstring VFX_IDS\n}\n\
             [Template FXC_PlaySFX inherit=FXC]\n{\n\ttype = PLAY_SFX\n\tstring audio_ids\n}\n\
             [Thing]\n{\n\tIDS = bunker\n\
             \t[View]\n\t{\n\
             \t\t[WhenConstructed Template=FXC_CreateVFX]\n\t\tvfx_ids = lit_window\n\
             \t\tlocation = window_01\n\t\tstop_on_destructed = false\n\
             \t\t[WhenDestroyed Template=FXC_PlaySFX]\n\t\taudio_ids = boom\n\
             \t\t[WhenDestroyed Template=FXC_CreateVFX]\n\t\tvfx_ids = rare\n\t\tfrequency = 0\n\
             \t}\n\
             \t[WhenEnter Template=FXC_CreateVFX]\n\tvfx_ids = dust\n\
             \tactor_state = construction\n\
             \t[WhenLeave Template=FXC_StopVFX]\n\tvfx_ids = dust\n\
             \tactor_state = construction\n\
             \t[BodyComponent]\n\t{\n\t\thealth = 100\n\
             \t\t[WhenHealth Template=FXC_CreateVFX]\n\t\t{\n\t\t\tvfx_ids = flame\n\
             \t\t\thealth = 0.4\n\t\t\tattached = false\n\
             \t\t\t[Chain Template=FXC_CreateVFX]\n\t\t\tvfx_ids = smoke\n\t\t}\n\t}\n\
             \t[PerformerComponent]\n\t{\n\t\t[Ability]\n\t\t{\n\
             \t\t\t[AbilityDefinition]\n\t\t\t{\n\t\t\t\tability_type = attack\n\
             \t\t\t\trange = 10\n\t\t\t\tprojectile_IDS = shell\n\
             \t\t\t\t[Effect]\n\t\t\t\ttype = MAIN_DAMAGE\n\
             \t\t\t\tdamage_type_IDS = gunfire\n\t\t\t\tamount = 5\n\
             \t\t\t\t[WhenStart Template=FXC_CreateVFX]\n\t\t\t\tvfx_ids = flash\n\
             \t\t\t\tlocation = cannon_ae_01\n\
             \t\t\t\t[WhenDo Template=FXC_CreateVFX]\n\t\t\t\tvfx_ids = puff\n\
             \t\t\t\tat = target_hit_position\n\t\t\t\ttarget_flags = +infantry\n\
             \t\t\t\tterrain_group_filter = snow\n\t\t\t\tdelay = 0.5\n\t\t\t\tfrequency = 2\n\
             \t\t\t}\n\t\t}\n\t}\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        let cue = |vfx: &str| VfxCue {
            vfx: Id::from(vfx),
            stop: false,
            at: "SELF".to_string(),
            location: None,
            attached: true,
            translation: [0.0; 3],
            orientation: [0.0, -1.0, 0.0],
            duration: None,
            delay: 0.0,
            every: 1,
            terrain: None,
            targets: Targets::default(),
            stop_on_destructed: true,
            chain: vec![],
            chain_immediate: vec![],
        };

        let effects = object_effects(&thing);
        let window = VfxCue {
            location: Some("window_01".to_string()),
            stop_on_destructed: false,
            ..cue("lit_window")
        };
        assert_eq!(effects.constructed, [window]);
        assert!(effects.destroyed.is_empty());
        let construction = Id::from("construction");
        assert_eq!(effects.entered, [(construction.clone(), cue("dust"))]);
        let stop = VfxCue {
            stop: true,
            ..cue("dust")
        };
        assert_eq!(effects.left, [(construction, stop)]);
        let flame = VfxCue {
            attached: false,
            chain: vec![cue("smoke")],
            ..cue("flame")
        };
        let health = HealthCue {
            health: 0.4,
            trigger: HealthTrigger::Below,
            cue: flame,
        };
        assert_eq!(effects.health, [health]);

        let weapon = &fighter(&thing, &HashMap::new()).unwrap().weapons[0];
        assert_eq!(weapon.projectile, Some(Id::from("shell")));
        let flash = VfxCue {
            location: Some("cannon_ae_01".to_string()),
            ..cue("flash")
        };
        assert_eq!(weapon.effects.start, [flash]);
        let puff = VfxCue {
            at: "TARGET_HIT_POSITION".to_string(),
            targets: Targets::parse("+infantry"),
            terrain: Some(Id::from("snow")),
            delay: 0.5,
            every: 2,
            ..cue("puff")
        };
        assert_eq!(weapon.effects.shot, [puff]);
    }

    #[test]
    fn vfx_take_their_template_defaults() {
        let parsed = tgi::parse(
            "[Template VFX]\n{\n\tcategory = VFX\n\tflag permanent\n\tflag critical = true\n\
             \tfloat duration = 0\n\tfloat attached_scale = 0\n}\n\
             [Template VFXModel inherit=VFX]\n{\n\tcategory = VFXMODEL\n\tstring model\n\
             \tflag gyroscope = true\n}\n\
             [Template VFXModelTemporary inherit=VFXModel]\n{\n\tpermanent = false\n\
             \tcritical = false\n\t[TextureSearchPath]\n\tpath = /Shared/SharedTextures\n}\n\
             [VFX Template=VFXModelTemporary]\n{\n\tIDS = boom\n\tmodel = Boom.NIF\n\
             \tduration = 2\n\t[Flags]\n\trandom_orientation = true\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("VFX").next().unwrap();
        let v = vfx(
            templates.deserialize(block).unwrap(),
            Some("Effects/Boom.NIF".to_string()),
        );
        assert_eq!(
            v,
            Vfx {
                category: "VFXMODEL".to_string(),
                model: Some("Effects/Boom.NIF".to_string()),
                texture_dirs: vec!["Shared/SharedTextures".to_string()],
                permanent: false,
                critical: false,
                duration: Some(2.0),
                attached_scale: 0.0,
                never_discard: false,
                night_only: false,
                constant_screen_size: false,
                real_time: false,
                random_orientation: true,
                gyroscope: true,
                tint: None,
            }
        );
    }

    #[test]
    fn tints_rise_then_fall_and_repeat_if_they_do() {
        let pulse = VfxTint {
            colour: [0.5, 0.5, 0.5],
            max: 1.0,
            rise: 1.0,
            fall: 0.8,
            repeating: true,
        };
        let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
        assert!(near(pulse.level(0.0), 0.0));
        assert!(near(pulse.level(0.5), 0.5));
        assert!(near(pulse.level(1.0), 1.0));
        assert!(near(pulse.level(1.4), 0.5));
        assert!(near(pulse.level(1.8 + 0.25), 0.25));
        assert_eq!(pulse.colour(0.0), [1.0; 3]);
        assert_eq!(pulse.colour(1.0), [0.5; 3]);
        assert_eq!(pulse.colour(0.5), [0.75; 3]);

        let once = VfxTint {
            repeating: false,
            ..pulse
        };
        assert!(near(once.level(1.4), 0.5));
        assert_eq!(once.level(5.0), 0.0);

        let held = VfxTint {
            max: 0.3,
            rise: 0.2,
            fall: 0.0,
            ..pulse
        };
        assert!(near(held.level(0.03), 0.15));
        assert_eq!(held.level(10.0), 0.3);

        let at_once = VfxTint {
            rise: 0.0,
            fall: 0.0,
            ..pulse
        };
        assert_eq!(at_once.level(0.0), 1.0);
    }

    #[test]
    fn vehicles_conform_by_their_soft_shape() {
        let parsed = tgi::parse(
            "[Template Vehicle]\n{\n\t[Flags]\n\tconform_to_terrain = true\n\
             \t[Size]\n\t{\n\t\tblocking_shape = CIRCLE\n\t\tblocking_radius = 0.7\n\
             \t\tsoft_shape = RECT\n\t\trect_width = 1.5\n\t\trect_height = 2.5\n\t}\n}\n\
             [Thing Template=Vehicle]\n{\n\tIDS = tank\n}\n\
             [Thing]\n{\n\tIDS = soldier\n\t[Size]\n\tsoft_shape = CIRCLE\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let conform = |i| {
            let block = parsed.root.blocks_of("Thing").nth(i).unwrap();
            conform(&templates.deserialize(block).unwrap())
        };
        let footprint = Shape::Rect {
            width: 1.5,
            height: 2.5,
        };
        assert_eq!(conform(0), Some(footprint));
        assert_eq!(conform(1), None);
    }

    #[test]
    fn target_shapes_left_null_are_the_soft_shape() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\tIDS = tank\n\t[Size]\n\tblocking_shape = CIRCLE\n\
             \tsoft_radius = 2\n\ttarget_shape = null\n\ttarget_radius = 3\n}\n\
             [Thing]\n{\n\tIDS = ship\n\t[Size]\n\tblocking_shape = RECT\n\trect_width = 3\n\
             \trect_height = 5\n\ttarget_shape = CIRCLE\n\ttarget_radius = 4\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let shapes: Vec<Shape> = parsed
            .root
            .blocks_of("Thing")
            .map(|b| target_shape(&templates.deserialize(b).unwrap()))
            .collect();
        let circle = |radius| Shape::Circle { radius };
        assert_eq!(shapes, [circle(2.0), circle(4.0)]);
        let square =
            tgi::parse("[Thing]\n{\n\tIDS = hut\n\t[Size]\n\tblocking_shape = SQUARE\n}\n");
        let templates = tgi::Templates::new([&square.root]);
        let hut = square.root.blocks_of("Thing").next().unwrap();
        assert_eq!(
            target_kind(&templates.deserialize(hut).unwrap()),
            Some("SQUARE")
        );
    }

    #[test]
    fn soft_shapes_left_null_take_the_blocking_kind() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\tIDS = hut\n\t[Size]\n\tblocking_shape = RECT\n\trect_width = 3\n\
             \trect_height = 5\n\tsoft_shape = null\n}\n\
             [Thing]\n{\n\tIDS = tree\n\t[Size]\n\tblocking_shape = CIRCLE\n\
             \tblocking_radius = 2\n\tsoft_shape = null\n}\n\
             [Thing]\n{\n\tIDS = rock\n\t[Size]\n\tblocking_shape = CIRCLE\n\
             \tsoft_shape = CIRCLE\n\tsoft_radius = 3\n}\n\
             [Thing]\n{\n\tIDS = flag\n\t[Size]\n\tblocking_shape = POINT\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let bodies: Vec<(Shape, Shape)> = parsed
            .root
            .blocks_of("Thing")
            .map(|b| body(&templates.deserialize(b).unwrap()))
            .map(|b| (b.shape, b.soft))
            .collect();
        let rect = Shape::Rect {
            width: 3.0,
            height: 5.0,
        };
        let circle = |radius| Shape::Circle { radius };
        assert_eq!(
            bodies,
            [
                (rect, rect),
                (circle(2.0), circle(0.5)),
                (circle(0.2), circle(3.0)),
                (Shape::Point, Shape::Point)
            ]
        );
    }

    #[test]
    fn textures_take_their_own_models_renames_then_the_search_paths() {
        let parsed = tgi::parse(
            "[View]\n{\n\t[AspectNormal]\n\t{\n\t\tmodel = hut.nif\n\
             \t\t[TextureRename]\n\t\tsubstring_from = Snow\n\t\tsubstring_to = Mud\n\
             \t\t[TextureRename]\n\t\tsubstring_from = Hut\n\t\tsubstring_to = HutSnow\n\
             \t}\n\t[Aspect]\n\t{\n\t\tmodel = ruin.nif\n\t}\n}\n",
        );
        let block = parsed.root.blocks_of("View").next().unwrap();
        let view: defs::View = tgi::from_block(block).unwrap();
        let path = |m: &str| format!("Village/{m}");
        let view = View {
            models: vec![path("hut.nif"), path("ruin.nif")],
            texture_dirs: vec!["Shared/Textures".into(), "../Common".into()],
            texture_renames: texture_renames(&view, Some("Village/hut.nif"), path),
            scale: 1.0,
            animations: Vec::new(),
            conform: None,
            footing: Footing::Ground,
            unpacking: None,
            packing: None,
            icon: None,
        };
        // The last rename goes first, so the first renames what it made;
        // they reach into folders too, and case matters.
        assert_eq!(
            texture_paths(&view, "Village/hut.nif", "Hut\\Hut_wall.tga"),
            [
                "Village/HutMud/HutMud_wall.tga",
                "Shared/Textures/HutMud_wall.tga",
                "Village/Common/HutMud_wall.tga"
            ]
        );
        let first = |model, file| texture_paths(&view, model, file).remove(0);
        assert_eq!(first("Village/hut.nif", "hut.tga"), "Village/hut.tga");
        assert_eq!(first("Village/ruin.nif", "Hut.tga"), "Village/Hut.tga");
    }

    #[test]
    fn footing_follows_the_last_flags_set() {
        let parsed = tgi::parse(
            "[Template Ship]\n{\n\t[Flags]\n\twater_based = true\n}\n\
             [Template Plane]\n{\n\t[Flags]\n\tflying = true\n\twater_based = false\n}\n\
             [Thing Template=Ship]\n{\n\tIDS = battleship\n}\n\
             [Thing Template=Plane]\n{\n\tIDS = fighter\n}\n\
             [Thing Template=Ship]\n{\n\tIDS = dock\n\t[Flags]\n\twater_based = false\n}\n\
             [Thing]\n{\n\tIDS = tank\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let footings: Vec<_> = parsed
            .root
            .blocks_of("Thing")
            .map(|b| footing(&templates.deserialize(b).unwrap()))
            .collect();
        assert_eq!(
            footings,
            [
                Footing::Water,
                Footing::Air,
                Footing::Ground,
                Footing::Ground
            ]
        );
    }

    #[test]
    fn definitions_keep_file_order_and_skip_blocks_without_ids_or_defined_twice() {
        let a = tgi::parse(
            "[Property]\n{\n\tIDS = vehicle\n}\n\
             [Thing]\n{\n\tIDS = jeep\n\t[BodyComponent]\n\thealth = 50\n}\n\
             [Thing]\n{\n\t[BodyComponent]\n\thealth = 60\n}\n",
        );
        let b = tgi::parse(
            "[Thing]\n{\n\tIDS = tank\n}\n[Thing]\n{\n\tIDS = jeep\n\t[BodyComponent]\n\thealth = 70\n}\n\
             [Thing]\n{\n\tIDS = car\n}\n[Property]\n{\n\tIDS = wheels\n}\n",
        );
        let files = [("a/a.tgi".to_string(), a), ("b/b.tgi".to_string(), b)];
        let definitions = Definitions::new(&files);
        let found: Vec<_> = definitions
            .things
            .iter()
            .map(|t| {
                let health = t.thing.body_component.as_ref().and_then(|b| b.health);
                (t.file.as_str(), t.id.as_str(), health)
            })
            .collect();
        assert_eq!(
            found,
            [("a/a.tgi", "jeep", Some(50.0)), ("b/b.tgi", "tank", None),]
        );
        assert!(definitions.properties.contains_key(&Id::from("vehicle")));
    }

    #[test]
    fn fighters_take_weapons_from_defenders() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\tIDS = hq\n\t[BodyComponent]\n\thealth = 3500\n\
             \t[DefenderComponent]\n\t{\n\t\tcontrol_range = 20\n\t\t[Ability]\n\t\t{\n\
             \t\t\tturret_ids = Turret\n\t\t\t[AbilityDefinition]\n\t\t\t{\n\
             \t\t\t\tability_type = attack\n\t\t\t\trange = 20\n\t\t\t\trecharge_time = 2\n\
             \t\t\t\t[Effect]\n\t\t\t\ttype = MAIN_DAMAGE\n\t\t\t\tdamage_type_IDS = pierce\n\
             \t\t\t\tamount = 20\n\t\t\t}\n\t\t}\n\t}\n             \t[PerformerComponent]\n\t{\n\t\t[Turret]\n\t\t{\n\t\t\tturret_ids = Other\n\t\t}\n             \t\t[Turret]\n\t\t{\n\t\t\tturret_ids = Turret\n\t\t\tyaw_rotation_per_sec_degrees = 60\n             \t\t\tyaw_rotation_range_degrees = 70\n\t\t\tyaw_start_degrees = 400\n             \t\t\tcan_reset = false\n\t\t\toffset = 1, -2\n\t\t}\n\t}\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        let hq = fighter(&thing, &HashMap::new()).unwrap();
        assert_eq!(hq.notice, Some(20.0));
        let gun = &hq.weapons[0];
        assert_eq!((gun.range, gun.recharge), (20.0, 2.0));
        assert!(gun.turret);
        assert_eq!(gun.aimed_by, Some(1));
        let full = std::f32::consts::TAU;
        let other = Turret {
            turn_rate: full,
            sweep: full,
            start: 0.0,
            reset: Some(10.0),
            offset: [0.0; 2],
        };
        let turret = Turret {
            turn_rate: 60f32.to_radians(),
            sweep: 70f32.to_radians(),
            start: full,
            reset: None,
            offset: [1.0, -2.0],
        };
        assert_eq!(hq.turrets, [other, turret]);
    }

    #[test]
    fn fighters_take_resistance_and_flags_from_properties() {
        let parsed = tgi::parse(
            "[Template Property]\n{\n\t[list Modifier]\n}\n\
             [Template Resist]\n{\n\ttype = RESIST_DAMAGE\n}\n\
             [Template Gunfire]\n{\n\ttype = MAIN_DAMAGE\n\tdamage_type_IDS = gunfire\n}\n\
             [Property Template=Property]\n{\n\tIDS = vehicle\n\tenabled_target_flags = vehicle\n\
             \t[Modifier Template=Resist]\n\tdamage_type = gunfire\n\tamount = 0.4\n}\n\
             [Property Template=Property]\n{\n\tIDS = tank\n\tenabled_target_flags = tank, vehicle\n\
             \t[Modifier Template=Resist]\n\tdamage_type = gunfire\n\tamount = 0.2\n}\n\
             [Thing]\n{\n\tIDS = tank\n\t[BodyComponent]\n\thealth = 400\n\tdefense = 6\n\
             \t[PropertiesComponent]\n\t{\n\t\t[Property]\n\t\tproperty_IDS = vehicle\n\
             \t\t[Property]\n\t\tproperty_IDS = tank\n\t}\n\
             \t[PerformerComponent]\n\t{\n\t\t[Ability]\n\t\t{\n\t\t\tanimation1 = attack1\n\
             \t\t\tturret_ids = turret\n\t\t\t[AbilityDefinition]\n\t\t\t{\n\t\t\t\tability_type = attack\n\t\t\t\trange = 12\n\
             \t\t\t\trecharge_time = 3\n\t\t\t\ttargets = +enemy, -air\n\
             \t\t\t\t[Effect Template=Gunfire]\n\t\t\t\tamount = 12\n\t\t\t}\n\t\t}\n\t}\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let properties = parsed
            .root
            .blocks_of("Property")
            .filter_map(|b| templates.deserialize::<defs::Property>(b).ok())
            .filter_map(|p| Some((p.ids.clone()?, p)))
            .collect();
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        let tank = fighter(&thing, &properties).unwrap();
        assert_eq!((tank.health, tank.defense), (400.0, 6.0));
        assert!((tank.resists[&Id::from("gunfire")] - 0.6).abs() < 1e-6);
        assert_eq!(tank.flags, [Id::from("vehicle"), Id::from("tank")]);
        let gun = &tank.weapons[0];
        assert_eq!((gun.range, gun.recharge), (12.0, 3.0));
        assert_eq!(gun.animation.as_deref(), Some("attack1"));
        assert!(gun.turret);
        assert_eq!(
            gun.damage,
            [Damage {
                kind: Id::from("gunfire"),
                amount: 12.0
            }]
        );
        let enemy = Id::from("enemy");
        assert!(gun.targets.allow([&enemy].into_iter()));
        assert!(!gun.targets.allow([&enemy, &Id::from("AIR")].into_iter()));
        assert!(!gun.targets.allow(tank.flags.iter()));
    }

    #[test]
    fn character_animations() {
        let parsed = tgi::parse(
            "[Template Animation]\n{\n\tstring type\n\tstring IDS\n\tstring kf = %s%s.kf\n\tfixed length = 1\n}\n\
             [Template IdleAnimation Template=Animation]\n{\n\ttype = IDLE\n\tlength = 3\n}\n\
             [Template Character]\n{\n\t[list Animation Template=Animation]\n}\n\
             [Template Soldier Template=Character]\n{\n\t[Animation Template=IdleAnimation]\n\tIDS = idle\n\
             \tkf = /Shared/Idle.kf\n\n\t[Animation Template=IdleAnimation]\n\tIDS = fidget\n}\n\
             [Thing]\n{\n\t[CharacterComponent Template=Soldier]\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        let animations = thing.character_component.unwrap().animation;
        let fields: Vec<_> = animations
            .iter()
            .map(|a| {
                (
                    a.kind.as_deref(),
                    a.ids.as_deref(),
                    a.kf.as_deref(),
                    a.length,
                )
            })
            .collect();
        assert_eq!(
            fields,
            [
                (
                    Some("IDLE"),
                    Some("idle"),
                    Some("/Shared/Idle.kf"),
                    Some(3.0)
                ),
                (Some("IDLE"), Some("fidget"), Some("%s%s.kf"), Some(3.0)),
            ]
        );
    }
}
