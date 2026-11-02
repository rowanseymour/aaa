//! Maps (`.rmp`): a container with 22 named, individually zlib-compressed sections.
//!
//! Section payloads are packed little-endian streams with no alignment; strings
//! are `u16 len` + UTF-16LE. Decoded so far:
//!
//! ```text
//! HEIGHT_MAP, MOUNTAIN_HEIGHT_MAP
//!     u32 version (1), f32 min (-10), f32 max (10), f32 default, u32 unknown (1),
//!     u32 count (side * side, side = 257 on standard maps),
//!     f32 heights[count]   row-major, one per vertex; HEIGHT_MAP holds the
//!                          water surface at water vertices, MOUNTAIN the
//!                          (negative) depth of the bed below it
//!     u8  layer[count]     HEIGHT_MAP: non-zero = water; MOUNTAIN: all 0
//!     trailing bytes       MOUNTAIN_HEIGHT_MAP has one more
//!
//! DYNAMIC_THINGS_INDEX     u32 unknown (0), u32 count, u32 record_size[count]
//! DYNAMIC_THINGS           u32 unknown (0), u32 version (11), then per record:
//!     string template, u32 class, u32 id, u32 version (2), string name,
//!     u32 unknown (0), f32 position[3], f32 facing[3],
//!     u32 flags, u32 version (5), f32 scale, i32 variation (-1 = none),
//!     u32 unknown (0), u32 unknown, u16 unknown (0),
//!     then by class:
//!       area (6)    u32 version (1), [u32 id, u32 version (2),] string name,
//!                   u32 shape (1 circle, 3 box), f32 radius, f32 bounds[4],
//!                   u32 unknown (0); the cutscene maps leave out the id
//!       actor (3)   u32 version (9-14), u32 kingdom, u64 flags,
//!                   u32 template_components, u32 components, then each
//!                   component's state and the actor's own: not decoded, as
//!                   each component's loader reads its own layout, unprefixed
//!       camera (7)  u32 version (1 or 2), string name, f32 unknown
//!       others      not decoded
//!   An organization's (a herd or regiment) component state holds its members:
//!     32 bytes unknown, string layout, u32 n, string slot_element[n],
//!     u32 positions, then per member: u32 index, a record as above whose
//!     undecoded rest runs up to the next member
//!
//! SUN
//!     u32 version (1 or 2), u32 unknown (1), u32 unknown (-1),
//!     f32 direction[3]     unit vector the light travels along, z up
//!     f32 diffuse[3], ambient[3], specular[3]   as in Templates/template_sun.tgi
//!     rest unknown: the same colours again, then 38 more bytes
//!
//! MAPSTRINGS               plain text in .tgi syntax (map name, objectives, ...)
//!
//! TERRAIN
//!     u32 version (6 or 7 in shipped maps; the game accepts up to 7)
//!     u32 unknown (must be 1)
//!     string biome          biome id, e.g. "temperate_grassland"
//!     u32 n, string terrain[n]
//!     u32 tile_count (width * height = (height-map side - 1)^2)
//!     u32 tile_size  (8 for v7, 6 for v5-6, 4 for v4, 2 before)
//!     tile[tile_count], row-major:
//!       v7: u16 terrain, u16 unknown, u8 water_body, u8 unknown2, u8 flags, u8 unused
//!       v6: u8 terrain, u8 flags, u16 unknown, u8 water_body, u8 unknown2
//!     remainder: cached AI map analysis, followed by one more object for v6+.
//!       The game can rebuild this data, so it is kept undecoded.
//! ```
//!
//! World coordinates span 0..(side - 1) * 2, i.e. two world units per height cell.

use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

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
        HeightMap::parse(&self.container.read_named("MOUNTAIN_HEIGHT_MAP")?)
    }

    pub fn terrain(&mut self) -> Result<Terrain> {
        Terrain::parse(&self.container.read_named("TERRAIN")?)
    }

    pub fn things(&mut self) -> Result<Vec<Thing>> {
        let index = self.container.read_named("DYNAMIC_THINGS_INDEX")?;
        let data = self.container.read_named("DYNAMIC_THINGS")?;
        parse_things(&index, &data)
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
    pub trailing: Vec<u8>,
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
        let trailing = cur.rest().to_vec();
        Ok(HeightMap {
            version,
            min,
            max,
            default,
            unknown,
            side,
            heights,
            layer,
            trailing,
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
        w.bytes(&self.trailing);
        w.0
    }
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
    /// Cached AI map analysis and what follows it; see the module docs.
    pub ai_cache: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    /// Index into [`Terrain::names`].
    pub terrain: u16,
    /// Water body id (0 = dry land); matches the height map's vertex layer.
    pub water_body: u8,
    /// Bits 0-3 are stored; the game recomputes bits 4-6 from the terrain's group.
    pub flags: u8,
    pub unknown: u16,
    /// Usually 0.
    pub unknown2: u8,
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
                        unknown: u16_at(2),
                        water_body: t[4],
                        unknown2: t[5],
                        flags: t[6],
                    }
                } else {
                    // Older tiles hold the same fields in a different order.
                    Tile {
                        terrain: t[0] as u16,
                        flags: t[1],
                        unknown: u16_at(2),
                        water_body: t.get(4).copied().unwrap_or(0),
                        unknown2: t.get(5).copied().unwrap_or(0),
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
        let ai_cache = cur.rest().to_vec();
        Ok(Terrain {
            version,
            unknown,
            biome,
            names,
            side,
            tiles,
            ai_cache,
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
                w.u16(t.unknown);
                w.bytes(&[t.water_body, t.unknown2, t.flags, 0]);
                continue;
            }
            let terrain = u8::try_from(t.terrain).map_err(|_| {
                Error::Unwritable(format!("terrain {} in a v{} tile", t.terrain, self.version))
            })?;
            let mut tile = [terrain, t.flags, 0, 0, t.water_body, t.unknown2];
            tile[2..4].copy_from_slice(&t.unknown.to_le_bytes());
            w.bytes(&tile[..size]);
        }
        w.bytes(&self.ai_cache);
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
    pub unknown: [u32; 2],
    /// Unit vector the light travels along, in map coordinates (z up).
    pub direction: [f32; 3],
    pub diffuse: [f32; 3],
    pub ambient: [f32; 3],
    pub specular: [f32; 3],
    pub rest: Vec<u8>,
}

impl Sun {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        Ok(Sun {
            version: cur.u32()?,
            unknown: [cur.u32()?, cur.u32()?],
            direction: cur.f32s()?,
            diffuse: cur.f32s()?,
            ambient: cur.f32s()?,
            specular: cur.f32s()?,
            rest: cur.rest().to_vec(),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(self.version);
        w.u32(self.unknown[0]);
        w.u32(self.unknown[1]);
        for v in [self.direction, self.diffuse, self.ambient, self.specular] {
            w.f32s(&v);
        }
        w.bytes(&self.rest);
        w.0
    }
}

#[derive(Debug, Clone)]
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
    pub unknown: u32,
    pub detail: Detail,
}

/// The [`Thing::class`] of actors: units, buildings, herds, roads and other
/// things made of components.
pub const ACTOR: u32 = 3;
/// The [`Thing::class`] of areas.
pub const AREA: u32 = 6;
/// The [`Thing::class`] of cameras.
pub const CAMERA: u32 = 7;

/// The class-specific rest of a [`Thing`]'s record.
#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    Actor(Actor),
    Area(Area),
    Camera(Camera),
    /// Not decoded.
    Raw(Vec<u8>),
}

/// A thing made of components, such as a unit or building.
#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    pub version: u32,
    /// The id of the kingdom that owns it; 0 for none.
    pub kingdom: u32,
    pub flags: u64,
    /// The components its template gives it, a bit each as in [`COMPONENTS`].
    pub template_components: u32,
    /// The components the record holds state for: the template's, plus any
    /// the game adds, such as a mind.
    pub components: u32,
    /// Each component's state, in bit order but for construction's first and
    /// mind's last, then the actor's own; not decoded.
    pub state: Vec<u8>,
}

/// Actor component names by their bit in [`Actor::components`], as the
/// templates' `[...Component]` sections name them less the suffix.
pub const COMPONENTS: [&str; 31] = [
    "Body",
    "Projectile",
    "Character",
    "Mover",
    "Mind",
    "Performer",
    "Properties",
    "Organization",
    "Element",
    "Economy",
    "Denizen",
    "Supply",
    "Wall",
    "Structure",
    "Settlement",
    "Construction",
    "Recruit",
    "Experience",
    "Builder",
    "MobileDeployment",
    "Defender",
    "Research",
    "Detection",
    "Aircraft",
    "Transport",
    "Lair",
    "Flare",
    "MetaGame",
    "Portal",
    "City",
    "HQ",
];

impl Actor {
    /// The first version that writes the flags in 64 bits; the layout of
    /// earlier ones is unconfirmed.
    const MIN_VERSION: u32 = 9;
    /// The newest version the game reads.
    const MAX_VERSION: u32 = 14;

    /// The names of the components the record holds.
    pub fn component_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        COMPONENTS
            .iter()
            .enumerate()
            .filter(|&(bit, _)| self.components >> bit & 1 != 0)
            .map(|(_, &name)| name)
    }

    pub fn has(&self, component: &str) -> bool {
        COMPONENTS
            .iter()
            .position(|&c| c == component)
            .is_some_and(|bit| self.components >> bit & 1 != 0)
    }
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
    pub unknown: f32,
}

/// The version DYNAMIC_THINGS records are written in.
const THINGS_VERSION: u32 = 11;

fn parse_things(index: &[u8], data: &[u8]) -> Result<Vec<Thing>> {
    let mut idx = Cursor::new(index);
    expect(&mut idx, "DYNAMIC_THINGS_INDEX header", 0)?;
    let count = idx.u32()?;
    let mut cur = Cursor::new(data);
    expect(&mut cur, "DYNAMIC_THINGS header", 0)?;
    expect(&mut cur, "DYNAMIC_THINGS version", THINGS_VERSION)?;
    let mut things = Vec::with_capacity(count.min(1 << 20) as usize);
    for i in 0..count {
        let size = idx.u32()? as usize;
        let thing = Thing::parse(cur.bytes(size)?).map_err(|e| match e {
            Error::Corrupt(m) => Error::Corrupt(format!("thing {i}: {m}")),
            e => e,
        })?;
        things.push(thing);
    }
    idx.finish("DYNAMIC_THINGS_INDEX")?;
    cur.finish("DYNAMIC_THINGS")?;
    Ok(things)
}

/// The DYNAMIC_THINGS_INDEX and DYNAMIC_THINGS sections holding `things`.
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

/// The parts of a herd or regiment, as its record stores them.
#[derive(Debug, Clone)]
pub struct Organization {
    /// The `OrganizationLayout` the members are arranged by.
    pub layout: String,
    /// Each layout slot's element template; empty for an empty slot.
    pub slots: Vec<String>,
    /// The members where they stand, in layout position order.
    pub members: Vec<Thing>,
}

impl Thing {
    fn parse(record: &[u8]) -> Result<Self> {
        let mut rec = Cursor::new(record);
        let mut thing = Thing::parse_shared(&mut rec)?;
        thing.detail = Detail::parse(thing.class, rec.rest())?;
        Ok(thing)
    }

    /// The fields every class has, leaving the detail empty.
    fn parse_shared(rec: &mut Cursor) -> Result<Self> {
        let template = rec.string()?;
        let class = rec.u32()?;
        let id = rec.u32()?;
        expect(rec, "thing version", 2)?;
        let name = rec.string()?;
        expect(rec, "field after the name", 0)?;
        let position = rec.f32s()?;
        let facing = rec.f32s()?;
        let flags = rec.u32()?;
        expect(rec, "placement version", 5)?;
        let scale = rec.f32()?;
        let variation = rec.i32()?;
        expect(rec, "field after the variation", 0)?;
        let unknown = rec.u32()?;
        match rec.u16()? {
            0 => {}
            v => return Err(Error::Corrupt(format!("placement ends in {v}, expected 0"))),
        }
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
            unknown,
            detail: Detail::Raw(Vec::new()),
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
        w.u32(5);
        w.f32(self.scale);
        w.i32(self.variation);
        w.u32(0);
        w.u32(self.unknown);
        w.u16(0);
        self.detail.write(w)
    }

    /// The organization this thing is, if its record holds one.
    pub fn organization(&self) -> Option<Organization> {
        let Detail::Actor(actor) = &self.detail else {
            return None;
        };
        if !actor.has("Organization") {
            return None;
        }
        let mut cur = Cursor::new(actor.state.get(32..)?);
        let layout = cur.string().ok().filter(|s| is_id(s))?;
        let n = cur.u32().ok().filter(|&n| n <= 64)?;
        let slots = (0..n)
            .map(|_| cur.string().ok())
            .collect::<Option<Vec<_>>>()?;
        let positions = cur.u32().ok().filter(|&n| n <= 256)?;
        // Members' records aren't decoded to the end, so each is found by a
        // later position index than the one before and a plausible record.
        // Empty positions are left out.
        let body = cur.rest();
        let member_at = |at: usize, next: u32| {
            let mut c = Cursor::new(&body[at..]);
            let index = c.u32().ok().filter(|i| (next..positions).contains(i))?;
            let thing = Thing::parse_shared(&mut c).ok()?;
            let [x, y, _] = thing.facing;
            let unit = (x * x + y * y - 1.0).abs() < 0.01;
            (is_id(&thing.template) && unit).then(|| (index, thing, at + c.pos()))
        };
        let mut members: Vec<Thing> = Vec::new();
        let (mut from, mut next) = (0, 0);
        while let Some((at, (index, thing, end))) =
            (from..body.len()).find_map(|at| Some((at, member_at(at, next)?)))
        {
            if let Some(last) = members.last_mut() {
                last.detail = Detail::Raw(body[from..at].to_vec());
            }
            members.push(thing);
            (from, next) = (end, index + 1);
        }
        if let Some(last) = members.last_mut() {
            last.detail = Detail::Raw(body[from..].to_vec());
        }
        Some(Organization {
            layout,
            slots,
            members,
        })
    }
}

impl Detail {
    fn parse(class: u32, data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let detail = match class {
            ACTOR => {
                let version = cur.u32()?;
                if !(Actor::MIN_VERSION..=Actor::MAX_VERSION).contains(&version) {
                    return Err(Error::Corrupt(format!("actor version {version}")));
                }
                Detail::Actor(Actor {
                    version,
                    kingdom: cur.u32()?,
                    flags: cur.u64()?,
                    template_components: cur.u32()?,
                    components: cur.u32()?,
                    state: cur.rest().to_vec(),
                })
            }
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
                unknown: cur.f32()?,
            }),
            _ => Detail::Raw(cur.rest().to_vec()),
        };
        cur.finish("thing record")?;
        Ok(detail)
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        match self {
            Detail::Actor(a) => {
                w.u32(a.version);
                w.u32(a.kingdom);
                w.u64(a.flags);
                w.u32(a.template_components);
                w.u32(a.components);
                w.bytes(&a.state);
            }
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
                w.f32(c.unknown);
            }
            Detail::Raw(raw) => w.bytes(raw),
        }
        Ok(())
    }
}

/// Whether `s` looks like a template id.
fn is_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{build_container, string};
    use std::io;

    fn height_section(side: u32, water_at: usize) -> Vec<u8> {
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

    fn thing_record(class: u32, template: &str, id: u32, pos: [f32; 3], detail: &[u8]) -> Vec<u8> {
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

    fn things_sections(records: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>) {
        let mut data = [0u32, 11].map(u32::to_le_bytes).concat();
        let mut index = [0u32, records.len() as u32].map(u32::to_le_bytes).concat();
        for r in records {
            data.extend(r);
            index.extend((r.len() as u32).to_le_bytes());
        }
        (index, data)
    }

    fn terrain_section(version: u32, tiles: &[&[u8]]) -> Vec<u8> {
        let mut b = version.to_le_bytes().to_vec();
        b.extend(1u32.to_le_bytes());
        b.extend(string("desert"));
        b.extend(2u32.to_le_bytes());
        b.extend(string("sand"));
        b.extend(string("ocean"));
        b.extend((tiles.len() as u32).to_le_bytes());
        b.extend((tiles[0].len() as u32).to_le_bytes());
        tiles.iter().for_each(|t| b.extend(*t));
        b.extend([0xAA, 0xBB]);
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
                unknown: 0,
                unknown2: 0,
            }
        );
        assert_eq!(v7.name_of(v7.at(0, 0)), "ocean");
        assert_eq!(v7.ai_cache, [0xAA, 0xBB]);

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

    #[test]
    fn parses_sun() {
        let mut b = [2u32, 1, u32::MAX].map(u32::to_le_bytes).concat();
        for v in [
            0.6, 0.0, -0.8, 1.0, 1.0, 1.0, 0.8, 0.8, 0.8, 0.0, 0.0, 0.0f32,
        ] {
            b.extend(v.to_le_bytes());
        }
        b.extend([0xAA, 0xBB]);
        let sun = Sun::parse(&b).unwrap();
        assert_eq!((sun.version, sun.direction), (2, [0.6, 0.0, -0.8]));
        assert_eq!((sun.diffuse, sun.ambient), ([1.0; 3], [0.8; 3]));
        assert_eq!(sun.rest, [0xAA, 0xBB]);
        assert!(Sun::parse(&b[..40]).is_err(), "truncated");
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
            ("HEIGHT_MAP", &height_section(3, 4), true),
            ("DYNAMIC_THINGS", &things, true),
            ("DYNAMIC_THINGS_INDEX", &index, true),
        ]);
        let mut map = Map::new(io::Cursor::new(bytes)).unwrap();
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
        assert_eq!((actor.kingdom, actor.flags), (2, 0x0800_0000));
        assert_eq!(
            actor.component_names().collect::<Vec<_>>(),
            ["Mind", "Economy"]
        );
        assert_eq!(actor.template_components, 0x200);
        assert_eq!(actor.state, [1, 2, 3]);
        assert_eq!(t[1].detail, Detail::Raw(vec![]));
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
        let things = parse_things(&index, &data).unwrap();
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
                unknown: 1.3
            })
        );
        assert_eq!(write_things(&things).unwrap(), (index, data));
    }

    #[test]
    fn rejects_unknown_thing_versions() {
        let mut record = thing_record(2, "tree", 1, [0.0; 3], &[]);
        let at = string("tree").len() + 8;
        record[at] = 3;
        let (index, data) = things_sections(&[record]);
        assert!(parse_things(&index, &data).is_err());

        let mut actor = actor_detail(0, &[]);
        actor[0] = 15;
        let (index, data) = things_sections(&[thing_record(ACTOR, "tank", 1, [0.0; 3], &actor)]);
        assert!(parse_things(&index, &data).is_err(), "newer than the game");
    }

    #[test]
    fn writes_sections_back() {
        let hm = height_section(3, 4);
        assert_eq!(HeightMap::parse(&hm).unwrap().to_bytes(), hm);
        for (version, tile) in [
            (7, &[1, 0, 2, 0, 3, 9, 0x21, 0][..]),
            (6, &[1, 0x21, 2, 0, 3, 9]),
        ] {
            let terrain = terrain_section(version, &[tile; 4]);
            let parsed = Terrain::parse(&terrain).unwrap();
            assert_eq!(parsed.at(0, 0).unknown2, 9);
            assert_eq!(parsed.to_bytes().unwrap(), terrain);
        }
        let mut sun = [2u32, 1, u32::MAX].map(u32::to_le_bytes).concat();
        sun.extend([0.5f32; 12].map(f32::to_le_bytes).concat());
        sun.extend([0xAA]);
        assert_eq!(Sun::parse(&sun).unwrap().to_bytes(), sun);
    }

    #[test]
    fn parses_organization_members() {
        let mut extra = vec![0u8; 32];
        extra.extend(string("herd_ambient_large"));
        extra.extend(3u32.to_le_bytes());
        for slot in ["sheep_ewe", "", "sheep_ram"] {
            extra.extend(string(slot));
        }
        extra.extend(7u32.to_le_bytes());
        // Positions 1 and 2 are empty; each member's rest is undecoded.
        for (index, template, id) in [(0u32, "sheep_ewe", 11), (3, "sheep_ram", 12)] {
            extra.extend(index.to_le_bytes());
            extra.extend(thing_record(3, template, id, [1.0, 2.0, 3.0], &[0xAA; 20]));
        }
        let parse = |components, state: &[u8]| {
            let detail = actor_detail(components, state);
            Thing::parse(&thing_record(ACTOR, "herd_sheep", 10, [0.0; 3], &detail)).unwrap()
        };
        let org = parse(0x80, &extra).organization().unwrap();
        assert_eq!(org.layout, "herd_ambient_large");
        assert_eq!(org.slots, ["sheep_ewe", "", "sheep_ram"]);
        let members: Vec<_> = org
            .members
            .iter()
            .map(|m| (m.template.as_str(), m.id))
            .collect();
        assert_eq!(members, [("sheep_ewe", 11), ("sheep_ram", 12)]);
        assert_eq!(org.members[0].position, [1.0, 2.0, 3.0]);
        assert_eq!(org.members[0].detail, Detail::Raw(vec![0xAA; 20]));
        assert_eq!(org.members[1].detail, Detail::Raw(vec![0xAA; 20]));

        assert!(
            parse(0, &extra).organization().is_none(),
            "not an organization"
        );
    }
}
