//! The nations sides play as, their generals and banners, and the
//! lobby's handicaps.

use std::collections::HashMap;

use crate::{GameData, Id, defs, not_null, scoped, strings::localized, ui_path};

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
    pub generals: Vec<General>,
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

/// A general who leads a side.
#[derive(Debug, Clone, PartialEq)]
pub struct General {
    /// E.g. `patton`.
    pub id: Id,
    /// E.g. `General Patton`.
    pub name: String,
    /// What the lobby calls them, e.g. `Patton`.
    pub short_name: String,
    /// The special operations they give their side, in the order the
    /// panel shows them.
    pub operations: Vec<GeneralOperation>,
    /// The most experience their side can hold, if there's a ceiling.
    pub experience_max: Option<f32>,
    /// Whether the lobby offers them, as it doesn't those only scenarios
    /// use.
    pub public: bool,
    /// The [`crate::Portrait`] the object display shows for their side's
    /// HQs, by id.
    pub portrait: Option<Id>,
}

/// A special operation as a general gives it.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneralOperation {
    pub id: Id,
    /// The side's experience using it takes.
    pub cost: f32,
}

/// What nations' banners are drawn with, by nation id, e.g. `nation_usa`.
#[derive(Debug, Clone, Default)]
pub struct NationBanners {
    /// The nation each organization template is of, by template id.
    pub of_template: HashMap<Id, Id>,
    /// The archive path of the frame round a nation's banners.
    pub frames: HashMap<Id, String>,
    /// The archive paths of the badges a nation's divisions wear, in the
    /// order HQs number them.
    pub badges: HashMap<Id, Vec<String>>,
}

impl GameData {
    /// What each nation's banners are drawn with, and which nation each
    /// organization template is of.
    pub fn nation_banners(&mut self) -> NationBanners {
        let files = self.parse_tgis(|n| {
            ["nations/", "templates/", "organizations/banners/"]
                .iter()
                .any(|d| n.starts_with(d))
        });
        let files = scoped(&files);
        let prefixes: HashMap<Id, String> = files
            .iter()
            .flat_map(|f| f.read::<defs::Banner>("Banner"))
            .filter_map(|b| Some((b.ids?, b.resource_prefix?)))
            .collect();
        let mut out = NationBanners::default();
        let mut by_group = HashMap::new();
        for nation in files.iter().flat_map(|f| f.read::<defs::Nation>("Nation")) {
            let Some(ids) = nation.ids else {
                continue;
            };
            let badges = nation.badge.iter();
            let paths = badges.map(|b| format!("Nations/{}", b.badge_texture));
            out.badges.insert(ids.clone(), paths.collect());
            // The banner named for the nation, `ger_banner` for
            // `nation_ger`.
            let lower = ids.as_str().to_ascii_lowercase();
            let banner = lower.strip_prefix("nation_").map(|n| format!("{n}_banner"));
            if let Some(prefix) = banner.and_then(|b| prefixes.get(&Id::from(b.as_str()))) {
                let path = format!("Organizations/Banners/{prefix}BannerBase.png");
                out.frames.insert(ids.clone(), path);
            }
            if let Some(group) = nation.placement_group_ids {
                by_group.insert(group, ids);
            }
        }
        out.of_template = self.by_template(|t| {
            let group = t.placement.as_ref()?.placement_group_ids.as_ref()?;
            by_group.get(group).cloned()
        });
        out
    }

    /// The nations sides can play as, by name.
    pub fn nations(&mut self) -> Vec<Nation> {
        let strings = self.strings();
        let generals = self.generals(&strings);
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
                    generals: (n.factions.as_deref().unwrap_or("").split(','))
                        .filter_map(|id| generals.get(&Id::from(id.trim())).cloned())
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
    pub fn handicaps(&mut self) -> Vec<Handicap> {
        let glyphs = self.glyphs();
        let strings = self.strings_with(&glyphs);
        let glyphs: HashMap<char, String> = (glyphs.into_iter())
            .filter_map(|g| Some((char::from_u32(g.index?)?, g.display_name?)))
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
    pub fn random_flag(&mut self) -> Option<String> {
        let files = self.parse_tgis(|n| n.starts_with("templates/"));
        let nation = (files.iter())
            .flat_map(|(_, p)| p.root.blocks_of("Template"))
            .find(|t| t.name().is_some_and(|n| n.eq_ignore_ascii_case("Nation")))?;
        nation.value("flag_icon").and_then(ui_path)
    }

    /// The generals who may lead a side, by id.
    fn generals(&mut self, strings: &HashMap<String, String>) -> HashMap<Id, General> {
        let definitions = self.definitions();
        (definitions.generals.iter())
            .filter_map(|g| {
                let id = g.ids.clone()?;
                let name = localized(strings, g.name.as_deref()).unwrap_or_else(|| id.to_string());
                let general = General {
                    short_name: localized(strings, g.staging_name.as_deref())
                        .unwrap_or_else(|| name.clone()),
                    name,
                    operations: (g.ability.iter())
                        .filter_map(|a| {
                            Some(GeneralOperation {
                                id: a.ability_ids.clone()?,
                                cost: a.cost_faction_experience.unwrap_or(0.0),
                            })
                        })
                        .collect(),
                    experience_max: g.experience_max.filter(|m| *m >= 0.0),
                    public: g.public != Some(false),
                    portrait: not_null(g.portrait_ids.clone()),
                    id: id.clone(),
                };
                Some((id, general))
            })
            .collect()
    }

    /// The colours players can take, by id, e.g. `blue`.
    pub fn kingdom_colors(&mut self) -> HashMap<Id, [f32; 3]> {
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
}
