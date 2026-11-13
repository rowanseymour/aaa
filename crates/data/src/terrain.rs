//! What terrain does to those standing on it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::supply::{is, listed};
use crate::{GameData, Id, Scoped, add_renames, defs, logged, not_null, scoped};

/// What a property a terrain gives does.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerrainEffect {
    /// The properties a thing needs to gain it.
    pub requires: Vec<Id>,
    /// What it adds to its holder's defence.
    pub defense: f32,
    /// The fraction of each damage type's damage it turns aside from its
    /// holder; negative where it adds to it.
    pub resists: Vec<(Id, f32)>,
    /// The speed limit it gives its terrain, for everyone on it.
    pub speed_limit: Option<f32>,
}

/// The settings terrain bears on those standing on it by.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainRules {
    /// The lowest a terrain's speed limit comes to.
    pub slowest: f32,
    /// What each shared property does when a terrain gives it, by its id.
    pub effects: HashMap<Id, TerrainEffect>,
    /// The properties whose holders no terrain's speed limit holds back.
    pub unlimited: Vec<Id>,
    /// The templates with such a property, shared or their own.
    pub free: HashSet<Id>,
}

impl Default for TerrainRules {
    fn default() -> Self {
        TerrainRules {
            slowest: 0.5,
            effects: HashMap::new(),
            unlimited: Vec::new(),
            free: HashSet::new(),
        }
    }
}

fn trailblazing(p: &defs::Property) -> bool {
    (p.modifier.iter()).any(|m| is(&m.modifier_flag, "TRAILBLAZING"))
}

fn effect(p: &defs::Property) -> TerrainEffect {
    let mut out = TerrainEffect {
        requires: listed(p.required_properties.as_deref()),
        ..TerrainEffect::default()
    };
    for m in &p.modifier {
        let amount = m.amount.unwrap_or(0.0);
        if is(&m.kind, "ADD") && is(&m.attribute, "DV") {
            out.defense += amount;
        }
        if is(&m.kind, "ADD") && is(&m.attribute, "TERRAIN_SPEED_CAP") && amount > 0.0 {
            out.speed_limit = Some(amount);
        }
        if let (true, Some(kind)) = (is(&m.kind, "RESIST_DAMAGE"), &m.damage_type) {
            out.resists.push((kind.clone(), amount));
        }
    }
    out
}

impl GameData {
    pub fn terrain_rules(&self) -> TerrainRules {
        let slowest = (self.svars().iter()).find_map(|v| v.terrain_minimum_move_speed);
        let definitions = self.definitions();
        let shared = definitions.properties.iter();
        let effects = shared.map(|(id, p)| (id.clone(), effect(p))).collect();
        let mut unlimited: Vec<Id> = (definitions.properties.values())
            .filter(|p| trailblazing(p))
            .filter_map(|p| p.ids.clone())
            .collect();
        unlimited.sort();
        let free = self.by_template(|t| {
            let c = t.properties_component.as_ref()?;
            let refs = c.property.iter().filter_map(|r| r.property_ids.as_ref());
            let shared = refs.filter_map(|p| definitions.properties.get(p));
            (shared.chain(&c.intrinsic_property))
                .any(trailblazing)
                .then_some(())
        });
        TerrainRules {
            slowest: slowest.unwrap_or(TerrainRules::default().slowest),
            effects,
            unlimited,
            free: free.into_keys().collect(),
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
    pub kind: TerrainKind,
    /// What effects take it for, e.g. `dirt`, `snow` or `water`.
    pub group: Option<Id>,
    /// The properties it gives those standing on it, as its terrain
    /// lists them.
    pub properties: Vec<Id>,
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

impl GameData {
    /// Reads every terrain definition and indexes the variations by IDS,
    /// which is how maps refer to them.
    pub fn terrain_variations(&self) -> Arc<HashMap<Id, Variation>> {
        self.kept("terrain_variations", || self.read_terrain_variations())
    }

    fn read_terrain_variations(&self) -> HashMap<Id, Variation> {
        let files = self.parse_tgis(|n| n.starts_with("terrain/") || n.starts_with("templates/"));
        let files = scoped(&files);
        let water = water_colors(&files);
        let mut out = HashMap::new();
        for scope in &files {
            if !scope.file.to_ascii_lowercase().starts_with("terrain/") {
                continue;
            }
            let dir = scope.file.rsplit_once('/').map_or("", |(d, _)| d);
            for block in scope.root.blocks_of("Terrain") {
                let Some(terrain) = scope.deserialize::<defs::Terrain>(block) else {
                    continue;
                };
                let water = terrain.ids.as_ref().and_then(|id| water.get(id));
                let kind = terrain_kind(&terrain);
                let properties: Vec<Id> = (terrain.property.iter())
                    .filter_map(|r| r.property_ids.clone())
                    .collect();
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
                            properties: properties.clone(),
                        },
                    );
                }
            }
        }
        add_renames(&mut out, &self.translations(), "Variation");
        out
    }

    /// The transition masks of the first transition set, keyed by the
    /// digits in their file names.
    pub fn transition_masks(&self) -> Vec<(String, Vec<u8>)> {
        const DIR: &str = "Terrain/Transitions/";
        let file = format!("{DIR}transition_sets.tgi");
        let Some(prefix) = self.parsed(&file).and_then(|parsed| {
            let set = parsed.root.blocks_of("TransitionSet").next()?;
            let set: defs::TransitionSet = logged(&file, set, tgi::from_block(set))?;
            set.path_prefix.map(|p| format!("{DIR}{p}"))
        }) else {
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

fn terrain_kind(t: &defs::Terrain) -> TerrainKind {
    match t.kind.as_deref().map(str::to_ascii_uppercase).as_deref() {
        Some("MOUNTAIN") => TerrainKind::Mountain {
            noise: [t.noise_min.unwrap_or(0.0), t.noise_max.unwrap_or(0.0)],
        },
        Some("WATER") => TerrainKind::Water,
        _ => TerrainKind::Land,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_properties_cap_speed_and_change_what_hits_do() {
        let parsed = tgi::parse(
            "[template Modifier]\n{\n\tstring type\n\tstring attribute\n\tfixed amount\n\
             \tstring damage_type\n}\n\
             [template AddModifier inherit=Modifier]\n{\n\ttype = ADD\n}\n\
             [template TerrainSpeedModifier template=AddModifier]\n{\n\
             \tattribute = TERRAIN_SPEED_CAP\n}\n\
             [template ResistDamageModifier inherit=Modifier]\n{\n\ttype = RESIST_DAMAGE\n}\n\
             [template Property]\n{\n\tstring IDS\n\tstring required_properties\n\
             \t[list Modifier]\n}\n\
             [template TerrainProperty inherit=Property]\n{\n\trequired_properties = mobile\n}\n\
             [Property template=TerrainProperty]\n{\n\tIDS = forest_infantry\n\
             \trequired_properties = infantry\n\
             \t[Modifier template=TerrainSpeedModifier]\n\tamount = 2.5\n\
             \t[Modifier template=AddModifier]\n\tattribute = DV\n\tamount = 4\n\
             \t[Modifier template=ResistDamageModifier]\n\tdamage_type = armor_piercing\n\
             \tamount = 0.20\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Property").next().unwrap();
        let property = templates.deserialize::<defs::Property>(block).unwrap();
        assert_eq!(
            effect(&property),
            TerrainEffect {
                requires: vec![Id::from("infantry")],
                defense: 4.0,
                resists: vec![(Id::from("armor_piercing"), 0.2)],
                speed_limit: Some(2.5),
            }
        );
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
}
