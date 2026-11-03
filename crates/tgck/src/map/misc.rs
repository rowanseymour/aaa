//! The small sections: MAP, DENIZEN_GROUPS and MISC_DATA, laid out as
//! [`rmp.ksy`] specifies.
//!
//! A denizen group's layout isn't decoded, so DENIZEN_GROUPS is only checked to
//! hold none.
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use super::header::{bool, check_version, optional};
use crate::{Cursor, Error, Result, Writer};

/// Reads MAP, the map resource's data version.
pub(super) fn parse_data_version(data: &[u8]) -> Result<u32> {
    let mut cur = Cursor::new(data);
    let version = check_version("map data", cur.u32()?, 0..=0)?;
    cur.finish("MAP")?;
    Ok(version)
}

/// Checks DENIZEN_GROUPS holds no groups, the only kind decoded.
pub(super) fn parse_denizen_groups(data: &[u8]) -> Result<()> {
    let mut cur = Cursor::new(data);
    match cur.u32()? {
        0 => cur.finish("DENIZEN_GROUPS"),
        n => Err(Error::Corrupt(format!(
            "{n} denizen groups; their layout isn't decoded"
        ))),
    }
}

/// The DENIZEN_GROUPS section holding no groups.
pub fn write_denizen_groups() -> Vec<u8> {
    0u32.to_le_bytes().to_vec()
}

/// World state that didn't fit elsewhere.
#[derive(Debug, Clone, PartialEq)]
pub struct MiscData {
    pub used_names: Vec<UsedName>,
    /// The speed to go back to when a cutscene ends.
    pub pre_cutscene_game_speed: f32,
    /// Empty for none.
    pub active_weather: String,
    /// The UI script events triggers listen for (the
    /// `effect_ui_script_listen` effect); from world version 24 on.
    pub ui_script_listens: Option<Vec<String>>,
    /// From world version 28 on.
    pub game_counter: Option<Option<GameCounter>>,
}

/// A name the game has given out from one of its name lists, so it isn't
/// given twice.
#[derive(Debug, Clone, PartialEq)]
pub struct UsedName {
    pub name_list: String,
    pub index: u32,
}

/// The countdown a trigger shows on screen (`game_counter_show`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameCounter {
    /// The game time it was started at.
    pub start_time: f32,
    /// Seconds it counts down from.
    pub duration: f32,
}

impl MiscData {
    /// The first world version whose MISC_DATA the game reads from the
    /// section; older ones read it from wherever the stream happens to be.
    const MIN_WORLD_VERSION: u32 = 18;

    pub fn parse(data: &[u8], world_version: u32) -> Result<Self> {
        check_version(
            "MISC_DATA world",
            world_version,
            Self::MIN_WORLD_VERSION..=u32::MAX,
        )?;
        let mut cur = Cursor::new(data);
        let cur = &mut cur;
        let n = cur.u32()?;
        let misc = MiscData {
            used_names: (0..n)
                .map(|_| {
                    Ok(UsedName {
                        name_list: cur.string()?,
                        index: cur.u32()?,
                    })
                })
                .collect::<Result<_>>()?,
            pre_cutscene_game_speed: cur.f32()?,
            active_weather: cur.string()?,
            ui_script_listens: (world_version >= 24)
                .then(|| {
                    let n = cur.u32()?;
                    (0..n).map(|_| cur.string()).collect()
                })
                .transpose()?,
            game_counter: (world_version >= 28)
                .then(|| GameCounter::parse(cur))
                .transpose()?,
        };
        cur.finish("MISC_DATA")?;
        Ok(misc)
    }

    pub fn to_bytes(&self, world_version: u32) -> Result<Vec<u8>> {
        check_version(
            "MISC_DATA world",
            world_version,
            Self::MIN_WORLD_VERSION..=u32::MAX,
        )?;
        let mut w = Writer::default();
        w.u32(self.used_names.len() as u32);
        for used in &self.used_names {
            w.string(&used.name_list)?;
            w.u32(used.index);
        }
        w.f32(self.pre_cutscene_game_speed);
        w.string(&self.active_weather)?;
        optional(
            world_version >= 24,
            "ui_script_listens",
            self.ui_script_listens.as_ref(),
            |names| {
                w.u32(names.len() as u32);
                names.iter().try_for_each(|name| w.string(name))
            },
        )?;
        optional(
            world_version >= 28,
            "game_counter",
            self.game_counter,
            |counter| {
                w.u8(counter.is_some().into());
                if let Some(counter) = counter {
                    w.f32(counter.start_time);
                    w.f32(counter.duration);
                }
                Ok(())
            },
        )?;
        Ok(w.0)
    }
}

impl GameCounter {
    /// Reads one if the presence byte says there is one.
    fn parse(cur: &mut Cursor) -> Result<Option<Self>> {
        if !bool(cur, "game counter presence")? {
            return Ok(None);
        }
        Ok(Some(GameCounter {
            start_time: cur.f32()?,
            duration: cur.f32()?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::string;

    fn misc(world_version: u32) -> MiscData {
        MiscData {
            used_names: vec![UsedName {
                name_list: "names_usa_soldier".into(),
                index: 3,
            }],
            pre_cutscene_game_speed: 1.0,
            active_weather: "snow_light".into(),
            ui_script_listens: (world_version >= 24).then(|| vec!["on_close".into()]),
            game_counter: (world_version >= 28).then_some(Some(GameCounter {
                start_time: 12.5,
                duration: 300.0,
            })),
        }
    }

    #[test]
    fn round_trips_misc_data() {
        for world_version in [21, 24, 28] {
            let misc = misc(world_version);
            let bytes = misc.to_bytes(world_version).unwrap();
            assert_eq!(MiscData::parse(&bytes, world_version).unwrap(), misc);
        }
        let mut no_counter = misc(28);
        no_counter.game_counter = Some(None);
        let bytes = no_counter.to_bytes(28).unwrap();
        assert_eq!(bytes.last(), Some(&0));
        assert_eq!(MiscData::parse(&bytes, 28).unwrap(), no_counter);
    }

    #[test]
    fn lays_out_misc_data() {
        // As in the snowy maps of world version 27.
        let mut want = vec![0; 8];
        want.extend(string("snow"));
        want.extend([0; 4]);
        let misc = MiscData::parse(&want, 27).unwrap();
        assert_eq!(misc.active_weather, "snow");
        assert_eq!(misc.ui_script_listens, Some(vec![]));
        assert_eq!(misc.game_counter, None);
        assert_eq!(misc.to_bytes(27).unwrap(), want);
    }

    #[test]
    fn refuses_what_the_version_lacks() {
        assert!(misc(28).to_bytes(27).is_err());
        assert!(misc(21).to_bytes(24).is_err());
        let bytes = misc(21).to_bytes(21).unwrap();
        assert!(MiscData::parse(&bytes, 24).is_err(), "too short");
        assert!(
            MiscData::parse(&bytes, 17).is_err(),
            "not read from MISC_DATA"
        );
    }

    #[test]
    fn reads_small_sections() {
        assert_eq!(parse_data_version(&[0; 4]).unwrap(), 0);
        assert!(parse_data_version(&[1, 0, 0, 0]).is_err());
        assert!(parse_data_version(&[0; 5]).is_err());
        assert!(parse_denizen_groups(&write_denizen_groups()).is_ok());
        assert!(parse_denizen_groups(&[1, 0, 0, 0]).is_err());
    }
}
