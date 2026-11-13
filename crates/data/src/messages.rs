//! What the message log is told and how it shows it: the kinds of message,
//! the game events that post them, and the interface settings it keeps to.

use std::collections::HashMap;

use crate::strings::localized;
use crate::{GameData, Id};

const MESSAGE_TYPES: &str = "game/message_types";
const GAME_EVENTS: &str = "game/game_events";
/// Where the kinds' icons are, as their paths are relative to it.
const ICON_DIR: &str = "Game";

/// A kind of message, which decides how its line looks.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageKind {
    pub id: Id,
    /// The archive path of the icon in front of its lines, where it exists.
    pub icon: Option<String>,
    /// Its lines' text colour; white where it sets none.
    pub colour: [f32; 3],
    /// The setting that hides it while false, if any.
    pub filter: Option<String>,
    /// The interface sound played as one is posted, if any.
    pub sound: Option<String>,
}

/// Something the game announces when it happens.
#[derive(Debug, Clone, PartialEq)]
pub struct GameEvent {
    pub id: Id,
    /// The line it posts, with `%` placeholders; none for an event that
    /// posts no line.
    pub message: Option<String>,
    /// The kind of message the line is.
    pub kind: Option<Id>,
    /// The setting that drops the whole event while false, if any.
    pub filter: Option<String>,
    /// The setting that drops only its line while false, if any.
    pub message_filter: Option<String>,
}

/// The interface's settings, by lower-case name, as the files write their
/// values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InterfaceVars(HashMap<String, String>);

impl InterfaceVars {
    /// The settings a file of them sets.
    pub fn parse(text: &str) -> InterfaceVars {
        let mut out = InterfaceVars::default();
        out.read(&tgi::parse(text));
        out
    }

    /// Adds what `parsed` sets that isn't set already.
    fn read(&mut self, parsed: &tgi::Parsed) {
        for block in parsed.root.blocks_of("Vars") {
            for f in block.fields() {
                if let Some(v) = &f.value {
                    let key = f.key.to_ascii_lowercase();
                    self.0.entry(key).or_insert_with(|| v.clone());
                }
            }
        }
    }

    pub fn get<T: std::str::FromStr>(&self, name: &str) -> Option<T> {
        self.0.get(&name.to_ascii_lowercase())?.trim().parse().ok()
    }

    /// Whether a flag is on; `default` for one the files don't set.
    pub fn flag(&self, name: &str, default: bool) -> bool {
        match self.0.get(&name.to_ascii_lowercase()) {
            Some(v) => {
                let v = v.trim();
                v.eq_ignore_ascii_case("true") || v == "1"
            }
            None => default,
        }
    }
}

/// `NULL` is how the files say there's none.
fn named(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    (!value.is_empty() && !value.eq_ignore_ascii_case("null")).then(|| value.to_string())
}

impl GameData {
    /// The kinds of message, in the order the files define them.
    pub fn message_kinds(&mut self) -> Vec<MessageKind> {
        let files = self.parse_tgis(|n| n.starts_with(MESSAGE_TYPES));
        let blocks = files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("MessageType"));
        let mut out: Vec<MessageKind> = Vec::new();
        for block in blocks {
            let Some(id) = block.value("IDS").map(|v| Id::from(v.trim())) else {
                continue;
            };
            if out.iter().any(|k| k.id == id) {
                continue;
            }
            let icon = named(block.value("icon")).map(|i| format!("{ICON_DIR}/{i}"));
            out.push(MessageKind {
                id,
                icon: icon.filter(|i| self.exists(i)),
                colour: block.value("color").and_then(colour).unwrap_or([1.0; 3]),
                filter: named(block.value("filter")),
                sound: named(block.value("message_sound")),
            });
        }
        out
    }

    /// The game events, by id, with their lines in the game's language.
    pub fn game_events(&mut self) -> HashMap<Id, GameEvent> {
        let strings = self.strings();
        let files = self.parse_tgis(|n| n.starts_with(GAME_EVENTS));
        let blocks = files
            .iter()
            .flat_map(|(_, p)| p.root.blocks_of("GameEvent"));
        let mut out = HashMap::new();
        for block in blocks {
            let Some(id) = block.value("IDS").map(|v| Id::from(v.trim())) else {
                continue;
            };
            // The line is keyed by the grammatical case it's for; `*` is
            // every case.
            let message = block
                .fields()
                .find(|f| f.key.to_ascii_lowercase().starts_with("game_message"))
                .and_then(|f| localized(&strings, f.value.as_deref()))
                .filter(|m| !m.is_empty());
            out.entry(id.clone()).or_insert(GameEvent {
                id,
                message,
                kind: named(block.value("message_type")).map(|k| Id::from(k.as_str())),
                filter: named(block.value("filter")),
                message_filter: named(block.value("message_filter")),
            });
        }
        out
    }

    /// The interface's settings, the game's own over the engine's.
    pub fn interface_vars(&mut self) -> InterfaceVars {
        let mut out = InterfaceVars::default();
        for name in ["UVars_AA.tgi", "UVars.tgi"] {
            if let Some(bytes) = self.read(name) {
                out.read(&tgi::parse_bytes(&bytes));
            }
        }
        out
    }
}

/// A colour written `r,g, b`.
pub(crate) fn colour(value: &str) -> Option<[f32; 3]> {
    let mut parts = value.split(',').map(|p| p.trim().parse::<f32>());
    let c = [
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    ];
    parts.next().is_none().then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_read_as_written() {
        assert_eq!(colour("1,0,1"), Some([1.0, 0.0, 1.0]));
        assert_eq!(colour(" 1, 1 ,1"), Some([1.0; 3]));
        assert_eq!(colour("1,0"), None);
        assert_eq!(colour("1,0,1,0"), None);
    }

    #[test]
    fn null_names_nothing() {
        assert_eq!(named(Some("NULL")), None);
        assert_eq!(
            named(Some(" UIMessageFilterChat")),
            Some("UIMessageFilterChat".into())
        );
        assert_eq!(named(None), None);
    }

    #[test]
    fn flags_read_as_the_game_does() {
        let vars =
            InterfaceVars::parse("[Vars]\n{\n flag a = true\n flag b = FALSE\n int n = 5\n}\n");
        assert!(vars.flag("A", false));
        assert!(!vars.flag("b", true));
        assert!(vars.flag("missing", true));
        assert_eq!(vars.get::<u32>("N"), Some(5));
    }
}
