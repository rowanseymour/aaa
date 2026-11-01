//! Access to the game's data archive: maps, terrain definitions, object
//! templates, models and textures.

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
    /// Water surface colour, for water terrains.
    pub water_color: Option<[f32; 3]>,
    /// Higher priority terrain is drawn over lower where they meet.
    pub priority: i32,
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
        let names = self.map_names();
        let stem = |n: &str| {
            let file = n.rsplit('/').next().unwrap_or(n);
            file[..file.len() - 4].to_string()
        };
        names
            .iter()
            .find(|n| n.eq_ignore_ascii_case(name) || stem(n).eq_ignore_ascii_case(name))
            .map(|n| n.to_string())
    }

    pub fn open_map(&mut self, name: &str) -> Result<Map<Cursor<Vec<u8>>>, String> {
        let bytes = self
            .read(name)
            .ok_or_else(|| format!("{name}: not found"))?;
        Map::new(Cursor::new(bytes)).map_err(|e| format!("{name}: {e}"))
    }

    /// Reads every terrain definition and indexes the variations by
    /// lower-case IDS, which is how maps refer to them.
    pub fn terrain_variations(&mut self) -> HashMap<String, Variation> {
        let files: Vec<String> = self
            .data
            .entries
            .iter()
            .map(|e| e.name.clone())
            .filter(|n| {
                let n = n.to_ascii_lowercase();
                n.starts_with("terrain/") && n.ends_with(".tgi")
            })
            .collect();
        let mut out = HashMap::new();
        for file in files {
            let Some(bytes) = self.read(&file) else {
                continue;
            };
            let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
            let parsed = tgi::parse_bytes(&bytes);
            for terrain in parsed.root.blocks_of("Terrain") {
                let terrain_color = terrain.value("terrain_color").and_then(floats);
                let water_color = terrain.value("color").and_then(floats);
                let priority = terrain
                    .value("base_priority")
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);
                for v in terrain.blocks_of("Variation") {
                    let (Some(ids), Some(texture)) = (v.value("ids"), v.value("texture")) else {
                        continue;
                    };
                    let size = |key| v.value(key).and_then(|s| s.trim().parse().ok());
                    out.insert(
                        ids.to_ascii_lowercase(),
                        Variation {
                            texture: format!("{dir}/{}", texture.trim_start_matches('/')),
                            repeat: [
                                size("texture_width").unwrap_or(8.0),
                                size("texture_height").unwrap_or(8.0),
                            ],
                            color: v
                                .value("terrain_color")
                                .and_then(floats)
                                .or(terrain_color)
                                .unwrap_or([0.5; 3]),
                            water_color,
                            priority,
                        },
                    );
                }
            }
        }
        out
    }

    /// The views of all object templates that name a model, by lower-case
    /// template id.
    pub fn views(&mut self) -> HashMap<String, View> {
        let files: Vec<String> = self
            .data
            .entries
            .iter()
            .map(|e| e.name.clone())
            .filter(|n| n.to_ascii_lowercase().ends_with(".tgi"))
            .collect();
        let mut out = HashMap::new();
        for file in files {
            let Some(bytes) = self.read(&file) else {
                continue;
            };
            let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
            let parsed = tgi::parse_bytes(&bytes);
            for thing in parsed.root.blocks_of("Thing") {
                let (Some(id), Some(view)) = (thing.value("ids"), thing.blocks_of("View").next())
                else {
                    continue;
                };
                if let Some(v) = self.view(dir, thing, view) {
                    out.insert(id.to_ascii_lowercase(), v);
                }
            }
        }
        out
    }

    /// A template's models: its aspects when they are variations, else its
    /// normal model, else whatever aspects it lists.
    fn view(&self, dir: &str, thing: &tgi::Block, view: &tgi::Block) -> Option<View> {
        let prefix = view.value("resource_prefix").unwrap_or("");
        let path = |m: &str| {
            let m = m.replace("%s", prefix).replace('\\', "/");
            match m.strip_prefix('/') {
                Some(abs) => abs.to_string(),
                None => format!("{dir}/{m}"),
            }
        };
        let aspects: Vec<String> = view
            .blocks_of("Aspect")
            .filter_map(|a| a.value("model"))
            .map(path)
            .filter(|p| self.exists(p))
            .collect();
        let normal = view
            .blocks_of("AspectNormal")
            .find_map(|a| a.value("model"))
            .map(path)
            .or_else(|| (!prefix.is_empty()).then(|| path("%s.nif")))
            .filter(|p| self.exists(p));
        let variations = view
            .value("aspects_as_variations")
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"));
        let models = match normal {
            Some(n) if !variations => vec![n],
            _ => aspects,
        };
        if models.is_empty() {
            return None;
        }
        let texture_dirs = view
            .blocks_of("TextureSearchPath")
            .filter_map(|b| b.value("path"))
            .map(|p| p.trim_matches('/').to_string())
            .collect();
        let texture_renames = view
            .blocks()
            .flat_map(|b| b.blocks_of("TextureRename"))
            .filter_map(|r| {
                Some((
                    r.value("substring_from")?.to_string(),
                    r.value("substring_to")?.to_string(),
                ))
            })
            .collect();
        let size = |key| {
            thing
                .blocks_of("Size")
                .find_map(|s| s.value(key))
                .and_then(|v| v.trim().parse::<f32>().ok())
        };
        let scale = match (size("scale_min"), size("scale_max")) {
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

    /// The transition masks of the first transition set, keyed by the
    /// digits in their file names.
    pub fn transition_masks(&mut self) -> Vec<(String, Vec<u8>)> {
        const DIR: &str = "Terrain/Transitions/";
        let Some(prefix) = self
            .read(&format!("{DIR}transition_sets.tgi"))
            .and_then(|b| {
                let parsed = tgi::parse_bytes(&b);
                let set = parsed.root.blocks_of("TransitionSet").next()?;
                set.value("path_prefix").map(|p| format!("{DIR}{p}"))
            })
        else {
            return Vec::new();
        };
        let names: Vec<String> = self
            .data
            .entries
            .iter()
            .map(|e| e.name.clone())
            .filter(|n| n.len() > prefix.len() && n[..prefix.len()].eq_ignore_ascii_case(&prefix))
            .collect();
        names
            .into_iter()
            .filter_map(|n| {
                let digits = n[prefix.len()..].split('.').next()?.to_string();
                Some((digits, self.read(&n)?))
            })
            .collect()
    }
}

/// Parses `0.451, 0.604, 0.294`.
fn floats(s: &str) -> Option<[f32; 3]> {
    let v: Vec<f32> = s
        .split(',')
        .map(|p| p.trim().parse())
        .collect::<Result<_, _>>()
        .ok()?;
    v.try_into().ok()
}

fn find_case_insensitive(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}
