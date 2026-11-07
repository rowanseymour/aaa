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
    /// The outline under which the ground is sampled to tilt the model to
    /// follow it, as vehicles do; `None` for models that stay upright.
    pub conform: Option<Shape>,
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

/// How a template's objects fight: what they can take and what they deal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fighter {
    pub health: f32,
    pub defense: f32,
    /// Whoever brings its health to nothing takes it over, unless it's a
    /// city's centre, which changes hands by who holds its zone instead.
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
    /// It's on a turret, which aims it without turning its user, so it can
    /// fire while its user moves. Others fire only when it stands.
    pub turret: bool,
}

/// Which flags a target must have, and must not, for a weapon to fire on
/// it. Besides a fighter's own [`Fighter::flags`], it has `enemy` or
/// `friend` by its side.
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

    /// How the objects of every template that can be hurt fight, by
    /// template id, including the ids the game has since renamed.
    pub fn fighters(&mut self) -> HashMap<Id, Fighter> {
        let files = self.parse_tgis(|_| true);
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let properties: HashMap<Id, defs::Property> = files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("Property"))
            .filter_map(|b| templates.deserialize::<defs::Property>(b).ok())
            .filter_map(|p| Some((p.ids.clone()?, p)))
            .collect();
        let mut out = HashMap::new();
        for (_, parsed) in &files {
            for block in parsed.root.blocks_of("Thing") {
                let Some(id) = block.value("ids") else {
                    continue;
                };
                let Ok(thing) = templates.deserialize::<defs::Thing>(block) else {
                    continue;
                };
                if let Some(f) = fighter(&thing, &properties) {
                    out.insert(Id::from(id), f);
                }
            }
        }
        if let Some(bytes) = self.read("translate.tgi") {
            add_renames(&mut out, &tgi::decode(&bytes), "Thing", |_| true);
        }
        out
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
        self.by_template(|t| {
            let icon = t.view.as_ref()?.banner_icon.as_deref()?;
            Some(icon.trim_start_matches('/').to_string())
        })
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

    /// How far from where they stand organizations take on enemies that
    /// fire on them, by template id, including the ids the game has since
    /// renamed.
    pub fn operational_ranges(&mut self) -> HashMap<Id, f32> {
        self.by_template(|t| t.organization_component.as_ref()?.operational_range)
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

    /// What `of` finds in each template that has it, by template id,
    /// including the ids the game has since renamed.
    fn by_template<T: Clone>(&mut self, of: impl Fn(&defs::Thing) -> Option<T>) -> HashMap<Id, T> {
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
                if let Some(v) = of(&thing) {
                    out.insert(Id::from(id), v);
                }
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
            conform: conform(thing),
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
    Body {
        blocking: blocking.then_some(shape),
        speed: thing
            .mover_component
            .as_ref()
            .and_then(|m| m.speed_max)
            .filter(|&s| s > 0.0),
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
                conform: None,
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
        let layout = layout(&l).unwrap();
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
