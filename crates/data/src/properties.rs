//! How properties show beside what holds them: their icon, name and what
//! they say they do.

use std::collections::HashMap;

use crate::strings::localized;
use crate::supply::{is, listed};
use crate::{GameData, Id, defs, icon_beside};

/// The folder of icons drawn for a wide screen, which a `%s` in an icon's
/// path stands for.
const WIDE: &str = "1280";

/// A property as it shows beside what holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyLook {
    pub name: String,
    /// Archive path of its icon.
    pub icon: String,
    /// What it says under its name: who it's for, then a line for each
    /// thing it does that the game has words for.
    pub lines: Vec<String>,
}

/// A range of speeds and the game's word for them.
struct Pace {
    range: [f32; 2],
    word: String,
}

/// A number as C's `%+g` writes it, as far as the game's amounts go.
fn signed(amount: f32) -> String {
    let rounded = (amount * 1000.0).round() / 1000.0;
    format!("{rounded:+}")
}

/// A format string of the game's with its one number or word filled in.
fn filled(format: &str, with: &str) -> String {
    let placeholder = ["%+g", "%g", "%s"].into_iter().find(|p| format.contains(p));
    let said = match placeholder {
        Some(p) => format.replacen(p, with, 1),
        None => format.to_string(),
    };
    // The glyph for a damage type has no letter of ours.
    said.replace("%lc", "").replace("%%", "%")
}

/// What the game says a modifier does, where it has words for it.
fn line(strings: &HashMap<String, String>, paces: &[Pace], m: &defs::Modifier) -> Option<String> {
    let said = |key: String| {
        strings
            .get(&key)
            .or_else(|| strings.get(&key.to_lowercase()))
    };
    let amount = m.amount.unwrap_or(0.0);
    let attribute = m.attribute.as_deref().unwrap_or("").to_ascii_lowercase();
    if is(&m.kind, "ADD") && attribute == "terrain_speed_cap" {
        // The first range to hold it, its upper end left out.
        let pace = paces
            .iter()
            .find(|p| (p.range[0]..p.range[1]).contains(&amount))?;
        let format = said("modifier_desc_add_terrain_speed_cap|s".into())?;
        return Some(filled(format, &pace.word));
    }
    if is(&m.kind, "ADD") {
        let format = said(format!("modifier_desc_add_{attribute}|f"))?;
        return Some(filled(format, &signed(amount)));
    }
    if is(&m.kind, "MULTIPLY") {
        let format = said(format!("modifier_desc_multiply_{attribute}|f"))?;
        return Some(filled(format, &signed(amount * 100.0)));
    }
    if is(&m.kind, "RESIST_DAMAGE") {
        let way = match amount < 0.0 {
            true => "negative",
            false => "positive",
        };
        let format = said(format!("modifier_desc_resist_damage_{way}|cf"))?;
        return Some(filled(format, &signed(amount.abs() * 100.0)));
    }
    let flag = m.modifier_flag.as_deref()?.to_ascii_lowercase();
    said(format!("modifier_desc_flag_{flag}")).cloned()
}

impl GameData {
    /// The game's words for speeds, in the order it lists them.
    fn paces(&mut self, strings: &HashMap<String, String>) -> Vec<Pace> {
        let definitions = self.definitions();
        let speeds = (definitions.descriptions.iter()).filter(|d| is(&d.category, "SPEED"));
        speeds
            .filter_map(|d| {
                let word = localized(strings, d.description.as_deref())?;
                Some(Pace {
                    range: d.range?,
                    word,
                })
            })
            .collect()
    }

    /// What each shared property says under its name, by id: who it's
    /// for, then a line for each thing it does that the game has words for.
    pub(crate) fn property_lines(
        &mut self,
        strings: &HashMap<String, String>,
    ) -> HashMap<Id, Vec<String>> {
        let paces = self.paces(strings);
        let definitions = self.definitions();
        let name = |p: &defs::Property| localized(strings, p.name.as_deref()).unwrap_or_default();
        let affects = strings
            .get("ui_tooltip_item_title_required_properties")
            .cloned()
            .unwrap_or_default();
        (definitions.properties.iter())
            .map(|(id, p)| {
                let needed: Vec<String> = listed(p.required_properties.as_deref())
                    .iter()
                    .filter_map(|r| definitions.properties.get(r))
                    .map(name)
                    .filter(|n| !n.is_empty())
                    .collect();
                let who = (!needed.is_empty()).then(|| format!("{affects} {}", needed.join(", ")));
                let does = p.modifier.iter().filter_map(|m| line(strings, &paces, m));
                (id.clone(), who.into_iter().chain(does).collect())
            })
            .collect()
    }

    /// How each shared property that has an icon shows, by id.
    pub fn property_looks(&mut self) -> HashMap<Id, PropertyLook> {
        let strings = self.strings();
        let mut lines = self.property_lines(&strings);
        let definitions = self.definitions();
        let mut out = HashMap::new();
        for (id, p) in &definitions.properties {
            let file = definitions
                .property_files
                .get(id)
                .map_or("", String::as_str);
            let Some(path) = icon_beside(file, p.icon.as_deref()) else {
                continue;
            };
            let places = [path.replace("%s", WIDE), path.replace("%s/", "")];
            let Some(icon) = places.into_iter().find(|p| self.exists(p)) else {
                continue;
            };
            out.insert(
                id.clone(),
                PropertyLook {
                    name: localized(&strings, p.name.as_deref()).unwrap_or_default(),
                    icon,
                    lines: lines.remove(id).unwrap_or_default(),
                },
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modifier(kind: &str, attribute: Option<&str>, amount: f32) -> defs::Modifier {
        defs::Modifier {
            kind: Some(kind.into()),
            damage_type: Some(Id::from("gunfire")),
            attribute: attribute.map(str::to_string),
            modifier_flag: None,
            amount: Some(amount),
        }
    }

    #[test]
    fn modifiers_say_what_they_do_in_the_games_words() {
        let strings: HashMap<String, String> = [
            ("modifier_desc_add_terrain_speed_cap|s", "Speed limit is %s"),
            ("modifier_desc_add_dv|f", "%+g to Defense Value"),
            (
                "modifier_desc_multiply_control_range|f",
                "%+g%% to Control Range",
            ),
            (
                "modifier_desc_resist_damage_negative|cf",
                "%lc%+g%% vulnerability",
            ),
            (
                "modifier_desc_resist_damage_positive|cf",
                "%lc%+g%% resistance",
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let pace = |range, word: &str| Pace {
            range,
            word: word.into(),
        };
        let paces = [pace([2.0, 3.0], "Slow"), pace([3.0, 4.0], "Medium")];
        let says = |m: defs::Modifier| line(&strings, &paces, &m);
        let said = |words: &str| Some(words.to_string());
        let cap = |amount| modifier("ADD", Some("TERRAIN_SPEED_CAP"), amount);
        assert_eq!(says(cap(2.0)), said("Speed limit is Slow"));
        assert_eq!(says(cap(3.0)), said("Speed limit is Medium"));
        // One no range holds says nothing.
        assert_eq!(says(cap(1.5)), None);
        assert_eq!(
            says(modifier("ADD", Some("DV"), 4.0)),
            said("+4 to Defense Value")
        );
        let wider = modifier("MULTIPLY", Some("control_range"), 0.25);
        assert_eq!(says(wider), said("+25% to Control Range"));
        let resist = |amount| modifier("RESIST_DAMAGE", None, amount);
        assert_eq!(says(resist(-0.2)), said("+20% vulnerability"));
        assert_eq!(says(resist(0.2)), said("+20% resistance"));
        // Nor does one the game has no words for.
        assert_eq!(says(modifier("ADD", Some("LUCK"), 1.0)), None);
    }
}
