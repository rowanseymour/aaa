//! Access to the game's data archive: maps, terrain definitions and textures.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};

use tgck::{Archive, Map};

pub struct Game {
    data: Archive<BufReader<File>>,
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
        Ok(Game { data })
    }

    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        let entry = self.data.find(name)?.clone();
        self.data.read(&entry).ok()
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
