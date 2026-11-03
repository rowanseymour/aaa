//! The world a map's sides play in, its teams and the kingdoms on them.
//!
//! ```text
//! GWORLD      u32 world_version (up to 28), u32 rng_seed, u32 game_ticks,
//!             f32 game_time_begin, f32 game_time_end, f32 game_time_per_tick,
//!             u32 next_player_id,
//!             v4: u32 next_camera_id, u32 next_area_id,
//!             v27: u32 next_structure_queue_item_id,
//!             v5: u32 map_status, v7: u32 mission_state
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
//!     v5: u32 n, (string, u32)[n], v6: u32, v14: u32 n, string[n],
//!     v7: u32 n, property[n] as in an actor's Properties, all of class 0
//!       and without it
//!     v8: u8 u8 string, v9: string
//!     if not independent, from v10, the strategic AI:
//!       v11: priorities: u32 version (0-2), then 4 lists (5 from v2) of
//!         u32 n, (string template, f32)[n]; from v1 as many more of
//!         u32 n, (u32 id, f32)[n]
//!       v11: weights: u32 version (0-1), u32[60] (66 from v1)
//!       u32 n, item[n]: u32 version (0), weights, u32[21]
//!       v10 only: weights
//!       u32 unknown
//!     v12: u32 n, entry[n]: u32 version (0), string, string, u32 n,
//!       u32[n], u8, u32
//!     v15: u32 n, (string template, u32 instance_number)[n]
//!     v16: u32 n, u32 used_badge[n]
//!     v17: u8, v18: u8, v19: u8, v20: u32 n, (string resource, f32)[n]
//! ```
//!
//! Relations are listed by the position of the kingdom or team they're with;
//! a kingdom's relation to itself is 0.
//! Teams' and kingdoms' indexes are their positions, which the game requires.

use super::header::{check_version, optional};
use super::{Actor, Detail, Head, Property, Thing, expect};
use crate::{Cursor, Error, Result, Writer};

/// The game's world state as a map saves it; the names are the game's own.
#[derive(Debug, Clone, PartialEq)]
pub struct World {
    /// The version of the game state, which decides the layout of this and
    /// several other sections.
    pub version: u32,
    pub rng_seed: u32,
    /// Ticks the game has run; 0 in every shipped map, as none has started.
    pub game_ticks: u32,
    /// The game time at the start and end of the last tick.
    pub game_time_begin: f32,
    pub game_time_end: f32,
    /// Seconds of game time per tick.
    pub game_time_per_tick: f32,
    pub next_player_id: u32,
    /// The numbers the editor gives the next camera and area it names
    /// (`camera_%d`, `area_%d`); from version 4 on.
    pub next_camera_id: Option<u32>,
    pub next_area_id: Option<u32>,
    /// For the items queued at structures; from version 27 on.
    pub next_structure_queue_item_id: Option<u32>,
    /// 0 unverified, 1 repaired, 2 verified; a mission only starts on a
    /// verified map. From version 5 on.
    pub map_status: Option<u32>,
    /// 0 not started, 1 running, 2 won; from version 7 on.
    pub mission_state: Option<u32>,
}

impl World {
    const MAX_VERSION: u32 = 28;

    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let cur = &mut cur;
        let version = check_version("world", cur.u32()?, 0..=Self::MAX_VERSION)?;
        let world = World {
            version,
            rng_seed: cur.u32()?,
            game_ticks: cur.u32()?,
            game_time_begin: cur.f32()?,
            game_time_end: cur.f32()?,
            game_time_per_tick: cur.f32()?,
            next_player_id: cur.u32()?,
            next_camera_id: (version >= 4).then(|| cur.u32()).transpose()?,
            next_area_id: (version >= 4).then(|| cur.u32()).transpose()?,
            next_structure_queue_item_id: (version >= 27).then(|| cur.u32()).transpose()?,
            map_status: (version >= 5).then(|| cur.u32()).transpose()?,
            mission_state: (version >= 7).then(|| cur.u32()).transpose()?,
        };
        cur.finish("GWORLD")?;
        Ok(world)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        let v = self.version;
        w.u32(v);
        w.u32(self.rng_seed);
        w.u32(self.game_ticks);
        w.f32(self.game_time_begin);
        w.f32(self.game_time_end);
        w.f32(self.game_time_per_tick);
        w.u32(self.next_player_id);
        for (has, what, value) in [
            (v >= 4, "next_camera_id", self.next_camera_id),
            (v >= 4, "next_area_id", self.next_area_id),
            (
                v >= 27,
                "next_structure_queue_item_id",
                self.next_structure_queue_item_id,
            ),
            (v >= 5, "map_status", self.map_status),
            (v >= 7, "mission_state", self.mission_state),
        ] {
            optional(has, what, value, |value| {
                w.u32(value);
                Ok(())
            })?;
        }
        Ok(w.0)
    }
}

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
    /// From version 5 on.
    pub counts: Option<Vec<(String, u32)>>,
    /// From version 6 on.
    pub unknown2: Option<u32>,
    /// From version 14 on.
    pub strings: Option<Vec<String>>,
    /// Shared properties, such as technologies; from version 7 on.
    pub properties: Option<Vec<Property>>,
    /// From version 8 on.
    pub unknown3: Option<([u8; 2], String)>,
    /// From version 9 on.
    pub unknown_string: Option<String>,
    /// The strategic AI's state, which independent kingdoms don't have;
    /// from version 10 on.
    pub ai: Option<StrategicAi>,
    /// From version 12 on.
    pub entries: Option<Vec<Entry>>,
    /// The next instance number by actor template; from version 15 on.
    pub instance_numbers: Option<Vec<(String, u32)>>,
    /// From version 16 on.
    pub used_badges: Option<Vec<u32>>,
    /// One from each of versions 17, 18 and 19 on.
    pub unknown4: Vec<u8>,
    /// From version 20 on.
    pub resources2: Option<Vec<(String, f32)>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StrategicAi {
    /// From version 11 on.
    pub priorities: Option<AiPriorities>,
    pub weights: AiWeights,
    pub items: Vec<AiItem>,
    pub unknown: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiPriorities {
    pub version: u32,
    /// Version 0 stores u32s, kept here by their bits.
    pub templates: Vec<Vec<(String, f32)>>,
    /// Empty for version 0.
    pub ids: Vec<Vec<(u32, f32)>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiWeights {
    pub version: u32,
    pub values: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiItem {
    pub weights: AiWeights,
    pub values: [u32; 21],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub unknown: String,
    pub ids: Vec<u32>,
    pub unknown2: u8,
    pub unknown3: u32,
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
        let kingdoms = (0..count)
            .map(|index| {
                present(&mut cur)?
                    .then(|| Kingdom::parse(&mut cur, world_version, index))
                    .transpose()
            })
            .collect::<Result<_>>()?;
        cur.finish("KINGDOMS")?;
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

impl Kingdom {
    const MIN_VERSION: u32 = 13;
    const MAX_VERSION: u32 = 20;

    fn parse(cur: &mut Cursor, world_version: u32, index: u32) -> Result<Self> {
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
        let nation = cur.string()?;
        let v = version;
        let counts = (v >= 5).then(|| pairs(cur, Cursor::u32)).transpose()?;
        let unknown2 = (v >= 6).then(|| cur.u32()).transpose()?;
        let strings = (v >= 14)
            .then(|| (0..cur.u32()?).map(|_| cur.string()).collect())
            .transpose()?;
        let properties = (v >= 7)
            .then(|| (0..cur.u32()?).map(|_| Property::parse(cur, 0)).collect())
            .transpose()?;
        let unknown3 = (v >= 8)
            .then(|| Result::<_>::Ok(([cur.u8()?, cur.u8()?], cur.string()?)))
            .transpose()?;
        let unknown_string = (v >= 9).then(|| cur.string()).transpose()?;
        let ai = (v >= 10 && independent == 0)
            .then(|| StrategicAi::parse(cur, v))
            .transpose()?;
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
            nation,
            counts,
            unknown2,
            strings,
            properties,
            unknown3,
            unknown_string,
            ai,
            entries: (v >= 12)
                .then(|| (0..cur.u32()?).map(|_| Entry::parse(cur)).collect())
                .transpose()?,
            instance_numbers: (v >= 15).then(|| pairs(cur, Cursor::u32)).transpose()?,
            used_badges: (v >= 16).then(|| u32s(cur)).transpose()?,
            unknown4: (17..=v.min(19)).map(|_| cur.u8()).collect::<Result<_>>()?,
            resources2: (v >= 20).then(|| pairs(cur, Cursor::f32)).transpose()?,
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
        self.write_tail(w, index)
    }

    /// Everything after the nation.
    fn write_tail(&self, w: &mut Writer, index: u32) -> Result<()> {
        let v = self.version;
        let unwritable = || {
            Error::Unwritable(format!(
                "kingdom {index}'s fields don't suit its version {v}"
            ))
        };
        let has = [
            (self.counts.is_some(), v >= 5),
            (self.unknown2.is_some(), v >= 6),
            (self.strings.is_some(), v >= 14),
            (self.properties.is_some(), v >= 7),
            (self.unknown3.is_some(), v >= 8),
            (self.unknown_string.is_some(), v >= 9),
            (self.ai.is_some(), v >= 10 && self.independent == 0),
            (self.entries.is_some(), v >= 12),
            (self.instance_numbers.is_some(), v >= 15),
            (self.used_badges.is_some(), v >= 16),
            (self.resources2.is_some(), v >= 20),
            (self.unknown4.len() == (17..=v.min(19)).count(), true),
        ];
        if has.iter().any(|(set, wanted)| set != wanted) {
            return Err(unwritable());
        }
        if let Some(counts) = &self.counts {
            write_pairs(w, counts, Writer::u32)?;
        }
        if let Some(v) = self.unknown2 {
            w.u32(v);
        }
        if let Some(strings) = &self.strings {
            w.u32(strings.len() as u32);
            strings.iter().try_for_each(|s| w.string(s))?;
        }
        if let Some(properties) = &self.properties {
            if properties.iter().any(|p| p.class != 0) {
                return Err(unwritable());
            }
            w.u32(properties.len() as u32);
            properties.iter().try_for_each(|p| p.write_body(w))?;
        }
        if let Some((bytes, s)) = &self.unknown3 {
            bytes.iter().for_each(|&b| w.u8(b));
            w.string(s)?;
        }
        if let Some(s) = &self.unknown_string {
            w.string(s)?;
        }
        if let Some(ai) = &self.ai {
            ai.write(w, v)?;
        }
        if let Some(entries) = &self.entries {
            w.u32(entries.len() as u32);
            entries.iter().try_for_each(|e| e.write(w))?;
        }
        if let Some(numbers) = &self.instance_numbers {
            write_pairs(w, numbers, Writer::u32)?;
        }
        if let Some(badges) = &self.used_badges {
            write_u32s(w, badges);
        }
        self.unknown4.iter().for_each(|&b| w.u8(b));
        if let Some(resources) = &self.resources2 {
            write_pairs(w, resources, Writer::f32)?;
        }
        Ok(())
    }
}

/// `u32 n`, then `(string, value)[n]`.
fn pairs<'a, T>(
    cur: &mut Cursor<'a>,
    value: impl Fn(&mut Cursor<'a>) -> Result<T>,
) -> Result<Vec<(String, T)>> {
    let n = cur.u32()?;
    (0..n).map(|_| Ok((cur.string()?, value(cur)?))).collect()
}

fn write_pairs<T: Copy>(
    w: &mut Writer,
    pairs: &[(String, T)],
    value: impl Fn(&mut Writer, T),
) -> Result<()> {
    w.u32(pairs.len() as u32);
    for (s, v) in pairs {
        w.string(s)?;
        value(w, *v);
    }
    Ok(())
}

impl StrategicAi {
    fn parse(cur: &mut Cursor, v: u32) -> Result<Self> {
        let priorities = (v >= 11).then(|| AiPriorities::parse(cur)).transpose()?;
        let weights = (v >= 11).then(|| AiWeights::parse(cur)).transpose()?;
        let items = (0..cur.u32()?)
            .map(|_| AiItem::parse(cur))
            .collect::<Result<_>>()?;
        let weights = match weights {
            Some(weights) => weights,
            None => AiWeights::parse(cur)?,
        };
        Ok(StrategicAi {
            priorities,
            weights,
            items,
            unknown: cur.u32()?,
        })
    }

    fn write(&self, w: &mut Writer, v: u32) -> Result<()> {
        if self.priorities.is_some() != (v >= 11) {
            return Err(Error::Unwritable(format!(
                "AI priorities must be set exactly from kingdom version 11, not {v}"
            )));
        }
        if let Some(priorities) = &self.priorities {
            priorities.write(w)?;
            self.weights.write(w)?;
        }
        w.u32(self.items.len() as u32);
        for item in &self.items {
            w.u32(0);
            item.weights.write(w)?;
            item.values.iter().for_each(|&v| w.u32(v));
        }
        if v < 11 {
            self.weights.write(w)?;
        }
        w.u32(self.unknown);
        Ok(())
    }
}

impl AiPriorities {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = check_version("AI priorities", cur.u32()?, 0..=2)?;
        let lists = if version >= 2 { 5 } else { 4 };
        let templates = (0..lists)
            .map(|_| match version {
                0 => pairs(cur, |c| c.u32().map(f32::from_bits)),
                _ => pairs(cur, Cursor::f32),
            })
            .collect::<Result<_>>()?;
        let ids = (0..if version >= 1 { lists } else { 0 })
            .map(|_| {
                let n = cur.u32()?;
                (0..n).map(|_| Ok((cur.u32()?, cur.f32()?))).collect()
            })
            .collect::<Result<_>>()?;
        Ok(AiPriorities {
            version,
            templates,
            ids,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        let lists = if self.version >= 2 { 5 } else { 4 };
        let ids = if self.version >= 1 { lists } else { 0 };
        if self.templates.len() != lists || self.ids.len() != ids {
            return Err(Error::Unwritable(format!(
                "AI priorities version {} has {lists} lists",
                self.version
            )));
        }
        w.u32(self.version);
        for list in &self.templates {
            write_pairs(w, list, |w, v| w.u32(v.to_bits()))?;
        }
        for list in &self.ids {
            w.u32(list.len() as u32);
            for &(id, v) in list {
                w.u32(id);
                w.f32(v);
            }
        }
        Ok(())
    }
}

impl AiWeights {
    fn count(version: u32) -> usize {
        if version >= 1 { 66 } else { 60 }
    }

    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = check_version("AI weights", cur.u32()?, 0..=1)?;
        let values = (0..Self::count(version))
            .map(|_| cur.u32())
            .collect::<Result<_>>()?;
        Ok(AiWeights { version, values })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        if self.values.len() != Self::count(self.version) {
            return Err(Error::Unwritable(format!(
                "AI weights version {} have {} values",
                self.version,
                Self::count(self.version)
            )));
        }
        w.u32(self.version);
        self.values.iter().for_each(|&v| w.u32(v));
        Ok(())
    }
}

impl AiItem {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        expect(cur, "AI item version", 0)?;
        let weights = AiWeights::parse(cur)?;
        let mut values = [0; 21];
        for v in &mut values {
            *v = cur.u32()?;
        }
        Ok(AiItem { weights, values })
    }
}

impl Entry {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        expect(cur, "kingdom entry version", 0)?;
        Ok(Entry {
            name: cur.string()?,
            unknown: cur.string()?,
            ids: u32s(cur)?,
            unknown2: cur.u8()?,
            unknown3: cur.u32()?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(0);
        w.string(&self.name)?;
        w.string(&self.unknown)?;
        write_u32s(w, &self.ids);
        w.u8(self.unknown2);
        w.u32(self.unknown3);
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

    fn world(version: u32) -> World {
        World {
            version,
            rng_seed: 0x9e0b_05df,
            game_ticks: 0,
            game_time_begin: 0.0,
            game_time_end: 0.0,
            game_time_per_tick: 0.0625,
            next_player_id: 0,
            next_camera_id: Some(11),
            next_area_id: Some(4),
            next_structure_queue_item_id: (version >= 27).then_some(0),
            map_status: Some(2),
            mission_state: Some(0),
        }
    }

    #[test]
    fn round_trips_world() {
        for (version, len) in [(21, 44), (27, 48)] {
            let world = world(version);
            let bytes = world.to_bytes().unwrap();
            assert_eq!(bytes.len(), len);
            assert_eq!(&bytes[20..24], &[0, 0, 0x80, 0x3d], "1/16 s per tick");
            assert_eq!(World::parse(&bytes).unwrap(), world);
        }
        let mut bytes = world(28).to_bytes().unwrap();
        bytes[0] = 29;
        assert!(World::parse(&bytes).is_err(), "newer than the game");
        let mut wrong = world(21);
        wrong.next_structure_queue_item_id = Some(0);
        assert!(wrong.to_bytes().is_err());
    }

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

    /// A version 20 kingdom record, a major one if it has a faction.
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
        // Counts, an unknown, technologies, two flags and a string, and
        // another string.
        write_pairs(w, &[("ger_tank".to_string(), 2)], Writer::u32).unwrap();
        w.u32(7);
        w.u32(1);
        w.string("objective_a").unwrap();
        w.u32(1);
        Property {
            class: 0,
            version: 3,
            unknown: 1,
            unknown2: Some(2),
            id: "tech_armor".into(),
            unknown3: Some((1, 0)),
            extra: None,
        }
        .write_body(w)
        .unwrap();
        w.bytes(&[1, 0]);
        w.string("gbr_sai").unwrap();
        w.string("").unwrap();
        if faction.is_some() {
            // The strategic AI: priorities, weights, an item and an unknown.
            w.u32(2);
            for list in 0..10 {
                w.u32((list == 0).into());
                if list == 0 {
                    w.string("ger_tank").unwrap();
                    w.f32(0.5);
                }
            }
            w.u32(1);
            (0..66).for_each(|v| w.u32(v));
            w.u32(1);
            w.u32(0);
            w.u32(0);
            (0..60).for_each(|_| w.u32(3));
            (0..21).for_each(|v| w.u32(v));
            w.u32(9);
        }
        // An entry, instance numbers, badges, three flags and resources.
        w.u32(1);
        w.u32(0);
        w.string("entry").unwrap();
        w.string("").unwrap();
        write_u32s(w, &[4]);
        w.u8(1);
        w.u32(5);
        write_pairs(w, &[("ger_tank".to_string(), 3)], Writer::u32).unwrap();
        write_u32s(w, &[6]);
        w.bytes(&[0, 1, 0]);
        write_pairs(w, &[("ammo".to_string(), 10.0)], Writer::f32).unwrap();
    }

    #[test]
    fn parses_kingdoms() {
        let mut w = Writer::default();
        w.u32(1);
        w.u32(4);
        w.u8(1);
        kingdom(&mut w, 0, Some("montgomery"));
        // Two empty positions.
        w.bytes(&[0, 0]);
        w.u8(1);
        kingdom(&mut w, 3, None);

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
        let tech = &first.properties.as_ref().unwrap()[0];
        assert_eq!(tech.id, "tech_armor");
        let ai = first.ai.as_ref().unwrap();
        assert_eq!(ai.priorities.as_ref().unwrap().templates[0][0].1, 0.5);
        assert_eq!((ai.items.len(), ai.unknown), (1, 9));
        assert_eq!(first.unknown4, [0, 1, 0]);
        assert_eq!(first.resources2, Some(vec![("ammo".to_string(), 10.0)]));
        let last = kingdoms.kingdoms[3].as_ref().unwrap();
        assert_eq!((last.major_index, last.faction.is_none()), (-1, true));
        assert!(last.ai.is_none(), "independents have no strategic AI");
        assert_eq!(last.instance_numbers.as_ref().unwrap()[0].1, 3);
        assert_eq!(kingdoms.to_bytes(28).unwrap(), w.0);
        assert!(
            kingdoms.to_bytes(21).is_err(),
            "updatable ids need world 22"
        );
    }
}
