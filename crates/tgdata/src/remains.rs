//! How long the remains of what dies lie on the map before they're taken
//! off, and the deaths units play before that time starts.

use std::collections::HashMap;

use crate::{GameData, Id, defs, scoped};

/// The seconds the remains of a body that sets none rot, then sink.
const ROT_TIME: f32 = 0.0;
const SINK_TIME: f32 = 2.0;

/// The seconds a death animation that sets no length takes.
const DEATH_LENGTH: f32 = 1.0;

/// A death animation, as what picks one sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Death {
    /// Which of its kind, e.g. `die2`.
    pub ids: Id,
    /// The [`GameData::damage_filters`] value of the last hit it plays
    /// after; [`ANY_DAMAGE`] for any.
    pub filter: i32,
    /// The least damage that hit did, where there are others to choose.
    pub threshold: f32,
    pub weight: f32,
    /// Seconds it takes.
    pub length: f32,
}

/// The [`Death::filter`] that fits every hit.
pub const ANY_DAMAGE: i32 = -1;

/// What a unit no enemy has hit dies of, for [`Death::filter`].
pub const BLEEDING: i32 = 666;

fn deaths(t: &defs::Thing) -> Option<Vec<Death>> {
    // Aircraft never play one.
    let flying = t.flags.iter().rev().find_map(|f| f.flying) == Some(true);
    let character = t.character_component.as_ref().filter(|_| !flying)?;
    let dies = (character.animation.iter()).filter(|a| {
        a.kind
            .as_deref()
            .is_some_and(|k| k.eq_ignore_ascii_case("die"))
    });
    let deaths: Vec<Death> = dies
        .map(|a| Death {
            ids: Id::from(a.ids.as_deref().unwrap_or("")),
            filter: a.damage_filter.unwrap_or(ANY_DAMAGE),
            threshold: a.damage_threshold.unwrap_or(0.0),
            weight: a.weight.unwrap_or(1.0),
            length: a.length.unwrap_or(DEATH_LENGTH),
        })
        .collect();
    (!deaths.is_empty()).then_some(deaths)
}

impl GameData {
    /// Seconds each template's things' remains lie rotting once dead, then
    /// sink before they leave the map, by template id, for those with a
    /// body.
    pub fn remains(&self) -> HashMap<Id, [f32; 2]> {
        self.by_template(|t| {
            let body = t.body_component.as_ref()?;
            Some([
                body.rot_time.unwrap_or(ROT_TIME),
                body.sink_time.unwrap_or(SINK_TIME),
            ])
        })
    }

    /// The death animations of each template's things, for those that
    /// play one.
    pub fn deaths(&self) -> HashMap<Id, Vec<Death>> {
        self.by_template(deaths)
    }

    /// Each kind of damage's number for death animations, by its id.
    pub fn damage_filters(&self) -> HashMap<Id, i32> {
        let files = self.parse_tgis(|n| n.eq_ignore_ascii_case("game/damage_types.tgi"));
        let files = scoped(&files);
        let types = files
            .iter()
            .flat_map(|f| f.read::<defs::DamageType>("DamageType"));
        types
            .filter_map(|d| Some((d.ids?, d.damage_filter?)))
            .collect()
    }
}
