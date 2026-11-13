//! The keys the game binds: its hotkey files' `map` lines, each a key,
//! what it's for and, for most, the command it gives, e.g.
//! `map c game|action|verify "ActorCommand hq_attach"`.

use crate::strings::localized;
use crate::{GameData, Id, defs, scoped};

/// The file listing the hotkey files every configuration shares.
const SHARED: &str = "Localization/Hotkeys/start.txt";
/// The file listing the configurations players choose between.
const CONFIGURATIONS: &str = "Localization/Hotkeys/Actions/hotkey_configuration_data.tgi";

/// A key bound to something the player can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotkey {
    /// The key, lower case, after any keys held with it, e.g. `ctrl-s`.
    pub chord: String,
    /// Where it applies, or the button it presses, e.g.
    /// `game|action|verify` or `ButtonSupplyBorder`.
    pub target: String,
    /// The command it gives there, e.g. `ActorCommand move`.
    pub command: Option<String>,
}

impl Hotkey {
    /// What it does, by the name the files give that: its command, or
    /// the button it presses.
    pub fn action(&self) -> &str {
        let button = self.target.split('|').next().unwrap_or(&self.target);
        self.command.as_deref().unwrap_or(button)
    }
}

/// The hotkeys a file binds, in its order.
pub fn parse(text: &str) -> Vec<Hotkey> {
    text.lines().filter_map(line).collect()
}

fn line(line: &str) -> Option<Hotkey> {
    // No command has a `#` in it, so one always starts a comment.
    let mut words = line.split('#').next()?.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("map") {
        return None;
    }
    let chord = words.next()?.to_ascii_lowercase();
    let target = words.next()?.to_string();
    let command = words.collect::<Vec<_>>().join(" ");
    let command = command.trim_matches('"').trim();
    Some(Hotkey {
        chord,
        target,
        command: (!command.is_empty()).then(|| command.to_string()),
    })
}

/// A set of hotkeys players can choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyConfiguration {
    pub id: Id,
    /// What the options call it.
    pub name: String,
    /// The archive path of the file binding its keys.
    pub file: Option<String>,
}

impl GameData {
    /// The hotkeys every configuration shares, then those of the one
    /// given.
    pub fn hotkeys(&self, configuration: Option<&HotkeyConfiguration>) -> Vec<Hotkey> {
        let shared = self.text(SHARED).unwrap_or_default();
        let mut files: Vec<String> = shared
            .lines()
            .filter_map(|l| l.trim().strip_prefix("exechere"))
            .map(|name| name.trim().to_string())
            .collect();
        files.extend(configuration.and_then(|c| c.file.clone()));
        files
            .iter()
            .filter_map(|name| self.text(name))
            .flat_map(|t| parse(&t))
            .collect()
    }

    fn text(&self, name: &str) -> Option<String> {
        let bytes = self.read(name)?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// The configurations players choose between, in the order the
    /// options list them, named from the game's `strings`.
    pub fn hotkey_configurations(&self, strings: &crate::Strings) -> Vec<HotkeyConfiguration> {
        let files = self.parse_named([CONFIGURATIONS]);
        scoped(&files)
            .iter()
            .flat_map(|f| f.read::<defs::HotkeyConfiguration>("HotkeyConfigurationData"))
            .filter_map(|c| {
                let id = c.ids?;
                let name = localized(strings, c.name.as_deref());
                Some(HotkeyConfiguration {
                    name: name.unwrap_or_else(|| id.to_string()),
                    id,
                    file: c.hotkey_file_location,
                })
            })
            .collect()
    }

    /// The id of the configuration in use until the player chooses one.
    pub fn default_hotkey_configuration(&self) -> Option<Id> {
        self.interface_setting(|v| v.hotkey_configuration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_lines_bind_keys_to_commands_and_buttons() {
        let keys = parse(
            "# game - action buttons\n\
             map c game|action|verify \"ActorCommand hq_attach\"\n\
             #map t game|action|verify \"ActorCommand attach\"\n\
             map Ctrl-S ButtonSupplyBorder \n\
             map f5 game \"CycleSelection 2\"\t  # cycle detached regiments\n\
             map a ActionButtonsButton_4|action\n\
             map ctrl-m editbox EditboxAddGlyph ch_MONEY\n\
             exechere other.txt\n",
        );
        let read: Vec<(&str, &str)> = keys
            .iter()
            .map(|k| (k.chord.as_str(), k.action()))
            .collect();
        assert_eq!(
            read,
            [
                ("c", "ActorCommand hq_attach"),
                ("ctrl-s", "ButtonSupplyBorder"),
                ("f5", "CycleSelection 2"),
                ("a", "ActionButtonsButton_4"),
                ("ctrl-m", "EditboxAddGlyph ch_MONEY"),
            ]
        );
        assert_eq!(keys[0].target, "game|action|verify");
    }
}
