//! How templates are drawn: their models, where the models' textures are
//! found, and their animations.

use std::collections::{HashMap, HashSet};

use crate::{
    GameData, Id, Shape, Template, add_renames, beside, combat::conform, defs, not_null,
    terrain::in_subfolder, ui_path,
};

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
    /// Archive path of the model it's drawn with as it falls, under its
    /// parachute.
    pub falling: Option<String>,
    /// Archive path of its picture in the interface, such as a unit's
    /// portrait in its regiment's panel or a building's button.
    pub icon: Option<String>,
    /// The [`Portrait`] the object display shows for it, by id.
    pub portrait: Option<Id>,
    /// Whether the object display shows its owner's general's portrait
    /// rather than its own.
    pub faction_portrait: bool,
    /// How far from its middle its normal model reaches over the ground,
    /// where the data says rather than leaving it to be measured.
    pub ground_reach: Option<f32>,
}

/// A model of a face for the object display to show, by its own camera.
#[derive(Debug, Clone)]
pub struct Portrait {
    /// Its model and where its textures are; its animations are those the
    /// model plays in the display: its `IDLE`, the `FIDGET`s that break
    /// that up and what it plays when `SELECTED`.
    pub view: View,
    /// How many times it idles between fidgets: at least the first, and
    /// fewer than the second.
    pub idle_loops: [u32; 2],
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
    pub kind: AnimationKind,
    /// Which one of its kind, e.g. `idle`, `fidget1`, `walk`.
    pub ids: String,
    /// Archive path of its `.kf` file.
    pub kf: String,
    /// Seconds one play takes; the file's keys are stretched to fit.
    pub length: f32,
    /// For moving animations, how far one play carries the unit.
    pub walk_distance: Option<f32>,
}

/// What an animation is for, from its `type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnimationKind {
    Idle,
    Move,
    /// Played once to use an ability, such as attacking.
    Ability,
    Die,
    /// Breaks up a portrait's idling.
    Fidget,
    /// Played as the thing is selected.
    Selected,
    /// Played after a random wait, over and over.
    Random,
    /// A kind nothing here acts on, uppercased.
    Other(String),
}

impl AnimationKind {
    /// Each named kind, as the data files spell it.
    const NAMES: [(&str, AnimationKind); 7] = [
        ("IDLE", AnimationKind::Idle),
        ("MOVE", AnimationKind::Move),
        ("ABILITY", AnimationKind::Ability),
        ("DIE", AnimationKind::Die),
        ("FIDGET", AnimationKind::Fidget),
        ("SELECTED", AnimationKind::Selected),
        ("RANDOM", AnimationKind::Random),
    ];

    /// From a `type` as the data files spell it, in any case.
    pub fn parse(kind: &str) -> Self {
        let kind = kind.to_ascii_uppercase();
        let named = Self::NAMES.into_iter().find(|(name, _)| *name == kind);
        named.map_or(AnimationKind::Other(kind), |(_, k)| k)
    }
}

/// As the data files spell it.
impl std::fmt::Display for AnimationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let name = match self {
            AnimationKind::Other(kind) => kind,
            named => (Self::NAMES.iter())
                .find(|(_, k)| k == named)
                .map_or("", |(name, _)| *name),
        };
        f.write_str(name)
    }
}

impl GameData {
    /// The archive path of a texture that `model`, one of `view`'s, names
    /// as `file`: beside the model, under the name the model's renames make
    /// of it, else that file name in each of the view's search paths in
    /// turn. `None` where none of them exists, as the game then draws the
    /// default texture.
    ///
    /// On a map whose biome keeps its own files in `subfolder`, each of
    /// those places is looked in there first, and a model that's read from
    /// there itself has its textures beside it there.
    pub fn find_texture(
        &self,
        subfolder: Option<&str>,
        view: &View,
        model: &str,
        file: &str,
    ) -> Option<String> {
        let read_from = self.path_in(subfolder, model);
        let places = texture_paths(view, model, &read_from, file);
        first_in(places, subfolder, |p| self.exists(p))
    }

    /// The archive path `path`'s file is read from on a map whose biome
    /// keeps its own files in `subfolder`: the file of that name there,
    /// beside it, where there is one.
    pub fn path_in(&self, subfolder: Option<&str>, path: &str) -> String {
        first_in(vec![path.to_string()], subfolder, |p| self.exists(p))
            .unwrap_or_else(|| path.to_string())
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

    /// The portraits that templates' views and generals name, by id.
    pub fn portraits(&self) -> HashMap<Id, Portrait> {
        let definitions = self.definitions();
        let of_views = definitions.things.iter().filter_map(|t| {
            let view = t.thing.view.as_ref()?;
            not_null(view.portrait_ids.clone())
        });
        let of_generals =
            (definitions.generals.iter()).filter_map(|g| not_null(g.portrait_ids.clone()));
        let named: HashSet<Id> = of_views.chain(of_generals).collect();
        let mut out = HashMap::new();
        for Template { file, id, thing } in &definitions.things {
            let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
            let Some(view) = thing.view.as_ref().filter(|_| named.contains(id)) else {
                continue;
            };
            let Some(mut drawn) = self.view(dir, thing, view) else {
                continue;
            };
            drawn.animations = object_animations(dir, view);
            drawn.animations.retain(|a| self.exists(&a.kf));
            let least = view.idle_loops_min.unwrap_or(1);
            let idle_loops = [least, view.idle_loops_max.unwrap_or(least + 1)];
            out.insert(
                id.clone(),
                Portrait {
                    view: drawn,
                    idle_loops,
                },
            );
        }
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
        let falling = state_model("falling");
        let aspects: Vec<String> = aspects
            .into_iter()
            .filter(|p| {
                let states = [&unpacking, &packing, &falling];
                self.exists(p) && states.iter().all(|s| Some(p) != s.as_ref())
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
                    kind: AnimationKind::parse(a.kind.as_deref().unwrap_or_default()),
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
            falling,
            icon,
            portrait: not_null(view.portrait_ids.clone()),
            faction_portrait: thing.show_faction_portrait == Some(true),
            ground_reach: (view.aspect_normal.iter())
                .find_map(|a| a.ground_size)
                .map(|[across, along]| across.max(along))
                .filter(|reach| *reach > 0.0),
        })
    }
}

/// The animations a view's normal aspect lists, as a portrait's does, with
/// the archive paths of their `.kf` files for a view defined in `dir`.
fn object_animations(dir: &str, view: &defs::View) -> Vec<Animation> {
    let prefix = view.resource_prefix.as_deref().unwrap_or("");
    (view.aspect_normal.iter())
        .flat_map(|a| &a.animation)
        .filter_map(|a| {
            let ids = a.ids.clone()?;
            let kf = (a.kf.as_deref()?.replacen("%s", prefix, 1)).replacen("%s", &ids, 1);
            Some(Animation {
                kind: AnimationKind::parse(a.kind.as_deref().unwrap_or_default()),
                ids,
                kf: beside(&format!("{dir}/"), &kf.replace('\\', "/")),
                length: a.length.unwrap_or(1.0),
                walk_distance: None,
            })
        })
        .collect()
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

/// The first of `places` with a file, by `exists`: for each, the file of
/// its name in `subfolder` of its folder, then the place itself.
fn first_in(
    places: Vec<String>,
    subfolder: Option<&str>,
    exists: impl Fn(&str) -> bool,
) -> Option<String> {
    places.into_iter().find_map(|place| {
        let own = subfolder.map(|s| in_subfolder(&place, s));
        own.filter(|own| exists(own))
            .or_else(|| exists(&place).then_some(place))
    })
}

/// Where a texture that `model`, one of `view`'s, names as `file` may be,
/// in the order it's looked for, `read_from` being the path the model's
/// file is read from.
fn texture_paths(view: &View, model: &str, read_from: &str, file: &str) -> Vec<String> {
    let dir = read_from.rsplit_once('/').map_or("", |(d, _)| d);
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
            falling: None,
            icon: None,
            portrait: None,
            faction_portrait: false,
            ground_reach: None,
        };
        // The last rename goes first, so the first renames what it made;
        // they reach into folders too, and case matters.
        assert_eq!(
            texture_paths(
                &view,
                "Village/hut.nif",
                "Village/hut.nif",
                "Hut\\Hut_wall.tga"
            ),
            [
                "Village/HutMud/HutMud_wall.tga",
                "Shared/Textures/HutMud_wall.tga",
                "Village/Common/HutMud_wall.tga"
            ]
        );
        let first = |model, file| texture_paths(&view, model, model, file).remove(0);
        // A model read from its biome's folder has its textures there,
        // under its own renames.
        assert_eq!(
            texture_paths(
                &view,
                "Village/hut.nif",
                "Village/Arctic/hut.nif",
                "hut.tga"
            )[0],
            "Village/Arctic/hut.tga"
        );
        assert_eq!(first("Village/hut.nif", "hut.tga"), "Village/hut.tga");
        assert_eq!(first("Village/ruin.nif", "Hut.tga"), "Village/Hut.tga");
    }

    #[test]
    fn a_biomes_own_files_are_found_before_the_plain_ones() {
        let has = [
            "Unit/Arctic/Breath.dds",
            "Shared/Breath.dds",
            "Unit/Base.dds",
        ];
        let exists = |p: &str| has.contains(&p);
        let places = || {
            vec![
                "Unit/Breath.dds".to_string(),
                "Shared/Breath.dds".to_string(),
            ]
        };
        // One only its biome has beside the model is found there, ahead
        // of the search paths.
        let arctic = first_in(places(), Some("Arctic"), exists);
        assert_eq!(arctic.as_deref(), Some("Unit/Arctic/Breath.dds"));
        let desert = first_in(places(), Some("Desert"), exists);
        assert_eq!(desert.as_deref(), Some("Shared/Breath.dds"));
        assert_eq!(first_in(places(), None, exists), desert);
        // One its biome has no version of is the plain one.
        let base = first_in(vec!["Unit/Base.dds".into()], Some("Arctic"), exists);
        assert_eq!(base.as_deref(), Some("Unit/Base.dds"));
        assert_eq!(
            first_in(vec!["Unit/None.dds".into()], Some("Arctic"), exists),
            None
        );
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
    fn portraits_and_who_shows_them() {
        let parsed = tgi::parse(
            "[Template ObjectAnimation]\n{\n\tstring ids\n\tstring type\n\tstring kf = %s%s.kf\n\
             \tfloat length = 1\n}\n\
             [Template ObjectAnimationIdle Inherit=ObjectAnimation]\n{\n\ttype = IDLE\n\tlength = 3\n}\n\
             [Template ObjectAnimationFidget Inherit=ObjectAnimation]\n{\n\ttype = FIDGET\n\tlength = 3\n}\n\
             [Template ObjectAnimationSelected Inherit=ObjectAnimation]\n{\n\ttype = SELECTED\n}\n\
             [Template Aspect]\n{\n\tpoint2 ground_size = 0,0\n\
             \t[list Animation Template=ObjectAnimation]\n}\n\
             [Template Object]\n{\n\tbool show_faction_portrait = false\n\t[View]\n\t{\n\
             \t\tstring resource_prefix\n\t\tstring portrait_ids = null\n\
             \t\tint idle_loops_min = 1\n\t\tint idle_loops_max = 3\n\
             \t\t[AspectNormal Template=Aspect]\n\t}\n}\n\
             [Template GeneralPortrait Template=Object]\n{\n\t[View]\n\t{\n\t\t[AspectNormal]\n\t\t{\n\
             \t\t\t[Animation Template=ObjectAnimationIdle]\n\t\t\t{\n\t\t\t\tids = idle\n\t\t\t}\n\
             \t\t\t[Animation Template=ObjectAnimationFidget]\n\t\t\t{\n\t\t\t\tids = fidget1\n\t\t\t}\n\
             \t\t}\n\t}\n}\n\
             [Template HQ Template=Object]\n{\n\tshow_faction_portrait = true\n}\n\
             [Thing Template=GeneralPortrait]\n{\n\tids = rommel_portrait\n\t[View]\n\t{\n\
             \t\tresource_prefix = Rommel/RommelPortrait\n\t\t[AspectNormal]\n\t\t{\n\
             \t\t\t[Animation Template=ObjectAnimationSelected]\n\t\t\t{\n\t\t\t\tids = talk1\n\
             \t\t\t\tlength = 2\n\t\t\t\tkf = /Shared/Talk.kf\n\t\t\t}\n\t\t}\n\t}\n}\n\
             [Thing Template=Object]\n{\n\tids = officer\n\t[View]\n\t{\n\
             \t\tportrait_ids = officer_portrait\n\t\t[AspectNormal]\n\t\t{\n\
             \t\t\tground_size = 4,6.5\n\t\t}\n\t}\n}\n\
             [Thing Template=HQ]\n{\n\tids = corps_hq\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let things: Vec<defs::Thing> = (parsed.root.blocks_of("Thing"))
            .map(|b| templates.deserialize(b).unwrap())
            .collect();
        let [portrait, officer, hq] = &things[..] else {
            panic!("three things");
        };
        // A portrait plays its template's animations and then its own.
        let view = portrait.view.as_ref().unwrap();
        let animations: Vec<_> = object_animations("Generals/GER", view)
            .into_iter()
            .map(|a| (a.kind, a.ids, a.kf, a.length))
            .collect();
        let animation =
            |kind, ids: &str, kf: &str, length| (kind, ids.to_string(), kf.to_string(), length);
        let prefix = "Generals/GER/Rommel/RommelPortrait";
        assert_eq!(
            animations,
            [
                animation(
                    AnimationKind::Idle,
                    "idle",
                    &format!("{prefix}idle.kf"),
                    3.0
                ),
                animation(
                    AnimationKind::Fidget,
                    "fidget1",
                    &format!("{prefix}fidget1.kf"),
                    3.0
                ),
                animation(AnimationKind::Selected, "talk1", "Shared/Talk.kf", 2.0),
            ]
        );
        assert_eq!(
            (view.idle_loops_min, view.idle_loops_max),
            (Some(1), Some(3))
        );
        let named = officer.view.as_ref().unwrap().portrait_ids.clone();
        assert_eq!(not_null(named), Some(Id::from("officer_portrait")));
        assert_eq!(not_null(view.portrait_ids.clone()), None);
        let ground = |thing: &defs::Thing| {
            let view = thing.view.as_ref().unwrap();
            view.aspect_normal.iter().find_map(|a| a.ground_size)
        };
        assert_eq!(ground(officer), Some([4.0, 6.5]));
        assert_eq!(ground(portrait), Some([0.0, 0.0]));
        assert_eq!(officer.show_faction_portrait, Some(false));
        assert_eq!(hq.show_faction_portrait, Some(true));
    }

    #[test]
    fn animation_kinds_read_in_any_case_and_print_as_the_files_spell_them() {
        assert_eq!(AnimationKind::parse("Random"), AnimationKind::Random);
        assert_eq!(AnimationKind::parse("DIE"), AnimationKind::Die);
        let open = AnimationKind::parse("open_gate");
        assert_eq!(open, AnimationKind::Other("OPEN_GATE".into()));
        for (name, kind) in AnimationKind::NAMES {
            assert_eq!(kind.to_string(), name);
        }
        assert_eq!(open.to_string(), "OPEN_GATE");
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
