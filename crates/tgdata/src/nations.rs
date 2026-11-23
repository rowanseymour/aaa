//! The nations sides play as, their factions, organizations' banners, and the
//! lobby's handicaps and computer player profiles.

use std::collections::{HashMap, HashSet};

use crate::{GameData, Id, beside, defs, not_null, scoped, strings::localized, ui_path};

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
    /// Those who may lead its sides.
    pub factions: Vec<Faction>,
    /// The archive path of the flag the lobby shows for it.
    pub flag: Option<String>,
    /// The music its player hears in a match.
    pub music: Option<Id>,
    /// Where its interface's skin is, as paths in the install, which
    /// [`GameData::skin`] opens.
    pub skin: Vec<String>,
}

/// A bonus or penalty the lobby can give a side the computer plays.
#[derive(Debug, Clone, PartialEq)]
pub struct Handicap {
    /// E.g. `handicap_easy`.
    pub id: Id,
    /// What its name draws in place of words, by the glyph's image name,
    /// e.g. `Rank1`.
    pub glyph: Option<String>,
    /// What it's called, where that's words rather than a glyph.
    pub name: Option<String>,
    /// E.g. `EASY: Gives a military and economic penalty to the AI.`
    pub description: String,
    /// The technology the side starts with, if any.
    pub property: Option<Id>,
    /// Added to what the side starts with.
    pub extra: crate::Amounts,
}

/// A faction that leads a side.
#[derive(Debug, Clone, PartialEq)]
pub struct Faction {
    /// E.g. `patton`.
    pub id: Id,
    /// E.g. `General Patton`.
    pub name: String,
    /// What the lobby calls them, e.g. `Patton`.
    pub short_name: String,
    /// The special operations they give their side, in the order the
    /// panel shows them.
    pub operations: Vec<FactionOperation>,
    /// The most experience their side can hold, if there's a ceiling.
    pub experience_max: Option<f32>,
    /// Whether the lobby offers them, as it doesn't those only scenarios
    /// use.
    pub public: bool,
    /// The [`crate::Portrait`] the object display shows for their side's
    /// HQs, by id.
    pub portrait: Option<Id>,
    /// What a computer player playing as them is called, e.g. `Kesselring
    /// AI`, from the profile it plays by.
    pub computer_name: Option<String>,
    /// The banner, by id, their side's organizations fly where their
    /// templates say to.
    pub banner: Option<Id>,
}

/// A computer player's profile, as the lobby offers it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sai {
    /// E.g. `chuikov_sai`.
    pub id: Id,
    /// What the computer player is called, e.g. `Chuikov AI`.
    pub name: String,
    /// The nation it plays as, where it names one.
    pub nation: Option<Id>,
    /// The faction it plays as, where it names just one.
    pub faction: Option<Id>,
    /// Whether the lobby offers it, as it doesn't those only scenarios use.
    pub public: bool,
}

/// A special operation as a faction gives it.
#[derive(Debug, Clone, PartialEq)]
pub struct FactionOperation {
    pub id: Id,
    /// The side's experience using it takes.
    pub cost: f32,
}

/// What organizations' banners are drawn with.
#[derive(Debug, Clone, Default)]
pub struct Banners {
    /// The nation each organization template is of, by template id.
    pub of_template: HashMap<Id, Id>,
    /// The banner each template's organizations fly, by template id.
    pub flown: HashMap<Id, Id>,
    /// The templates whose organizations fly their faction's banner
    /// instead, where it has one.
    pub follow_faction: HashSet<Id>,
    /// Each banner's decals, by banner id: when each shows, and the
    /// archive path of its first image, in the order the banner lists them.
    pub decals: HashMap<Id, Vec<(BannerDecal, String)>>,
    /// Each banner's rank decal, by banner id: the archive path of its
    /// image for each experience level, lowest first.
    pub ranks: HashMap<Id, Vec<String>>,
    /// Each banner's plate, by banner id: the archive path of its model
    /// and the name of the shape in it that's always shown with no decals
    /// over it, whose own image is tinted in the owner's colour.
    pub plates: HashMap<Id, (String, String)>,
    /// The archive paths of the badges a nation's divisions wear, in the
    /// order HQs number them.
    pub badges: HashMap<Id, Vec<String>>,
}

/// When a banner's decal shows, of those drawn from the banner's own
/// images.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerDecal {
    /// Always: the frame round the banner.
    Always,
    Engaged,
    OutOfSupply,
    UpkeepShortage,
    Entrenched,
    /// By experience level, one image for each.
    Experience,
}

impl BannerDecal {
    /// Its `function` in the data, matched ignoring case.
    fn read(function: &str) -> Option<BannerDecal> {
        let is = |f: &str| function.trim().eq_ignore_ascii_case(f);
        [
            ("NONE", BannerDecal::Always),
            ("ENGAGED", BannerDecal::Engaged),
            ("OUT_OF_SUPPLY", BannerDecal::OutOfSupply),
            ("UPKEEP_SHORTAGE", BannerDecal::UpkeepShortage),
            ("ENTRENCHED", BannerDecal::Entrenched),
            ("EXPERIENCE", BannerDecal::Experience),
        ]
        .into_iter()
        .find_map(|(f, d)| is(f).then_some(d))
    }
}

impl Banners {
    /// The banner an organization of `template` flies, by id, for a side
    /// whose faction flies `faction`'s: the faction's, where the template
    /// follows it and it's defined, else the template's own.
    pub fn banner<'a>(&'a self, template: &Id, faction: Option<&'a Id>) -> Option<&'a Id> {
        let own = self.flown.get(template);
        let faction = faction.filter(|f| self.decals.contains_key(*f));
        match self.follow_faction.contains(template) {
            true => faction.or(own),
            false => own,
        }
    }

    /// The archive path of `banner`'s model and the name of its plate's
    /// shape in it.
    pub fn plate(&self, banner: &Id) -> Option<(&str, &str)> {
        let (model, shape) = self.plates.get(banner)?;
        Some((model, shape))
    }

    /// The archive path of the image `banner`'s rank decal shows at
    /// experience level `level`, counting from 0: a level past the last
    /// image keeps the last.
    pub fn rank(&self, banner: &Id, level: usize) -> Option<&str> {
        let ranks = self.ranks.get(banner)?;
        ranks.get(level).or(ranks.last()).map(String::as_str)
    }

    /// The archive path of the first image `banner` shows `when`.
    pub fn decal(&self, banner: &Id, when: BannerDecal) -> Option<&str> {
        let decals = self.decals.get(banner)?;
        decals
            .iter()
            .find(|(d, _)| *d == when)
            .map(|(_, p)| p.as_str())
    }
}

/// `banner`'s decals that show `when` they're read as, each by its first
/// image's archive path, and the images of its first rank decal: relative
/// to `file`'s folder, with `%s` its prefix.
fn decals(file: &str, banner: &defs::Banner) -> (Vec<(BannerDecal, String)>, Vec<String>) {
    let prefix = banner.resource_prefix.as_deref().unwrap_or("");
    let mut ranks = None;
    let mut first = Vec::new();
    for d in banner.plane.iter().flat_map(|p| &p.decal) {
        let Some(when) = d.function.as_deref().and_then(BannerDecal::read) else {
            continue;
        };
        let textures = d.texture.iter().filter_map(|t| t.texture.as_deref());
        let mut paths: Vec<String> = textures
            .map(|t| beside(file, &t.trim().replace("%s", prefix)))
            .collect();
        if paths.is_empty() {
            continue;
        }
        if when == BannerDecal::Experience && ranks.is_none() {
            ranks = Some(paths.clone());
        }
        first.push((when, paths.swap_remove(0)));
    }
    (first, ranks.unwrap_or_default())
}

/// The archive path of `banner`'s model, relative to `file`'s folder with
/// `%s` its prefix, and the name of the first of its planes that's always
/// shown and carries no decals: the plate.
fn plate(file: &str, banner: &defs::Banner) -> Option<(String, String)> {
    let prefix = banner.resource_prefix.as_deref().unwrap_or("");
    let model = banner.model.as_deref().unwrap_or("%sBanner.nif");
    let always = |p: &&defs::BannerPlane| {
        let function = p.function.as_deref().map(str::trim);
        function.is_some_and(|f| f.eq_ignore_ascii_case("NONE")) && p.decal.is_empty()
    };
    let shape = banner.plane.iter().find(always)?.name.clone()?;
    Some((beside(file, &model.trim().replace("%s", prefix)), shape))
}

impl GameData {
    /// What organizations' banners are drawn with, and which nation each
    /// organization template is of. A template or faction naming a banner
    /// that isn't defined is an error in the data, logged.
    pub fn banners(&self) -> Banners {
        let files = self.parse_tgis(|n| {
            ["nations/", "templates/", "organizations/banners/"]
                .iter()
                .any(|d| n.starts_with(d))
        });
        let files = scoped(&files);
        let mut out = Banners::default();
        for f in &files {
            for b in f.read::<defs::Banner>("Banner") {
                if let Some(id) = b.ids.clone() {
                    if let Some(plate) = plate(f.file, &b) {
                        out.plates.entry(id.clone()).or_insert(plate);
                    }
                    if !out.decals.contains_key(&id) {
                        let (first, ranks) = decals(f.file, &b);
                        out.ranks.insert(id.clone(), ranks);
                        out.decals.insert(id, first);
                    }
                }
            }
        }
        let mut by_group = HashMap::new();
        for nation in files.iter().flat_map(|f| f.read::<defs::Nation>("Nation")) {
            let Some(ids) = nation.ids else {
                continue;
            };
            let badges = nation.badge.iter();
            let paths = badges.map(|b| format!("Nations/{}", b.badge_texture));
            out.badges.insert(ids.clone(), paths.collect());
            if let Some(group) = nation.placement_group_ids {
                by_group.insert(group, ids);
            }
        }
        out.of_template = self.by_template(|t| {
            let group = t.placement.as_ref()?.placement_group_ids.as_ref()?;
            by_group.get(group).cloned()
        });
        let flies = self.by_template(|t| {
            let view = t.view.as_ref()?;
            let banner = not_null(view.banner_ids.clone())?;
            Some((banner, view.use_faction_banner == Some(true)))
        });
        for (template, (banner, follows)) in flies {
            if follows {
                out.follow_faction.insert(template.clone());
            }
            out.flown.insert(template, banner);
        }
        let undefined = |banner: &Id| !out.decals.contains_key(banner);
        let mut missing: Vec<_> = (out.flown.iter())
            .filter(|(_, b)| undefined(b))
            .map(|(t, b)| format!("template {t} flies banner {b}"))
            .collect();
        for f in &self.definitions().factions {
            if let (Some(id), Some(b)) = (&f.ids, not_null(f.banner_ids.clone()))
                && undefined(&b)
            {
                missing.push(format!("faction {id} flies banner {b}"));
            }
        }
        missing.sort();
        for m in missing {
            log::warn!("{m}, which isn't defined");
        }
        out
    }

    /// Each nation's emblem's image, by the nation's id, as this project's
    /// own interface draws it: from the `[Emblem]` blocks of the `.tgi`
    /// files under `Modern/`, the first for a nation winning. An SVG is
    /// drawn at the emblem's size, any other image at its own.
    pub fn emblems(&self) -> HashMap<Id, String> {
        let files = self.modern_files();
        let mut emblems = HashMap::new();
        for file in scoped(&files) {
            for emblem in file.read::<defs::Emblem>("Emblem") {
                let (Some(nation), Some(texture)) = (not_null(emblem.nation_ids), emblem.texture)
                else {
                    continue;
                };
                let texture = beside(file.file, texture.trim());
                emblems.entry(nation).or_insert(texture);
            }
        }
        emblems
    }

    /// The nations sides can play as, by name.
    pub fn nations(&self) -> Vec<Nation> {
        let strings = self.strings();
        let factions = self.factions(&strings);
        let files = self.parse_tgis(|n| n.starts_with("nations/"));
        let files = scoped(&files);
        let mut nations: Vec<Nation> = files
            .iter()
            .flat_map(|f| f.read::<defs::Nation>("Nation"))
            .filter_map(|n| {
                let id = n.ids?;
                let placeholders = n.placeholder.into_iter();
                Some(Nation {
                    name: localized(&strings, n.name.as_deref()).unwrap_or_else(|| id.to_string()),
                    id,
                    placeholders: placeholders
                        .filter_map(|p| Some((p.placeholder_ids?, p.thing_ids?)))
                        .collect(),
                    factions: (n.factions.as_deref().unwrap_or("").split(','))
                        .filter_map(|id| factions.get(&Id::from(id.trim())).cloned())
                        .collect(),
                    flag: n.flag_icon.as_deref().and_then(ui_path),
                    music: n
                        .game_music
                        .filter(|m| !m.as_str().eq_ignore_ascii_case("null")),
                    skin: (n.depot_name.as_deref().unwrap_or("").split(','))
                        .map(str::trim)
                        .filter(|d| !d.is_empty() && !d.eq_ignore_ascii_case("null"))
                        .map(str::to_string)
                        .collect(),
                })
            })
            .collect();
        nations.sort_by(|a, b| a.name.cmp(&b.name));
        nations
    }

    /// The handicaps the lobby offers, in the order the game lists them.
    pub fn handicaps(&self) -> Vec<Handicap> {
        let strings = self.strings();
        let glyphs: HashMap<char, String> = (self.glyphs().iter())
            .filter_map(|g| Some((char::from_u32(g.index?)?, g.display_name.clone()?)))
            .collect();
        let files = self.parse_tgis(|n| n.starts_with("game/handicaps"));
        let handicaps: Vec<Handicap> = (scoped(&files).iter())
            .flat_map(|f| f.read::<defs::Handicap>("Handicap"))
            .filter_map(|h| {
                let id = h.ids?;
                let name = localized(&strings, h.name.as_deref()).unwrap_or_default();
                let description = localized(&strings, h.description.as_deref()).unwrap_or_default();
                let glyph = only_char(&name).and_then(|g| glyphs.get(&g).cloned());
                Some(Handicap {
                    name: Some(name).filter(|n| glyph.is_none() && !n.trim().is_empty()),
                    glyph,
                    description,
                    property: (h.property_ids.as_deref().map(str::trim))
                        .filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("null"))
                        .map(Id::from),
                    extra: h
                        .extra_resources
                        .as_ref()
                        .map(crate::Amounts::from)
                        .unwrap_or_default(),
                    id,
                })
            })
            .collect();
        handicaps
    }

    /// The archive path of the flag the lobby shows for a side whose nation
    /// is left to chance: the one a nation has where it names none.
    pub fn random_flag(&self) -> Option<String> {
        let files = self.parse_tgis(|n| n.starts_with("templates/"));
        let nation = (files.iter())
            .flat_map(|(_, p)| p.root.blocks_of("Template"))
            .find(|t| t.name().is_some_and(|n| n.eq_ignore_ascii_case("Nation")))?;
        nation.value("flag_icon").and_then(ui_path)
    }

    /// The factions that may lead a side, by id.
    fn factions(&self, strings: &crate::Strings) -> HashMap<Id, Faction> {
        let computer_names = self.computer_names(strings);
        let definitions = self.definitions();
        (definitions.factions.iter())
            .filter_map(|g| {
                let id = g.ids.clone()?;
                let name = localized(strings, g.name.as_deref()).unwrap_or_else(|| id.to_string());
                let faction = Faction {
                    short_name: localized(strings, g.staging_name.as_deref())
                        .unwrap_or_else(|| name.clone()),
                    name,
                    operations: (g.ability.iter())
                        .filter_map(|a| {
                            Some(FactionOperation {
                                id: a.ability_ids.clone()?,
                                cost: a.cost_faction_experience.unwrap_or(0.0),
                            })
                        })
                        .collect(),
                    experience_max: g.experience_max.filter(|m| *m >= 0.0),
                    public: g.public != Some(false),
                    portrait: not_null(g.portrait_ids.clone()),
                    computer_name: computer_names.get(&id).cloned(),
                    banner: not_null(g.banner_ids.clone()),
                    id: id.clone(),
                };
                Some((id, faction))
            })
            .collect()
    }

    /// The computer player profiles, by name.
    pub fn sais(&self) -> Vec<Sai> {
        let strings = self.strings();
        let files = self.parse_tgis(|n| n.starts_with("sai/"));
        let one = |choices: Vec<defs::SaiChoice>| -> Option<Id> {
            let mut ids = choices.into_iter().filter_map(|c| c.ids);
            ids.next().filter(|_| ids.next().is_none())
        };
        let mut sais: Vec<Sai> = (scoped(&files).iter())
            .flat_map(|f| f.read::<defs::Sai>("SAI"))
            .filter_map(|s| {
                let id = s.ids?;
                Some(Sai {
                    name: localized(&strings, s.name.as_deref()).unwrap_or_else(|| id.to_string()),
                    id,
                    nation: one(s.nation),
                    faction: one(s.faction),
                    public: s.public != Some(false),
                })
            })
            .collect();
        sais.sort_by(|a, b| a.name.cmp(&b.name));
        sais
    }

    /// What the computer player of each faction is called, by the
    /// faction's id: the name of the profile made for them, rather than
    /// one a campaign plays as several of a nation's factions.
    fn computer_names(&self, strings: &crate::Strings) -> HashMap<Id, String> {
        let files = self.parse_tgis(|n| n.starts_with("sai/"));
        let files = scoped(&files);
        // Each faction's name so far, with how many factions its profile
        // plays as.
        let mut names: HashMap<Id, (usize, String)> = HashMap::new();
        for profile in files.iter().flat_map(|f| f.read::<defs::Sai>("SAI")) {
            let Some(name) = localized(strings, profile.name.as_deref()) else {
                continue;
            };
            let factions: Vec<Id> = profile.faction.into_iter().filter_map(|f| f.ids).collect();
            for faction in &factions {
                let better = (names.get(faction)).is_none_or(|(many, _)| factions.len() < *many);
                if better {
                    names.insert(faction.clone(), (factions.len(), name.clone()));
                }
            }
        }
        names.into_iter().map(|(g, (_, name))| (g, name)).collect()
    }

    /// The colours players can take, by id, e.g. `blue`.
    pub fn kingdom_colors(&self) -> HashMap<Id, [f32; 3]> {
        let files = self.parse_tgis(|n| n == "game/kingdom_colors.tgi");
        let files = scoped(&files);
        files
            .iter()
            .flat_map(|f| f.read::<defs::KingdomColor>("KingdomColor"))
            .filter_map(|c| Some((c.ids?, c.color)))
            .collect()
    }
}

/// The one character `text` is, as a name that's just a glyph is.
fn only_char(text: &str) -> Option<char> {
    let mut chars = text.trim().chars();
    chars.next().filter(|_| chars.next().is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handicap_names_that_are_one_glyph_show_its_image() {
        assert_eq!(only_char("\u{86}"), Some('\u{86}'));
        assert_eq!(only_char("Easy"), None);
    }

    #[test]
    fn handicaps_can_add_to_what_a_side_starts_with() {
        let parsed = tgi::parse(
            "[Handicap]\n{\n\tIDS = rich\n\t[ExtraResources]\n\t{\n\t\tmoney = 300\n\t}\n}\n",
        );
        let block = parsed.root.blocks_of("Handicap").next().unwrap();
        let h: defs::Handicap = tgi::from_block(block).unwrap();
        let extra = crate::Amounts::from(h.extra_resources.as_ref().unwrap());
        assert_eq!(extra.get("money"), 300.0);
    }

    #[test]
    fn a_computer_player_profile_names_its_nation_and_factions() {
        let parsed = tgi::parse(
            "[SAI]\n{\n\tIDS = chuikov_sai\n\tpublic = false\n\
             \t[Nation]\n\tIDS = nation_rus\n\t[Faction]\n\tids = chuikov\n\t[Faction]\n\tids = konev\n}\n",
        );
        let block = parsed.root.blocks_of("SAI").next().unwrap();
        let s: defs::Sai = tgi::from_block(block).unwrap();
        assert_eq!(s.ids, Some(Id::from("chuikov_sai")));
        assert_eq!(s.public, Some(false));
        let ids =
            |c: &[defs::SaiChoice]| c.iter().filter_map(|c| c.ids.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&s.nation), vec![Id::from("nation_rus")]);
        assert_eq!(
            ids(&s.faction),
            vec![Id::from("chuikov"), Id::from("konev")]
        );
    }

    #[test]
    fn a_banners_decals_are_its_images_by_when_they_show() {
        let parsed = tgi::parse(
            "[Template Base]\n{\n\t[Plane]\n\t{\n\
             \t\t[Decal]\n\t\t{\n\t\t\tfunction = ROUT\n\t\t\t[Texture]\n\t\t\ttexture = Shared/Rout.png\n\t\t}\n\
             \t\t[Decal]\n\t\t{\n\t\t\tfunction = none \n\t\t\t[Texture]\n\t\t\ttexture = %sBannerBase.png\n\t\t}\n\
             \t\t[Decal]\n\t\t{\n\t\t\tfunction = ENGAGED\n\t\t\t[Texture]\n\t\t\ttexture = Shared/Engaged1.png\n\t\t}\n\
             \t\t[Decal]\n\t\t{\n\t\t\tfunction = ENGAGED\n\t\t\t[Texture]\n\t\t\ttexture = Shared/Engaged2.png\n\t\t}\n\
             \t\t[Decal]\n\t\t{\n\t\t\tfunction = EXPERIENCE\n\t\t\t[Texture]\n\t\t\ttexture = Shared/Rank0.png\n\
             \t\t\t[Texture]\n\t\t\ttexture = Shared/Rank1.png\n\t\t}\n\
             \t}\n}\n\
             [Banner Template=Base]\n{\n\tids = ger_banner\n\tresource_prefix = GER/GER\n}\n",
        );
        let files = [("organizations/banners/ger_banner.tgi".to_string(), parsed)];
        let files = scoped(&files);
        let banner = files[0].read::<defs::Banner>("Banner").next().unwrap();
        let mut banners = Banners::default();
        let id = Id::from("ger_banner");
        let (first, ranks) = decals(files[0].file, &banner);
        banners.decals.insert(id.clone(), first);
        banners.ranks.insert(id.clone(), ranks);
        let decal = |when| banners.decal(&id, when);
        let frame = "organizations/banners/GER/GERBannerBase.png";
        assert_eq!(decal(BannerDecal::Always), Some(frame));
        // The first of two.
        let engaged = "organizations/banners/Shared/Engaged1.png";
        assert_eq!(decal(BannerDecal::Engaged), Some(engaged));
        assert_eq!(decal(BannerDecal::OutOfSupply), None);
        // A rank for each level, lowest first, the last kept past the end.
        let rank = |level| banners.rank(&id, level);
        assert_eq!(rank(0), Some("organizations/banners/Shared/Rank0.png"));
        assert_eq!(rank(1), Some("organizations/banners/Shared/Rank1.png"));
        assert_eq!(rank(5), rank(1));
    }

    #[test]
    fn templates_that_follow_their_faction_fly_its_banner_where_it_has_one() {
        let mut banners = Banners::default();
        for b in ["ger_banner", "default_banner", "royalist_banner"] {
            banners.decals.insert(b.into(), Vec::new());
        }
        banners.flown.insert("infantry".into(), "ger_banner".into());
        banners
            .flown
            .insert("company".into(), "default_banner".into());
        banners.follow_faction.insert("company".into());
        let royalist = Id::from("royalist_banner");
        let flies = |t: &str, f| banners.banner(&Id::from(t), f).map(Id::to_string);
        assert_eq!(
            flies("infantry", Some(&royalist)).as_deref(),
            Some("ger_banner")
        );
        assert_eq!(
            flies("company", Some(&royalist)).as_deref(),
            Some("royalist_banner")
        );
        assert_eq!(flies("company", None).as_deref(), Some("default_banner"));
        // A faction's banner that isn't defined leaves the template's.
        let missing = Id::from("missing_banner");
        assert_eq!(
            flies("company", Some(&missing)).as_deref(),
            Some("default_banner")
        );
        assert_eq!(flies("unknown", None), None);
    }

    #[test]
    fn a_banners_plate_is_its_models_plane_shown_always_with_no_decals() {
        let read = |text: &str| {
            let files = [("organizations/banners/b.tgi".to_string(), tgi::parse(text))];
            let files = scoped(&files);
            let banner = files[0].read::<defs::Banner>("Banner").next().unwrap();
            plate(files[0].file, &banner)
        };
        let planes = "\t[Plane]\n\t{\n\t\tfunction = NONE\n\t\tname = DecalPlane\n\
                      \t\t[Decal]\n\t\t{\n\t\t\tfunction = NONE\n\t\t}\n\t}\n\
                      \t[Plane]\n\t{\n\t\tfunction = ENTRENCHED\n\t\tname = EntrenchedPlane\n\t}\n\
                      \t[Plane]\n\t{\n\t\tfunction = NONE\n\t\tname = PlayerColorPlane\n\t}\n";
        let k2 =
            format!("[Banner]\n{{\n\tids = b\n\tresource_prefix = Default/Default\n{planes}}}\n");
        let want = |model: &str| Some((model.to_string(), "PlayerColorPlane".to_string()));
        assert_eq!(
            read(&k2),
            want("organizations/banners/Default/DefaultBanner.nif")
        );
        let aa = format!("[Banner]\n{{\n\tids = b\n\tmodel = Shared/Banner.nif\n{planes}}}\n");
        assert_eq!(read(&aa), want("organizations/banners/Shared/Banner.nif"));
        assert_eq!(read("[Banner]\n{\n\tids = b\n}\n"), None);
    }
}
