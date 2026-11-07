//! What TERRAIN holds after its tiles: the AI's path-finding map and the
//! shore mesh, laid out as `ai_map` and `shore_mesh` in [`rmp.ksy`].
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use super::expect;
use crate::{Cursor, Result, Writer};

/// The map cut into rectangular blocks of tiles, grouped into regions for
/// the AI's path finding. The game can rebuild it.
#[derive(Debug, Clone, PartialEq)]
pub struct AiMap {
    /// None when the map holds no analysis.
    pub data: Option<AiData>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiData {
    pub regions: Vec<AiRegion>,
    /// They cover every tile once.
    pub blocks: Vec<AiBlock>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiRegion {
    /// In world units.
    pub center: [f32; 2],
    /// The regions whose blocks touch this one's, and the distance between
    /// the two centers.
    pub neighbors: Vec<(u32, f32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiBlock {
    /// The index of the region it belongs to.
    pub region: u32,
    /// The connected stretch of land or water it's in; every block of a
    /// region shares it.
    pub area: u32,
    /// Top, right, bottom and left.
    pub sides: [AiBlockSide; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiBlockSide {
    /// The block's first or last row or column on this side, inclusive.
    pub bound: u16,
    /// The blocks across this side.
    pub links: Vec<AiBlockLink>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiBlockLink {
    /// The span of this block's edge tiles that the other block touches,
    /// inclusive: min x, max x, min y, max y.
    pub span: [u16; 4],
    /// The index of the other block.
    pub block: u32,
}

/// Triangles over the tiles along the water's edge, where the ground
/// crosses the water level; empty in most shipped maps.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShoreMesh {
    /// In world units.
    pub points: Vec<[f32; 2]>,
    /// Indices into the points, three per triangle.
    pub corners: Vec<u32>,
    pub tiles: Vec<ShoreTile>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShoreTile {
    pub x: u32,
    pub y: u32,
    /// The tile's triangles' corners, `first..=last`.
    pub first_corner: u32,
    pub last_corner: u32,
}

impl AiMap {
    pub(super) fn parse(cur: &mut Cursor) -> Result<Self> {
        expect(cur, "AI map version", 0)?;
        if !super::header::bool(cur, "AI map flag")? {
            return Ok(AiMap { data: None });
        }
        let regions = cur.list(|cur| {
            expect(cur, "AI region version", 0)?;
            Ok(AiRegion {
                center: cur.f32s()?,
                neighbors: cur.list(|cur| Ok((cur.u32()?, cur.f32()?)))?,
            })
        })?;
        let blocks = cur.list(AiBlock::parse)?;
        Ok(AiMap {
            data: Some(AiData { regions, blocks }),
        })
    }

    pub(super) fn write(&self, w: &mut Writer) {
        w.u32(0);
        w.u8(self.data.is_some().into());
        let Some(data) = &self.data else { return };
        w.u32(data.regions.len() as u32);
        for r in &data.regions {
            w.u32(0);
            w.f32s(&r.center);
            w.u32(r.neighbors.len() as u32);
            for &(region, distance) in &r.neighbors {
                w.u32(region);
                w.f32(distance);
            }
        }
        w.u32(data.blocks.len() as u32);
        for b in &data.blocks {
            b.write(w);
        }
    }
}

impl AiBlock {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        expect(cur, "AI block version", 0)?;
        let region = cur.u32()?;
        let area = cur.u32()?;
        let mut side = || -> Result<AiBlockSide> {
            Ok(AiBlockSide {
                bound: cur.u16()?,
                links: cur.list(|cur| {
                    expect(cur, "AI block link version", 0)?;
                    Ok(AiBlockLink {
                        span: [cur.u16()?, cur.u16()?, cur.u16()?, cur.u16()?],
                        block: cur.u32()?,
                    })
                })?,
            })
        };
        Ok(AiBlock {
            region,
            area,
            sides: [side()?, side()?, side()?, side()?],
        })
    }

    fn write(&self, w: &mut Writer) {
        w.u32(0);
        w.u32(self.region);
        w.u32(self.area);
        for side in &self.sides {
            w.u16(side.bound);
            w.u32(side.links.len() as u32);
            for link in &side.links {
                w.u32(0);
                link.span.iter().for_each(|&v| w.u16(v));
                w.u32(link.block);
            }
        }
    }
}

impl ShoreMesh {
    pub(super) fn parse(cur: &mut Cursor) -> Result<Self> {
        expect(cur, "shore mesh version", 0)?;
        Ok(ShoreMesh {
            points: cur.list(|cur| cur.f32s())?,
            corners: cur.list(|cur| cur.u32())?,
            tiles: cur.list(|cur| {
                Ok(ShoreTile {
                    x: cur.u32()?,
                    y: cur.u32()?,
                    first_corner: cur.u32()?,
                    last_corner: cur.u32()?,
                })
            })?,
        })
    }

    pub(super) fn write(&self, w: &mut Writer) {
        w.u32(0);
        w.u32(self.points.len() as u32);
        self.points.iter().for_each(|p| w.f32s(p));
        w.u32(self.corners.len() as u32);
        self.corners.iter().for_each(|&c| w.u32(c));
        w.u32(self.tiles.len() as u32);
        for t in &self.tiles {
            [t.x, t.y, t.first_corner, t.last_corner]
                .iter()
                .for_each(|&v| w.u32(v));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: PartialEq + std::fmt::Debug>(
        value: &T,
        write: impl Fn(&T, &mut Writer),
        parse: impl Fn(&mut Cursor) -> Result<T>,
    ) {
        let mut w = Writer::default();
        write(value, &mut w);
        let mut cur = Cursor::new(&w.0);
        assert_eq!(&parse(&mut cur).unwrap(), value);
        cur.finish("test").unwrap();
    }

    #[test]
    fn round_trips_ai_map() {
        let side = |bound, links| AiBlockSide { bound, links };
        let link = |block| AiBlockLink {
            span: [0, 3, 4, 4],
            block,
        };
        let map = AiMap {
            data: Some(AiData {
                regions: vec![AiRegion {
                    center: [4.0, 6.0],
                    neighbors: vec![(1, 12.5)],
                }],
                blocks: vec![
                    AiBlock {
                        region: 0,
                        area: 1,
                        sides: [
                            side(0, vec![]),
                            side(3, vec![]),
                            side(3, vec![link(1)]),
                            side(0, vec![]),
                        ],
                    },
                    AiBlock {
                        region: 1,
                        area: 1,
                        sides: [
                            side(4, vec![link(0)]),
                            side(3, vec![]),
                            side(7, vec![]),
                            side(0, vec![]),
                        ],
                    },
                ],
            }),
        };
        round_trip(&map, AiMap::write, AiMap::parse);
        round_trip(&AiMap { data: None }, AiMap::write, AiMap::parse);

        let mut w = Writer::default();
        AiMap { data: None }.write(&mut w);
        assert_eq!(w.0, [0, 0, 0, 0, 0], "a version and a clear flag");
    }

    #[test]
    fn round_trips_shore_mesh() {
        let mesh = ShoreMesh {
            points: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]],
            corners: vec![0, 1, 2],
            tiles: vec![ShoreTile {
                x: 0,
                y: 0,
                first_corner: 0,
                last_corner: 2,
            }],
        };
        round_trip(&mesh, ShoreMesh::write, ShoreMesh::parse);
        round_trip(&ShoreMesh::default(), ShoreMesh::write, ShoreMesh::parse);
    }

    #[test]
    fn rejects_huge_counts() {
        let mut w = Writer::default();
        w.u32(0);
        w.u32(u32::MAX);
        assert!(ShoreMesh::parse(&mut Cursor::new(&w.0)).is_err());
    }
}
