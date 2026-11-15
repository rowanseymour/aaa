//! MAP_HEADER, what a map offers before it's loaded, and RULES, laid out as
//! [`rmp.ksy`] specifies.
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use crate::{Cursor, Error, Result, Writer};

/// The MAP_HEADER section, the `map_header` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct MapHeader {
    pub version: u32,
    pub rules: WorldRules,
    pub parameters: WorldParameters,
    pub header_version: u32,
    pub checksum: u32,
    pub name: String,
    pub description: String,
    pub size: Option<[f32; 2]>,
    pub build: f32,
    pub version_number: u32,
    pub app_version: String,
    pub app_id: String,
    pub edition: String,
    pub unused: u8,
    pub path: String,
    pub campaign: String,
    pub campaign_mission: Option<i32>,
    pub saved: u64,
    pub players: Vec<Player>,
    pub creator: WorldCreator,
}

/// A `world_rules` of `rmp.ksy`, as the RULES section and the headers hold.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldRules {
    pub map_type: u32,
    pub game_type: String,
    pub rules: Vec<Rule>,
}

/// A `rule` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub name: String,
    pub locked: bool,
    pub value: RuleValue,
}

/// A [`Rule`]'s value, as its type decides.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RuleValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    Choice(u32),
}

/// A `world_parameters` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldParameters {
    pub version: u32,
    pub teams: Vec<TeamSetup>,
    pub kingdoms: Vec<KingdomSetup>,
}

/// A `team_setup` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct TeamSetup {
    pub name: String,
    pub display_name: String,
}

/// A `kingdom_setup` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct KingdomSetup {
    pub name: String,
    pub independent: bool,
    pub nation: String,
    pub faction: String,
    pub color: String,
    pub handicap: String,
    pub display_name: String,
    pub color_markup: Option<String>,
    pub ai: String,
    pub required: bool,
    pub playable: bool,
    pub team: String,
    pub objectives: Vec<String>,
    pub technologies: Vec<String>,
    pub merge_into: String,
    pub actors: Vec<String>,
    pub removed: bool,
    pub ai_controlled: bool,
    pub resources: Option<Vec<(String, f32)>>,
}

/// A present `player` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub ai: bool,
    pub kingdom: String,
    pub name: String,
    pub local: Option<bool>,
}

/// The `world_creator` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldCreator {
    pub rules: WorldRules,
    pub parameters: WorldParameters,
    pub size: [f32; 2],
    pub biome: String,
    pub weather: String,
    pub seed: u32,
}

const CREATOR_CLASS: &str = "KKC_WorldCreator";

pub(super) fn check_version(
    what: &str,
    version: u32,
    versions: std::ops::RangeInclusive<u32>,
) -> Result<u32> {
    if versions.contains(&version) {
        Ok(version)
    } else {
        Err(Error::Corrupt(format!("{what} version {version}")))
    }
}

pub(super) fn bool(cur: &mut Cursor, what: &str) -> Result<bool> {
    match cur.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        b => Err(Error::Corrupt(format!("{what} is {b}"))),
    }
}

impl MapHeader {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let cur = &mut cur;
        let version = check_version("map header", cur.u32()?, 4..=7)?;
        let header = MapHeader {
            version,
            rules: WorldRules::read(cur)?,
            parameters: WorldParameters::parse(cur)?,
            header_version: cur.u32()?,
            checksum: cur.u32()?,
            name: cur.string()?,
            description: cur.string()?,
            size: (version >= 7).then(|| cur.f32s()).transpose()?,
            build: cur.f32()?,
            version_number: cur.u32()?,
            app_version: cur.string()?,
            app_id: cur.string()?,
            edition: cur.string()?,
            unused: cur.u8()?,
            path: cur.string()?,
            campaign: cur.string()?,
            campaign_mission: (version >= 6).then(|| cur.i32()).transpose()?,
            saved: cur.u64()?,
            players: {
                let mut players = Vec::new();
                while bool(cur, "player presence")? {
                    players.push(Player {
                        ai: bool(cur, "player ai")?,
                        kingdom: cur.string()?,
                        name: cur.string()?,
                        local: (version >= 5)
                            .then(|| bool(cur, "player local"))
                            .transpose()?,
                    });
                }
                players
            },
            creator: WorldCreator::parse(cur)?,
        };
        cur.finish("MAP_HEADER")?;
        Ok(header)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        let version = self.version;
        w.u32(version);
        self.rules.write(&mut w)?;
        self.parameters.write(&mut w)?;
        w.u32(self.header_version);
        w.u32(self.checksum);
        w.string(&self.name)?;
        w.string(&self.description)?;
        optional_plain(version >= 7, "size", self.size, |size| w.f32s(&size))?;
        w.f32(self.build);
        w.u32(self.version_number);
        w.string(&self.app_version)?;
        w.string(&self.app_id)?;
        w.string(&self.edition)?;
        w.u8(self.unused);
        w.string(&self.path)?;
        w.string(&self.campaign)?;
        optional(
            version >= 6,
            "campaign_mission",
            self.campaign_mission,
            |v| {
                w.i32(v);
                Ok(())
            },
        )?;
        w.u64(self.saved);
        for player in &self.players {
            w.u8(1);
            w.u8(player.ai.into());
            w.string(&player.kingdom)?;
            w.string(&player.name)?;
            optional_plain(version >= 5, "player local", player.local, |v| {
                w.u8(v.into())
            })?;
        }
        w.u8(0);
        self.creator.write(&mut w)?;
        Ok(w.0)
    }
}

/// Writes `value` with `write` if the version has it, checking that it's set
/// exactly when it does.
pub(super) fn optional<T>(
    has: bool,
    what: &str,
    value: Option<T>,
    write: impl FnOnce(T) -> Result<()>,
) -> Result<()> {
    match (has, value) {
        (true, Some(v)) => write(v),
        (false, None) => Ok(()),
        _ => Err(Error::Unwritable(format!(
            "{what} must be set exactly when its version has it"
        ))),
    }
}

/// [`optional`] for a value written by a call that can't fail.
pub(super) fn optional_plain<T>(
    has: bool,
    what: &str,
    value: Option<T>,
    write: impl FnOnce(T),
) -> Result<()> {
    optional(has, what, value, |v| {
        write(v);
        Ok(())
    })
}

impl WorldRules {
    const VERSION: u32 = 4;

    /// Parses the RULES section.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let rules = Self::read(&mut cur)?;
        cur.finish("RULES")?;
        Ok(rules)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        self.write(&mut w)?;
        Ok(w.0)
    }

    fn read(cur: &mut Cursor) -> Result<Self> {
        check_version("world rules", cur.u32()?, Self::VERSION..=Self::VERSION)?;
        let map_type = cur.u32()?;
        let game_type = cur.string()?;
        let n = cur.u32()?;
        let rules = (0..n)
            .map(|_| {
                let name = cur.string()?;
                let locked = bool(cur, "rule locked")?;
                let value = match cur.u32()? {
                    0 => RuleValue::Bool(bool(cur, "rule value")?),
                    1 => RuleValue::Int(cur.i32()?),
                    2 => RuleValue::Float(cur.f32()?),
                    3 => RuleValue::Choice(cur.u32()?),
                    t => return Err(Error::Corrupt(format!("rule {name:?} has type {t}"))),
                };
                Ok(Rule {
                    name,
                    locked,
                    value,
                })
            })
            .collect::<Result<_>>()?;
        Ok(WorldRules {
            map_type,
            game_type,
            rules,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(Self::VERSION);
        w.u32(self.map_type);
        w.string(&self.game_type)?;
        w.u32(self.rules.len() as u32);
        for rule in &self.rules {
            w.string(&rule.name)?;
            w.u8(rule.locked.into());
            match rule.value {
                RuleValue::Bool(v) => {
                    w.u32(0);
                    w.u8(v.into());
                }
                RuleValue::Int(v) => {
                    w.u32(1);
                    w.i32(v);
                }
                RuleValue::Float(v) => {
                    w.u32(2);
                    w.f32(v);
                }
                RuleValue::Choice(v) => {
                    w.u32(3);
                    w.u32(v);
                }
            }
        }
        Ok(())
    }
}

impl WorldParameters {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = check_version("world parameters", cur.u32()?, 11..=13)?;
        let teams = (0..cur.u32()?)
            .map(|_| {
                Ok(TeamSetup {
                    name: cur.string()?,
                    display_name: cur.string()?,
                })
            })
            .collect::<Result<_>>()?;
        let kingdoms = (0..cur.u32()?)
            .map(|_| KingdomSetup::parse(cur, version))
            .collect::<Result<_>>()?;
        Ok(WorldParameters {
            version,
            teams,
            kingdoms,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(self.version);
        w.u32(self.teams.len() as u32);
        for team in &self.teams {
            w.string(&team.name)?;
            w.string(&team.display_name)?;
        }
        w.u32(self.kingdoms.len() as u32);
        self.kingdoms
            .iter()
            .try_for_each(|k| k.write(w, self.version))
    }
}

impl KingdomSetup {
    fn parse(cur: &mut Cursor, version: u32) -> Result<Self> {
        Ok(KingdomSetup {
            name: cur.string()?,
            independent: bool(cur, "independent")?,
            nation: cur.string()?,
            faction: cur.string()?,
            color: cur.string()?,
            handicap: cur.string()?,
            display_name: cur.string()?,
            color_markup: (version >= 12).then(|| cur.string()).transpose()?,
            ai: cur.string()?,
            required: bool(cur, "required")?,
            playable: bool(cur, "playable")?,
            team: cur.string()?,
            objectives: cur.strings()?,
            technologies: cur.strings()?,
            merge_into: cur.string()?,
            actors: cur.strings()?,
            removed: bool(cur, "removed")?,
            ai_controlled: bool(cur, "ai controlled")?,
            resources: (version >= 13)
                .then(|| {
                    let n = cur.u32()?;
                    (0..n)
                        .map(|_| Ok((cur.string()?, cur.f32()?)))
                        .collect::<Result<_>>()
                })
                .transpose()?,
        })
    }

    fn write(&self, w: &mut Writer, version: u32) -> Result<()> {
        w.string(&self.name)?;
        w.u8(self.independent.into());
        for s in [
            &self.nation,
            &self.faction,
            &self.color,
            &self.handicap,
            &self.display_name,
        ] {
            w.string(s)?;
        }
        optional(
            version >= 12,
            "color_markup",
            self.color_markup.as_ref(),
            |s| w.string(s),
        )?;
        w.string(&self.ai)?;
        w.u8(self.required.into());
        w.u8(self.playable.into());
        w.string(&self.team)?;
        w.strings(&self.objectives)?;
        w.strings(&self.technologies)?;
        w.string(&self.merge_into)?;
        w.strings(&self.actors)?;
        w.u8(self.removed.into());
        w.u8(self.ai_controlled.into());
        optional(version >= 13, "resources", self.resources.as_ref(), |rs| {
            w.u32(rs.len() as u32);
            rs.iter().try_for_each(|(name, amount)| {
                w.string(name)?;
                w.f32(*amount);
                Ok(())
            })
        })
    }
}

impl WorldCreator {
    const VERSION: u32 = 3;

    fn parse(cur: &mut Cursor) -> Result<Self> {
        let class = cur.string()?;
        if class != CREATOR_CLASS {
            return Err(Error::Corrupt(format!("world creator class {class:?}")));
        }
        check_version("world creator", cur.u32()?, Self::VERSION..=Self::VERSION)?;
        Ok(WorldCreator {
            rules: WorldRules::read(cur)?,
            parameters: WorldParameters::parse(cur)?,
            size: cur.f32s()?,
            biome: cur.string()?,
            weather: cur.string()?,
            seed: cur.u32()?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.string(CREATOR_CLASS)?;
        w.u32(Self::VERSION);
        self.rules.write(w)?;
        self.parameters.write(w)?;
        w.f32s(&self.size);
        w.string(&self.biome)?;
        w.string(&self.weather)?;
        w.u32(self.seed);
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    fn rules() -> WorldRules {
        WorldRules {
            map_type: 0,
            game_type: "NORMAL".into(),
            rules: vec![
                Rule {
                    name: "Multiplayer".into(),
                    locked: false,
                    value: RuleValue::Bool(true),
                },
                Rule {
                    name: "ResourceStockpileDefault".into(),
                    locked: true,
                    value: RuleValue::Float(500.0),
                },
                Rule {
                    name: "DifficultyLevel".into(),
                    locked: false,
                    value: RuleValue::Choice(1),
                },
            ],
        }
    }

    fn parameters(version: u32) -> WorldParameters {
        WorldParameters {
            version,
            teams: vec![TeamSetup {
                name: "team_0".into(),
                display_name: "team_0".into(),
            }],
            kingdoms: vec![KingdomSetup {
                name: "kingdom_0".into(),
                independent: false,
                nation: "nation_ger".into(),
                faction: "rommel".into(),
                color: "blue".into(),
                handicap: String::new(),
                display_name: "kingdom_0".into(),
                color_markup: (version >= 12).then(|| "<color=0,0.3,1>".into()),
                ai: "ger_sai".into(),
                required: true,
                playable: false,
                team: "team_0".into(),
                objectives: vec!["objective_0".into()],
                technologies: Vec::new(),
                merge_into: String::new(),
                actors: Vec::new(),
                removed: false,
                ai_controlled: true,
                resources: (version >= 13).then(|| vec![("money".into(), 250.0)]),
            }],
        }
    }

    pub(in crate::map) fn header(version: u32, parameters_version: u32) -> MapHeader {
        MapHeader {
            version,
            rules: rules(),
            parameters: parameters(parameters_version),
            header_version: version,
            checksum: 0,
            name: "map_name".into(),
            description: "map_description".into(),
            size: (version >= 7).then_some([448.0, 448.0]),
            build: 4250.0,
            version_number: 30100,
            app_version: "0.3.1.0".into(),
            app_id: "A&A".into(),
            edition: "Dev AA Dev".into(),
            unused: 0,
            path: "Maps/test.RMP".into(),
            campaign: String::new(),
            campaign_mission: (version >= 6).then_some(-1),
            saved: 0x01c4_a7e3_629f_3b0e,
            players: vec![Player {
                ai: false,
                kingdom: "kingdom_0".into(),
                name: "Player".into(),
                local: (version >= 5).then_some(true),
            }],
            creator: WorldCreator {
                rules: rules(),
                parameters: parameters(parameters_version),
                size: [448.0, 448.0],
                biome: "temperate_forest".into(),
                weather: "snow".into(),
                seed: 0xb846_ce0c,
            },
        }
    }

    #[test]
    fn round_trips_each_version() {
        for (version, parameters_version) in [(7, 13), (6, 12), (4, 11)] {
            let header = header(version, parameters_version);
            let bytes = header.to_bytes().unwrap();
            assert_eq!(MapHeader::parse(&bytes).unwrap(), header);
        }
    }

    #[test]
    fn lays_out_rules() {
        let mut w = Writer::default();
        rules().write(&mut w).unwrap();
        let mut want = Writer::default();
        want.u32(4);
        want.u32(0);
        want.string("NORMAL").unwrap();
        want.u32(3);
        want.string("Multiplayer").unwrap();
        want.bytes(&[0, 0, 0, 0, 0, 1]);
        want.string("ResourceStockpileDefault").unwrap();
        want.bytes(&[1, 2, 0, 0, 0]);
        want.f32(500.0);
        want.string("DifficultyLevel").unwrap();
        want.bytes(&[0, 3, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(w.0, want.0);
        assert_eq!(rules().to_bytes().unwrap(), want.0, "as RULES");
        assert_eq!(WorldRules::parse(&want.0).unwrap(), rules());
        want.u8(0);
        assert!(WorldRules::parse(&want.0).is_err(), "trailing byte");
    }

    #[test]
    fn refuses_fields_the_version_lacks() {
        let mut header = header(6, 12);
        header.size = Some([1.0, 1.0]);
        assert!(header.to_bytes().is_err());
        let mut bytes = self::header(7, 13).to_bytes().unwrap();
        bytes.push(0);
        assert!(MapHeader::parse(&bytes).is_err());
    }
}
