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
//! DYNAMIC_THINGS           u32 unknown (0), u32 unknown (11), then per record:
//!     string template, u32 unknown, u32 id, 10 bytes unknown,
//!     f32 position[3], f32 facing[3] (unit vector), rest of record unknown
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
//!       v7: u16 terrain, u16 unknown, u8 water_body, u8 unused, u8 flags, u8 unused
//!       v6: u8 terrain, u8 flags, u16 unknown, u8 water_body, u8 unused
//!     remainder: cached AI map analysis, followed by one more object for v6+.
//!       The game can rebuild this data, so it is kept undecoded.
//! ```
//!
//! World coordinates span 0..(side - 1) * 2, i.e. two world units per height cell.

use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use crate::container::{Container, Section};
use crate::{Cursor, Error, Result};

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
        if container.section("MAP_HEADER").is_none() {
            return Err(Error::MissingSection("MAP_HEADER"));
        }
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
        let expected = match version {
            7.. => 8,
            5..=6 => 6,
            4 => 4,
            _ => 2,
        };
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
                        flags: t[6],
                    }
                } else {
                    // Older tiles hold the same fields in a different order.
                    Tile {
                        terrain: t[0] as u16,
                        flags: t[1],
                        unknown: u16_at(2),
                        water_body: t.get(4).copied().unwrap_or(0),
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
        let header = [cur.u32()?, cur.u32()?, cur.u32()?];
        let v3 =
            |cur: &mut Cursor| -> Result<[f32; 3]> { Ok([cur.f32()?, cur.f32()?, cur.f32()?]) };
        let direction = v3(&mut cur)?;
        let diffuse = v3(&mut cur)?;
        let ambient = v3(&mut cur)?;
        let specular = v3(&mut cur)?;
        Ok(Sun {
            version: header[0],
            unknown: [header[1], header[2]],
            direction,
            diffuse,
            ambient,
            specular,
            rest: cur.rest().to_vec(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Thing {
    /// Template name, e.g. `temperate_trees`, `start_loc`.
    pub template: String,
    pub id: u32,
    pub position: [f32; 3],
    /// Unit vector the object faces.
    pub facing: [f32; 3],
    pub unknown0: u32,
    pub unknown1: [u8; 10],
    /// Template-specific remainder of the record, not yet decoded.
    pub extra: Vec<u8>,
}

fn parse_things(index: &[u8], data: &[u8]) -> Result<Vec<Thing>> {
    let mut idx = Cursor::new(index);
    idx.u32()?;
    let count = idx.u32()?;
    let mut cur = Cursor::new(data);
    cur.bytes(8)?;
    let mut things = Vec::with_capacity(count.min(1 << 20) as usize);
    for i in 0..count {
        let size = idx.u32()? as usize;
        let mut rec = Cursor::new(cur.bytes(size)?);
        let corrupt = |e| match e {
            Error::Corrupt(m) => Error::Corrupt(format!("thing {i}: {m}")),
            e => e,
        };
        let parse = |rec: &mut Cursor| -> Result<Thing> {
            let template = rec.string()?;
            let unknown0 = rec.u32()?;
            let id = rec.u32()?;
            let unknown1 = rec.bytes(10)?.try_into().unwrap();
            let position = [rec.f32()?, rec.f32()?, rec.f32()?];
            let facing = [rec.f32()?, rec.f32()?, rec.f32()?];
            let extra = rec.rest().to_vec();
            Ok(Thing {
                template,
                id,
                position,
                facing,
                unknown0,
                unknown1,
                extra,
            })
        };
        things.push(parse(&mut rec).map_err(corrupt)?);
    }
    idx.finish("DYNAMIC_THINGS_INDEX")?;
    cur.finish("DYNAMIC_THINGS")?;
    Ok(things)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{build_container, utf16};
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

    fn thing_record(template: &str, id: u32, pos: [f32; 3], extra: &[u8]) -> Vec<u8> {
        let mut r = (template.encode_utf16().count() as u16)
            .to_le_bytes()
            .to_vec();
        r.extend(utf16(template));
        r.extend(9u32.to_le_bytes());
        r.extend(id.to_le_bytes());
        r.extend([0u8; 10]);
        for v in pos.into_iter().chain([0.0, -1.0, 0.0]) {
            r.extend(v.to_le_bytes());
        }
        r.extend(extra);
        r
    }

    fn terrain_section(version: u32, tiles: &[&[u8]]) -> Vec<u8> {
        let s = |b: &mut Vec<u8>, v: &str| {
            b.extend((v.encode_utf16().count() as u16).to_le_bytes());
            b.extend(utf16(v));
        };
        let mut b = version.to_le_bytes().to_vec();
        b.extend(1u32.to_le_bytes());
        s(&mut b, "desert");
        b.extend(2u32.to_le_bytes());
        s(&mut b, "sand");
        s(&mut b, "ocean");
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
                unknown: 0
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
        let recs = [
            thing_record("start_loc", 7, [10.0, 20.0, 2.0], &[1, 2, 3]),
            thing_record("tree", 8, [0.0; 3], &[]),
        ];
        let mut things = [0u32.to_le_bytes(), 11u32.to_le_bytes()].concat();
        let mut index = [0u32.to_le_bytes(), 2u32.to_le_bytes()].concat();
        for r in &recs {
            things.extend(r);
            index.extend((r.len() as u32).to_le_bytes());
        }
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
        assert_eq!(t[0].extra, [1, 2, 3]);
    }
}
