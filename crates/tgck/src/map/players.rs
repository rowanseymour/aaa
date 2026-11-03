//! The sides of a map: its teams and the kingdoms on them.
//!
//! ```text
//! GWORLD      u32 world_version, then not decoded
//! TEAMS       u32 count, then per team: u8 present, and if present:
//!     head: string template, u32 class (5), u32 id, u32 version (2),
//!           string name, u32 unknown (0), f32 position[3], f32 facing[3],
//!           u32 flags
//!     u32 version (0 or 1), u32 index, string display_name,
//!     u32 n, u32 kingdom_relation[n], u32 m, u32 team_relation[m],
//!     v1: string restriction_group
//! KINGDOMS    u32 majors, u32 count, then per kingdom: u8 present, and if
//!             present:
//!     a thing record (class 4, as in DYNAMIC_THINGS) whose actor has no
//!       component but Properties, holding no properties
//!     the actor's trailer: i32 unknown, v2: u32, v3-11: u32,
//!       v4: u8 flag [string], v5: u8 flag [u32], v6: string, v8: u32 u32,
//!       v10: u32, v11: three u32 lists, then u32 n and 12 bytes each,
//!       v13: u32
//!     world_version 22+: u32 version (1), i32 updatable id
//!     u32 version (13-20), u32 index, i32 major_index (-1 if independent),
//!     u32 independent, string name, string color,
//!     u32 n, u32 relation[n], u32 n, (string resource, f32 amount)[n],
//!     u32 team (its thing id, 0 for none), u32 n, u32 team_relation[n],
//!     u8 unknown, u32 version (1), u32 recruits (0), i32 unknown,
//!     u8 has_faction, if set: string faction, u32 version (0), u32 unknown,
//!       u32 n, ability[n]: u32 version (0 or 1), u32 state, f32 time,
//!       u32 unknown, u8 unknown, v1: u32 n, u32 target[n]
//!     string nation
//!     the rest, mostly the strategic AI's state: not decoded
//! ```
//!
//! Relations are listed by the position of the kingdom or team they're with;
//! a kingdom's relation to itself is 0.
//! Teams' and kingdoms' indexes are their positions, which the game requires.
//!
//! A kingdom's undecoded rest isn't prefixed with its length, so it's taken
//! to run up to the next record that parses as the following kingdom, less
//! one byte for each empty position in between.

use super::{Actor, Detail, Head, Thing, expect};
use crate::{Cursor, Error, Result, Writer};

/// A side kingdoms join, sharing victory.
#[derive(Debug, Clone, PartialEq)]
pub struct Team {
    pub head: Head,
    pub version: u32,
    /// Its name as shown, a string id.
    pub display_name: String,
    pub kingdom_relations: Vec<u32>,
    pub team_relations: Vec<u32>,
    /// From version 1 on.
    pub restriction_group: Option<String>,
}

const TEAM: u32 = 5;
const KINGDOM: u32 = 4;

pub(super) fn parse_teams(data: &[u8]) -> Result<Vec<Option<Team>>> {
    let mut cur = Cursor::new(data);
    let count = cur.u32()?;
    let teams = (0..count)
        .map(|index| {
            present(&mut cur)?
                .then(|| Team::parse(&mut cur, index))
                .transpose()
        })
        .collect::<Result<_>>()?;
    cur.finish("TEAMS")?;
    Ok(teams)
}

/// The TEAMS section holding `teams`.
pub fn write_teams(teams: &[Option<Team>]) -> Result<Vec<u8>> {
    let mut w = Writer::default();
    w.u32(teams.len() as u32);
    for (index, team) in teams.iter().enumerate() {
        w.u8(team.is_some().into());
        if let Some(team) = team {
            team.write(&mut w, index as u32)?;
        }
    }
    Ok(w.0)
}

fn present(cur: &mut Cursor) -> Result<bool> {
    match cur.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        b => Err(Error::Corrupt(format!("presence byte is {b}"))),
    }
}

fn parse_head(cur: &mut Cursor, class: u32) -> Result<Head> {
    let head = Head::parse(cur)?;
    if head.class != class {
        return Err(Error::Corrupt(format!(
            "class {} where {class} was expected",
            head.class
        )));
    }
    Ok(head)
}

fn u32s(cur: &mut Cursor) -> Result<Vec<u32>> {
    let n = cur.u32()?;
    (0..n).map(|_| cur.u32()).collect()
}

fn write_u32s(w: &mut Writer, vs: &[u32]) {
    w.u32(vs.len() as u32);
    vs.iter().for_each(|&v| w.u32(v));
}

impl Team {
    const MAX_VERSION: u32 = 1;

    fn parse(cur: &mut Cursor, index: u32) -> Result<Self> {
        let head = parse_head(cur, TEAM)?;
        let version = cur.u32()?;
        if version > Self::MAX_VERSION {
            return Err(Error::Corrupt(format!("team version {version}")));
        }
        expect(cur, "team index", index)?;
        Ok(Team {
            head,
            version,
            display_name: cur.string()?,
            kingdom_relations: u32s(cur)?,
            team_relations: u32s(cur)?,
            restriction_group: (version >= 1).then(|| cur.string()).transpose()?,
        })
    }

    fn write(&self, w: &mut Writer, index: u32) -> Result<()> {
        self.head.write(w)?;
        w.u32(self.version);
        w.u32(index);
        w.string(&self.display_name)?;
        write_u32s(w, &self.kingdom_relations);
        write_u32s(w, &self.team_relations);
        if let Some(group) = &self.restriction_group {
            w.string(group)?;
        }
        Ok(())
    }
}

/// The KINGDOMS section.
#[derive(Debug, Clone)]
pub struct Kingdoms {
    /// How many kingdoms are major, that is not independent.
    pub majors: u32,
    pub kingdoms: Vec<Option<Kingdom>>,
}

/// A player, human or AI, or an independent power such as neutral civilians.
#[derive(Debug, Clone)]
pub struct Kingdom {
    /// The shared fields; the detail is the [`Actor`], whose state holds its
    /// Properties component's and its trailer, undecoded.
    pub thing: Thing,
    /// Present in maps saved from world version 22 on.
    pub updatable: Option<i32>,
    pub version: u32,
    /// Its position among the major kingdoms; -1 if independent.
    pub major_index: i32,
    /// 1 if independent.
    pub independent: u32,
    pub name: String,
    pub color: String,
    pub relations: Vec<u32>,
    /// Stockpiles by resource id, e.g. `money` or `ammo`.
    pub resources: Vec<(String, f32)>,
    /// The [`Team`]'s head id; 0 for none.
    pub team: u32,
    pub team_relations: Vec<u32>,
    pub unknown: u8,
    pub recruit_unknown: i32,
    /// The commander, set on campaign maps and chosen in the lobby otherwise.
    pub faction: Option<Faction>,
    /// The nation id, e.g. `nation_gbr`; empty when chosen in the lobby.
    pub nation: String,
    pub rest: Vec<u8>,
}

/// A kingdom's commander and the state of their abilities.
#[derive(Debug, Clone, PartialEq)]
pub struct Faction {
    /// The faction id, e.g. `montgomery`.
    pub name: String,
    pub unknown: u32,
    pub abilities: Vec<Ability>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ability {
    pub version: u32,
    pub state: u32,
    pub time: f32,
    pub unknown: u32,
    pub unknown2: u8,
    /// From version 1 on.
    pub targets: Option<Vec<u32>>,
}

/// The world version from which kingdoms save an updatable id.
const UPDATABLE_WORLD_VERSION: u32 = 22;

impl Kingdoms {
    pub(super) fn parse(data: &[u8], world_version: u32) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let majors = cur.u32()?;
        let count = cur.u32()?;
        let mut kingdoms: Vec<Option<Kingdom>> = Vec::new();
        let mut at = cur.pos();
        while kingdoms.len() < count as usize {
            let index = kingdoms.len() as u32;
            let mut cur = Cursor::new(&data[at..]);
            if !present(&mut cur)? {
                kingdoms.push(None);
                at += 1;
                continue;
            }
            let mut kingdom = Kingdom::parse_decoded(&mut cur, world_version, index)?;
            let rest_at = at + cur.pos();
            let (next, empty) = match next_kingdom(data, rest_at, world_version, index, count) {
                Some((next, next_index)) => (next, next_index - index - 1),
                None => (data.len(), count - index - 1),
            };
            let rest_end = next
                .checked_sub(empty as usize)
                .filter(|&end| end >= rest_at && data[end..next].iter().all(|&b| b == 0))
                .ok_or_else(|| {
                    Error::Corrupt(format!("kingdom {index}'s end can't be told apart"))
                })?;
            kingdom.rest = data[rest_at..rest_end].to_vec();
            kingdoms.push(Some(kingdom));
            kingdoms.extend((0..empty).map(|_| None));
            at = next;
        }
        Ok(Kingdoms { majors, kingdoms })
    }

    pub fn to_bytes(&self, world_version: u32) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        w.u32(self.majors);
        w.u32(self.kingdoms.len() as u32);
        for (index, kingdom) in self.kingdoms.iter().enumerate() {
            w.u8(kingdom.is_some().into());
            if let Some(kingdom) = kingdom {
                kingdom.write(&mut w, world_version, index as u32)?;
            }
        }
        Ok(w.0)
    }
}

/// Where the first kingdom after `index` starts, at or after `from`, and its
/// index.
fn next_kingdom(
    data: &[u8],
    from: usize,
    world_version: u32,
    index: u32,
    count: u32,
) -> Option<(usize, u32)> {
    (from..data.len()).find_map(|at| {
        (data[at] == 1).then_some(())?;
        (index + 1..count).find_map(|next| {
            let mut cur = Cursor::new(&data[at + 1..]);
            Kingdom::parse_decoded(&mut cur, world_version, next)
                .ok()
                .map(|_| (at, next))
        })
    })
}

impl Kingdom {
    const MIN_VERSION: u32 = 13;
    const MAX_VERSION: u32 = 20;

    /// Everything but the rest.
    fn parse_decoded(cur: &mut Cursor, world_version: u32, index: u32) -> Result<Self> {
        let head = parse_head(cur, KINGDOM)?;
        let mut thing = Thing::parse_placement(head, cur)?;
        thing.detail = Detail::Actor(Actor::parse(cur, world_version)?);

        let updatable = (world_version >= UPDATABLE_WORLD_VERSION)
            .then(|| {
                expect(cur, "kingdom updatable version", 1)?;
                cur.i32()
            })
            .transpose()?;
        let version = cur.u32()?;
        if !(Self::MIN_VERSION..=Self::MAX_VERSION).contains(&version) {
            return Err(Error::Corrupt(format!("kingdom version {version}")));
        }
        expect(cur, "kingdom index", index)?;
        let major_index = cur.i32()?;
        let independent = cur.u32()?;
        let name = cur.string()?;
        let color = cur.string()?;
        let relations = u32s(cur)?;
        let n = cur.u32()?;
        let resources = (0..n)
            .map(|_| Ok((cur.string()?, cur.f32()?)))
            .collect::<Result<_>>()?;
        let team = cur.u32()?;
        let team_relations = u32s(cur)?;
        let unknown = cur.u8()?;
        expect(cur, "recruits version", 1)?;
        expect(cur, "recruit count", 0)?;
        let recruit_unknown = cur.i32()?;
        let faction = (cur.u8()? != 0).then(|| Faction::parse(cur)).transpose()?;
        Ok(Kingdom {
            thing,
            updatable,
            version,
            major_index,
            independent,
            name,
            color,
            relations,
            resources,
            team,
            team_relations,
            unknown,
            recruit_unknown,
            faction,
            nation: cur.string()?,
            rest: Vec::new(),
        })
    }

    fn write(&self, w: &mut Writer, world_version: u32, index: u32) -> Result<()> {
        self.thing.write(w)?;
        match (&self.updatable, world_version >= UPDATABLE_WORLD_VERSION) {
            (Some(id), true) => {
                w.u32(1);
                w.i32(*id);
            }
            (None, false) => {}
            _ => {
                return Err(Error::Unwritable(format!(
                    "kingdom {index}'s updatable id doesn't suit world version {world_version}"
                )));
            }
        }
        w.u32(self.version);
        w.u32(index);
        w.i32(self.major_index);
        w.u32(self.independent);
        w.string(&self.name)?;
        w.string(&self.color)?;
        write_u32s(w, &self.relations);
        w.u32(self.resources.len() as u32);
        for (resource, amount) in &self.resources {
            w.string(resource)?;
            w.f32(*amount);
        }
        w.u32(self.team);
        write_u32s(w, &self.team_relations);
        w.u8(self.unknown);
        w.u32(1);
        w.u32(0);
        w.i32(self.recruit_unknown);
        w.u8(self.faction.is_some().into());
        if let Some(faction) = &self.faction {
            faction.write(w)?;
        }
        w.string(&self.nation)?;
        w.bytes(&self.rest);
        Ok(())
    }
}

impl Faction {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let name = cur.string()?;
        expect(cur, "faction version", 0)?;
        let unknown = cur.u32()?;
        let n = cur.u32()?;
        let abilities = (0..n).map(|_| Ability::parse(cur)).collect::<Result<_>>()?;
        Ok(Faction {
            name,
            unknown,
            abilities,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.string(&self.name)?;
        w.u32(0);
        w.u32(self.unknown);
        w.u32(self.abilities.len() as u32);
        self.abilities.iter().for_each(|a| a.write(w));
        Ok(())
    }
}

impl Ability {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = cur.u32()?;
        if version > 1 {
            return Err(Error::Corrupt(format!("ability version {version}")));
        }
        Ok(Ability {
            version,
            state: cur.u32()?,
            time: cur.f32()?,
            unknown: cur.u32()?,
            unknown2: cur.u8()?,
            targets: (version >= 1).then(|| u32s(cur)).transpose()?,
        })
    }

    fn write(&self, w: &mut Writer) {
        w.u32(self.version);
        w.u32(self.state);
        w.f32(self.time);
        w.u32(self.unknown);
        w.u8(self.unknown2);
        if let Some(targets) = &self.targets {
            write_u32s(w, targets);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Component, Properties, State, Trailer, TrailerLists, Updatable};

    fn head(w: &mut Writer, template: &str, class: u32, id: u32) {
        Head {
            template: template.into(),
            class,
            id,
            name: format!("{template}_{id}"),
            position: [0.0; 3],
            facing: [0.0, -1.0, 0.0],
            flags: 0x2004_2800,
        }
        .write(w)
        .unwrap();
    }

    #[test]
    fn parses_teams() {
        let mut w = Writer::default();
        w.u32(2);
        w.u8(1);
        head(&mut w, "team", TEAM, 30);
        w.u32(1);
        w.u32(0);
        w.string("team_0").unwrap();
        write_u32s(&mut w, &[3, 2]);
        write_u32s(&mut w, &[1, 3]);
        w.string("restriction_group_0000").unwrap();
        w.u8(0);
        let teams = parse_teams(&w.0).unwrap();
        let team = teams[0].as_ref().unwrap();
        assert_eq!((team.head.id, team.display_name.as_str()), (30, "team_0"));
        assert_eq!(team.kingdom_relations, [3, 2]);
        assert_eq!(
            team.restriction_group.as_deref(),
            Some("restriction_group_0000")
        );
        assert!(teams[1].is_none());
        assert_eq!(write_teams(&teams).unwrap(), w.0);
    }

    /// A kingdom record up to its undecoded rest.
    fn kingdom(w: &mut Writer, index: u32, faction: Option<&str>) {
        head(w, "kingdom", KINGDOM, 100 + index);
        for v in [5, 1f32.to_bits(), u32::MAX, 0, 0] {
            w.u32(v);
        }
        w.u16(0);
        Actor {
            version: 14,
            kingdom: 0,
            flags: 0,
            template_components: 1 << 6,
            components: 1 << 6,
            state: State::Decoded {
                components: vec![Component::Properties(Properties {
                    updatable: Updatable { version: 1, id: 7 },
                    version: 1,
                    properties: Vec::new(),
                })],
                trailer: Trailer {
                    main_component: -1,
                    unknown: Some(0),
                    unknown2: None,
                    unknown_string: Some(None),
                    unknown3: Some(None),
                    unknown_string2: Some(String::new()),
                    unknown4: Some([0; 2]),
                    unknown5: Some(0),
                    lists: Some(TrailerLists {
                        lists: Default::default(),
                        triples: Vec::new(),
                    }),
                    unknown6: Some(0),
                },
            },
        }
        .write(w)
        .unwrap();
        w.u32(1); // updatable
        w.i32(index as i32);
        w.u32(20);
        w.u32(index);
        w.i32(if faction.is_some() { 0 } else { -1 });
        w.u32(faction.is_none().into());
        w.string(&format!("kingdom_{index}")).unwrap();
        w.string("blue").unwrap();
        write_u32s(w, &[0, 3, 3]);
        w.u32(1);
        w.string("money").unwrap();
        w.f32(500.0);
        w.u32(30);
        write_u32s(w, &[1]);
        w.u8(1);
        w.u32(1);
        w.u32(0);
        w.i32(-1);
        w.u8(faction.is_some().into());
        if let Some(name) = faction {
            w.string(name).unwrap();
            w.u32(0);
            w.u32(0);
            w.u32(1);
            Ability {
                version: 1,
                state: 2,
                time: 1.5,
                unknown: 0,
                unknown2: 1,
                targets: Some(vec![9]),
            }
            .write(w);
        }
        w.string(if faction.is_some() { "nation_gbr" } else { "" })
            .unwrap();
    }

    #[test]
    fn parses_kingdoms() {
        let mut w = Writer::default();
        w.u32(1);
        w.u32(4);
        w.u8(1);
        kingdom(&mut w, 0, Some("montgomery"));
        // The rest, holding a byte that could start a record, then two
        // empty positions.
        w.bytes(&[0xAA, 1, 0, 0]);
        w.bytes(&[0, 0]);
        w.u8(1);
        kingdom(&mut w, 3, None);
        w.bytes(&[0xBB]);

        let kingdoms = Kingdoms::parse(&w.0, 28).unwrap();
        assert_eq!(kingdoms.majors, 1);
        let present: Vec<_> = kingdoms.kingdoms.iter().map(Option::is_some).collect();
        assert_eq!(present, [true, false, false, true]);
        let first = kingdoms.kingdoms[0].as_ref().unwrap();
        assert_eq!((first.name.as_str(), first.thing.id), ("kingdom_0", 100));
        assert_eq!((first.team, first.updatable), (30, Some(0)));
        assert_eq!(first.resources, [("money".to_string(), 500.0)]);
        let faction = first.faction.as_ref().unwrap();
        assert_eq!(
            (faction.name.as_str(), faction.abilities.len()),
            ("montgomery", 1)
        );
        assert_eq!(first.nation, "nation_gbr");
        assert_eq!(first.rest, [0xAA, 1, 0, 0]);
        let last = kingdoms.kingdoms[3].as_ref().unwrap();
        assert_eq!((last.major_index, last.faction.is_none()), (-1, true));
        assert_eq!(last.rest, [0xBB]);
        assert_eq!(kingdoms.to_bytes(28).unwrap(), w.0);
        assert!(
            kingdoms.to_bytes(21).is_err(),
            "updatable ids need world 22"
        );
    }
}
