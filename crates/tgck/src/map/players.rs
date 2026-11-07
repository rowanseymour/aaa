//! The world a map's sides play in, its teams and the kingdoms on them: GWORLD,
//! TEAMS, RESTRICTION_GROUPS and KINGDOMS, laid out as [`rmp.ksy`] specifies.
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use super::header::{bool, check_version, optional};
use super::{Actor, Detail, Head, Property, Thing, expect};
use crate::{Cursor, Error, Result, Writer};

/// The game's world state as a map saves it.
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
    /// (`camera_3`, `area_4`); from version 4 on.
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

/// What the teams set to a group may build and research, as set in the
/// editor.
#[derive(Debug, Clone, PartialEq)]
pub struct RestrictionGroups {
    /// What the editor numbers the next group it creates.
    pub next_number: u32,
    pub groups: Vec<RestrictionGroup>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RestrictionGroup {
    /// E.g. `restriction_group_0003`, by which teams refer to it.
    pub name: String,
    /// The number in its name.
    pub number: u32,
    /// A string id; the same as the name in every shipped map.
    pub display_name: String,
    /// Actor templates.
    pub allowed_actors: Vec<String>,
    pub allowed_technologies: Vec<String>,
    pub required_actors: Vec<String>,
    pub required_technologies: Vec<String>,
}

impl RestrictionGroups {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let cur = &mut cur;
        expect(cur, "restriction groups version", 0)?;
        let next_number = cur.u32()?;
        let n = cur.u32()?;
        let groups = (0..n)
            .map(|_| {
                // Older groups are laid out differently.
                expect(cur, "restriction group version", RestrictionGroup::VERSION)?;
                Ok(RestrictionGroup {
                    name: cur.string()?,
                    number: cur.u32()?,
                    display_name: cur.string()?,
                    allowed_actors: cur.strings()?,
                    allowed_technologies: cur.strings()?,
                    required_actors: cur.strings()?,
                    required_technologies: cur.strings()?,
                })
            })
            .collect::<Result<_>>()?;
        cur.finish("RESTRICTION_GROUPS")?;
        Ok(RestrictionGroups {
            next_number,
            groups,
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        w.u32(0);
        w.u32(self.next_number);
        w.u32(self.groups.len() as u32);
        for group in &self.groups {
            w.u32(RestrictionGroup::VERSION);
            w.string(&group.name)?;
            w.u32(group.number);
            w.string(&group.display_name)?;
            w.strings(&group.allowed_actors)?;
            w.strings(&group.allowed_technologies)?;
            w.strings(&group.required_actors)?;
            w.strings(&group.required_technologies)?;
        }
        Ok(w.0)
    }
}

impl RestrictionGroup {
    const VERSION: u32 = 4;
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
            bool(&mut cur, "presence byte")?
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

impl Team {
    const MAX_VERSION: u32 = 1;

    fn parse(cur: &mut Cursor, index: u32) -> Result<Self> {
        let head = parse_head(cur, TEAM)?;
        let version = check_version("team", cur.u32()?, 0..=Self::MAX_VERSION)?;
        expect(cur, "team index", index)?;
        Ok(Team {
            head,
            version,
            display_name: cur.string()?,
            kingdom_relations: cur.u32s()?,
            team_relations: cur.u32s()?,
            restriction_group: (version >= 1).then(|| cur.string()).transpose()?,
        })
    }

    fn write(&self, w: &mut Writer, index: u32) -> Result<()> {
        self.head.write(w)?;
        w.u32(self.version);
        w.u32(index);
        w.string(&self.display_name)?;
        w.u32s(&self.kingdom_relations);
        w.u32s(&self.team_relations);
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
    /// Its [`Relation`] to each kingdom, by position.
    pub relations: Vec<u32>,
    /// Stockpiles by resource id, e.g. `money` or `ammo`.
    pub resources: Vec<(String, f32)>,
    /// The [`Team`]'s head id; 0 for none.
    pub team: u32,
    /// Its [`Relation`] to each team, by position.
    pub team_relations: Vec<u32>,
    /// Whether the mission's start replaces the stockpiles with the game's
    /// default amount.
    pub use_default_stockpile: bool,
    /// The recruit being worked on; -1 for none.
    pub current_recruit: i32,
    /// The commander, set on campaign maps and chosen in the lobby otherwise.
    pub faction: Option<Faction>,
    /// The nation id, e.g. `nation_gbr`; empty when chosen in the lobby.
    pub nation: String,
    /// Its objectives' states by objective name: 0 active, 1 completed,
    /// 2 failed.
    pub objectives: Vec<(String, u32)>,
    /// 0 still playing, 1 won, 2 lost.
    pub outcome: u32,
    /// Technologies it's given when the mission starts; from version 14 on.
    pub start_technologies: Option<Vec<String>>,
    /// Shared properties it has, such as technologies.
    pub properties: Vec<Property>,
    /// It must be in the game.
    pub required: bool,
    /// A human may play it.
    pub playable: bool,
    /// The strategic AI that plays it, e.g. `ger_sai`; empty for none.
    pub ai_profile: String,
    /// The [`RestrictionGroup`] it's in; empty for none.
    pub restriction_group: String,
    /// What its score is drawn from; independent kingdoms have none.
    pub stats: Option<KingdomStats>,
    /// The player's saved selections of units.
    pub saved_groups: Vec<SavedGroup>,
    /// The next instance number by actor template; from version 15 on.
    pub instance_numbers: Option<Vec<(String, u32)>>,
    /// From version 16 on.
    pub used_badges: Option<Vec<u32>>,
    /// Whether the usual victory check applies, which triggers can turn off;
    /// from version 17 on.
    pub default_victory_condition: Option<bool>,
    /// From version 18 on.
    pub resigned: Option<bool>,
    /// Its team won by holding the share of cities the rules require; from
    /// version 19 on.
    pub city_ratio_victory: Option<bool>,
    /// Amounts added to the stockpiles when the mission starts; from version
    /// 20 on.
    pub bonus_stockpile: Option<Vec<(String, f32)>>,
}

/// How a kingdom stands toward another kingdom or a team.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    Itself = 0,
    Ally = 1,
    Neutral = 2,
    Enemy = 3,
}

impl Relation {
    pub fn from_u32(v: u32) -> Option<Self> {
        [Self::Itself, Self::Ally, Self::Neutral, Self::Enemy]
            .get(v as usize)
            .copied()
    }
}

/// A kingdom's statistics, which the score screen and graph show.
#[derive(Debug, Clone, PartialEq)]
pub struct KingdomStats {
    pub favorites: Favorites,
    pub totals: Totals,
    /// Taken every 60 seconds of game time, for the score graph.
    pub snapshots: Vec<Snapshot>,
    /// Seconds toward the next snapshot.
    pub time_since_snapshot: f32,
}

/// Tallies from which the game picks the kingdom's favourite unit, hero,
/// building, enemy and, from version 2, organization.
#[derive(Debug, Clone, PartialEq)]
pub struct Favorites {
    pub version: u32,
    /// By template, a list per category. Version 0 stores u32s, which the
    /// game ignores, kept here by their bits.
    pub by_template: Vec<Vec<(String, f32)>>,
    /// By thing id, a list per category; empty for version 0.
    pub by_id: Vec<Vec<(u32, f32)>>,
}

/// Running totals: each resource's total production, ten slots in the
/// game's resource order, then a counter per statistic in the order of the
/// game's `Scoring/Scoring.tgi` ids (`TRIBUTE_GIVEN`, `TRIBUTE_RECEIVED`,
/// ...). The slots of the counts the game works out live (`..._OWNED`)
/// hold leftover memory.
#[derive(Debug, Clone, PartialEq)]
pub struct Totals {
    pub version: u32,
    /// 66 values, or 60 in version 0, which leaves out a few statistics.
    pub values: Vec<f32>,
}

/// A sample for the score graph.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub totals: Totals,
    /// Each resource's rate, ten slots; the hero inventory's size; then the
    /// scores.
    pub values: [f32; 21],
}

/// A selection of units the player saved, to recall with a key.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedGroup {
    /// Its digits are the group's number.
    pub name: String,
    pub unknown: String,
    /// The actors' thing ids.
    pub ids: Vec<u32>,
    /// Whether the units move together as one group.
    pub grouped: bool,
    /// The thing that leads them when they do.
    pub group_thing: u32,
}

/// A kingdom's commander and the state of their abilities.
#[derive(Debug, Clone, PartialEq)]
pub struct Faction {
    /// The faction id, e.g. `montgomery`.
    pub name: String,
    /// The commander's experience, which abilities cost.
    pub experience: f32,
    pub abilities: Vec<Ability>,
}

/// The state of an ability a commander or an actor has.
#[derive(Debug, Clone, PartialEq)]
pub struct Ability {
    pub version: u32,
    pub use_count: u32,
    /// The game time it was last used; -1000 or the lowest f32 for never.
    pub last_used: f32,
    pub unknown: u32,
    pub unknown2: u8,
    /// The things it's acting on; from version 1 on.
    pub active_targets: Option<Vec<u32>>,
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
                bool(&mut cur, "presence byte")?
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
        let versions = Self::MIN_VERSION..=Self::MAX_VERSION;
        let version = check_version("kingdom", cur.u32()?, versions)?;
        let v = version;
        expect(cur, "kingdom index", index)?;
        let major_index = cur.i32()?;
        let independent = cur.u32()?;
        let name = cur.string()?;
        let color = cur.string()?;
        let relations = cur.u32s()?;
        let resources = pairs(cur, Cursor::f32)?;
        let team = cur.u32()?;
        let team_relations = cur.u32s()?;
        let use_default_stockpile = bool(cur, "use default stockpile")?;
        expect(cur, "recruits version", 1)?;
        expect(cur, "recruit count", 0)?;
        let current_recruit = cur.i32()?;
        let faction = (cur.u8()? != 0).then(|| Faction::parse(cur)).transpose()?;
        let nation = cur.string()?;
        let objectives = pairs(cur, Cursor::u32)?;
        let outcome = cur.u32()?;
        let start_technologies = (v >= 14).then(|| cur.strings()).transpose()?;
        let properties = (0..cur.u32()?)
            .map(|_| Property::parse(cur, 0))
            .collect::<Result<_>>()?;
        let required = bool(cur, "required")?;
        let playable = bool(cur, "playable")?;
        let ai_profile = cur.string()?;
        let restriction_group = cur.string()?;
        let stats = (independent == 0)
            .then(|| KingdomStats::parse(cur))
            .transpose()?;
        let saved_groups = (0..cur.u32()?)
            .map(|_| SavedGroup::parse(cur))
            .collect::<Result<_>>()?;
        let flag = |cur: &mut Cursor, from: u32, what: &str| {
            (v >= from).then(|| bool(cur, what)).transpose()
        };
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
            use_default_stockpile,
            current_recruit,
            faction,
            nation,
            objectives,
            outcome,
            start_technologies,
            properties,
            required,
            playable,
            ai_profile,
            restriction_group,
            stats,
            saved_groups,
            instance_numbers: (v >= 15).then(|| pairs(cur, Cursor::u32)).transpose()?,
            used_badges: (v >= 16).then(|| cur.u32s()).transpose()?,
            default_victory_condition: flag(cur, 17, "default victory condition")?,
            resigned: flag(cur, 18, "resigned")?,
            city_ratio_victory: flag(cur, 19, "city ratio victory")?,
            bonus_stockpile: (v >= 20).then(|| pairs(cur, Cursor::f32)).transpose()?,
        })
    }

    fn write(&self, w: &mut Writer, world_version: u32, index: u32) -> Result<()> {
        let v = self.version;
        let has = [
            (self.start_technologies.is_some(), v >= 14),
            (self.stats.is_some(), self.independent == 0),
            (self.instance_numbers.is_some(), v >= 15),
            (self.used_badges.is_some(), v >= 16),
            (self.default_victory_condition.is_some(), v >= 17),
            (self.resigned.is_some(), v >= 18),
            (self.city_ratio_victory.is_some(), v >= 19),
            (self.bonus_stockpile.is_some(), v >= 20),
        ];
        if has.iter().any(|(set, wanted)| set != wanted) {
            return Err(Error::Unwritable(format!(
                "kingdom {index}'s fields don't suit its version {v}"
            )));
        }
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
        w.u32(v);
        w.u32(index);
        w.i32(self.major_index);
        w.u32(self.independent);
        w.string(&self.name)?;
        w.string(&self.color)?;
        w.u32s(&self.relations);
        write_pairs(w, &self.resources, Writer::f32)?;
        w.u32(self.team);
        w.u32s(&self.team_relations);
        w.u8(self.use_default_stockpile.into());
        w.u32(1);
        w.u32(0);
        w.i32(self.current_recruit);
        w.u8(self.faction.is_some().into());
        if let Some(faction) = &self.faction {
            faction.write(w)?;
        }
        w.string(&self.nation)?;
        write_pairs(w, &self.objectives, Writer::u32)?;
        w.u32(self.outcome);
        if let Some(technologies) = &self.start_technologies {
            w.strings(technologies)?;
        }
        if self.properties.iter().any(|p| p.class != 0) {
            return Err(Error::Unwritable(format!(
                "kingdom {index} has a property of a class other than 0"
            )));
        }
        w.u32(self.properties.len() as u32);
        self.properties.iter().try_for_each(|p| p.write_body(w))?;
        w.u8(self.required.into());
        w.u8(self.playable.into());
        w.string(&self.ai_profile)?;
        w.string(&self.restriction_group)?;
        if let Some(stats) = &self.stats {
            stats.write(w)?;
        }
        w.u32(self.saved_groups.len() as u32);
        self.saved_groups.iter().try_for_each(|g| g.write(w))?;
        if let Some(numbers) = &self.instance_numbers {
            write_pairs(w, numbers, Writer::u32)?;
        }
        if let Some(badges) = &self.used_badges {
            w.u32s(badges);
        }
        for flag in [
            self.default_victory_condition,
            self.resigned,
            self.city_ratio_victory,
        ]
        .into_iter()
        .flatten()
        {
            w.u8(flag.into());
        }
        if let Some(bonus) = &self.bonus_stockpile {
            write_pairs(w, bonus, Writer::f32)?;
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

impl KingdomStats {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        Ok(KingdomStats {
            favorites: Favorites::parse(cur)?,
            totals: Totals::parse(cur)?,
            snapshots: (0..cur.u32()?)
                .map(|_| Snapshot::parse(cur))
                .collect::<Result<_>>()?,
            time_since_snapshot: cur.f32()?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        self.favorites.write(w)?;
        self.totals.write(w)?;
        w.u32(self.snapshots.len() as u32);
        for snapshot in &self.snapshots {
            w.u32(0);
            snapshot.totals.write(w)?;
            w.f32s(&snapshot.values);
        }
        w.f32(self.time_since_snapshot);
        Ok(())
    }
}

impl Favorites {
    fn lists(version: u32) -> usize {
        if version >= 2 { 5 } else { 4 }
    }

    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = check_version("favorites", cur.u32()?, 0..=2)?;
        let lists = Self::lists(version);
        let by_template = (0..lists)
            .map(|_| match version {
                0 => pairs(cur, |c| c.u32().map(f32::from_bits)),
                _ => pairs(cur, Cursor::f32),
            })
            .collect::<Result<_>>()?;
        let by_id = (0..if version >= 1 { lists } else { 0 })
            .map(|_| {
                let n = cur.u32()?;
                (0..n).map(|_| Ok((cur.u32()?, cur.f32()?))).collect()
            })
            .collect::<Result<_>>()?;
        Ok(Favorites {
            version,
            by_template,
            by_id,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        let lists = Self::lists(self.version);
        let ids = if self.version >= 1 { lists } else { 0 };
        if self.by_template.len() != lists || self.by_id.len() != ids {
            return Err(Error::Unwritable(format!(
                "favorites version {} has {lists} lists",
                self.version
            )));
        }
        w.u32(self.version);
        for list in &self.by_template {
            write_pairs(w, list, |w, v| w.u32(v.to_bits()))?;
        }
        for list in &self.by_id {
            w.u32(list.len() as u32);
            for &(id, v) in list {
                w.u32(id);
                w.f32(v);
            }
        }
        Ok(())
    }
}

impl Totals {
    fn count(version: u32) -> usize {
        if version >= 1 { 66 } else { 60 }
    }

    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = check_version("totals", cur.u32()?, 0..=1)?;
        let values = (0..Self::count(version))
            .map(|_| cur.f32())
            .collect::<Result<_>>()?;
        Ok(Totals { version, values })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        if self.values.len() != Self::count(self.version) {
            return Err(Error::Unwritable(format!(
                "totals version {} have {} values",
                self.version,
                Self::count(self.version)
            )));
        }
        w.u32(self.version);
        w.f32s(&self.values);
        Ok(())
    }
}

impl Snapshot {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        expect(cur, "snapshot version", 0)?;
        Ok(Snapshot {
            totals: Totals::parse(cur)?,
            values: cur.f32s()?,
        })
    }
}

impl SavedGroup {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        expect(cur, "saved group version", 0)?;
        Ok(SavedGroup {
            name: cur.string()?,
            unknown: cur.string()?,
            ids: cur.u32s()?,
            grouped: bool(cur, "grouped")?,
            group_thing: cur.u32()?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(0);
        w.string(&self.name)?;
        w.string(&self.unknown)?;
        w.u32s(&self.ids);
        w.u8(self.grouped.into());
        w.u32(self.group_thing);
        Ok(())
    }
}

impl Faction {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let name = cur.string()?;
        expect(cur, "faction version", 0)?;
        let experience = cur.f32()?;
        let n = cur.u32()?;
        let abilities = (0..n).map(|_| Ability::parse(cur)).collect::<Result<_>>()?;
        Ok(Faction {
            name,
            experience,
            abilities,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.string(&self.name)?;
        w.u32(0);
        w.f32(self.experience);
        w.u32(self.abilities.len() as u32);
        self.abilities.iter().for_each(|a| a.write(w));
        Ok(())
    }
}

impl Ability {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = check_version("ability", cur.u32()?, 0..=1)?;
        Ok(Ability {
            version,
            use_count: cur.u32()?,
            last_used: cur.f32()?,
            unknown: cur.u32()?,
            unknown2: cur.u8()?,
            active_targets: (version >= 1).then(|| cur.u32s()).transpose()?,
        })
    }

    fn write(&self, w: &mut Writer) {
        w.u32(self.version);
        w.u32(self.use_count);
        w.f32(self.last_used);
        w.u32(self.unknown);
        w.u8(self.unknown2);
        if let Some(targets) = &self.active_targets {
            w.u32s(targets);
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::map::{Component, Properties, State, Trailer, TrailerLists, Updatable};

    pub(in crate::map) fn world(version: u32) -> World {
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
    fn round_trips_restriction_groups() {
        let groups = RestrictionGroups {
            next_number: 4,
            groups: vec![RestrictionGroup {
                name: "restriction_group_0003".into(),
                number: 3,
                display_name: "restriction_group_0003".into(),
                allowed_actors: vec!["gbr_tank_flame".into(), "sas_commando".into()],
                allowed_technologies: vec!["tech_usa_gyrostabilizers".into()],
                required_actors: vec!["ger_hq_mechanized_bunker".into()],
                required_technologies: Vec::new(),
            }],
        };
        let bytes = groups.to_bytes().unwrap();
        assert_eq!(
            &bytes[..16],
            &[0, 0, 0, 0, 4, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0]
        );
        assert_eq!(RestrictionGroups::parse(&bytes).unwrap(), groups);
        let empty = [0; 12];
        let parsed = RestrictionGroups::parse(&empty).unwrap();
        assert_eq!(parsed.to_bytes().unwrap(), empty);
        let mut old = bytes.clone();
        old[12] = 3;
        assert!(
            RestrictionGroups::parse(&old).is_err(),
            "laid out differently"
        );
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
        w.u32s(&[3, 2]);
        w.u32s(&[1, 3]);
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
            states: 0,
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
                    required_object: Some(0),
                    old_stance: None,
                    saved_group: Some(None),
                    group: Some(None),
                    map_string: Some(String::new()),
                    towing: Some([0; 2]),
                    number: Some(0),
                    lists: Some(TrailerLists {
                        lists: Default::default(),
                        recent_attacks: Vec::new(),
                    }),
                    num_deploying: Some(0),
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
        w.u32s(&[0, 3, 3]);
        w.u32(1);
        w.string("money").unwrap();
        w.f32(500.0);
        w.u32(30);
        w.u32s(&[1]);
        w.u8(1);
        w.u32(1);
        w.u32(0);
        w.i32(-1);
        w.u8(faction.is_some().into());
        if let Some(name) = faction {
            w.string(name).unwrap();
            w.u32(0);
            w.f32(25.0);
            w.u32(1);
            Ability {
                version: 1,
                use_count: 2,
                last_used: 1.5,
                unknown: 0,
                unknown2: 1,
                active_targets: Some(vec![9]),
            }
            .write(w);
        }
        w.string(if faction.is_some() { "nation_gbr" } else { "" })
            .unwrap();
        // Objectives, the outcome, start technologies, properties, two
        // flags, the AI profile and the restriction group.
        write_pairs(w, &[("objective_0".to_string(), 1)], Writer::u32).unwrap();
        w.u32(0);
        w.u32(1);
        w.string("tech_usa_gyrostabilizers").unwrap();
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
        w.string("restriction_group_0000").unwrap();
        if faction.is_some() {
            // Stats: favorites, totals, a snapshot and the time since.
            w.u32(2);
            for list in 0..10 {
                w.u32((list == 0).into());
                if list == 0 {
                    w.string("ger_tank").unwrap();
                    w.f32(0.5);
                }
            }
            w.u32(1);
            (0..66).for_each(|v| w.f32(v as f32));
            w.u32(1);
            w.u32(0);
            w.u32(0);
            (0..60).for_each(|_| w.f32(3.0));
            (0..21).for_each(|v| w.f32(v as f32));
            w.f32(60.0);
        }
        // A saved group, instance numbers, badges, three flags and the bonus
        // stockpile.
        w.u32(1);
        w.u32(0);
        w.string("entry").unwrap();
        w.string("").unwrap();
        w.u32s(&[4]);
        w.u8(1);
        w.u32(5);
        write_pairs(w, &[("ger_tank".to_string(), 3)], Writer::u32).unwrap();
        w.u32s(&[6]);
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
            (faction.name.as_str(), faction.experience),
            ("montgomery", 25.0)
        );
        assert_eq!(faction.abilities[0].last_used, 1.5);
        assert_eq!(first.nation, "nation_gbr");
        assert_eq!(first.objectives, [("objective_0".to_string(), 1)]);
        assert_eq!(first.properties[0].id, "tech_armor");
        assert_eq!((first.required, first.playable), (true, false));
        assert_eq!(first.ai_profile, "gbr_sai");
        assert_eq!(first.restriction_group, "restriction_group_0000");
        let stats = first.stats.as_ref().unwrap();
        assert_eq!(stats.favorites.by_template[0][0].1, 0.5);
        assert_eq!(stats.totals.values[8], 8.0);
        assert_eq!(stats.snapshots.len(), 1);
        assert_eq!(stats.time_since_snapshot, 60.0);
        assert_eq!(first.saved_groups[0].ids, [4]);
        assert_eq!(
            (first.default_victory_condition, first.resigned),
            (Some(false), Some(true))
        );
        assert_eq!(
            first.bonus_stockpile,
            Some(vec![("ammo".to_string(), 10.0)])
        );
        assert_eq!(
            Relation::from_u32(first.relations[1]),
            Some(Relation::Enemy)
        );
        let last = kingdoms.kingdoms[3].as_ref().unwrap();
        assert_eq!((last.major_index, last.faction.is_none()), (-1, true));
        assert!(last.stats.is_none(), "independents have no stats");
        assert_eq!(last.instance_numbers.as_ref().unwrap()[0].1, 3);
        assert_eq!(kingdoms.to_bytes(28).unwrap(), w.0);
        assert!(
            kingdoms.to_bytes(21).is_err(),
            "updatable ids need world 22"
        );
    }
}
