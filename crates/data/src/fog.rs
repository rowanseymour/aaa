//! What each template sees, and how much of the map a match hides.

use std::collections::{HashMap, HashSet};

use crate::{GameData, Id, defs, scoped};

/// How far a thing sees, in world units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sight {
    /// The radius of the ground it shows its own side.
    pub range: f32,
    /// The radius of the ground round it shown to the enemies fighting it.
    pub remote: f32,
}

/// How much of the map a match hides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FogSetting {
    /// Nothing: every side sees everywhere.
    None,
    /// Only what's out of sight, the whole map being explored at the start.
    Explored,
    /// Everything out of sight, nothing being explored at the start.
    #[default]
    Full,
}

impl FogSetting {
    /// The setting a world rule's choice or a tuning value names.
    pub fn named(name: &str) -> Option<FogSetting> {
        Some(match name.trim().to_ascii_uppercase().as_str() {
            "NONE" | "0" => FogSetting::None,
            "EXPLORED" | "1" => FogSetting::Explored,
            "FULL" | "2" => FogSetting::Full,
            _ => return None,
        })
    }
}

/// A template's [`Sight`], if it sees at all, its ranges corrected as the
/// game corrects them as it loads.
pub(crate) fn sight(t: &defs::Thing) -> Option<Sight> {
    let d = t.detection_component.as_ref()?;
    let mut range = d.detection_range.unwrap_or(0.0);
    let mut remote = d.remote_detection_range.unwrap_or(0.0);
    let organization = t.organization_component.as_ref();
    let control = organization.and_then(|o| o.control_range).unwrap_or(0.0);
    let defends = (t.defender_component.as_ref())
        .and_then(|d| d.control_range)
        .unwrap_or(0.0);
    let supply = (t.supply_component.as_ref())
        .and_then(|s| s.supply_range)
        .unwrap_or(0.0);
    let denizen = t.denizen_component.as_ref();
    let guard = denizen.and_then(|d| d.guard_range).unwrap_or(0.0);
    let guarding = (denizen.and_then(|d| d.guard_operational_range)).unwrap_or(0.0);
    // A thing sees at least as far as it reaches.
    let reach = control.max(defends);
    if range < reach.max(supply).max(guard).max(guarding) {
        range = range.max(reach.max(supply));
    }
    if remote <= 0.0 && defends <= 0.0 {
        return None;
    }
    if organization.is_some() {
        remote = control;
    }
    Some(Sight { range, remote })
}

impl GameData {
    /// Each template's [`Sight`], for those that see, by template id.
    pub fn sights(&self) -> HashMap<Id, Sight> {
        self.by_template(sight)
    }

    /// The templates whose things every side has explored from the start
    /// when the whole map starts explored.
    pub fn revealed(&self) -> HashSet<Id> {
        let revealed = |t: &defs::Thing| t.flags.iter().rev().find_map(|f| f.revealed);
        (self.by_template(|t| revealed(t).filter(|&r| r)))
            .into_keys()
            .collect()
    }

    /// The match's fog setting: the world rule's default, the game's own
    /// before the engine's, else the tuning value.
    pub fn fog_setting(&self) -> FogSetting {
        let files =
            self.parse_tgis(|n| n == "game/world_rules_aa.tgi" || n == "game/world_rules.tgi");
        let mut files = scoped(&files);
        files.sort_by_key(|f| !f.file.to_ascii_lowercase().ends_with("_aa.tgi"));
        let rule = files
            .iter()
            .flat_map(|f| f.read::<defs::WorldRule>("WorldRule"))
            .filter(|r| {
                (r.ids.as_ref()).is_some_and(|id| id.as_str().eq_ignore_ascii_case("FogOfWar"))
            })
            .find_map(|r| FogSetting::named(r.default.as_deref()?));
        let tuned = || {
            let svars = self.svars();
            let value = svars.iter().find_map(|s| s.fog_of_war)?;
            FogSetting::named(&value.to_string())
        };
        rule.or_else(tuned).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thing(body: &str) -> defs::Thing {
        let parsed = tgi::parse(&format!("[Thing]\n{{\n\tIDS = t\n{body}}}\n"));
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        templates.deserialize(block).unwrap()
    }

    #[test]
    fn things_see_as_far_as_they_reach() {
        let city = thing(
            "\t[DetectionComponent]\n\tdetection_range = 55\n\tremote_detection_range = 30\n",
        );
        assert_eq!(
            sight(&city),
            Some(Sight {
                range: 55.0,
                remote: 30.0
            })
        );
        assert_eq!(sight(&thing("\t[BodyComponent]\n\thealth = 5\n")), None);

        let depot = thing(
            "\t[DetectionComponent]\n\tdetection_range = 16\n\tremote_detection_range = 12\n\
             \t[SupplyComponent]\n\tsupply_range = 40\n",
        );
        assert_eq!(
            sight(&depot),
            Some(Sight {
                range: 40.0,
                remote: 12.0
            })
        );

        // A regiment shows its enemies its zone of control.
        let regiment = thing(
            "\t[DetectionComponent]\n\tdetection_range = 26\n\tremote_detection_range = 12\n\
             \t[OrganizationComponent]\n\t{\n\t\tcontrol_range = 20\n\t}\n",
        );
        assert_eq!(
            sight(&regiment),
            Some(Sight {
                range: 26.0,
                remote: 20.0
            })
        );

        // Mines, with no remote range and nothing they defend, see nothing.
        let mine =
            thing("\t[DetectionComponent]\n\tdetection_range = 0\n\tremote_detection_range = 0\n");
        assert_eq!(sight(&mine), None);
    }

    #[test]
    fn settings_read_by_name_or_number() {
        assert_eq!(FogSetting::named("EXPLORED"), Some(FogSetting::Explored));
        assert_eq!(FogSetting::named("full"), Some(FogSetting::Full));
        assert_eq!(FogSetting::named("0"), Some(FogSetting::None));
        assert_eq!(FogSetting::named("3"), None);
    }
}
