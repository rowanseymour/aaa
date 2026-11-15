//! The nations sides play as, their factions, organizations' banners, and the
//! lobby's handicaps.

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
/// image's archive path: relative to `file`'s folder, with `%s` its prefix.
fn decals(file: &str, banner: &defs::Banner) -> Vec<(BannerDecal, String)> {
    let prefix = banner.resource_prefix.as_deref().unwrap_or("");
    let decals = banner.plane.iter().flat_map(|p| &p.decal);
    decals
        .filter_map(|d| {
            let when = BannerDecal::read(d.function.as_deref()?)?;
            let texture = d.texture.iter().find_map(|t| t.texture.as_deref())?;
            Some((when, beside(file, &texture.trim().replace("%s", prefix))))
        })
        .collect()
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
                    out.decals.entry(id).or_insert_with(|| decals(f.file, &b));
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
    fn a_banners_decals_are_its_images_by_when_they_show() {
        let parsed = tgi::parse(
            "[Template Base]\n{\n\t[Plane]\n\t{\n\
             \t\t[Decal]\n\t\t{\n\t\t\tfunction = ROUT\n\t\t\t[Texture]\n\t\t\ttexture = Shared/Rout.png\n\t\t}\n\
             \t\t[Decal]\n\t\t{\n\t\t\tfunction = none \n\t\t\t[Texture]\n\t\t\ttexture = %sBannerBase.png\n\t\t}\n\
             \t\t[Decal]\n\t\t{\n\t\t\tfunction = ENGAGED\n\t\t\t[Texture]\n\t\t\ttexture = Shared/Engaged1.png\n\t\t}\n\
             \t\t[Decal]\n\t\t{\n\t\t\tfunction = ENGAGED\n\t\t\t[Texture]\n\t\t\ttexture = Shared/Engaged2.png\n\t\t}\n\
             \t}\n}\n\
             [Banner Template=Base]\n{\n\tids = ger_banner\n\tresource_prefix = GER/GER\n}\n",
        );
        let files = [("organizations/banners/ger_banner.tgi".to_string(), parsed)];
        let files = scoped(&files);
        let banner = files[0].read::<defs::Banner>("Banner").next().unwrap();
        let mut banners = Banners::default();
        let id = Id::from("ger_banner");
        banners
            .decals
            .insert(id.clone(), decals(files[0].file, &banner));
        let decal = |when| banners.decal(&id, when);
        let frame = "organizations/banners/GER/GERBannerBase.png";
        assert_eq!(decal(BannerDecal::Always), Some(frame));
        // The first of two.
        let engaged = "organizations/banners/Shared/Engaged1.png";
        assert_eq!(decal(BannerDecal::Engaged), Some(engaged));
        assert_eq!(decal(BannerDecal::OutOfSupply), None);
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
}
