//! The game's text in its default language, and the glyphs quoted text
//! can name.

use std::collections::HashMap;

use crate::{GameData, Id, defs, scoped};

impl GameData {
    /// The game's text in its default language, by key.
    pub fn strings(&mut self) -> HashMap<String, String> {
        let glyphs = self.glyphs();
        self.strings_with(&glyphs)
    }

    /// [`GameData::strings`], with the [`GameData::glyphs`] already read.
    pub(crate) fn strings_with(&mut self, glyphs: &[defs::Glyph]) -> HashMap<String, String> {
        let glyphs: HashMap<Id, char> = (glyphs.iter())
            .filter_map(|g| Some((g.ids.clone()?, char::from_u32(g.index?)?)))
            .collect();
        let files = self.parse_tgis(|n| n.starts_with("localization/strings_"));
        let mut out = HashMap::new();
        let blocks = files.iter().flat_map(|(_, p)| p.root.blocks_of("Text"));
        for (key, text) in blocks.flat_map(text_strings) {
            // A key defined again keeps its first text.
            out.entry(key).or_insert_with(|| unescape(&text, &glyphs));
        }
        out
    }

    /// The characters quoted text names by id, such as `n` for a newline
    /// or `ch_gem` for one the font draws as an icon.
    pub(crate) fn glyphs(&mut self) -> Vec<defs::Glyph> {
        let files = self.parse_tgis(|n| n.starts_with("fonts/special_glyphs"));
        let files = scoped(&files);
        files
            .iter()
            .flat_map(|f| f.read::<defs::Glyph>("Glyph"))
            .collect()
    }
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
pub(crate) fn localized(strings: &HashMap<String, String>, name: Option<&str>) -> Option<String> {
    let name = name?;
    match name.strip_prefix('#') {
        Some(key) => strings.get(&key.to_ascii_lowercase()).cloned(),
        None => Some(name.to_string()),
    }
}

/// What the game calls a resource, by its field name in the data, such as
/// Money for `money`.
pub fn resource_name(strings: &HashMap<String, String>, field: &str) -> String {
    strings
        .get(&format!("resources_{field}_name"))
        .cloned()
        .unwrap_or_else(|| field.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_path;

    #[test]
    fn commands_take_names_and_icons_from_the_interface() {
        let parsed = tgi::parse(
            "[Text language = default]\n{\n\tname = \"@strings.tgi\"\n\tmove_name = \"Move\"\n\
             \tcount|d = \"%d left\"\n}\n",
        );
        let block = parsed.root.blocks_of("Text").next().unwrap();
        let strings: HashMap<String, String> = text_strings(block).collect();
        assert_eq!(
            localized(&strings, Some("#Move_name")).as_deref(),
            Some("Move")
        );
        assert_eq!(localized(&strings, Some("Plain")).as_deref(), Some("Plain"));
        // A key's format is part of it, and the block's name isn't a string.
        assert_eq!(strings.get("count|d").map(String::as_str), Some("%d left"));
        assert_eq!(strings.get("count"), None);
        assert_eq!(strings.get("name"), None);
        assert_eq!(
            ui_path("/UI/%s/Game/ActionButtons/IconMove.png").as_deref(),
            Some("UI/Game/ActionButtons/IconMove.png")
        );
        assert_eq!(ui_path("NULL"), None);
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
