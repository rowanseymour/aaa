//! The game's data as a whole: access to the installed archive, and the
//! definitions built from its files (terrain, object views, ...).

pub mod defs;
mod id;

pub use id::Id;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};

use tgck::{Archive, Map};

pub struct GameData {
    data: Archive<BufReader<File>>,
    /// Lower-case path to entry index.
    paths: HashMap<String, usize>,
    /// Lower-case file name to entry indices.
    files: HashMap<String, Vec<usize>>,
}

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
}

/// How an organization's members can be arranged.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The formations it can take, in the order the layout lists them.
    pub formations: Vec<Formation>,
    /// Index into `formations` of the one it starts in.
    pub default: usize,
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
}

/// An outline centred on an object, in world units at its template's size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    Point,
    Circle {
        radius: f32,
    },
    /// Sides along the model's x and y axes, which turn with the object.
    Rect {
        width: f32,
        height: f32,
    },
}

impl Shape {
    pub fn scaled(self, by: f32) -> Shape {
        match self {
            Shape::Point => Shape::Point,
            Shape::Circle { radius } => Shape::Circle {
                radius: radius * by,
            },
            Shape::Rect { width, height } => Shape::Rect {
                width: width * by,
                height: height * by,
            },
        }
    }
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

impl GameData {
    /// Opens `Data.rwd` in the game's install directory.
    pub fn open(dir: &Path) -> Result<Self, String> {
        let path = find_case_insensitive(dir, "Data.rwd")
            .ok_or_else(|| format!("no Data.rwd in {}", dir.display()))?;
        let data = Archive::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut paths = HashMap::new();
        let mut files: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, e) in data.entries.iter().enumerate() {
            let lower = e.name.to_ascii_lowercase();
            let file = lower.rsplit('/').next().unwrap_or(&lower).to_string();
            files.entry(file).or_default().push(i);
            paths.insert(lower, i);
        }
        Ok(GameData { data, paths, files })
    }

    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        let i = *self.paths.get(&name.to_ascii_lowercase())?;
        let entry = self.data.entries[i].clone();
        self.data.read(&entry).ok()
    }

    pub fn exists(&self, name: &str) -> bool {
        self.paths.contains_key(&name.to_ascii_lowercase())
    }

    /// Archive paths whose lower-case form passes `filter`.
    fn entry_names(&self, filter: impl Fn(&str) -> bool) -> Vec<String> {
        self.data
            .entries
            .iter()
            .map(|e| e.name.clone())
            .filter(|n| filter(&n.to_ascii_lowercase()))
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
            .map(|&i| self.data.entries[i].name.as_str())
            .max_by_key(|p| shared(&p.to_ascii_lowercase()))
            .map(str::to_string)
    }

    /// Archive paths of all maps, sorted.
    pub fn map_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .data
            .entries
            .iter()
            .map(|e| e.name.as_str())
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
        let files = self.parse_tgis(|_| true);
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let mut out = HashMap::new();
        for (file, parsed) in &files {
            let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
            for block in parsed.root.blocks_of("Thing") {
                let Some(id) = block.value("ids") else {
                    continue;
                };
                let Ok(thing) = templates.deserialize::<defs::Thing>(block) else {
                    continue;
                };
                let invisible = thing.flags.iter().any(|f| f.invisible);
                let view = match &thing.view {
                    _ if invisible => None,
                    Some(view) => self.view(dir, &thing, view),
                    None => continue,
                };
                // Model paths are relative to the defining file, so where an
                // id is defined more than once keep a definition that finds
                // its models.
                let drawable = |v: &Option<View>| v.as_ref().is_some_and(|v| !v.models.is_empty());
                let id = Id::from(id);
                if drawable(&view) || !out.get(&id).is_some_and(drawable) {
                    out.insert(id, view);
                }
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
        let files = self.parse_tgis(|_| true);
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let mut out = HashMap::new();
        for (_, parsed) in &files {
            for block in parsed.root.blocks_of("Thing") {
                let Some(id) = block.value("ids") else {
                    continue;
                };
                let Ok(thing) = templates.deserialize::<defs::Thing>(block) else {
                    continue;
                };
                out.insert(Id::from(id), body(&thing));
            }
        }
        if let Some(bytes) = self.read("translate.tgi") {
            add_renames(&mut out, &tgi::decode(&bytes), "Thing", |_| true);
        }
        out
    }

    /// The organization layouts, by id.
    pub fn layouts(&mut self) -> HashMap<Id, Layout> {
        let files =
            self.parse_tgis(|n| n.starts_with("organizations/") || n.starts_with("templates/"));
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("OrganizationLayout"))
            .filter_map(|b| templates.deserialize::<defs::OrganizationLayout>(b).ok())
            .filter_map(|l| Some((l.ids.clone()?, layout(&l)?)))
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
        let aspects: Vec<String> = aspects.into_iter().filter(|p| self.exists(p)).collect();
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
                _ => {
                    coverage.skip(block, t);
                    continue;
                }
            };
        }
        coverage
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
fn layout(l: &defs::OrganizationLayout) -> Option<Layout> {
    let formations: Vec<Formation> = l
        .formation
        .iter()
        .filter_map(|f| {
            Some(Formation {
                id: f.formation_ids.clone()?,
                slots: slot_positions(&l.slot, f)?,
            })
        })
        .collect();
    // Layouts may inherit a default they don't define.
    let default = l
        .default_formation_ids
        .as_ref()
        .and_then(|id| formations.iter().position(|f| &f.id == id))
        .unwrap_or(0);
    (!formations.is_empty()).then(|| Layout {
        formations,
        default,
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
    let size = |f: fn(&defs::Size) -> Option<f32>| thing.size.iter().rev().find_map(f);
    let kind = thing
        .size
        .iter()
        .rev()
        .find_map(|s| s.blocking_shape.as_deref())
        .unwrap_or("CIRCLE");
    let shape = match kind.to_ascii_uppercase().as_str() {
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
            radius: size(|s| s.blocking_radius).unwrap_or(0.2),
        },
    };
    Body {
        blocking: blocking.then_some(shape),
        speed: thing
            .mover_component
            .as_ref()
            .and_then(|m| m.speed_max)
            .filter(|&s| s > 0.0),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
             \t[Slot]\n\tproperty_IDS = front\n\tnumber = 2\n\
             \t[Slot]\n\tproperty_IDS = captain\n\tprimary = true\n\
             \t[Formation]\n\t{\n\t\tformation_IDS = line\n\
             \t\t[Position]\n\t\tproperty_IDS = captain\n\t\toffset = 0,0\n\
             \t\t[Position]\n\t\tproperty_IDS = front\n\t\toffset = -1,1\n\
             \t\t[Position]\n\t\tproperty_IDS = front\n\t\toffset = 1,1\n\t}\n}\n",
        );
        let block = parsed.root.blocks_of("OrganizationLayout").next().unwrap();
        let l: defs::OrganizationLayout = tgi::from_block(block).unwrap();
        let layout = layout(&l).unwrap();
        assert_eq!(layout.default, 0);
        assert_eq!(layout.primary, Some(1));
        assert_eq!(layout.formations[0].id, Id::from("line"));
        assert_eq!(
            layout.formations[0].slots,
            [vec![[-1.0, 1.0], [1.0, 1.0]], vec![[0.0, 0.0]]]
        );
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
