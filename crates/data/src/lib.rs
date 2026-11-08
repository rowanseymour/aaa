//! The game's data as a whole: access to the installed archives, and the
//! definitions built from their files (terrain, object views, ...).

pub mod defs;
mod id;

pub use id::Id;

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
}

pub struct GameData {
    archives: Vec<Archive<BufReader<File>>>,
    /// Lower-case path to entry; where archives share a path the first wins.
    paths: HashMap<String, EntryRef>,
    /// Lower-case file name to entries.
    files: HashMap<String, Vec<EntryRef>>,
    /// See [`GameData::definitions`].
    definitions: Option<Arc<Definitions>>,
}

/// The object templates and shared properties the `.tgi` files define,
/// deserialized.
struct Definitions {
    /// In the order the files and their blocks come, which decides between
    /// definitions sharing an id.
    things: Vec<Template>,
    properties: HashMap<Id, defs::Property>,
}

/// An object template as some `.tgi` file defines it.
struct Template {
    /// The defining file's archive path.
    file: String,
    id: Id,
    thing: defs::Thing,
}

impl Definitions {
    fn new(files: &[(String, tgi::Parsed)]) -> Self {
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let things = files
            .iter()
            .flat_map(|(file, parsed)| {
                parsed.root.blocks_of("Thing").filter_map(|block| {
                    Some(Template {
                        file: file.clone(),
                        id: Id::from(block.value("ids")?),
                        thing: templates.deserialize(block).ok()?,
                    })
                })
            })
            .collect();
        let properties = files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("Property"))
            .filter_map(|b| templates.deserialize::<defs::Property>(b).ok())
            .filter_map(|p| Some((p.ids.clone()?, p)))
            .collect();
        Definitions { things, properties }
    }
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
    /// Substitutions applied to texture names, `(from, to)`.
    pub texture_renames: Vec<(String, String)>,
    /// Size multiplier: the middle of the template's random size range, which
    /// each placed object scales again by its own factor.
    pub scale: f32,
    /// The animations the models can play; units have them.
    pub animations: Vec<Animation>,
    /// The outline under which the ground is sampled to tilt the model to
    /// follow it, as vehicles do; `None` for models that stay upright.
    pub conform: Option<Shape>,
    /// Archive path of the model a building plays once as it unpacks.
    pub unpacking: Option<String>,
    /// Archive path of the model a building plays once as it packs.
    pub packing: Option<String>,
    /// Archive path of its picture in the interface, such as a unit's
    /// portrait in its regiment's panel or a building's button.
    pub icon: Option<String>,
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
    /// Played each shot.
    pub sounds: Vec<Cue>,
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
        for word in text.split([',', ' ']).filter(|w| !w.is_empty()) {
            match word.split_at(1) {
                ("-", flag) => out.excluded.push(Id::from(flag)),
                ("+", flag) => out.required.push(Id::from(flag)),
                _ => out.required.push(Id::from(word)),
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
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
    /// Money it costs.
    pub cost: f32,
}

/// How a template packs and unpacks.
#[derive(Debug, Clone, PartialEq)]
pub struct Deployment {
    /// For a packed building, such as a supply truck, the template it
    /// unpacks into.
    pub unpacks_into: Option<Id>,
    /// Where the packed building stands from the building, x across the
    /// building's facing and y along it, and the way it faces there.
    pub packed_offset: [f32; 2],
    pub packed_facing: [f32; 2],
    /// For a building, the seconds packing and unpacking take.
    pub packing: f32,
    pub unpacking: f32,
    /// For a building, the share of its health at or below which it can't
    /// pack.
    pub pack_health_min: f32,
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
    /// For `RANDOM` animations, the range of seconds between plays.
    pub interval: Option<[f32; 2]>,
    pub audio: Id,
    pub looping: bool,
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
        let mut files: HashMap<String, Vec<EntryRef>> = HashMap::new();
        for (a, archive) in archives.iter().enumerate() {
            for (i, e) in archive.entries.iter().enumerate() {
                let lower = e.name.to_ascii_lowercase();
                let file = lower.rsplit('/').next().unwrap_or(&lower).to_string();
                files.entry(file).or_default().push((a, i));
                paths.entry(lower).or_insert((a, i));
            }
        }
        Ok(GameData {
            archives,
            paths,
            files,
            definitions: None,
        })
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

    /// Every `.tgi` file whose lower-case path passes `filter`, parsed.
    fn parse_tgis(&mut self, filter: impl Fn(&str) -> bool) -> Vec<(String, tgi::Parsed)> {
        self.entry_names(|n| n.ends_with(".tgi") && filter(n))
            .into_iter()
            .filter_map(|n| {
                let parsed = tgi::parse_bytes(&self.read(&n)?);
                Some((n, parsed))
            })
            .collect()
    }

    /// Finds a texture a model refers to by file name: in the given folders
    /// first, then anywhere in the archive, preferring the path that shares
    /// the longest folder prefix with `near`.
    pub fn find_texture(&self, file: &str, dirs: &[String], near: &str) -> Option<String> {
        let file = file.replace('\\', "/");
        let name = file.rsplit('/').next().unwrap_or(&file);
        for dir in dirs {
            let path = format!("{dir}/{name}");
            if self.exists(&path) {
                return Some(path);
            }
        }
        let near = near.to_ascii_lowercase();
        let shared = |p: &str| {
            p.bytes()
                .zip(near.bytes())
                .take_while(|(a, b)| a == b)
                .count()
        };
        self.files
            .get(&name.to_ascii_lowercase())?
            .iter()
            .map(|&e| self.entry_name(e))
            .max_by_key(|p| shared(&p.to_ascii_lowercase()))
            .map(str::to_string)
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
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let water = water_colors(&templates, files.iter().map(|(_, p)| &p.root));
        let mut out = HashMap::new();
        for (file, parsed) in &files {
            if !file.to_ascii_lowercase().starts_with("terrain/") {
                continue;
            }
            let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
            for block in parsed.root.blocks_of("Terrain") {
                let Ok(terrain) = templates.deserialize::<defs::Terrain>(block) else {
                    continue;
                };
                let water = terrain.ids.as_ref().and_then(|id| water.get(id));
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
                        },
                    );
                }
            }
        }
        if let Some(bytes) = self.read("translate.tgi") {
            add_renames(&mut out, &tgi::decode(&bytes), "Variation", |_| true);
        }
        out
    }

    /// The views of all object templates, by template id,
    /// including the ids the game has since renamed: `None` for templates
    /// that name no model, such as sound emitters, or are invisible.
    /// Templates without a view, of their own or inherited, are left out.
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
            // Model paths are relative to the defining file, so where an id
            // is defined more than once keep a definition that finds its
            // models.
            let drawable = |v: &Option<View>| v.as_ref().is_some_and(|v| !v.models.is_empty());
            if drawable(&view) || !out.get(id).is_some_and(drawable) {
                out.insert(id.clone(), view);
            }
        }
        if let Some(bytes) = self.read("translate.tgi") {
            add_renames(&mut out, &tgi::decode(&bytes), "Thing", complete);
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
        self.by_template(|t| fighter(t, &definitions.properties))
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
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let block = files
            .iter()
            .find_map(|(_, p)| p.root.blocks_of("Minimap").next())?;
        templates.deserialize(block).ok()
    }

    /// The archive paths of the badges that divisions of each organization
    /// template's nation wear, in the order HQs number them, by template id.
    pub fn division_badges(&mut self) -> HashMap<Id, Vec<String>> {
        let files = self.parse_tgis(|n| n.starts_with("nations/") || n.starts_with("templates/"));
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let by_group: HashMap<Id, Vec<String>> = files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("Nation"))
            .filter_map(|b| templates.deserialize::<defs::Nation>(b).ok())
            .filter_map(|n| {
                let badges = n.badge.iter();
                let paths = badges.map(|b| format!("Nations/{}", b.badge_texture));
                Some((n.placement_group_ids?, paths.collect()))
            })
            .collect();
        self.by_template(|t| {
            let group = t.placement.as_ref()?.placement_group_ids.as_ref()?;
            by_group.get(group).cloned()
        })
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

    /// The old ids of object templates the game has since renamed, which
    /// maps may still use and the other methods answer for too.
    pub fn renamed_things(&mut self) -> std::collections::HashSet<Id> {
        let Some(bytes) = self.read("translate.tgi") else {
            return Default::default();
        };
        // As in `add_renames`, including those a stray comment hides.
        let translate = tgi::decode(&bytes).replace("/*", "").replace("*/", "");
        renames(&translate, "Thing").into_keys().collect()
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
    /// commissions or recruits it, by template id.
    pub fn listings(&mut self) -> HashMap<Id, Listing> {
        let strings = self.strings();
        self.by_template(|t| {
            let view = t.view.as_ref();
            Some(Listing {
                name: localized(&strings, t.name.as_deref())?,
                icon: view
                    .and_then(|v| v.icon.as_deref())
                    .filter(|i| i.to_ascii_lowercase().starts_with("/ui/"))
                    .and_then(ui_path),
                cost: t
                    .economy_component
                    .as_ref()
                    .and_then(|e| e.cost.as_ref()?.money)
                    .unwrap_or(0.0),
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
            add_renames(&mut out, &tgi::decode(&bytes), "Thing", |_| true);
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
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let aggressive: HashSet<Id> = files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("Formation"))
            .filter_map(|b| templates.deserialize::<defs::Formation>(b).ok())
            .filter(|f| f.aggressive)
            .filter_map(|f| f.ids)
            .collect();
        files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("OrganizationLayout"))
            .filter_map(|b| templates.deserialize::<defs::OrganizationLayout>(b).ok())
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
        let models = match normal {
            Some(n) if !view.aspects_as_variations => vec![n],
            _ => aspects,
        };
        let texture_dirs = view
            .texture_search_path
            .iter()
            .filter_map(|b| b.path.as_deref())
            .map(|p| p.trim_matches('/').to_string())
            .collect();
        let texture_renames = view
            .aspect_normal
            .iter()
            .chain(&view.aspect)
            .flat_map(|a| &a.texture_rename)
            .filter_map(|r| Some((r.substring_from.clone()?, r.substring_to.clone()?)))
            .collect();
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
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let mut coverage = tgi::de::Coverage::default();
        for block in files.iter().flat_map(|(_, p)| p.root.blocks()) {
            let t = Some(&templates);
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
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let mut out: Vec<defs::Sun> = files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("Sun"))
            .filter_map(|b| templates.deserialize(b).ok())
            .collect();
        out.sort_by(|a, b| a.ids.cmp(&b.ids));
        out
    }

    /// The nations sides can play as, by name.
    pub fn nations(&mut self) -> Vec<Nation> {
        let strings = self.strings();
        let files = self.parse_tgis(|n| n.starts_with("nations/"));
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let mut nations: Vec<Nation> = files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("Nation"))
            .filter_map(|b| templates.deserialize::<defs::Nation>(b).ok())
            .filter_map(|n| {
                let id = n.ids?;
                let placeholders = n.placeholder.into_iter();
                Some(Nation {
                    name: localized(&strings, n.name.as_deref()).unwrap_or_else(|| id.to_string()),
                    id,
                    placeholders: placeholders
                        .filter_map(|p| Some((p.placeholder_ids?, p.thing_ids?)))
                        .collect(),
                })
            })
            .collect();
        nations.sort_by(|a, b| a.name.cmp(&b.name));
        nations
    }

    /// The colours players can take, by id, e.g. `blue`.
    pub fn kingdom_colors(&mut self) -> HashMap<Id, [f32; 3]> {
        let files = self.parse_tgis(|n| n == "game/kingdom_colors.tgi");
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("KingdomColor"))
            .filter_map(|b| templates.deserialize::<defs::KingdomColor>(b).ok())
            .filter_map(|c| Some((c.ids?, c.color)))
            .collect()
    }

    /// How far beyond a blocking building's footprint the ground must be
    /// clear for it to be put there, in world units.
    pub fn building_padding(&mut self) -> Option<f32> {
        let files = self.parse_tgis(|n| n == "game/svars.tgi");
        files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("SVars"))
            .find_map(|b| b.parse_value("BuildingPaddingMeters"))
    }

    /// The orders the player can give, in the order the files list them.
    pub fn commands(&mut self) -> Vec<Command> {
        let strings = self.strings();
        let files = self.parse_tgis(|n| {
            n.starts_with("game/actor_commands") || n.starts_with("game/actorcommands/")
        });
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("ActorCommand"))
            .filter_map(|b| templates.deserialize::<defs::ActorCommand>(b).ok())
            .filter_map(|c| {
                let ids = c.ids?;
                Some(Command {
                    name: localized(&strings, c.name.as_deref()).unwrap_or(ids.to_string()),
                    ids,
                    icon: c.icon.as_deref().and_then(ui_path),
                    slot: c.preferred_position.and_then(|p| usize::try_from(p).ok()),
                    cursor: c.cursor_ids,
                    marker: c.command_vfx_ids_location,
                    group_offset: c.apply_group_offset.unwrap_or(true),
                })
            })
            .collect()
    }

    /// The mouse cursors whose frames are all there, by id.
    pub fn cursors(&mut self) -> HashMap<Id, CursorLook> {
        let files = self.parse_tgis(|n| n.starts_with("cursors/"));
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let mut out = HashMap::new();
        for (file, parsed) in &files {
            for block in parsed.root.blocks_of("Cursor") {
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

    /// The archive path of each effect's model that's there, by effect id.
    pub fn vfx_models(&mut self) -> HashMap<Id, String> {
        let files = self.parse_tgis(|n| n.starts_with("effects/"));
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let mut out = HashMap::new();
        for (file, parsed) in &files {
            for block in parsed.root.blocks_of("VFX") {
                let Ok(v) = templates.deserialize::<defs::Vfx>(block) else {
                    continue;
                };
                if let (Some(ids), Some(model)) = (v.ids, v.model) {
                    let model = beside(file, &model);
                    if self.exists(&model) {
                        out.insert(ids, model);
                    }
                }
            }
        }
        out
    }

    /// How each formation shows on its button, by formation id.
    pub fn formation_looks(&mut self) -> HashMap<Id, FormationLook> {
        let strings = self.strings();
        let files = self.parse_tgis(|n| n.starts_with("organizations/"));
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let mut out = HashMap::new();
        for (file, parsed) in &files {
            let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
            for block in parsed.root.blocks_of("Formation") {
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

    /// The game's text in its default language, by key.
    pub fn strings(&mut self) -> HashMap<String, String> {
        let files = self.parse_tgis(|n| n.starts_with("localization/strings_"));
        files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("Text"))
            .flat_map(text_strings)
            .collect()
    }

    /// The audio of each object template that plays any, by template id.
    pub fn object_audio(&mut self) -> HashMap<Id, ObjectAudio> {
        let mut out = self.by_template(|t| Some(object_audio(t)));
        out.retain(|_, a| !a.is_empty());
        out
    }

    /// Every sound, speech and music, by id.
    pub fn audio(&mut self) -> HashMap<Id, Audio> {
        let files = self.parse_tgis(|n| n.starts_with("audio/") || n.starts_with("templates/"));
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("Audio"))
            .filter_map(|b| templates.deserialize::<defs::Audio>(b).ok())
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
    let blocking = thing.flags.iter().rev().find_map(|f| f.blocking) == Some(true);
    let kind = thing
        .size
        .iter()
        .rev()
        .find_map(|s| s.blocking_shape.as_deref());
    let shape = shape(thing, kind, |s| s.blocking_radius);
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
        blocking: blocking.then_some(shape),
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
    let kind = thing
        .size
        .iter()
        .rev()
        .find_map(|s| s.soft_shape.as_deref());
    conforms.then(|| shape(thing, kind, |s| s.soft_radius))
}

/// A shape of the template's `[Size]`, which its shapes share all but the
/// radius of.
fn shape(thing: &defs::Thing, kind: Option<&str>, radius: fn(&defs::Size) -> Option<f32>) -> Shape {
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
            radius: size(radius).unwrap_or(0.2),
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
    let abilities = thing.performer_component.iter().flat_map(|p| &p.ability);
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
            sounds: cues(&d.when_do),
        });
    }
    Some(fighter)
}

/// The depth colours of each water terrain, by IDS. Biomes name a
/// `WaterInfo` template for each of their water terrains.
fn water_colors<'a>(
    templates: &tgi::Templates,
    roots: impl Iterator<Item = &'a tgi::Block>,
) -> HashMap<Id, Vec<DepthColor>> {
    roots
        .flat_map(|r| r.blocks_of("Biome"))
        .filter_map(|b| templates.deserialize::<defs::Biome>(b).ok())
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

/// Gives old ids that maps may still use the entries of their replacements,
/// as `translate.tgi` lists them for `node`, e.g. `Thing`. An entry that
/// isn't `complete` is treated as missing.
fn add_renames<T: Clone>(
    entries: &mut HashMap<Id, T>,
    translate: &str,
    node: &str,
    complete: impl Fn(&T) -> bool,
) {
    let live = renames(translate, node);
    for old in live.keys() {
        if let Some(v) = entries.get(&resolve(&live, old)).cloned() {
            entries.insert(old.clone(), v);
        }
    }
    // A stray `/*` comments out most of the file, including renames that
    // older maps rely on, so use those too, but only for missing entries.
    let mut all = renames(&translate.replace("/*", "").replace("*/", ""), node);
    all.extend(live);
    for old in all.keys() {
        if entries.get(old).is_some_and(&complete) {
            continue;
        }
        if let Some(v) = entries.get(&resolve(&all, old)).filter(|v| complete(v)) {
            entries.insert(old.clone(), v.clone());
        }
    }
}

/// Whether a view names readable models, or deliberately none.
fn complete(view: &Option<View>) -> bool {
    view.as_ref().is_none_or(|v| !v.models.is_empty())
}

/// Old ids of `node`s that maps may still use, to their replacements, from
/// `translate.tgi`. Where an id is renamed more than
/// once the first wins, as the file lists the latest first.
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

/// The latest id of a template that may have been renamed more than once,
/// stopping at a rename cycle.
fn resolve(renames: &HashMap<Id, Id>, id: &Id) -> Id {
    let mut id = id;
    for _ in 0..renames.len() {
        match renames.get(id) {
            Some(new) => id = new,
            None => break,
        }
    }
    id.clone()
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
                ("RANDOM", Some(min), Some(max)) => Some([min / 1000.0, max / 1000.0]),
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
    let percents = |p: &Option<[String; 2]>| {
        p.as_ref().map_or([0.0; 2], |[a, b]| {
            [percent(a).unwrap_or(0.0), percent(b).unwrap_or(0.0)]
        })
    };
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
        volume: a.volume.as_deref().and_then(percent).unwrap_or(0.8),
        volume_adjust: a.volume_adjust.as_deref().and_then(percent).unwrap_or(0.0),
        pitch_adjust: percents(&a.pitch_adjust),
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

/// A percentage as a fraction, e.g. `80%` as 0.8; the `%` is optional.
fn percent(text: &str) -> Option<f32> {
    let n: f32 = text.trim().trim_end_matches('%').trim().parse().ok()?;
    Some(n / 100.0)
}

/// A map's file name without its `.rmp` extension.
pub fn map_title(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    &file[..file.len() - 4]
}

fn find_case_insensitive(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

/// A `Text` block's strings by key, without the format a key may carry
/// after a `|`.
fn text_strings(block: &tgi::Block) -> impl Iterator<Item = (String, String)> + '_ {
    block.fields().filter(|f| f.quoted).filter_map(|f| {
        let key = f.key.split('|').next()?.to_ascii_lowercase();
        Some((key, f.value.clone()?))
    })
}

/// The text a `#` key names, or a name written out as is.
fn localized(strings: &HashMap<String, String>, name: Option<&str>) -> Option<String> {
    let name = name?;
    match name.strip_prefix('#') {
        Some(key) => strings.get(&key.to_ascii_lowercase()).cloned(),
        None => Some(name.to_string()),
    }
}

/// The archive path of an interface image, whose `%s` folder, for a
/// screen size, is empty for the default images.
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
/// long as they're there.
fn frames(pattern: &str, exists: impl Fn(&str) -> bool) -> Vec<String> {
    let frame = |n: usize| pattern.replace("%d", &n.to_string());
    let first = usize::from(!exists(&frame(0)));
    (first..).map(frame).take_while(|f| exists(f)).collect()
}

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
            "[Text language = default]\n{\n\tmove_name = \"Move\"\n\
             \tcount|d = \"%d left\"\n}\n",
        );
        let block = parsed.root.blocks_of("Text").next().unwrap();
        let strings: HashMap<String, String> = text_strings(block).collect();
        assert_eq!(
            localized(&strings, Some("#Move_name")).as_deref(),
            Some("Move")
        );
        assert_eq!(localized(&strings, Some("Plain")).as_deref(), Some("Plain"));
        assert_eq!(strings.get("count").map(String::as_str), Some("%d left"));
        assert_eq!(
            ui_path("/UI/%s/Game/ActionButtons/IconMove.png").as_deref(),
            Some("UI/Game/ActionButtons/IconMove.png")
        );
        assert_eq!(ui_path("NULL"), None);
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
    }

    #[test]
    fn renames() {
        let renames: HashMap<Id, Id> = [("a", "B"), ("b", "c"), ("x", "y"), ("y", "x")]
            .into_iter()
            .map(|(o, n)| (Id::from(o), Id::from(n)))
            .collect();
        let resolve = |id| resolve(&renames, &Id::from(id)).to_string();
        assert_eq!(resolve("A"), "c");
        assert_eq!(resolve("c"), "c");
        assert_eq!(resolve("z"), "z");
        let cycle = resolve("x");
        assert!(cycle == "x" || cycle == "y");
    }
    #[test]
    fn commented_renames() {
        let view = |model: &str| {
            Some(View {
                models: vec![model.to_string()],
                texture_dirs: Vec::new(),
                texture_renames: Vec::new(),
                scale: 1.0,
                animations: Vec::new(),
                conform: None,
                unpacking: None,
                packing: None,
                icon: None,
            })
        };
        let mut views: HashMap<Id, Option<View>> = [
            ("new", view("new.nif")),
            ("newer", view("newer.nif")),
            ("kept", view("kept.nif")),
            ("hidden", None),
        ]
        .into_iter()
        .map(|(id, v)| (Id::from(id), v))
        .collect();
        // Renames the game reads, then ones commented out.
        let rename = |node, old, new| {
            format!("[Translate]\nnode = {node}\nids_old = {old}\nids_new = {new}\n")
        };
        let translate = format!(
            "{}/*\n{}{}{}{}{}*/\n",
            rename("Thing", "Live", "NEW"),
            rename("Thing", "kept", "new"),
            rename("Thing", "old", "newer"),
            rename("Thing", "old", "new"),
            rename("Thing", "hidden", "new"),
            rename("Variation", "terrain", "new"),
        );
        add_renames(&mut views, &translate, "Thing", complete);
        let model = |id: &str| views[&Id::from(id)].as_ref().map(|v| v.models[0].as_str());
        assert_eq!(model("live"), Some("new.nif"));
        assert_eq!(model("old"), Some("newer.nif"));
        assert_eq!(model("kept"), Some("kept.nif"));
        assert_eq!(model("hidden"), None);
        assert!(!views.contains_key(&Id::from("terrain")));
    }
    #[test]
    fn variation_renames() {
        let translate = "[Translate]\nnode = Variation\nids_old = live\nids_new = new\n\
            /*\n[Translate]\nnode = Variation\nids_old = old\nids_new = live\n*/\n";
        let mut variations: HashMap<Id, &str> = [(Id::from("new"), "new.tga")].into();
        add_renames(&mut variations, translate, "Variation", |_| true);
        assert_eq!(variations[&Id::from("live")], "new.tga");
        assert_eq!(variations[&Id::from("old")], "new.tga");
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
            interval: Some([0.02, 0.06]),
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
    fn definitions_keep_file_order_and_skip_blocks_without_ids() {
        let a = tgi::parse(
            "[Property]\n{\n\tIDS = vehicle\n}\n\
             [Thing]\n{\n\tIDS = jeep\n\t[BodyComponent]\n\thealth = 50\n}\n\
             [Thing]\n{\n\t[BodyComponent]\n\thealth = 60\n}\n",
        );
        let b = tgi::parse(
            "[Thing]\n{\n\tIDS = tank\n}\n[Thing]\n{\n\tIDS = jeep\n\t[BodyComponent]\n\thealth = 70\n}\n",
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
            [
                ("a/a.tgi", "jeep", Some(50.0)),
                ("b/b.tgi", "tank", None),
                ("b/b.tgi", "jeep", Some(70.0)),
            ]
        );
        assert!(definitions.properties.contains_key(&Id::from("vehicle")));
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
