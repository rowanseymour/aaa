//! How templates are drawn: their models, where the models' textures are
//! found, and their animations.

use std::collections::HashMap;

use crate::{GameData, Id, Shape, Template, add_renames, combat::conform, defs, ui_path};

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

impl GameData {
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

    /// The views of all object templates, by template id,
    /// including the ids the game has since renamed: `None` for templates
    /// that name no model, such as sound emitters, or are invisible.
    /// Visible templates without a view, of their own or inherited, are left
    /// out.
    pub fn views(&self) -> HashMap<Id, Option<View>> {
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
        add_renames(&mut out, &self.translations(), "Thing");
        out
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

/// The id, unless it's the data's word for none.
/// The archive folders a model's textures are searched for in.
pub(crate) fn texture_dirs(paths: &[defs::TextureSearchPath]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|b| b.path.as_deref())
        .map(|p| p.trim_matches('/').to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
