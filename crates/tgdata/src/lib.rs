//! A game's data as a whole: access to the installed archives, and the
//! definitions built from their files (terrain, object views, ...).
//!
//! Parsing is pure: [`tgi`] turns a file's bytes into blocks, and [`defs`]
//! reads blocks as types. [`GameData`] is the layer above that reads the
//! archives and keeps what it parses, and the tables much of the game asks
//! for, so that each is made once. It can be shared between threads, which
//! share what it keeps: a table read on one is there for the others.

mod audio;
mod combat;
pub mod defs;
mod economy;
mod effects;
mod fog;
mod guard;
mod hotkeys;
mod id;
mod interface;
mod messages;
mod nations;
mod objectives;
mod organizations;
mod outcome;
mod properties;
mod reads;
mod recruiting;
mod research;
mod settlements;
mod special_ops;
mod strikes;
mod strings;
mod supply;
mod terrain;
mod views;

pub use audio::{
    AnimationAudio, Audio, AudioCategory, AudioControl, AudioPriority, AudioWho, AudioWorld, Cue,
    MusicSettings, ObjectAudio, SoundSwitches, Volumes,
};
pub use combat::{
    AimHeights, Body, Damage, Fighter, Flight, FlightKind, Shape, Targets, Turret, Weapon,
};
pub use economy::{Amounts, Economy, EconomyRules, Logistics, ResourceKind, ResourceRole, Supply};
pub use effects::{
    Decal, HealthCue, HealthTrigger, ObjectEffects, Vfx, VfxCue, VfxTint, WeaponEffects,
};
pub use fog::{FogSetting, Sight};
pub use guard::{GUARDING, GuardRules};
pub use hotkeys::{Hotkey, HotkeyConfiguration};
pub use id::Id;
pub use interface::{Command, ConnectionLook, CursorLook, FormationLook, SupplyBorder};
pub use messages::{GameEvent, InterfaceVars, MessageKind};
pub use nations::{BannerDecal, Banners, Faction, FactionOperation, Handicap, Nation};
pub use objectives::{ObjectiveLook, ObjectiveLooks};
pub use organizations::{Formation, Layout, OrganizationComponent, Ranges};
pub use properties::PropertyLook;
pub use reads::BY_HAND as READ_BY_HAND;
pub use recruiting::{
    Airlift, Amount, DeployPoints, Deployment, Fall, Listing, Requires, Technology,
};
pub use research::{Grant, Research};
pub use special_ops::{EffectLine, ExperienceRules, Marker, OperationCues, SpecialOperation};
pub use strikes::{Affliction, Lookout, Strike};
pub use strings::{PingLook, Strings};
pub use supply::{Kind, Operation, Summon, SupplyEffect, SupplyRules};
pub use terrain::{
    DepthColor, TerrainEffect, TerrainKind, TerrainRules, TextureAnimation, Variation,
};
pub use views::{Animation, AnimationKind, Footing, Portrait, View};

use std::any::Any;
use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use tgck::{Archive, Entry, Map};

pub struct GameData {
    /// Each behind a lock of its own, as reading one moves its place in
    /// the file.
    archives: Vec<Mutex<Archive<BufReader<File>>>>,
    /// Each archive's entries, to look them up without the lock.
    entries: Vec<Vec<Entry>>,
    /// Lower-case path to entry; where archives share a path the first wins.
    paths: HashMap<String, EntryRef>,
    /// The archive paths of the `.tgi` files the game loads, in the order
    /// it loads them.
    tgis: Vec<String>,
    /// What the game's own versions of the files every game has are named
    /// with, as [`game_tag`] finds it.
    tag: Option<String>,
    /// Each `.tgi` file read, parsed, by lower-case path, kept as long as
    /// this is: tables are often built from the same files.
    parsed: Mutex<HashMap<String, Arc<tgi::Parsed>>>,
    /// What [`GameData::kept`] has built, by name.
    tables: Mutex<HashMap<&'static str, Arc<dyn Any + Send + Sync>>>,
}

/// A lock that a thread panicking while holding it leaves usable: the maps
/// these locks guard are only ever added to whole, and an archive's reader
/// seeks to each entry it reads, wherever a read before it stopped.
fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// The object templates and shared properties the `.tgi` files define,
/// deserialized.
struct Definitions {
    /// In the order the game loads them. Defining an id twice is an error
    /// in the data, and only the first definition is kept.
    things: Vec<Template>,
    properties: HashMap<Id, defs::Property>,
    /// The archive path of the file defining each of `properties`.
    property_files: HashMap<Id, String>,
    /// The abilities defined on their own, such as special operations,
    /// in the order they load, the first kept of those defined twice.
    abilities: Vec<(Id, defs::AbilityDefinition)>,
    /// The factions, in the order they load, the first kept of those
    /// defined twice.
    factions: Vec<defs::Faction>,
    /// The game's words for ranges of figures, in the order they load.
    descriptions: Vec<defs::Description>,
}

/// An object template as some `.tgi` file defines it.
pub(crate) struct Template {
    /// The defining file's archive path.
    file: String,
    id: Id,
    thing: defs::Thing,
}

/// Whether a block of `kind` defines a thing: `Unit` and `Projectile` are
/// other names for `Thing`, sharing its ids and fields.
pub(crate) fn is_thing(kind: &str) -> bool {
    ["Thing", "Unit", "Projectile"]
        .iter()
        .any(|k| k.eq_ignore_ascii_case(kind))
}

impl Definitions {
    /// Defining a Thing id again is an error that ends the reading of its
    /// file there, the blocks before it kept.
    fn new<P: Borrow<tgi::Parsed>>(files: &[(String, P)]) -> Self {
        let mut ids = HashSet::new();
        let mut things = Vec::new();
        let mut properties = HashMap::new();
        let mut property_files = HashMap::new();
        let mut abilities: Vec<(Id, defs::AbilityDefinition)> = Vec::new();
        let mut factions: Vec<defs::Faction> = Vec::new();
        let mut descriptions: Vec<defs::Description> = Vec::new();
        for scope in scoped(files) {
            let file = scope.file;
            for block in scope.root.blocks() {
                let kind = block.kind().unwrap_or("");
                if is_thing(kind) {
                    let Some(id) = block.value("ids").map(Id::from) else {
                        continue;
                    };
                    if ids.contains(&id) {
                        break;
                    }
                    if let Some(thing) = scope.deserialize(block) {
                        ids.insert(id.clone());
                        things.push(Template {
                            file: file.to_string(),
                            id,
                            thing,
                        });
                    }
                } else if kind.eq_ignore_ascii_case("Property")
                    && let Some(p) = scope.deserialize::<defs::Property>(block)
                    && let Some(id) = p.ids.clone()
                {
                    property_files.insert(id.clone(), file.to_string());
                    properties.insert(id, p);
                } else if kind.eq_ignore_ascii_case("AbilityDefinition")
                    && let Some(a) = scope.deserialize::<defs::AbilityDefinition>(block)
                    && let Some(id) = a.ids.clone()
                    && !abilities.iter().any(|(known, _)| *known == id)
                {
                    abilities.push((id, a));
                } else if kind.eq_ignore_ascii_case("Faction")
                    && let Some(g) = scope.deserialize::<defs::Faction>(block)
                    && let Some(id) = &g.ids
                    && !factions.iter().any(|known| known.ids.as_ref() == Some(id))
                {
                    factions.push(g);
                } else if kind.eq_ignore_ascii_case("Description")
                    && let Some(d) = scope.deserialize::<defs::Description>(block)
                {
                    descriptions.push(d);
                }
            }
        }
        Definitions {
            things,
            properties,
            property_files,
            abilities,
            factions,
            descriptions,
        }
    }
}

/// A `.tgi` file that loads, with the templates its blocks see.
pub(crate) struct Scoped<'a> {
    file: &'a str,
    root: &'a tgi::Block,
    templates: tgi::Templates<'a>,
}

impl Scoped<'_> {
    /// Its blocks of `kind` that read as `T`.
    fn read<'s, T: serde::de::DeserializeOwned + 'static>(
        &'s self,
        kind: &'s str,
    ) -> impl Iterator<Item = T> + 's {
        (self.root.blocks_of(kind)).filter_map(|b| self.deserialize(b))
    }

    /// One of its blocks as `T`, resolved against its templates.
    fn deserialize<T: serde::de::DeserializeOwned + 'static>(
        &self,
        block: &tgi::Block,
    ) -> Option<T> {
        logged(
            self.file,
            block,
            reads::decode(block, Some(&self.templates)),
        )
    }
}

/// What a block read as, or `None` with a warning: a block that doesn't
/// read is left out, as the game leaves it out, but is most likely a type
/// here that doesn't match the data.
pub(crate) fn logged<T>(file: &str, block: &tgi::Block, read: Result<T, tgi::Error>) -> Option<T> {
    read.map_err(|e| log::warn!("{}", warning(file, block, &e)))
        .ok()
}

/// Where a block that doesn't read is, at the line it failed on if known,
/// and why.
fn warning(file: &str, block: &tgi::Block, e: &tgi::Error) -> String {
    let kind = block.kind().unwrap_or("");
    let line = e.line.unwrap_or(block.line);
    format!("{file}:{line}: {kind} block doesn't read: {}", e.message)
}

/// The files, in the order they load, that don't fail to, with the
/// templates each sees.
pub(crate) fn scoped<P: Borrow<tgi::Parsed>>(files: &[(String, P)]) -> Vec<Scoped<'_>> {
    let scopes = tgi::Templates::scoped(files.iter().map(|(_, p)| &p.borrow().root));
    (files.iter().zip(scopes))
        .filter_map(|((file, parsed), templates)| {
            Some(Scoped {
                file,
                root: &parsed.borrow().root,
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
        let entries: Vec<Vec<Entry>> = archives.iter().map(|a| a.entries.clone()).collect();
        let mut paths = HashMap::new();
        for (a, entries) in entries.iter().enumerate() {
            for (i, e) in entries.iter().enumerate() {
                paths.entry(e.name.to_ascii_lowercase()).or_insert((a, i));
            }
        }
        let mut game = GameData {
            archives: archives.into_iter().map(Mutex::new).collect(),
            entries,
            paths,
            tgis: Vec::new(),
            tag: None,
            parsed: Mutex::default(),
            tables: Mutex::default(),
        };
        let names: Vec<String> = game.entries().map(str::to_string).collect();
        game.tag = game_tag(&names);
        let tgis = listed_tgis(&names, |name| game.read(name).map(|b| tgi::decode(&b)));
        game.tgis = tgis;
        Ok(game)
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        let (a, i) = *self.paths.get(&name.to_ascii_lowercase())?;
        locked(&self.archives[a]).read(&self.entries[a][i]).ok()
    }

    /// Every entry's path, in archive order.
    fn entries(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().flatten().map(|e| e.name.as_str())
    }

    pub(crate) fn entry_name(&self, (a, i): EntryRef) -> &str {
        &self.entries[a][i].name
    }

    /// A `.tgi` file, parsed the first time it's asked for and kept.
    pub(crate) fn parsed(&self, name: &str) -> Option<Arc<tgi::Parsed>> {
        let key = name.to_ascii_lowercase();
        if let Some(parsed) = locked(&self.parsed).get(&key) {
            return Some(parsed.clone());
        }
        // Parsed without the lock, so other threads can read other files.
        let parsed = Arc::new(tgi::parse_bytes(&self.read(name)?));
        Some(locked(&self.parsed).entry(key).or_insert(parsed).clone())
    }

    /// The `.tgi` files named, parsed, in the order given, leaving out any
    /// that aren't there.
    pub(crate) fn parse_named<'n>(
        &self,
        names: impl IntoIterator<Item = &'n str>,
    ) -> Vec<(String, Arc<tgi::Parsed>)> {
        (names.into_iter())
            .filter_map(|n| Some((n.to_string(), self.parsed(n)?)))
            .collect()
    }

    /// A file every game has, lower-case, after the game's own version of
    /// it, which takes precedence: `Game/SVars.tgi` gives
    /// `game/svars_aa.tgi` then `game/svars.tgi` for AA.
    pub(crate) fn versions(&self, file: &str) -> Vec<String> {
        let file = file.to_ascii_lowercase();
        let own = self.tag.as_ref().and_then(|tag| {
            let (stem, ext) = file.rsplit_once('.')?;
            Some(format!("{stem}_{tag}.{ext}"))
        });
        own.into_iter().chain([file]).collect()
    }

    /// The world rule `ids`, from the game's own world rules before the
    /// ones every game has.
    pub(crate) fn world_rule(&self, ids: &str) -> Option<defs::WorldRule> {
        let versions = self.versions("Game/world_rules.tgi");
        let at = |n: &str| versions.iter().position(|v| v.eq_ignore_ascii_case(n));
        // The templates are there for the rules to inherit from.
        let files = self.parse_tgis(|n| at(n).is_some() || n.starts_with("templates/"));
        let mut files: Vec<_> = (scoped(&files).into_iter())
            .filter_map(|f| Some((at(f.file)?, f)))
            .collect();
        files.sort_by_key(|&(at, _)| at);
        (files.iter())
            .flat_map(|(_, f)| f.read::<defs::WorldRule>("WorldRule"))
            .find(|r| (r.ids.as_ref()).is_some_and(|id| id.as_str().eq_ignore_ascii_case(ids)))
    }

    /// What `build` makes, made the first time `key` is asked for and kept.
    /// Each key is always built as the same type. Built without holding
    /// any lock, as building one table often asks for others; two threads
    /// asking at once may both build it, and the first one kept wins.
    pub(crate) fn kept<T: Send + Sync + 'static>(
        &self,
        key: &'static str,
        build: impl FnOnce() -> T,
    ) -> Arc<T> {
        let found = locked(&self.tables).get(key).cloned();
        let kept = found.unwrap_or_else(|| {
            let built: Arc<dyn Any + Send + Sync> = Arc::new(build());
            locked(&self.tables).entry(key).or_insert(built).clone()
        });
        kept.downcast()
            .unwrap_or_else(|_| panic!("{key} kept as another type"))
    }

    pub fn exists(&self, name: &str) -> bool {
        self.paths.contains_key(&name.to_ascii_lowercase())
    }

    /// Archive paths whose lower-case form passes `filter`.
    pub(crate) fn entry_names(&self, filter: impl Fn(&str) -> bool) -> Vec<String> {
        self.entries()
            .filter(|n| filter(&n.to_ascii_lowercase()))
            .map(str::to_string)
            .collect()
    }

    /// Every `.tgi` file the game loads whose lower-case path passes
    /// `filter`, parsed, in the order the game loads them.
    pub(crate) fn parse_tgis(
        &self,
        filter: impl Fn(&str) -> bool,
    ) -> Vec<(String, Arc<tgi::Parsed>)> {
        let names = (self.tgis.iter()).filter(|n| filter(&n.to_ascii_lowercase()));
        self.parse_named(names.map(String::as_str))
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

    pub fn open_map(&self, name: &str) -> Result<Map<Cursor<Vec<u8>>>, String> {
        let bytes = self
            .read(name)
            .ok_or_else(|| format!("{name}: not found"))?;
        Map::new(Cursor::new(bytes)).map_err(|e| format!("{name}: {e}"))
    }

    /// Whether it's night under a sun of this diffuse colour; never, if the
    /// settings don't say how dark night is.
    pub fn night(&self, [r, g, b]: [f32; 3]) -> bool {
        let svars = self.svars();
        let threshold = svars.iter().find_map(|v| v.sun_night_threshold);
        threshold.is_some_and(|t| r + g + b < t)
    }

    /// Seconds an enemy's damage keeps what it hurt under siege.
    pub fn siege_seconds(&self) -> Option<f32> {
        let svars = self.svars();
        svars
            .iter()
            .find_map(|v| v.under_siege_recently_damaged_seconds)
    }

    /// How far around them city centres hold sway, by template id, including
    /// the ids the game has since renamed.
    pub fn city_zones(&self) -> HashMap<Id, f32> {
        // A city's other buildings have the component with no zone.
        self.by_template(|t| {
            let zone = t.city_component.as_ref()?.zone_radius;
            zone.filter(|&r| r > 0.0)
        })
    }

    /// The organization templates that fly no banner, such as animal herds:
    /// part of the scenery, which players can't pick.
    pub fn scenery(&self) -> HashSet<Id> {
        self.by_template(|t| {
            let icon = t.view.as_ref().and_then(|v| v.banner_icon.as_deref());
            (t.organization_component.is_some() && icon.and_then(ui_path).is_none()).then_some(())
        })
        .into_keys()
        .collect()
    }

    /// What `of` finds in each template that has it, by template id,
    /// including the ids the game has since renamed.
    pub(crate) fn by_template<T: Clone>(
        &self,
        of: impl Fn(&defs::Thing) -> Option<T>,
    ) -> HashMap<Id, T> {
        let mut out = HashMap::new();
        for t in &self.definitions().things {
            if let Some(v) = of(&t.thing) {
                out.insert(t.id.clone(), v);
            }
        }
        add_renames(&mut out, &self.translations(), "Thing");
        out
    }

    /// The renames `translate.tgi` lists, read once.
    pub(crate) fn translations(&self) -> Arc<Vec<defs::Translate>> {
        self.kept("translations", || {
            let text = self.read("translate.tgi").map(|b| tgi::decode(&b));
            translations(text.as_deref().unwrap_or(""))
        })
    }

    /// Every `.tgi` file's [`Definitions`]: there are thousands of
    /// templates and much of the game asks after them.
    pub(crate) fn definitions(&self) -> Arc<Definitions> {
        self.kept("definitions", || {
            Definitions::new(&self.parse_tgis(|_| true))
        })
    }

    /// How much of the `.tgi` data the definitions read: each top-level
    /// block is read as every type its kind is read as, or counted as unread
    /// where there is none. Templates count through the blocks that inherit
    /// from them.
    pub fn coverage(&self) -> tgi::Coverage {
        let files = self.parse_tgis(|_| true);
        let files = scoped(&files);
        let mut coverage = tgi::Coverage::default();
        for (block, t) in
            (files.iter()).flat_map(|f| f.root.blocks().map(|b| (b, Some(&f.templates))))
        {
            if !block.kind().unwrap_or("").eq_ignore_ascii_case("template") {
                reads::count(&mut coverage, block, t);
            }
        }
        coverage
    }

    /// The lighting presets the player can pick from, by id.
    pub fn sun_presets(&self) -> Vec<defs::Sun> {
        let files = self.parse_tgis(|n| n.starts_with("sun/"));
        let files = scoped(&files);
        let mut out: Vec<defs::Sun> = files.iter().flat_map(|f| f.read("Sun")).collect();
        out.sort_by(|a, b| a.ids.cmp(&b.ids));
        out
    }

    /// How far beyond a blocking building's footprint the ground must be
    /// clear for it to be put there, in world units.
    pub fn building_padding(&self) -> Option<f32> {
        self.svars().iter().find_map(|v| v.building_padding_meters)
    }

    /// The templates whose things each side numbers.
    pub fn numbered(&self) -> HashSet<Id> {
        let numbered = self.by_template(|t| t.numbered.filter(|&n| n));
        numbered.into_keys().collect()
    }
}

/// Gives the old ids that maps may still use the entries of their
/// replacements, as `translate.tgi` lists them for `node`, e.g. `Thing`:
/// one step on, so an id renamed to one that's itself only a rename has
/// none, and in place of any entry still made under the old id.
pub(crate) fn add_renames<T: Clone>(
    entries: &mut HashMap<Id, T>,
    translations: &[defs::Translate],
    node: &str,
) {
    let renamed: Vec<(Id, Option<T>)> = renames(translations, node)
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

/// The renames a `translate.tgi` text lists.
fn translations(text: &str) -> Vec<defs::Translate> {
    (tgi::parse(text).root.blocks_of("Translate"))
        .filter_map(|t| logged("translate.tgi", t, reads::decode(t, None)))
        .collect()
}

/// Old ids of `node`s that maps may still use, to their replacements, from
/// `translate.tgi`. Where an id is renamed more than once the first counts.
pub(crate) fn renames(translations: &[defs::Translate], node: &str) -> HashMap<Id, Id> {
    let mut out = HashMap::new();
    for t in translations {
        if !t
            .node
            .as_deref()
            .is_some_and(|n| n.trim().eq_ignore_ascii_case(node))
        {
            continue;
        }
        if let (Some(old), Some(new)) = (&t.ids_old, &t.ids_new) {
            out.entry(old.clone()).or_insert(new.clone());
        }
    }
    out
}

/// The id the game looks `id` up by.
pub(crate) fn renamed(renames: &HashMap<Id, Id>, id: &Id) -> Id {
    renames.get(id).unwrap_or(id).clone()
}

/// What a game names its own versions of the files every game has with:
/// the part after `index_` of a load list at the top of its archives, such
/// as `aa` for `index_aa.lst`. Where there are several such lists, the one
/// whose tag the game's own tuning file carries. None where there's no such
/// list.
fn game_tag(entries: &[String]) -> Option<String> {
    let mut tags: Vec<String> = (entries.iter())
        .filter(|n| !n.contains('/'))
        .filter_map(|n| {
            let lower = n.to_ascii_lowercase();
            let tag = lower.strip_prefix("index_")?.strip_suffix(".lst")?;
            (!tag.is_empty()).then(|| tag.to_string())
        })
        .collect();
    tags.sort_unstable();
    tags.dedup();
    if tags.len() > 1 {
        let own = |tag: &String| {
            let svars = format!("game/svars_{tag}.tgi");
            entries.iter().any(|n| n.eq_ignore_ascii_case(&svars))
        };
        let owned: Vec<String> = tags.iter().filter(|t| own(t)).cloned().collect();
        if !owned.is_empty() {
            tags = owned;
        }
        if tags.len() > 1 {
            log::warn!("load lists for more than one game: {}", tags.join(", "));
        }
    }
    tags.into_iter().next()
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

pub(crate) fn not_null(id: Option<Id>) -> Option<Id> {
    id.filter(|id| !id.as_str().eq_ignore_ascii_case("null"))
}

/// A path a file gives relative to its own folder, unless it starts with `/`.
/// The archive path of an icon a block in `file` names, if it names one.
pub(crate) fn icon_beside(file: &str, icon: Option<&str>) -> Option<String> {
    let icon = icon?.split_whitespace().next()?;
    (!icon.eq_ignore_ascii_case("null")).then(|| beside(file, icon))
}

pub(crate) fn beside(file: &str, path: &str) -> String {
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
pub(crate) fn frames(pattern: &str, exists: impl Fn(&str) -> bool) -> Vec<String> {
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
pub(crate) fn ui_path(path: &str) -> Option<String> {
    let path = path.split_whitespace().next()?;
    (!path.eq_ignore_ascii_case("null"))
        .then(|| path.replace("%s/", "").trim_start_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_that_dont_read_are_left_out_with_a_warning() {
        let files = [(
            "terrain/t.tgi".to_string(),
            tgi::parse(
                "[Terrain]\n{\n\tbase_priority = high\n}\n[Terrain]\n{\n\tbase_priority = 2\n}\n",
            ),
        )];
        let files = scoped(&files);
        let mut read: Vec<defs::Terrain> = Vec::new();
        let logged = warnings(|| read = files[0].read("Terrain").collect());
        assert_eq!(
            read.iter().map(|t| t.base_priority).collect::<Vec<_>>(),
            [2]
        );
        assert_eq!(logged.len(), 1, "{logged:?}");
        assert!(
            logged[0].starts_with("terrain/t.tgi:3: Terrain block doesn't read: "),
            "{}",
            logged[0]
        );
    }

    /// The warnings `f` logs on this thread, which is its own for each test.
    fn warnings(f: impl FnOnce()) -> Vec<String> {
        use std::cell::RefCell;

        thread_local! {
            static LOGGED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        }
        struct Capture;
        impl log::Log for Capture {
            fn enabled(&self, metadata: &log::Metadata) -> bool {
                metadata.level() <= log::Level::Warn
            }
            fn log(&self, record: &log::Record) {
                LOGGED.with(|l| l.borrow_mut().push(record.args().to_string()));
            }
            fn flush(&self) {}
        }
        static INSTALL: std::sync::Once = std::sync::Once::new();
        INSTALL.call_once(|| {
            log::set_logger(&Capture).unwrap();
            log::set_max_level(log::LevelFilter::Warn);
        });
        LOGGED.with(|l| l.borrow_mut().clear());
        f();
        LOGGED.with(|l| l.take())
    }

    /// Game data with no archives, for what doesn't read any.
    fn empty() -> GameData {
        GameData {
            archives: Vec::new(),
            entries: Vec::new(),
            paths: HashMap::new(),
            tgis: Vec::new(),
            tag: None,
            parsed: Mutex::default(),
            tables: Mutex::default(),
        }
    }

    #[test]
    fn a_games_own_files_are_named_by_its_load_list() {
        let names = |of: &[&str]| of.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        let tag = game_tag(&names(&[
            "Game/index_x.lst",
            "templates.lst",
            "INDEX_K2.lst",
        ]));
        assert_eq!(tag.as_deref(), Some("k2"));
        assert_eq!(game_tag(&names(&["index.lst"])), None);
        // Another list beside the game's own doesn't take its place.
        let beside = names(&["index_addon.lst", "index_zz.lst", "Game/SVars_ZZ.tgi"]);
        assert_eq!(game_tag(&beside).as_deref(), Some("zz"));
        let mut game = empty();
        assert_eq!(game.versions("Game/SVars.tgi"), ["game/svars.tgi"]);
        game.tag = tag;
        assert_eq!(
            game.versions("Game/SVars.tgi"),
            ["game/svars_k2.tgi", "game/svars.tgi"]
        );
    }

    #[test]
    fn tables_are_built_once_and_shared() {
        let game = empty();
        let builds = std::cell::Cell::new(0);
        let build = || {
            builds.set(builds.get() + 1);
            vec![1, 2]
        };
        let first = game.kept("numbers", build);
        let again = game.kept("numbers", build);
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(builds.get(), 1);
        // Another key is another table.
        assert_eq!(*game.kept("other", || vec![3]), [3]);
        assert_eq!(game.parsed("absent.tgi"), None);
    }

    #[test]
    #[should_panic(expected = "numbers kept as another type")]
    fn a_key_kept_as_another_type_panics() {
        let game = empty();
        game.kept("numbers", || vec![1]);
        game.kept("numbers", || "one");
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
    fn settings_read_whether_objectives_sort_and_teams_win_together() {
        let parsed = tgi::parse(
            "[SVars]\n{\n\tbool SortObjectives = false\n\tflag TeamVictory = false\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("SVars").next().unwrap();
        let svars: defs::SVars = templates.deserialize(block).unwrap();
        assert_eq!(svars.sort_objectives, Some(false));
        assert_eq!(svars.team_victory, Some(false));
    }

    #[test]
    fn settings_read_how_dark_night_is() {
        let parsed = tgi::parse("[SVars]\n{\n\tSunNightThreshold = 1.6\n}\n");
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("SVars").next().unwrap();
        let svars: defs::SVars = templates.deserialize(block).unwrap();
        assert_eq!(svars.sun_night_threshold, Some(1.6));
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
        add_renames(&mut entries, &translations(&translate), "Thing");
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
    fn units_and_projectiles_are_things() {
        let a = tgi::parse(
            "[Unit]\n{\n\tIDS = bowman\n}\n[Projectile]\n{\n\tIDS = arrow\n}\n\
             [Unit]\n{\n\tIDS = bowman\n}\n",
        );
        let files = [("a/a.tgi".to_string(), a)];
        let definitions = Definitions::new(&files);
        let found: Vec<_> = definitions.things.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(found, ["bowman", "arrow"]);
    }
}
