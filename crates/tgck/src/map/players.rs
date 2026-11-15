//! The world a map's sides play in, its teams and the kingdoms on them: GWORLD,
//! TEAMS, RESTRICTION_GROUPS and KINGDOMS, laid out as [`rmp.ksy`] specifies.
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use super::{Actor, Detail, Head, KINGDOM, Property, ReadAs, TEAM, Thing};
use crate::{Cursor, Error, Result, Writer};

/// The GWORLD section, the `gworld` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct World {
    pub version: u32,
    pub rng_seed: u32,
    pub game_ticks: u32,
    pub game_time_begin: f32,
    pub game_time_end: f32,
    pub game_time_per_tick: f32,
    pub next_player_id: u32,
    pub next_camera_id: Option<u32>,
    pub next_area_id: Option<u32>,
    pub next_structure_queue_item_id: Option<u32>,
    pub map_status: Option<u32>,
    pub mission_state: Option<u32>,
}

/// The RESTRICTION_GROUPS section, the `restriction_groups` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct RestrictionGroups {
    pub next_number: u32,
    pub groups: Vec<RestrictionGroup>,
}

/// A `restriction_group` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct RestrictionGroup {
    pub name: String,
    pub number: u32,
    pub display_name: String,
    pub allowed_actors: Vec<String>,
    pub allowed_technologies: Vec<String>,
    pub required_actors: Vec<String>,
    pub required_technologies: Vec<String>,
}

impl RestrictionGroups {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let cur = &mut cur;
        cur.expect("restriction groups version", 0)?;
        let next_number = cur.u32()?;
        let groups = cur.list(|cur| {
            // Older groups are laid out differently.
            cur.expect("restriction group version", RestrictionGroup::VERSION)?;
            Ok(RestrictionGroup {
                name: cur.string()?,
                number: cur.u32()?,
                display_name: cur.string()?,
                allowed_actors: cur.strings()?,
                allowed_technologies: cur.strings()?,
                required_actors: cur.strings()?,
                required_technologies: cur.strings()?,
            })
        })?;
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
        w.list("groups", &self.groups, |w, group| {
            w.u32(RestrictionGroup::VERSION);
            w.string(&group.name)?;
            w.u32(group.number);
            w.string(&group.display_name)?;
            w.strings("allowed actors", &group.allowed_actors)?;
            w.strings("allowed technologies", &group.allowed_technologies)?;
            w.strings("required actors", &group.required_actors)?;
            w.strings("required technologies", &group.required_technologies)?;
            Ok(())
        })?;
        Ok(w.0)
    }
}

impl RestrictionGroup {
    const VERSION: u32 = 4;
}

impl World {
    pub(super) const MAX_VERSION: u32 = 28;

    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let cur = &mut cur;
        let version = cur.version("world", 0..=Self::MAX_VERSION)?;
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
            w.optional_plain(has, what, value, Writer::u32)?;
        }
        Ok(w.0)
    }
}

/// A present `team` of `rmp.ksy`, less its index.
#[derive(Debug, Clone, PartialEq)]
pub struct Team {
    pub head: Head,
    pub version: u32,
    pub display_name: String,
    /// Each a [`Relation`].
    pub kingdom_relations: Vec<u32>,
    /// Each a [`Relation`].
    pub team_relations: Vec<u32>,
    pub restriction_group: Option<String>,
}

pub(super) fn parse_teams(data: &[u8]) -> Result<Vec<Option<Team>>> {
    let mut cur = Cursor::new(data);
    let teams = cur.slots("team presence", Team::parse)?;
    cur.finish("TEAMS")?;
    Ok(teams)
}

/// The TEAMS section holding `teams`.
pub(super) fn write_teams(teams: &[Option<Team>]) -> Result<Vec<u8>> {
    let mut w = Writer::default();
    w.slots("teams", teams, |w, team, index| team.write(w, index))?;
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
        let version = cur.version("team", 0..=Self::MAX_VERSION)?;
        cur.expect("team index", index)?;
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
        w.u32s("kingdom relations", &self.kingdom_relations)?;
        w.u32s("team relations", &self.team_relations)?;
        let group = self.restriction_group.as_deref();
        w.optional(
            self.version >= 1,
            "team's restriction group",
            group,
            Writer::string,
        )
    }
}

/// The KINGDOMS section, the `kingdoms` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Kingdoms {
    pub majors: u32,
    pub kingdoms: Vec<Option<Kingdom>>,
}

/// A present `kingdom` of `rmp.ksy`, less its index.
#[derive(Debug, Clone, PartialEq)]
pub struct Kingdom {
    /// The shared fields; the detail is the kingdom's [`Actor`].
    pub thing: Thing,
    /// The `updatable_id`.
    pub updatable: Option<i32>,
    pub version: u32,
    pub major_index: i32,
    pub independent: u32,
    pub name: String,
    pub color: String,
    /// Each a [`Relation`].
    pub relations: Vec<u32>,
    pub resources: Vec<(String, f32)>,
    /// The [`Team`]'s `head.id`; 0 for none.
    pub team: u32,
    /// Each a [`Relation`].
    pub team_relations: Vec<u32>,
    pub use_default_stockpile: bool,
    pub current_recruit: i32,
    /// `None` where the map leaves it to the lobby.
    pub faction: Option<Faction>,
    pub nation: String,
    pub objectives: Vec<(String, u32)>,
    pub outcome: u32,
    pub start_technologies: Option<Vec<String>>,
    pub properties: Vec<Property>,
    pub required: bool,
    pub playable: bool,
    pub ai_profile: String,
    pub restriction_group: String,
    /// `None` for an independent kingdom.
    pub stats: Option<KingdomStats>,
    pub saved_groups: Vec<SavedGroup>,
    pub instance_numbers: Option<Vec<(String, u32)>>,
    pub used_badges: Option<Vec<u32>>,
    pub default_victory_condition: Option<bool>,
    pub resigned: Option<bool>,
    pub city_ratio_victory: Option<bool>,
    pub bonus_stockpile: Option<Vec<(String, f32)>>,
}

/// A `relation` of `rmp.ksy`.
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

/// A `kingdom_stats` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct KingdomStats {
    pub favorites: Favorites,
    pub totals: Totals,
    pub snapshots: Vec<Snapshot>,
    pub time_since_snapshot: f32,
}

/// The `favorites` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Favorites {
    pub version: u32,
    /// A list per category; version 0's `u32`s are kept as `f32`s of the
    /// same bits.
    pub by_template: Vec<Vec<(String, f32)>>,
    /// A list per category; empty for version 0.
    pub by_id: Vec<Vec<(u32, f32)>>,
}

/// A `totals` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Totals {
    pub version: u32,
    pub values: Vec<f32>,
}

/// A `snapshot` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub totals: Totals,
    pub values: [f32; 21],
}

/// A `saved_group` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedGroup {
    pub name: String,
    pub display_name: String,
    pub ids: Vec<u32>,
    pub grouped: bool,
    pub group_thing: u32,
}

/// A `faction` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Faction {
    pub name: String,
    pub experience: f32,
    pub abilities: Vec<Ability>,
}

/// An `ability` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Ability {
    pub version: u32,
    pub use_count: u32,
    pub last_used: f32,
    pub unused1: u32,
    pub usable: u8,
    pub active_targets: Option<Vec<u32>>,
}

/// The world version from which kingdoms save an updatable id.
const UPDATABLE_WORLD_VERSION: u32 = 22;

impl Kingdoms {
    pub(super) fn parse(data: &[u8], world_version: u32) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let majors = cur.u32()?;
        let kingdoms = cur.slots("kingdom presence", |cur, index| {
            Kingdom::parse(cur, world_version, index)
        })?;
        cur.finish("KINGDOMS")?;
        Ok(Kingdoms { majors, kingdoms })
    }

    pub fn to_bytes(&self, world_version: u32) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        w.u32(self.majors);
        w.slots("kingdoms", &self.kingdoms, |w, kingdom, index| {
            kingdom.write(w, world_version, index)
        })?;
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
                cur.expect("kingdom updatable version", 1)?;
                cur.i32()
            })
            .transpose()?;
        let versions = Self::MIN_VERSION..=Self::MAX_VERSION;
        let version = cur.version("kingdom", versions)?;
        let v = version;
        cur.expect("kingdom index", index)?;
        let major_index = cur.i32()?;
        let independent = cur.u32()?;
        let name = cur.string()?;
        let color = cur.string()?;
        let relations = cur.u32s()?;
        let resources = cur.pairs(Cursor::f32)?;
        let team = cur.u32()?;
        let team_relations = cur.u32s()?;
        let use_default_stockpile = cur.bool("use default stockpile")?;
        cur.expect("recruits version", 1)?;
        cur.expect("recruit count", 0)?;
        let current_recruit = cur.i32()?;
        let faction = cur
            .bool("faction flag")?
            .then(|| Faction::parse(cur))
            .transpose()?;
        let nation = cur.string()?;
        let objectives = cur.pairs(Cursor::u32)?;
        let outcome = cur.u32()?;
        let start_technologies = (v >= 14).then(|| cur.strings()).transpose()?;
        let properties = cur.list(|cur| Property::parse(cur, 0))?;
        let required = cur.bool("required")?;
        let playable = cur.bool("playable")?;
        let ai_profile = cur.string()?;
        let restriction_group = cur.string()?;
        let stats = (independent == 0)
            .then(|| KingdomStats::parse(cur))
            .transpose()?;
        let saved_groups = cur.list(SavedGroup::parse)?;
        let flag = |cur: &mut Cursor, from: u32, what: &str| {
            (v >= from).then(|| cur.bool(what)).transpose()
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
            instance_numbers: (v >= 15).then(|| cur.pairs(Cursor::u32)).transpose()?,
            used_badges: (v >= 16).then(|| cur.u32s()).transpose()?,
            default_victory_condition: flag(cur, 17, "default victory condition")?,
            resigned: flag(cur, 18, "resigned")?,
            city_ratio_victory: flag(cur, 19, "city ratio victory")?,
            bonus_stockpile: (v >= 20).then(|| cur.pairs(Cursor::f32)).transpose()?,
        })
    }

    fn write(&self, w: &mut Writer, world_version: u32, index: u32) -> Result<()> {
        let v = self.version;
        self.thing.write(w, ReadAs::Actor(KINGDOM))?;
        let has_updatable = world_version >= UPDATABLE_WORLD_VERSION;
        w.optional_plain(
            has_updatable,
            "kingdom's updatable id",
            self.updatable,
            |w, id| {
                w.u32(1);
                w.i32(id);
            },
        )?;
        w.u32(v);
        w.u32(index);
        w.i32(self.major_index);
        w.u32(self.independent);
        w.string(&self.name)?;
        w.string(&self.color)?;
        w.u32s("relations", &self.relations)?;
        w.pairs("resources", &self.resources, Writer::f32)?;
        w.u32(self.team);
        w.u32s("team relations", &self.team_relations)?;
        w.u8(self.use_default_stockpile.into());
        w.u32(1);
        w.u32(0);
        w.i32(self.current_recruit);
        w.u8(self.faction.is_some().into());
        if let Some(faction) = &self.faction {
            faction.write(w)?;
        }
        w.string(&self.nation)?;
        w.pairs("objectives", &self.objectives, Writer::u32)?;
        w.u32(self.outcome);
        let technologies = self.start_technologies.as_deref();
        w.optional(v >= 14, "start technologies", technologies, |w, ts| {
            w.strings("start technologies", ts)
        })?;
        if self.properties.iter().any(|p| p.class != 0) {
            return Err(Error::Unwritable(format!(
                "kingdom {index} has a property of a class other than 0"
            )));
        }
        w.list("properties", &self.properties, |w, p| p.write_body(w))?;
        w.u8(self.required.into());
        w.u8(self.playable.into());
        w.string(&self.ai_profile)?;
        w.string(&self.restriction_group)?;
        let independent = self.independent != 0;
        w.optional(
            !independent,
            "kingdom's stats",
            self.stats.as_ref(),
            |w, stats| stats.write(w),
        )?;
        w.list("saved groups", &self.saved_groups, |w, g| g.write(w))?;
        let numbers = self.instance_numbers.as_deref();
        w.optional(v >= 15, "instance numbers", numbers, |w, ns| {
            w.pairs("instance numbers", ns, Writer::u32)
        })?;
        w.optional(
            v >= 16,
            "used badges",
            self.used_badges.as_deref(),
            |w, bs| w.u32s("used badges", bs),
        )?;
        for (from, what, flag) in [
            (
                17,
                "default victory condition",
                self.default_victory_condition,
            ),
            (18, "resigned", self.resigned),
            (19, "city ratio victory", self.city_ratio_victory),
        ] {
            w.optional_plain(v >= from, what, flag, |w, f| w.u8(f.into()))?;
        }
        let bonus = self.bonus_stockpile.as_deref();
        w.optional(v >= 20, "bonus stockpile", bonus, |w, bs| {
            w.pairs("bonus stockpile", bs, Writer::f32)
        })?;
        Ok(())
    }
}

impl KingdomStats {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        Ok(KingdomStats {
            favorites: Favorites::parse(cur)?,
            totals: Totals::parse(cur)?,
            snapshots: cur.list(Snapshot::parse)?,
            time_since_snapshot: cur.f32()?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        self.favorites.write(w)?;
        self.totals.write(w)?;
        w.list("snapshots", &self.snapshots, |w, s| s.write(w))?;
        w.f32(self.time_since_snapshot);
        Ok(())
    }
}

impl Favorites {
    fn lists(version: u32) -> usize {
        if version >= 2 { 5 } else { 4 }
    }

    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = cur.version("favorites", 0..=2)?;
        let lists = Self::lists(version);
        let by_template = (0..lists)
            .map(|_| match version {
                0 => cur.pairs(|c| c.u32().map(f32::from_bits)),
                _ => cur.pairs(Cursor::f32),
            })
            .collect::<Result<_>>()?;
        let by_id = (0..if version >= 1 { lists } else { 0 })
            .map(|_| cur.list(|cur| Ok((cur.u32()?, cur.f32()?))))
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
                "favorites version {} has {lists} lists by template and {ids} by id, not {} and {}",
                self.version,
                self.by_template.len(),
                self.by_id.len()
            )));
        }
        w.u32(self.version);
        for list in &self.by_template {
            w.pairs("favorites by template", list, |w, v| w.u32(v.to_bits()))?;
        }
        for list in &self.by_id {
            w.list("favorites by id", list, |w, &(id, v)| {
                w.u32(id);
                w.f32(v);
                Ok(())
            })?;
        }
        Ok(())
    }
}

impl Totals {
    fn count(version: u32) -> usize {
        if version >= 1 { 66 } else { 60 }
    }

    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = cur.version("totals", 0..=1)?;
        let values = (0..Self::count(version))
            .map(|_| cur.f32())
            .collect::<Result<_>>()?;
        Ok(Totals { version, values })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        if self.values.len() != Self::count(self.version) {
            return Err(Error::Unwritable(format!(
                "totals version {} have {} values, not {}",
                self.version,
                self.values.len(),
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
        cur.expect("snapshot version", 0)?;
        Ok(Snapshot {
            totals: Totals::parse(cur)?,
            values: cur.f32s()?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(0);
        self.totals.write(w)?;
        w.f32s(&self.values);
        Ok(())
    }
}

impl SavedGroup {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        cur.expect("saved group version", 0)?;
        Ok(SavedGroup {
            name: cur.string()?,
            display_name: cur.string()?,
            ids: cur.u32s()?,
            grouped: cur.bool("grouped")?,
            group_thing: cur.u32()?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(0);
        w.string(&self.name)?;
        w.string(&self.display_name)?;
        w.u32s("ids", &self.ids)?;
        w.u8(self.grouped.into());
        w.u32(self.group_thing);
        Ok(())
    }
}

impl Faction {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let name = cur.string()?;
        cur.expect("faction version", 0)?;
        let experience = cur.f32()?;
        let abilities = cur.list(Ability::parse)?;
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
        w.list("abilities", &self.abilities, |w, a| a.write(w))
    }
}

impl Ability {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = cur.version("ability", 0..=1)?;
        Ok(Ability {
            version,
            use_count: cur.u32()?,
            last_used: cur.f32()?,
            unused1: cur.u32()?,
            usable: cur.u8()?,
            active_targets: (version >= 1).then(|| cur.u32s()).transpose()?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(self.version);
        w.u32(self.use_count);
        w.f32(self.last_used);
        w.u32(self.unused1);
        w.u8(self.usable);
        let targets = self.active_targets.as_deref();
        w.optional(self.version >= 1, "ability's targets", targets, |w, v| {
            w.u32s("active targets", v)
        })
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
        w.u32s("test", &[3, 2]).unwrap();
        w.u32s("test", &[1, 3]).unwrap();
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
        w.u32s("test", &[0, 3, 3]).unwrap();
        w.u32(1);
        w.string("money").unwrap();
        w.f32(500.0);
        w.u32(30);
        w.u32s("test", &[1]).unwrap();
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
                unused1: 0,
                usable: 1,
                active_targets: Some(vec![9]),
            }
            .write(w)
            .unwrap();
        }
        w.string(if faction.is_some() { "nation_gbr" } else { "" })
            .unwrap();
        // Objectives, the outcome, start technologies, properties, two
        // flags, the AI profile and the restriction group.
        w.pairs("test", &[("objective_0".to_string(), 1)], Writer::u32)
            .unwrap();
        w.u32(0);
        w.u32(1);
        w.string("tech_usa_gyrostabilizers").unwrap();
        w.u32(1);
        Property {
            class: 0,
            version: 3,
            gained_at: 1.0,
            source: Some(2),
            id: "tech_armor".into(),
            duplicate: Some(1),
            from_actor: Some(0),
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
        w.u32s("test", &[4]).unwrap();
        w.u8(1);
        w.u32(5);
        w.pairs("test", &[("ger_tank".to_string(), 3)], Writer::u32)
            .unwrap();
        w.u32s("test", &[6]).unwrap();
        w.bytes(&[0, 1, 0]);
        w.pairs("test", &[("ammo".to_string(), 10.0)], Writer::f32)
            .unwrap();
    }

    #[test]
    fn refuses_fields_their_version_lacks() {
        let ability = Ability {
            version: 0,
            use_count: 0,
            last_used: -1000.0,
            unused1: 0,
            usable: 1,
            active_targets: Some(Vec::new()),
        };
        assert!(ability.write(&mut Writer::default()).is_err());

        let team = |version, restriction_group: Option<&str>| Team {
            head: Head {
                template: "team".into(),
                class: TEAM,
                id: 30,
                name: "team_30".into(),
                position: [0.0; 3],
                facing: [0.0, -1.0, 0.0],
                flags: 0,
            },
            version,
            display_name: String::new(),
            kingdom_relations: Vec::new(),
            team_relations: Vec::new(),
            restriction_group: restriction_group.map(Into::into),
        };
        let write = |t: Team| t.write(&mut Writer::default(), 0);
        assert!(write(team(0, None)).is_ok());
        assert!(write(team(0, Some("group"))).is_err());
        assert!(write(team(1, None)).is_err());
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
        assert_eq!(Kingdoms::parse(&w.0, 28).unwrap(), kingdoms);
        assert!(
            kingdoms.to_bytes(21).is_err(),
            "updatable ids need world 22"
        );

        // A kingdom reads back only as an actor of its own class.
        let mut wrong = kingdoms.clone();
        wrong.kingdoms[0].as_mut().unwrap().thing.detail = Detail::None;
        assert!(matches!(wrong.to_bytes(28), Err(Error::Unwritable(_))));
        let mut wrong = kingdoms.clone();
        wrong.kingdoms[0].as_mut().unwrap().thing.class = super::super::ACTOR;
        assert!(matches!(wrong.to_bytes(28), Err(Error::Unwritable(_))));
    }
}
