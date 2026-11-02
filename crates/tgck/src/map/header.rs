//! MAP_HEADER: what a map offers before it's loaded — its game options, sides
//! and editor details.
//!
//! ```text
//! MAP_HEADER
//!     u32 version (4-7), rules, parameters,
//!     u32 unknown (the version again), u32 unknown (0),
//!     string name, string description      string ids, defined in MAPSTRINGS
//!     v7: f32 size[2]
//!     f32 unknown, u32 editor_build, string editor_version, string game,
//!     string author, u8 unknown, string path, string unknown,
//!     v6: i32 unknown (-1)
//!     u64 saved                            a Windows FILETIME
//!     players: per player u8 1, u8 unknown, string kingdom, string name,
//!       v5: u8 unknown; then u8 0
//!     string creator_class ("KKC_WorldCreator"), u32 version (3), rules,
//!     parameters, f32 size[2], string biome, string weather, u32 seed
//!
//! rules       u32 version (4), u32 unknown (0), string game_type,
//!             u32 n, rule[n]: string name, u8 unknown, u32 type, value:
//!             u8 for type 0 (bool), else 4 bytes (1 int, 2 float, 3 choice)
//! parameters  u32 version (11-13), u32 n, (string name, string display_name)[n]
//!             for teams, u32 n, kingdom[n]:
//!     string name, u8 independent, string nation, string faction,
//!     string color, string handicap, string display_name,
//!     v12: string color_markup, string ai, u8 unknown, u8 unknown,
//!     string team, u32 n, string objective[n],
//!     u32 n, string shared_property[n], string unknown,
//!     u32 n, string actor[n], u8 unknown, u8 unknown,
//!     v13: u32 n, (string resource, f32 amount)[n]
//! ```
//!
//! The creator holds a second copy of the rules and parameters, the same in
//! every shipped map.

use crate::{Cursor, Error, Result, Writer};

#[derive(Debug, Clone, PartialEq)]
pub struct MapHeader {
    pub version: u32,
    pub rules: WorldRules,
    pub parameters: WorldParameters,
    pub unknown: [u32; 2],
    /// A string id, defined in MAPSTRINGS.
    pub name: String,
    /// A string id, defined in MAPSTRINGS.
    pub description: String,
    /// From version 7 on.
    pub size: Option<[f32; 2]>,
    pub unknown_f32: f32,
    pub editor_build: u32,
    pub editor_version: String,
    pub game: String,
    pub author: String,
    pub unknown_u8: u8,
    /// The map's path when it was saved.
    pub path: String,
    pub unknown_string: String,
    /// From version 6 on.
    pub unknown_i32: Option<i32>,
    /// When the map was saved, as a Windows FILETIME.
    pub saved: u64,
    pub players: Vec<Player>,
    pub creator: WorldCreator,
}

/// The game options a map is played with.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldRules {
    pub unknown: u32,
    pub game_type: String,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub name: String,
    pub unknown: u8,
    pub value: RuleValue,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RuleValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    /// The index of one of the rule's choices.
    Choice(u32),
}

/// The sides a map is set up with.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldParameters {
    pub version: u32,
    pub teams: Vec<TeamSetup>,
    pub kingdoms: Vec<KingdomSetup>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeamSetup {
    pub name: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KingdomSetup {
    pub name: String,
    pub independent: bool,
    pub nation: String,
    pub faction: String,
    pub color: String,
    pub handicap: String,
    pub display_name: String,
    /// Text markup in the kingdom's colour; from version 12 on.
    pub color_markup: Option<String>,
    /// The strategic AI that plays it, if set.
    pub ai: String,
    pub unknown: [u8; 2],
    pub team: String,
    pub objectives: Vec<String>,
    pub shared_properties: Vec<String>,
    pub unknown_string: String,
    pub actors: Vec<String>,
    pub unknown2: [u8; 2],
    /// From version 13 on.
    pub resources: Option<Vec<(String, f32)>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub unknown: u8,
    pub kingdom: String,
    pub name: String,
    /// From version 5 on.
    pub unknown2: Option<u8>,
}

/// What the editor generated the map from.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldCreator {
    pub rules: WorldRules,
    pub parameters: WorldParameters,
    pub size: [f32; 2],
    pub biome: String,
    /// Empty for none.
    pub weather: String,
    pub seed: u32,
}

const CREATOR_CLASS: &str = "KKC_WorldCreator";

fn check_version(what: &str, version: u32, versions: std::ops::RangeInclusive<u32>) -> Result<u32> {
    if versions.contains(&version) {
        Ok(version)
    } else {
        Err(Error::Corrupt(format!("{what} version {version}")))
    }
}

fn strings(cur: &mut Cursor) -> Result<Vec<String>> {
    let n = cur.u32()?;
    (0..n).map(|_| cur.string()).collect()
}

fn write_strings(w: &mut Writer, vs: &[String]) -> Result<()> {
    w.u32(vs.len() as u32);
    vs.iter().try_for_each(|s| w.string(s))
}

fn bool(cur: &mut Cursor, what: &str) -> Result<bool> {
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
            rules: WorldRules::parse(cur)?,
            parameters: WorldParameters::parse(cur)?,
            unknown: [cur.u32()?, cur.u32()?],
            name: cur.string()?,
            description: cur.string()?,
            size: (version >= 7).then(|| cur.f32s()).transpose()?,
            unknown_f32: cur.f32()?,
            editor_build: cur.u32()?,
            editor_version: cur.string()?,
            game: cur.string()?,
            author: cur.string()?,
            unknown_u8: cur.u8()?,
            path: cur.string()?,
            unknown_string: cur.string()?,
            unknown_i32: (version >= 6).then(|| cur.i32()).transpose()?,
            saved: cur.u64()?,
            players: {
                let mut players = Vec::new();
                while bool(cur, "player presence")? {
                    players.push(Player {
                        unknown: cur.u8()?,
                        kingdom: cur.string()?,
                        name: cur.string()?,
                        unknown2: (version >= 5).then(|| cur.u8()).transpose()?,
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
        self.unknown.iter().for_each(|&v| w.u32(v));
        w.string(&self.name)?;
        w.string(&self.description)?;
        optional(version >= 7, "size", self.size, |size| {
            w.f32s(&size);
            Ok(())
        })?;
        w.f32(self.unknown_f32);
        w.u32(self.editor_build);
        w.string(&self.editor_version)?;
        w.string(&self.game)?;
        w.string(&self.author)?;
        w.u8(self.unknown_u8);
        w.string(&self.path)?;
        w.string(&self.unknown_string)?;
        optional(version >= 6, "unknown_i32", self.unknown_i32, |v| {
            w.i32(v);
            Ok(())
        })?;
        w.u64(self.saved);
        for player in &self.players {
            w.u8(1);
            w.u8(player.unknown);
            w.string(&player.kingdom)?;
            w.string(&player.name)?;
            optional(version >= 5, "player unknown2", player.unknown2, |v| {
                w.u8(v);
                Ok(())
            })?;
        }
        w.u8(0);
        self.creator.write(&mut w)?;
        Ok(w.0)
    }
}

/// Writes `value` with `write` if the version has it, checking that it's set
/// exactly when it does.
fn optional<T>(
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

impl WorldRules {
    const VERSION: u32 = 4;

    fn parse(cur: &mut Cursor) -> Result<Self> {
        check_version("world rules", cur.u32()?, Self::VERSION..=Self::VERSION)?;
        let unknown = cur.u32()?;
        let game_type = cur.string()?;
        let n = cur.u32()?;
        let rules = (0..n)
            .map(|_| {
                let name = cur.string()?;
                let unknown = cur.u8()?;
                let value = match cur.u32()? {
                    0 => RuleValue::Bool(bool(cur, "rule value")?),
                    1 => RuleValue::Int(cur.i32()?),
                    2 => RuleValue::Float(cur.f32()?),
                    3 => RuleValue::Choice(cur.u32()?),
                    t => return Err(Error::Corrupt(format!("rule {name:?} has type {t}"))),
                };
                Ok(Rule {
                    name,
                    unknown,
                    value,
                })
            })
            .collect::<Result<_>>()?;
        Ok(WorldRules {
            unknown,
            game_type,
            rules,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(Self::VERSION);
        w.u32(self.unknown);
        w.string(&self.game_type)?;
        w.u32(self.rules.len() as u32);
        for rule in &self.rules {
            w.string(&rule.name)?;
            w.u8(rule.unknown);
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
            unknown: [cur.u8()?, cur.u8()?],
            team: cur.string()?,
            objectives: strings(cur)?,
            shared_properties: strings(cur)?,
            unknown_string: cur.string()?,
            actors: strings(cur)?,
            unknown2: [cur.u8()?, cur.u8()?],
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
        w.bytes(&self.unknown);
        w.string(&self.team)?;
        write_strings(w, &self.objectives)?;
        write_strings(w, &self.shared_properties)?;
        w.string(&self.unknown_string)?;
        write_strings(w, &self.actors)?;
        w.bytes(&self.unknown2);
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
            rules: WorldRules::parse(cur)?,
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
mod tests {
    use super::*;

    fn rules() -> WorldRules {
        WorldRules {
            unknown: 0,
            game_type: "NORMAL".into(),
            rules: vec![
                Rule {
                    name: "Multiplayer".into(),
                    unknown: 0,
                    value: RuleValue::Bool(true),
                },
                Rule {
                    name: "ResourceStockpileDefault".into(),
                    unknown: 1,
                    value: RuleValue::Float(500.0),
                },
                Rule {
                    name: "DifficultyLevel".into(),
                    unknown: 0,
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
                unknown: [1, 0],
                team: "team_0".into(),
                objectives: vec!["objective_0".into()],
                shared_properties: Vec::new(),
                unknown_string: String::new(),
                actors: Vec::new(),
                unknown2: [0, 1],
                resources: (version >= 13).then(|| vec![("money".into(), 250.0)]),
            }],
        }
    }

    fn header(version: u32, parameters_version: u32) -> MapHeader {
        MapHeader {
            version,
            rules: rules(),
            parameters: parameters(parameters_version),
            unknown: [version, 0],
            name: "map_name".into(),
            description: "map_description".into(),
            size: (version >= 7).then_some([448.0, 448.0]),
            unknown_f32: 4250.0,
            editor_build: 30100,
            editor_version: "0.3.1.0".into(),
            game: "A&A".into(),
            author: "someone".into(),
            unknown_u8: 0,
            path: "Maps/test.RMP".into(),
            unknown_string: String::new(),
            unknown_i32: (version >= 6).then_some(-1),
            saved: 0x01c4_a7e3_629f_3b0e,
            players: vec![Player {
                unknown: 0,
                kingdom: "kingdom_0".into(),
                name: "Player".into(),
                unknown2: (version >= 5).then_some(1),
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
