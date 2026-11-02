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

pub struct Game {
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
}

/// One texture variation of a terrain type, e.g. `temperate_grass01`.
#[derive(Debug, Clone)]
pub struct Variation {
    /// Archive path of the texture.
    pub texture: String,
    /// Size of one texture repeat, in tiles.
    pub repeat: [f32; 2],
    /// Average colour, used where the texture is missing.
    pub color: [f32; 3],
    /// How water terrains colour the surface by depth, shallowest first.
    pub water: Option<Vec<DepthColor>>,
    /// Higher priority terrain is drawn over lower where they meet.
    pub priority: i32,
}

/// A water surface's colour and opacity at a depth below it.
#[derive(Debug, Clone, Copy)]
pub struct DepthColor {
    pub depth: f32,
    pub color: [f32; 3],
    pub alpha: f32,
}

impl Game {
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
        Ok(Game { data, paths, files })
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
                let Ok(terrain) = tgi::from_block::<defs::Terrain>(block) else {
                    continue;
                };
                let water = terrain.ids.as_ref().and_then(|id| water.get(id));
                for v in terrain.variation {
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
                                .or(terrain.terrain_color)
                                .unwrap_or([0.5; 3]),
                            water: water.cloned(),
                            priority: terrain.base_priority,
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

    /// A template's models: its aspects when they are variations, else its
    /// normal model, else whatever aspects it lists. Models missing from the
    /// archive are left out, so a view may have none.
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
        Some(View {
            models,
            texture_dirs,
            texture_renames,
            scale,
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
                "terrain" => coverage.read::<defs::Terrain>(block, None).map(drop),
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
}
