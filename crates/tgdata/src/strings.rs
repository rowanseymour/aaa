//! The game's text in its default language, and the glyphs quoted text
//! can name.

use std::collections::HashMap;
use std::sync::Arc;

use crate::{GameData, Id, defs};

/// The game's text by key, whose case doesn't matter, as it doesn't to the
/// data that names one.
#[derive(Clone, Debug, Default)]
pub struct Strings(HashMap<String, String>);

impl Strings {
    pub fn get(&self, key: &str) -> Option<&String> {
        self.0.get(&key.to_ascii_lowercase())
    }
}

impl FromIterator<(String, String)> for Strings {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(pairs: I) -> Self {
        let pairs = pairs.into_iter();
        Self(pairs.map(|(k, v)| (k.to_ascii_lowercase(), v)).collect())
    }
}

impl GameData {
    /// The game's text in its default language, by key.
    pub fn strings(&self) -> Arc<Strings> {
        self.kept("strings", || self.read_strings())
    }

    /// A map's own text by key, such as the name it gives a city
    /// (`name_3954`) or a side (`independent_5`), from its string table as
    /// stored, in `.tgi` syntax.
    pub fn map_strings(&self, table: &[u8]) -> Strings {
        table_strings([&tgi::parse_bytes(table)], &self.glyph_chars())
    }

    fn glyph_chars(&self) -> Arc<HashMap<Id, char>> {
        self.kept("glyph_chars", || {
            (self.glyphs().iter())
                .filter_map(|g| Some((g.ids.clone()?, char::from_u32(g.index?)?)))
                .collect()
        })
    }

    fn read_strings(&self) -> Strings {
        let glyphs = self.glyph_chars();
        // The computer players' and random maps' tables too, which sit in
        // folders of their own; the hotkeys' hold no text.
        let files = self.parse_tgis(|n| {
            n.starts_with("localization/") && !n.starts_with("localization/hotkeys/")
        });
        table_strings(files.iter().map(|(_, p)| &**p), &glyphs)
    }

    /// The characters quoted text names by id, such as `n` for a newline
    /// or `ch_gem` for one the font draws as an icon.
    pub(crate) fn glyphs(&self) -> Arc<Vec<defs::Glyph>> {
        self.kept("glyphs", || self.read_glyphs())
    }

    /// What the font's image of the glyph that marks a side is called,
    /// e.g. `Gem`: drawn in the side's colour wherever a side is named.
    pub fn side_glyph(&self) -> Option<String> {
        let id = self.avar(|v| v.kingdom_glyph_info.clone())?;
        glyph_image(&self.glyphs(), &id)
    }

    /// What the font's image of the glyph `id` names is called, e.g.
    /// `ALLY` for `ch_ally`.
    pub fn glyph_named(&self, id: &str) -> Option<String> {
        glyph_image(&self.glyphs(), &Id::from(id))
    }

    /// What the font's image of the glyph drawn before a side's experience
    /// is called, e.g. `FactionExperience`.
    pub fn experience_glyph(&self) -> Option<String> {
        let index = self.avar(|v| v.font_faction_experience_glyph_index)?;
        let glyphs = self.glyphs();
        image_name(glyphs.iter().find(|g| g.index == Some(index))?)
    }

    /// How a ping is shown: the font's images of the glyphs drawn before
    /// it, by how good it is, and where good gives way to middling and
    /// middling to bad.
    pub fn ping_look(&self) -> PingLook {
        let glyphs = self.glyphs();
        let glyph = |of: fn(&defs::AVars) -> Option<Id>| {
            let id = self.avar(of)?;
            glyph_image(&glyphs, &id)
        };
        let cutoff = |of: fn(&defs::AVars) -> Option<u32>, stock| self.avar(of).unwrap_or(stock);
        PingLook {
            good: glyph(|v| v.ping_icon_green_glyph_info.clone()),
            middling: glyph(|v| v.ping_icon_yellow_glyph_info.clone()),
            bad: glyph(|v| v.ping_icon_red_glyph_info.clone()),
            low: cutoff(|v| v.server_browser_low_ping_cutoff, 200),
            medium: cutoff(|v| v.server_browser_medium_ping_cutoff, 500),
        }
    }

    fn read_glyphs(&self) -> Vec<defs::Glyph> {
        self.read_all(|n| n.starts_with("fonts/special_glyphs"), "Glyph")
    }
}

/// How a ping is shown, as [`GameData::ping_look`] reads it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PingLook {
    pub good: Option<String>,
    pub middling: Option<String>,
    pub bad: Option<String>,
    /// In milliseconds: a ping below `low` is good, below `medium`
    /// middling, and otherwise bad.
    pub low: u32,
    pub medium: u32,
}

impl PingLook {
    /// The image of the glyph drawn before a ping of `ms` milliseconds.
    pub fn glyph(&self, ms: u32) -> Option<&str> {
        let glyph = match ms {
            ms if ms < self.low => &self.good,
            ms if ms < self.medium => &self.middling,
            _ => &self.bad,
        };
        glyph.as_deref()
    }
}

/// What the font's image of the glyph `id` is called: its display name
/// run together, as `Faction Experience` is drawn from
/// `FactionExperience.png`.
pub(crate) fn glyph_image(glyphs: &[defs::Glyph], id: &Id) -> Option<String> {
    image_name(glyphs.iter().find(|g| g.ids.as_ref() == Some(id))?)
}

/// What the font's image of `glyph` is called.
pub(crate) fn image_name(glyph: &defs::Glyph) -> Option<String> {
    Some(glyph.display_name.as_deref()?.replace(' ', ""))
}

/// The strings of the tables' `Text` blocks, the first text of a key kept.
fn table_strings<'a>(
    tables: impl IntoIterator<Item = &'a tgi::Parsed>,
    glyphs: &HashMap<Id, char>,
) -> Strings {
    let mut out = HashMap::new();
    let blocks = tables.into_iter().flat_map(|t| t.root.blocks_of("Text"));
    for (key, text) in blocks.flat_map(text_strings) {
        out.entry(key).or_insert_with(|| unescape(&text, glyphs));
    }
    Strings(out)
}

/// A `Text` block's strings by lower-case key, which keeps any `|` and
/// format after it. The block's `ids` and `name` describe it and aren't
/// strings.
fn text_strings(block: &tgi::Block) -> impl Iterator<Item = (String, String)> + '_ {
    block.fields().filter(|f| f.quoted).filter_map(|f| {
        let key = f.key.to_ascii_lowercase();
        (key != "ids" && key != "name").then_some((key, f.value.clone()?))
    })
}

/// Quoted text with its escapes read: `\c` and `&name.` are the glyphs
/// called `c` and `name`, and `\\`, `\&` and `&&` a plain `\` and `&`.
/// What names no glyph is left as written.
fn unescape(text: &str, glyphs: &HashMap<Id, char>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(['\\', '&']) {
        out.push_str(&rest[..i]);
        let escape = rest[i..].chars().next().unwrap_or('&');
        let after = &rest[i + 1..];
        let next = after.chars().next();
        let (read, used) = match (escape, next) {
            ('\\', Some(c @ ('\\' | '&'))) | ('&', Some(c @ '&')) => (Some(c), 1),
            ('\\', Some(c)) => {
                let name = &after[..c.len_utf8()];
                (glyphs.get(&Id::from(name)).copied(), name.len())
            }
            ('&', Some(_)) => match after.split_once('.') {
                Some((name, _)) => (glyphs.get(&Id::from(name)).copied(), name.len() + 1),
                None => (None, 0),
            },
            _ => (None, 0),
        };
        match read {
            Some(c) => {
                out.push(c);
                rest = &after[used..];
            }
            None => {
                out.push(escape);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text a `#` key names, or a name written out as is.
pub(crate) fn localized(strings: &Strings, name: Option<&str>) -> Option<String> {
    let name = name?;
    match name.strip_prefix('#') {
        Some(key) => strings.get(key).cloned(),
        None => Some(name.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_path;

    #[test]
    fn a_maps_strings_are_read_in_its_own_encoding() {
        // Windows-1252's right quote, which Latin-1 has as a control code.
        let table = b"[Text language=Default]\n{\n\tids = map_strings_table\n\tName_3954 = \"Rommel\x92s HQ\"\n}\n";
        let strings = table_strings([&tgi::parse_bytes(table)], &HashMap::new());
        assert_eq!(strings.get("name_3954").unwrap(), "Rommel\u{2019}s HQ");
        assert!(strings.get("ids").is_none());
    }

    #[test]
    fn commands_take_names_and_icons_from_the_interface() {
        let parsed = tgi::parse(
            "[Text language = default]\n{\n\tname = \"@strings.tgi\"\n\tmove_name = \"Move\"\n\
             \tcount|d = \"%d left\"\n}\n",
        );
        let block = parsed.root.blocks_of("Text").next().unwrap();
        let strings: Strings = text_strings(block).collect();
        assert_eq!(
            localized(&strings, Some("#Move_name")).as_deref(),
            Some("Move")
        );
        assert_eq!(localized(&strings, Some("Plain")).as_deref(), Some("Plain"));
        // A key's format is part of it, and the block's name isn't a string.
        assert_eq!(strings.get("count|d").map(String::as_str), Some("%d left"));
        assert_eq!(strings.get("Count|D").map(String::as_str), Some("%d left"));
        assert_eq!(strings.get("count"), None);
        assert_eq!(strings.get("name"), None);
        assert_eq!(
            ui_path("/UI/%s/Game/ActionButtons/IconMove.png").as_deref(),
            Some("UI/Game/ActionButtons/IconMove.png")
        );
        assert_eq!(ui_path("NULL"), None);
    }

    #[test]
    fn a_glyph_is_drawn_from_the_image_its_display_name_names() {
        let glyph = |ids: &str, name: &str| defs::Glyph {
            ids: Some(Id::from(ids)),
            index: Some(132),
            display_name: Some(name.into()),
        };
        let glyphs = [glyph("ch_gem", "Gem"), glyph("ch_xp", "Faction Experience")];
        assert_eq!(
            glyph_image(&glyphs, &Id::from("CH_GEM")).as_deref(),
            Some("Gem")
        );
        assert_eq!(
            glyph_image(&glyphs, &Id::from("ch_xp")).as_deref(),
            Some("FactionExperience")
        );
        assert_eq!(glyph_image(&glyphs, &Id::from("ch_none")), None);
    }

    #[test]
    fn quoted_text_reads_its_escapes() {
        let glyphs = HashMap::from([(Id::from("n"), '\n'), (Id::from("ch_gem"), '\u{84}')]);
        let read = |text: &str| unescape(text, &glyphs);
        assert_eq!(read(r"a\nb &CH_Gem. 5"), "a\nb \u{84} 5");
        assert_eq!(read(r"50%% \\ \& && R&D"), r"50%% \ & & R&D");
        // What names no glyph stays as written, a trailing escape too.
        assert_eq!(read(r"\q &nope. é\"), r"\q &nope. é\");
    }
}
