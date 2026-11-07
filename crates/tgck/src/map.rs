//! Maps (`.rmp`), laid out as [`rmp.ksy`] specifies.
//!
//! [`Map`] reads each section into a type that writes itself back, and
//! [`MapFile`] holds them all and saves the whole map.
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

mod actor;
mod ai;
mod bookkeeping;
mod file;
mod header;
mod misc;
mod objectives;
mod organization;
mod players;
mod scenery;
mod triggers;

pub use actor::{
    Actor, Body, COMPONENTS, City, Component, Construction, Mover, MoverFall, Properties, Property,
    State, Trailer, TrailerLists, Updatable, Upgrade,
};
pub use ai::{AiBlock, AiBlockLink, AiBlockSide, AiData, AiMap, AiRegion, ShoreMesh, ShoreTile};
pub use bookkeeping::{IdManager, Updatables, Update, UpdateQueue};
pub use file::MapFile;
pub use header::{
    KingdomSetup, MapHeader, Player, Rule, RuleValue, TeamSetup, WorldCreator, WorldParameters,
    WorldRules,
};
pub use misc::{GameCounter, MiscData, UsedName, write_denizen_groups};
pub use objectives::{Objective, Objectives};
pub use organization::{Damage, DamageTail, ElementState, Fighting, Organization, Position};
pub use players::{
    Ability, Faction, Favorites, Kingdom, KingdomStats, Kingdoms, Relation, RestrictionGroup,
    RestrictionGroups, SavedGroup, Snapshot, Team, Totals, World, write_teams,
};
pub use scenery::{ReactiveScenery, Topple, ToppleState};
pub use triggers::{Element, Parameter, Trigger, Triggers, Value};

use crate::container::{Container, Section};
use crate::{Cursor, Error, Result, Writer};

/// World units between adjacent height-map vertices.
pub const CELL_SIZE: f32 = 2.0;

pub struct Map<R> {
    container: Container<R>,
}

impl Map<BufReader<File>> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::new(BufReader::new(File::open(path)?))
    }
}

impl<R: Read + Seek> Map<R> {
    pub fn new(reader: R) -> Result<Self> {
        let container = Container::new(reader)?;
        container.require("MAP_HEADER")?;
        Ok(Map { container })
    }

    pub fn sections(&self) -> &[Section] {
        &self.container.sections
    }

    pub fn read_section(&mut self, section: &Section) -> Result<Vec<u8>> {
        self.container.read_section(section)
    }

    pub fn height_map(&mut self) -> Result<HeightMap> {
        HeightMap::parse(&self.container.read_named("HEIGHT_MAP")?)
    }

    pub fn mountain_height_map(&mut self) -> Result<HeightMap> {
        Ok(self.mountain_section()?.0)
    }

    /// Whether the world was saved mid-cutscene; None before world version 10.
    pub fn cutscene_mode(&mut self) -> Result<Option<bool>> {
        Ok(self.mountain_section()?.1)
    }

    fn mountain_section(&mut self) -> Result<(HeightMap, Option<bool>)> {
        let world_version = self.world_version()?;
        let data = self.container.read_named("MOUNTAIN_HEIGHT_MAP")?;
        let (data, cutscene_mode) = match data.split_last() {
            Some((&flag, rest)) if world_version >= 10 => (
                rest,
                Some(header::bool(&mut Cursor::new(&[flag]), "cutscene mode")?),
            ),
            _ => (&data[..], None),
        };
        Ok((HeightMap::parse(data)?, cutscene_mode))
    }

    pub fn terrain(&mut self) -> Result<Terrain> {
        Terrain::parse(&self.container.read_named("TERRAIN")?)
    }

    pub fn things(&mut self) -> Result<Vec<Thing>> {
        let world_version = self.world_version()?;
        let index = self.container.read_named("DYNAMIC_THINGS_INDEX")?;
        let data = self.container.read_named("DYNAMIC_THINGS")?;
        parse_things("DYNAMIC_THINGS", &index, &data, world_version)
    }

    /// None if the map is older than the sections.
    pub fn used_required_objects(&mut self) -> Result<Option<Vec<Thing>>> {
        if self.container.section("USED_REQUIRED_OBJECTS").is_none() {
            return Ok(None);
        }
        let world_version = self.world_version()?;
        let index = self.container.read_named("USED_REQUIRED_OBJECTS_INDEX")?;
        let data = self.container.read_named("USED_REQUIRED_OBJECTS")?;
        parse_things("USED_REQUIRED_OBJECTS", &index, &data, world_version).map(Some)
    }

    /// The version of the game state the map was saved with, which decides
    /// the layout of some sections.
    pub fn world_version(&mut self) -> Result<u32> {
        Cursor::new(&self.container.read_named("GWORLD")?).u32()
    }

    pub fn world(&mut self) -> Result<World> {
        World::parse(&self.container.read_named("GWORLD")?)
    }

    pub fn ids(&mut self) -> Result<IdManager> {
        IdManager::parse(&self.container.read_named("IDMGR")?)
    }

    pub fn updatables(&mut self) -> Result<Updatables> {
        let world_version = self.world_version()?;
        Updatables::parse(&self.container.read_named("UPDATABLES")?, world_version)
    }

    pub fn restriction_groups(&mut self) -> Result<RestrictionGroups> {
        RestrictionGroups::parse(&self.container.read_named("RESTRICTION_GROUPS")?)
    }

    pub fn rules(&mut self) -> Result<WorldRules> {
        WorldRules::parse(&self.container.read_named("RULES")?)
    }

    pub fn misc_data(&mut self) -> Result<MiscData> {
        let world_version = self.world_version()?;
        MiscData::parse(&self.container.read_named("MISC_DATA")?, world_version)
    }

    /// The map resource's data version (MAP).
    pub fn data_version(&mut self) -> Result<u32> {
        misc::parse_data_version(&self.container.read_named("MAP")?)
    }

    /// Checks the map saves no denizen groups, whose layout isn't decoded.
    pub fn denizen_groups(&mut self) -> Result<()> {
        misc::parse_denizen_groups(&self.container.read_named("DENIZEN_GROUPS")?)
    }

    pub fn teams(&mut self) -> Result<Vec<Option<Team>>> {
        players::parse_teams(&self.container.read_named("TEAMS")?)
    }

    pub fn kingdoms(&mut self) -> Result<Kingdoms> {
        let world_version = self.world_version()?;
        Kingdoms::parse(&self.container.read_named("KINGDOMS")?, world_version)
    }

    pub fn header(&mut self) -> Result<MapHeader> {
        MapHeader::parse(&self.container.read_named("MAP_HEADER")?)
    }

    pub fn objectives(&mut self) -> Result<Objectives> {
        Objectives::parse(&self.container.read_named("OBJECTIVES")?)
    }

    pub fn triggers(&mut self) -> Result<Triggers> {
        Triggers::parse(&self.container.read_named("TRIGGERS")?)
    }

    pub fn sun(&mut self) -> Result<Sun> {
        Sun::parse(&self.container.read_named("SUN")?)
    }

    /// The map's string table, in .tgi text syntax.
    pub fn strings(&mut self) -> Result<String> {
        let bytes = self.container.read_named("MAPSTRINGS")?;
        // Windows-1252 in practice; Latin-1 is identical for everything but 0x80..0x9F.
        Ok(bytes.iter().map(|&b| b as char).collect())
    }
}

#[derive(Debug, Clone)]
pub struct HeightMap {
    pub version: u32,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unknown: u32,
    /// Vertices per side.
    pub side: usize,
    /// Row-major heights, `side * side` entries.
    pub heights: Vec<f32>,
    /// One byte per vertex; in HEIGHT_MAP non-zero marks water.
    pub layer: Vec<u8>,
}

impl HeightMap {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let version = cur.u32()?;
        let min = cur.f32()?;
        let max = cur.f32()?;
        let default = cur.f32()?;
        let unknown = cur.u32()?;
        let count = cur.u32()? as usize;
        let side = count.isqrt();
        if side * side != count {
            return Err(Error::Corrupt(format!(
                "height map has {count} vertices, not a square"
            )));
        }
        let heights = cur
            .bytes(count * 4)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| f32::from_le_bytes(c))
            .collect();
        let layer = cur.bytes(count)?.to_vec();
        cur.finish("height map")?;
        Ok(HeightMap {
            version,
            min,
            max,
            default,
            unknown,
            side,
            heights,
            layer,
        })
    }

    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.heights[y * self.side + x]
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(self.version);
        w.f32(self.min);
        w.f32(self.max);
        w.f32(self.default);
        w.u32(self.unknown);
        w.u32(self.heights.len() as u32);
        w.f32s(&self.heights);
        w.bytes(&self.layer);
        w.0
    }
}

/// The MAPSTRINGS section holding `text`, one byte per character as
/// [`Map::strings`] reads it.
pub fn write_strings(text: &str) -> Result<Vec<u8>> {
    text.chars()
        .map(|c| u8::try_from(c).map_err(|_| Error::Unwritable(format!("{c:?} in MAPSTRINGS"))))
        .collect()
}

/// The MOUNTAIN_HEIGHT_MAP section holding `mountain` and the world's
/// cutscene-mode flag, which is None exactly before world version 10.
pub fn write_mountain_height_map(mountain: &HeightMap, cutscene_mode: Option<bool>) -> Vec<u8> {
    let mut data = mountain.to_bytes();
    data.extend(cutscene_mode.map(u8::from));
    data
}

#[derive(Debug, Clone)]
pub struct Terrain {
    pub version: u32,
    pub unknown: u32,
    pub biome: String,
    /// Terrain type ids, e.g. `temperate_grass03`; tiles index into this.
    pub names: Vec<String>,
    /// Tiles per side.
    pub side: usize,
    /// Row-major, `side * side` entries.
    pub tiles: Vec<Tile>,
    /// From version 2 on.
    pub ai_map: Option<AiMap>,
    /// From version 6 on.
    pub shore_mesh: Option<ShoreMesh>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    /// Index into [`Terrain::names`].
    pub terrain: u16,
    /// Water body id (0 = dry land); matches the height map's vertex layer.
    pub water_body: u8,
    /// Bits 0-3 are stored; the game recomputes bits 4-6 from the terrain's group.
    pub flags: u8,
    /// The kingdoms that have explored it, a bit per kingdom slot.
    pub seen_by: u16,
    /// Never read; kept because some version 6 maps hold leftover bytes here.
    pub padding: u8,
}

impl Terrain {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let version = cur.u32()?;
        if version > 7 {
            return Err(Error::Corrupt(format!(
                "TERRAIN version {version} is newer than the game supports"
            )));
        }
        let unknown = cur.u32()?;
        let biome = cur.string()?;
        let n = cur.u32()?;
        let names = (0..n).map(|_| cur.string()).collect::<Result<Vec<_>>>()?;
        let count = cur.u32()? as usize;
        let side = count.isqrt();
        if side * side != count {
            return Err(Error::Corrupt(format!(
                "TERRAIN has {count} tiles, not a square"
            )));
        }
        let size = cur.u32()? as usize;
        let expected = tile_size(version);
        if size != expected {
            return Err(Error::Corrupt(format!(
                "TERRAIN v{version} tile size {size}, expected {expected}"
            )));
        }
        let raw = cur.bytes(count * size)?;
        let tiles: Vec<Tile> = raw
            .chunks_exact(size)
            .map(|t| {
                let u16_at = |o: usize| {
                    t.get(o..o + 2)
                        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
                };
                if version >= 7 {
                    Tile {
                        terrain: u16_at(0),
                        seen_by: u16_at(2),
                        water_body: t[4],
                        padding: t[5],
                        flags: t[6],
                    }
                } else {
                    // Older tiles hold the same fields in a different order.
                    Tile {
                        terrain: t[0] as u16,
                        flags: t[1],
                        seen_by: u16_at(2),
                        water_body: t.get(4).copied().unwrap_or(0),
                        padding: t.get(5).copied().unwrap_or(0),
                    }
                }
            })
            .collect();
        if let Some(i) = tiles.iter().position(|t| t.terrain as usize >= names.len()) {
            return Err(Error::Corrupt(format!(
                "tile {i} uses terrain {} of {}",
                tiles[i].terrain,
                names.len()
            )));
        }
        let ai_map = (version >= 2).then(|| AiMap::parse(&mut cur)).transpose()?;
        let shore_mesh = (version >= 6)
            .then(|| ShoreMesh::parse(&mut cur))
            .transpose()?;
        cur.finish("TERRAIN")?;
        Ok(Terrain {
            version,
            unknown,
            biome,
            names,
            side,
            tiles,
            ai_map,
            shore_mesh,
        })
    }

    pub fn at(&self, x: usize, y: usize) -> &Tile {
        &self.tiles[y * self.side + x]
    }

    pub fn name_of(&self, tile: &Tile) -> &str {
        &self.names[tile.terrain as usize]
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        w.u32(self.version);
        w.u32(self.unknown);
        w.string(&self.biome)?;
        w.u32(self.names.len() as u32);
        for name in &self.names {
            w.string(name)?;
        }
        let size = tile_size(self.version);
        w.u32(self.tiles.len() as u32);
        w.u32(size as u32);
        for t in &self.tiles {
            if self.version >= 7 {
                w.u16(t.terrain);
                w.u16(t.seen_by);
                w.bytes(&[t.water_body, t.padding, t.flags, 0]);
                continue;
            }
            let terrain = u8::try_from(t.terrain).map_err(|_| {
                Error::Unwritable(format!("terrain {} in a v{} tile", t.terrain, self.version))
            })?;
            let mut tile = [terrain, t.flags, 0, 0, t.water_body, t.padding];
            tile[2..4].copy_from_slice(&t.seen_by.to_le_bytes());
            w.bytes(&tile[..size]);
        }
        header::optional(self.version >= 2, "AI map", self.ai_map.as_ref(), |m| {
            m.write(&mut w);
            Ok(())
        })?;
        header::optional(
            self.version >= 6,
            "shore mesh",
            self.shore_mesh.as_ref(),
            |m| {
                m.write(&mut w);
                Ok(())
            },
        )?;
        Ok(w.0)
    }
}

/// Bytes per TERRAIN tile in a version.
fn tile_size(version: u32) -> usize {
    match version {
        7.. => 8,
        5..=6 => 6,
        4 => 4,
        _ => 2,
    }
}

#[derive(Debug, Clone)]
pub struct Sun {
    pub version: u32,
    /// Registered only while a fade runs; -1 otherwise.
    pub updatable: Updatable,
    /// Unit vector the light travels along, in map coordinates (z up).
    pub direction: [f32; 3],
    pub diffuse: [f32; 3],
    pub ambient: [f32; 3],
    pub specular: [f32; 3],
    /// The colours the current ones fade toward.
    pub target_diffuse: [f32; 3],
    pub target_ambient: [f32; 3],
    pub target_specular: [f32; 3],
    /// Seconds left in the colour fade.
    pub color_fade_time: f32,
    /// A brightness multiplier.
    pub dimmer: f32,
    pub target_dimmer: f32,
    pub dimmer_fade_time: f32,
    /// The game time of the last fade step; -1 before any.
    pub last_update_time: f32,
    /// The cinematic letterbox and the screen fade; from version 1 on.
    pub overlays: Option<[Overlay; 2]>,
}

/// A script-driven screen effect, such as the letterbox.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Overlay {
    pub on: bool,
    /// The game time it was turned on or off at; the game ignores it in
    /// version 1.
    pub start: f32,
    /// Seconds it takes to come or go; the game ignores it in version 1.
    pub duration: f32,
}

impl Sun {
    const MAX_VERSION: u32 = 2;

    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let cur = &mut cur;
        let version = header::check_version("sun", cur.u32()?, 0..=Self::MAX_VERSION)?;
        let sun = Sun {
            version,
            updatable: Updatable::parse(cur)?,
            direction: cur.f32s()?,
            diffuse: cur.f32s()?,
            ambient: cur.f32s()?,
            specular: cur.f32s()?,
            target_diffuse: cur.f32s()?,
            target_ambient: cur.f32s()?,
            target_specular: cur.f32s()?,
            color_fade_time: cur.f32()?,
            dimmer: cur.f32()?,
            target_dimmer: cur.f32()?,
            dimmer_fade_time: cur.f32()?,
            last_update_time: cur.f32()?,
            overlays: (version >= 1)
                .then(|| Result::<_>::Ok([Overlay::parse(cur)?, Overlay::parse(cur)?]))
                .transpose()?,
        };
        cur.finish("SUN")?;
        Ok(sun)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        w.u32(self.version);
        self.updatable.write(&mut w);
        for v in [
            self.direction,
            self.diffuse,
            self.ambient,
            self.specular,
            self.target_diffuse,
            self.target_ambient,
            self.target_specular,
        ] {
            w.f32s(&v);
        }
        w.f32s(&[
            self.color_fade_time,
            self.dimmer,
            self.target_dimmer,
            self.dimmer_fade_time,
            self.last_update_time,
        ]);
        header::optional(
            self.version >= 1,
            "sun overlays",
            self.overlays,
            |overlays| {
                for o in overlays {
                    w.u8(o.on.into());
                    w.f32(o.start);
                    w.f32(o.duration);
                }
                Ok(())
            },
        )?;
        Ok(w.0)
    }
}

impl Overlay {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        Ok(Overlay {
            on: header::bool(cur, "overlay flag")?,
            start: cur.f32()?,
            duration: cur.f32()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Thing {
    /// Template name, e.g. `temperate_trees`, `start_loc`.
    pub template: String,
    /// The kind of object the record holds, which decides its [`Detail`]:
    /// one for scenery, another for units and buildings, and so on.
    pub class: u32,
    pub id: u32,
    /// The name scripts refer to it by; empty for most.
    pub name: String,
    pub position: [f32; 3],
    /// Direction the object faces, a unit vector in x and y; cameras add
    /// their tilt in z.
    pub facing: [f32; 3],
    pub flags: u32,
    /// Size relative to the template's.
    pub scale: f32,
    /// Which of the template's model variations it shows; -1 when none is set.
    pub variation: i32,
    /// The kingdoms that have seen it, a bit per kingdom slot.
    pub seen_by: u16,
    pub detail: Detail,
}

/// The [`Thing::class`] of plain scenery, such as rocks and street lights,
/// whose record has no detail.
pub const SCENERY: u32 = 2;
/// The [`Thing::class`] of actors: units, buildings, herds, roads and other
/// things made of components.
pub const ACTOR: u32 = 3;
/// The [`Thing::class`] of areas.
pub const AREA: u32 = 6;
/// The [`Thing::class`] of cameras.
pub const CAMERA: u32 = 7;
/// The [`Thing::class`] of scenery that stirs or topples as units pass.
pub const REACTIVE_SCENERY: u32 = 9;

/// The class-specific rest of a [`Thing`]'s record.
#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    /// The detail of a class that adds nothing, such as [`SCENERY`].
    None,
    Actor(Actor),
    Area(Area),
    Camera(Camera),
    ReactiveScenery(ReactiveScenery),
    /// Not decoded.
    Raw(Vec<u8>),
}

/// A region scripts and triggers test units against.
#[derive(Debug, Clone, PartialEq)]
pub struct Area {
    pub version: u32,
    /// Left out by some maps, such as the cutscenes.
    pub id: Option<u32>,
    /// The name the map's objectives and triggers use.
    pub name: String,
    /// 1 for a circle, 3 for a box.
    pub shape: u32,
    pub radius: f32,
    /// The box about the position: min x, min y, max x, max y.
    pub bounds: [f32; 4],
}

/// A camera position for cutscenes and scripted views.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    pub version: u32,
    pub name: String,
    pub zoom: f32,
}

/// The version DYNAMIC_THINGS records are written in.
const THINGS_VERSION: u32 = 11;

/// Reads a thing list from its `{name}_INDEX` and `{name}` sections.
fn parse_things(name: &str, index: &[u8], data: &[u8], world_version: u32) -> Result<Vec<Thing>> {
    let index_name = format!("{name}_INDEX");
    let mut idx = Cursor::new(index);
    expect(&mut idx, &format!("{index_name} data version"), 0)?;
    let count = idx.u32()?;
    let mut cur = Cursor::new(data);
    expect(&mut cur, &format!("{name} data version"), 0)?;
    expect(&mut cur, &format!("{name} version"), THINGS_VERSION)?;
    let mut things = Vec::with_capacity(count.min(1 << 20) as usize);
    for i in 0..count {
        let size = idx.u32()? as usize;
        let thing = Thing::parse(cur.bytes(size)?, world_version).map_err(|e| match e {
            Error::Corrupt(m) => Error::Corrupt(format!("thing {i}: {m}")),
            e => e,
        })?;
        things.push(thing);
    }
    idx.finish(&index_name)?;
    cur.finish(name)?;
    Ok(things)
}

/// The DYNAMIC_THINGS_INDEX and DYNAMIC_THINGS sections holding `things`, or
/// the like pair of USED_REQUIRED_OBJECTS sections.
pub fn write_things(things: &[Thing]) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut index = Writer::default();
    index.u32(0);
    index.u32(things.len() as u32);
    let mut data = Writer::default();
    data.u32(0);
    data.u32(THINGS_VERSION);
    for thing in things {
        let start = data.0.len();
        thing.write(&mut data)?;
        index.u32((data.0.len() - start) as u32);
    }
    Ok((index.0, data.0))
}

/// Reads a `u32` that every known file has the same value in, so that
/// writing the value back reproduces the file.
fn expect(cur: &mut Cursor, what: &str, want: u32) -> Result<()> {
    match cur.u32()? {
        v if v == want => Ok(()),
        v => Err(Error::Corrupt(format!("{what} is {v}, expected {want}"))),
    }
}

/// What every saved object's record starts with, things or not.
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    pub template: String,
    pub class: u32,
    pub id: u32,
    pub name: String,
    pub position: [f32; 3],
    pub facing: [f32; 3],
    pub flags: u32,
}

impl Head {
    fn parse(rec: &mut Cursor) -> Result<Self> {
        let template = rec.string()?;
        let class = rec.u32()?;
        let id = rec.u32()?;
        expect(rec, "thing version", 2)?;
        let name = rec.string()?;
        expect(rec, "creation time", 0)?;
        Ok(Head {
            template,
            class,
            id,
            name,
            position: rec.f32s()?,
            facing: rec.f32s()?,
            flags: rec.u32()?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.string(&self.template)?;
        w.u32(self.class);
        w.u32(self.id);
        w.u32(2);
        w.string(&self.name)?;
        w.u32(0);
        w.f32s(&self.position);
        w.f32s(&self.facing);
        w.u32(self.flags);
        Ok(())
    }
}

impl Thing {
    fn parse(record: &[u8], world_version: u32) -> Result<Self> {
        let mut rec = Cursor::new(record);
        let mut thing = Thing::parse_shared(&mut rec)?;
        thing.detail = Detail::parse(thing.class, rec.rest(), world_version)?;
        Ok(thing)
    }

    /// An organization's member, whose record has no length: only actors'
    /// can be told where they end.
    fn parse_member(rec: &mut Cursor, world_version: u32) -> Result<Self> {
        let mut thing = Thing::parse_shared(rec)?;
        if thing.class != ACTOR {
            return Err(Error::Corrupt(format!("member of class {}", thing.class)));
        }
        thing.detail = Detail::Actor(Actor::parse(rec, world_version)?);
        Ok(thing)
    }

    /// The fields every class has, leaving the detail empty.
    fn parse_shared(rec: &mut Cursor) -> Result<Self> {
        let head = Head::parse(rec)?;
        Self::parse_placement(head, rec)
    }

    /// The fields after the head, leaving the detail empty.
    fn parse_placement(head: Head, rec: &mut Cursor) -> Result<Self> {
        let Head {
            template,
            class,
            id,
            name,
            position,
            facing,
            flags,
        } = head;
        expect(rec, "placement version", 5)?;
        let scale = rec.f32()?;
        let variation = rec.i32()?;
        expect(rec, "placement's actor", 0)?;
        let seen_by = rec.u16()?;
        expect(rec, "remembered views", 0)?;
        Ok(Thing {
            template,
            class,
            id,
            name,
            position,
            facing,
            flags,
            scale,
            variation,
            seen_by,
            detail: Detail::Raw(Vec::new()),
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        Head {
            template: self.template.clone(),
            class: self.class,
            id: self.id,
            name: self.name.clone(),
            position: self.position,
            facing: self.facing,
            flags: self.flags,
        }
        .write(w)?;
        w.u32(5);
        w.f32(self.scale);
        w.i32(self.variation);
        w.u32(0);
        w.u16(self.seen_by);
        w.u32(0);
        self.detail.write(w)
    }

    /// The organization this thing is, if its record holds one.
    pub fn organization(&self) -> Option<&Organization> {
        match &self.detail {
            Detail::Actor(actor) => actor.organization(),
            _ => None,
        }
    }

    /// Which of its nation's division badges this thing wears as an HQ.
    pub fn hq_badge(&self) -> Option<i32> {
        match &self.detail {
            Detail::Actor(actor) => actor.hq_badge(),
            _ => None,
        }
    }
}

impl Detail {
    fn parse(class: u32, data: &[u8], world_version: u32) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let detail = match class {
            ACTOR => Detail::Actor(Actor::parse_whole(&mut cur, world_version)?),
            AREA => {
                // Records without the id are 8 bytes shorter; the name's
                // length tells which this is.
                let name_len = |at: usize| {
                    data.get(at..at + 2)
                        .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize * 2)
                };
                let has_id = name_len(4) != data.len().checked_sub(34);
                let version = cur.u32()?;
                let id = if has_id {
                    let id = cur.u32()?;
                    expect(&mut cur, "area id version", 2)?;
                    Some(id)
                } else {
                    None
                };
                let area = Area {
                    version,
                    id,
                    name: cur.string()?,
                    shape: cur.u32()?,
                    radius: cur.f32()?,
                    bounds: cur.f32s()?,
                };
                expect(&mut cur, "area's last field", 0)?;
                Detail::Area(area)
            }
            CAMERA => Detail::Camera(Camera {
                version: cur.u32()?,
                name: cur.string()?,
                zoom: cur.f32()?,
            }),
            SCENERY => Detail::None,
            REACTIVE_SCENERY => Detail::ReactiveScenery(ReactiveScenery::parse(&mut cur)?),
            _ => Detail::Raw(cur.rest().to_vec()),
        };
        cur.finish("thing record")?;
        Ok(detail)
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        match self {
            Detail::Actor(a) => a.write(w)?,
            Detail::Area(a) => {
                w.u32(a.version);
                if let Some(id) = a.id {
                    w.u32(id);
                    w.u32(2);
                }
                w.string(&a.name)?;
                w.u32(a.shape);
                w.f32(a.radius);
                w.f32s(&a.bounds);
                w.u32(0);
            }
            Detail::Camera(c) => {
                w.u32(c.version);
                w.string(&c.name)?;
                w.f32(c.zoom);
            }
            Detail::ReactiveScenery(s) => s.write(w)?,
            Detail::None => {}
            Detail::Raw(raw) => w.bytes(raw),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{build_container, string};
    use std::io;

    pub(super) fn height_section(side: u32, water_at: usize) -> Vec<u8> {
        let n = (side * side) as usize;
        let mut b = Vec::new();
        b.extend(1u32.to_le_bytes());
        b.extend((-10f32).to_le_bytes());
        b.extend(10f32.to_le_bytes());
        b.extend(2f32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend((n as u32).to_le_bytes());
        for i in 0..n {
            b.extend((i as f32).to_le_bytes());
        }
        b.extend((0..n).map(|i| u8::from(i == water_at)));
        b
    }

    pub(super) fn thing_record(
        class: u32,
        template: &str,
        id: u32,
        pos: [f32; 3],
        detail: &[u8],
    ) -> Vec<u8> {
        let mut r = string(template);
        r.extend([class, id, 2].map(u32::to_le_bytes).concat());
        r.extend(string(""));
        r.extend(0u32.to_le_bytes());
        for v in pos.into_iter().chain([0.0, -1.0, 0.0]) {
            r.extend(v.to_le_bytes());
        }
        r.extend(
            [0x2400a800, 5, 1f32.to_bits(), u32::MAX, 0, 4]
                .map(u32::to_le_bytes)
                .concat(),
        );
        r.extend([0, 0]);
        r.extend(detail);
        r
    }

    /// An actor's detail: version 14, owned by kingdom 2.
    fn actor_detail(components: u32, state: &[u8]) -> Vec<u8> {
        let mut d = [14u32, 2].map(u32::to_le_bytes).concat();
        d.extend(0x0800_0000u64.to_le_bytes());
        d.extend(
            [components, components | 0x10]
                .map(u32::to_le_bytes)
                .concat(),
        );
        d.extend(state);
        d
    }

    pub(super) fn things_sections(records: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>) {
        let mut data = [0u32, 11].map(u32::to_le_bytes).concat();
        let mut index = [0u32, records.len() as u32].map(u32::to_le_bytes).concat();
        for r in records {
            data.extend(r);
            index.extend((r.len() as u32).to_le_bytes());
        }
        (index, data)
    }

    pub(super) fn terrain_section(version: u32, tiles: &[&[u8]]) -> Vec<u8> {
        let mut b = version.to_le_bytes().to_vec();
        b.extend(1u32.to_le_bytes());
        b.extend(string("desert"));
        b.extend(2u32.to_le_bytes());
        b.extend(string("sand"));
        b.extend(string("ocean"));
        b.extend((tiles.len() as u32).to_le_bytes());
        b.extend((tiles[0].len() as u32).to_le_bytes());
        tiles.iter().for_each(|t| b.extend(*t));
        // No AI map, and from version 6 an empty shore mesh.
        b.extend([0, 0, 0, 0, 0]);
        if version >= 6 {
            b.extend([0u32; 4].map(u32::to_le_bytes).concat());
        }
        b
    }

    #[test]
    fn parses_terrain_v7_and_v6() {
        let v7 = Terrain::parse(&terrain_section(
            7,
            &[&[1, 0, 0, 0, 3, 0, 0x21, 0], &[0; 8], &[0; 8], &[0; 8]],
        ))
        .unwrap();
        assert_eq!(
            (v7.biome.as_str(), v7.side, v7.names.len()),
            ("desert", 2, 2)
        );
        assert_eq!(
            *v7.at(0, 0),
            Tile {
                terrain: 1,
                water_body: 3,
                flags: 0x21,
                seen_by: 0,
                padding: 0,
            }
        );
        assert_eq!(v7.name_of(v7.at(0, 0)), "ocean");
        assert_eq!(v7.ai_map, Some(AiMap { data: None }));
        assert_eq!(v7.shore_mesh, Some(ShoreMesh::default()));
        assert_eq!(
            v7.to_bytes().unwrap(),
            terrain_section(
                7,
                &[&[1, 0, 0, 0, 3, 0, 0x21, 0], &[0; 8], &[0; 8], &[0; 8]]
            )
        );

        let v6 = Terrain::parse(&terrain_section(
            6,
            &[&[1, 0x21, 0, 0, 3, 0], &[0; 6], &[0; 6], &[0; 6]],
        ))
        .unwrap();
        assert_eq!(v6.at(0, 0), v7.at(0, 0));

        assert!(
            Terrain::parse(&terrain_section(7, &[&[9, 0, 0, 0, 0, 0, 0, 0]])).is_err(),
            "bad terrain index"
        );
        assert!(
            Terrain::parse(&terrain_section(6, &[&[0; 8]])).is_err(),
            "wrong tile size"
        );
    }

    pub(super) fn sun_section(version: u32) -> Vec<u8> {
        let mut b = [version, 1, u32::MAX].map(u32::to_le_bytes).concat();
        for v in [
            0.6, 0.0, -0.8, 1.0, 1.0, 1.0, 0.8, 0.8, 0.8, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.8, 0.8,
            0.8, 0.0, 0.0, 0.0, 0.0, 1.0, 0.5, 2.0, -1.0f32,
        ] {
            b.extend(v.to_le_bytes());
        }
        if version >= 1 {
            for (on, start, duration) in [(1u8, 3.0f32, 1.5f32), (0, 0.0, 0.0)] {
                b.push(on);
                b.extend(start.to_le_bytes());
                b.extend(duration.to_le_bytes());
            }
        }
        b
    }

    #[test]
    fn parses_sun() {
        let b = sun_section(2);
        assert_eq!(b.len(), 134);
        let sun = Sun::parse(&b).unwrap();
        assert_eq!((sun.version, sun.direction), (2, [0.6, 0.0, -0.8]));
        assert_eq!((sun.updatable.version, sun.updatable.id), (1, -1));
        assert_eq!((sun.diffuse, sun.ambient), ([1.0; 3], [0.8; 3]));
        assert_eq!(sun.target_diffuse, sun.diffuse);
        assert_eq!((sun.target_dimmer, sun.dimmer_fade_time), (0.5, 2.0));
        let [letterbox, fade] = sun.overlays.unwrap();
        assert_eq!(
            (letterbox.on, letterbox.duration, fade.on),
            (true, 1.5, false)
        );
        assert!(Sun::parse(&b[..40]).is_err(), "truncated");
        assert!(
            Sun::parse(&[&b[..], &[0]].concat()).is_err(),
            "trailing byte"
        );
        let v0 = Sun::parse(&sun_section(0)).unwrap();
        assert!(v0.overlays.is_none());
    }

    #[test]
    fn parses_height_map_and_things() {
        let (index, things) = things_sections(&[
            thing_record(
                ACTOR,
                "start_loc",
                7,
                [10.0, 20.0, 2.0],
                &actor_detail(0x200, &[1, 2, 3]),
            ),
            thing_record(2, "tree", 8, [0.0; 3], &[]),
        ]);
        let bytes = build_container(&[
            ("MAP", &[0; 4], false),
            ("MAP_HEADER", &[0; 64], true),
            ("GWORLD", &28u32.to_le_bytes(), true),
            ("HEIGHT_MAP", &height_section(3, 4), true),
            ("DYNAMIC_THINGS", &things, true),
            ("DYNAMIC_THINGS_INDEX", &index, true),
        ]);
        let mut map = Map::new(io::Cursor::new(bytes)).unwrap();
        assert_eq!(map.data_version().unwrap(), 0);
        assert!(map.used_required_objects().unwrap().is_none(), "too old");
        let hm = map.height_map().unwrap();
        assert_eq!((hm.side, hm.default), (3, 2.0));
        assert_eq!(hm.at(2, 1), 5.0);
        assert_eq!(hm.layer[4], 1);
        let t = map.things().unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(
            (t[0].template.as_str(), t[0].id, t[0].position),
            ("start_loc", 7, [10.0, 20.0, 2.0])
        );
        assert_eq!((t[0].scale, t[0].variation), (1.0, -1));
        let Detail::Actor(actor) = &t[0].detail else {
            panic!("{:?}", t[0].detail)
        };
        assert_eq!((actor.kingdom, actor.states), (2, 0x0800_0000));
        assert_eq!(
            actor.component_names().collect::<Vec<_>>(),
            ["Mind", "Economy"]
        );
        assert_eq!(actor.template_components, 0x200);
        assert_eq!(actor.state, State::Raw(vec![1, 2, 3]), "Mind has no class");
        assert_eq!(t[1].detail, Detail::None);
        assert_eq!(write_things(&t).unwrap(), (index, things));
    }

    #[test]
    fn parses_areas_and_cameras() {
        let mut circle = 1u32.to_le_bytes().to_vec();
        circle.extend([0x74u32, 2].map(u32::to_le_bytes).concat());
        circle.extend(string("Ambush"));
        circle.extend(1u32.to_le_bytes());
        circle.extend(
            [30.5f32, -5.0, -50.0, 5.0, 50.0]
                .map(f32::to_le_bytes)
                .concat(),
        );
        circle.extend(0u32.to_le_bytes());
        // As in the cutscene maps, which leave out the area's id.
        let mut no_id = 1u32.to_le_bytes().to_vec();
        no_id.extend(string("area_0"));
        no_id.extend(circle[circle.len() - 28..].iter());
        let mut camera = 2u32.to_le_bytes().to_vec();
        camera.extend(string("Intro"));
        camera.extend(1.3f32.to_le_bytes());

        let (index, data) = things_sections(&[
            thing_record(AREA, "circle_area", 1, [0.0; 3], &circle),
            thing_record(AREA, "circle_area", 2, [0.0; 3], &no_id),
            thing_record(CAMERA, "camera", 3, [0.0; 3], &camera),
        ]);
        let things = parse_things("DYNAMIC_THINGS", &index, &data, 28).unwrap();
        let Detail::Area(area) = &things[0].detail else {
            panic!("{:?}", things[0].detail)
        };
        assert_eq!((area.id, area.name.as_str()), (Some(0x74), "Ambush"));
        assert_eq!((area.shape, area.radius), (1, 30.5));
        assert_eq!(area.bounds, [-5.0, -50.0, 5.0, 50.0]);
        let Detail::Area(area) = &things[1].detail else {
            panic!("{:?}", things[1].detail)
        };
        assert_eq!((area.id, area.name.as_str()), (None, "area_0"));
        assert_eq!(
            things[2].detail,
            Detail::Camera(Camera {
                version: 2,
                name: "Intro".into(),
                zoom: 1.3
            })
        );
        assert_eq!(write_things(&things).unwrap(), (index, data));
    }

    #[test]
    fn reads_cutscene_mode_after_the_mountain_height_map() {
        let hm = height_section(3, 4);
        for (world_version, flag, want) in [(28, &[1][..], Some(true)), (9, &[], None)] {
            let mountain = [&hm[..], flag].concat();
            let bytes = build_container(&[
                ("MAP_HEADER", &[0; 64], true),
                ("GWORLD", &(world_version as u32).to_le_bytes(), true),
                ("MOUNTAIN_HEIGHT_MAP", &mountain, true),
            ]);
            let mut map = Map::new(io::Cursor::new(bytes)).unwrap();
            assert_eq!(map.cutscene_mode().unwrap(), want);
            let parsed = map.mountain_height_map().unwrap();
            assert_eq!(parsed.side, 3);
            assert_eq!(write_mountain_height_map(&parsed, want), mountain);
        }
    }

    #[test]
    fn writes_strings_back() {
        let text = "[Text]\n{\n\tname_3954 = \"Stettin\"\n\tteam_0 = \"K\u{f6}nigsberg\"\n}\n";
        let bytes = write_strings(text).unwrap();
        assert_eq!(bytes.len(), text.chars().count());
        assert!(bytes.contains(&0xf6), "one byte per character");
        let container =
            build_container(&[("MAP_HEADER", &[0; 64], true), ("MAPSTRINGS", &bytes, true)]);
        let mut map = Map::new(io::Cursor::new(container)).unwrap();
        assert_eq!(map.strings().unwrap(), text);
        assert!(write_strings("\u{2019}").is_err(), "beyond one byte");
    }

    #[test]
    fn reads_used_required_objects() {
        let (index, data) = write_things(&[]).unwrap();
        assert_eq!(data, [0, 0, 0, 0, 11, 0, 0, 0]);
        let bytes = build_container(&[
            ("MAP_HEADER", &[0; 64], true),
            ("GWORLD", &25u32.to_le_bytes(), true),
            ("USED_REQUIRED_OBJECTS", &data, true),
            ("USED_REQUIRED_OBJECTS_INDEX", &index, true),
        ]);
        let mut map = Map::new(io::Cursor::new(bytes)).unwrap();
        assert_eq!(
            map.used_required_objects().unwrap().map(|t| t.len()),
            Some(0)
        );
    }

    #[test]
    fn rejects_unknown_thing_versions() {
        let mut record = thing_record(2, "tree", 1, [0.0; 3], &[]);
        let at = string("tree").len() + 8;
        record[at] = 3;
        let (index, data) = things_sections(&[record]);
        assert!(parse_things("DYNAMIC_THINGS", &index, &data, 28).is_err());

        let mut actor = actor_detail(0, &[]);
        actor[0] = 15;
        let (index, data) = things_sections(&[thing_record(ACTOR, "tank", 1, [0.0; 3], &actor)]);
        assert!(
            parse_things("DYNAMIC_THINGS", &index, &data, 28).is_err(),
            "newer than the game"
        );
    }

    #[test]
    fn writes_sections_back() {
        let hm = height_section(3, 4);
        assert_eq!(HeightMap::parse(&hm).unwrap().to_bytes(), hm);
        assert!(
            HeightMap::parse(&[&hm[..], &[0]].concat()).is_err(),
            "trailing byte"
        );
        for (version, tile) in [
            (7, &[1, 0, 2, 0, 3, 9, 0x21, 0][..]),
            (6, &[1, 0x21, 2, 0, 3, 9]),
        ] {
            let terrain = terrain_section(version, &[tile; 4]);
            let parsed = Terrain::parse(&terrain).unwrap();
            assert_eq!(parsed.at(0, 0).padding, 9);
            assert_eq!(parsed.to_bytes().unwrap(), terrain);
        }
        for version in [0, 2] {
            let sun = sun_section(version);
            assert_eq!(Sun::parse(&sun).unwrap().to_bytes().unwrap(), sun);
        }
    }
}
