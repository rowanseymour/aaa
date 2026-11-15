//! What TERRAIN holds after its tiles: the AI's path-finding map and the
//! shore mesh, laid out as `ai_map` and `shore_mesh` in [`rmp.ksy`].
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use crate::{Cursor, Result, Writer};

/// The `ai_map` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct AiMap {
    /// `None` where `has_data` is clear.
    pub data: Option<AiData>,
}

/// An `ai_map`'s regions and blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct AiData {
    pub regions: Vec<AiRegion>,
    pub blocks: Vec<AiBlock>,
}

/// An `ai_region` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct AiRegion {
    pub center: [f32; 2],
    /// Each `ai_neighbor`'s region and distance.
    pub neighbors: Vec<(u32, f32)>,
}

/// An `ai_block` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct AiBlock {
    /// Index into [`AiData::regions`].
    pub region: u32,
    pub area: u32,
    pub sides: [AiBlockSide; 4],
}

/// An `ai_block_side` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct AiBlockSide {
    pub bound: u16,
    pub links: Vec<AiBlockLink>,
}

/// An `ai_block_link` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct AiBlockLink {
    /// `min_x`, `max_x`, `min_y`, `max_y`.
    pub span: [u16; 4],
    /// Index into [`AiData::blocks`].
    pub block: u32,
}

/// The `shore_mesh` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShoreMesh {
    pub points: Vec<[f32; 2]>,
    pub corners: Vec<u32>,
    pub tiles: Vec<ShoreTile>,
}

/// A `shore_tile` of `rmp.ksy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShoreTile {
    pub x: u32,
    pub y: u32,
    pub first_corner: u32,
    pub last_corner: u32,
}

impl AiMap {
    pub(super) fn parse(cur: &mut Cursor) -> Result<Self> {
        cur.expect("AI map version", 0)?;
        if !cur.bool("AI map flag")? {
            return Ok(AiMap { data: None });
        }
        let regions = cur.list(|cur| {
            cur.expect("AI region version", 0)?;
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

    pub(super) fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(0);
        w.u8(self.data.is_some().into());
        let Some(data) = &self.data else {
            return Ok(());
        };
        w.list("regions", &data.regions, |w, r| {
            w.u32(0);
            w.f32s(&r.center);
            w.list("neighbors", &r.neighbors, |w, &(region, distance)| {
                w.u32(region);
                w.f32(distance);
                Ok(())
            })
        })?;
        w.list("blocks", &data.blocks, |w, b| b.write(w))
    }
}

impl AiBlock {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        cur.expect("AI block version", 0)?;
        let region = cur.u32()?;
        let area = cur.u32()?;
        let mut side = || -> Result<AiBlockSide> {
            Ok(AiBlockSide {
                bound: cur.u16()?,
                links: cur.list(|cur| {
                    cur.expect("AI block link version", 0)?;
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

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(0);
        w.u32(self.region);
        w.u32(self.area);
        self.sides.iter().try_for_each(|side| {
            w.u16(side.bound);
            w.list("links", &side.links, |w, link| {
                w.u32(0);
                link.span.iter().for_each(|&v| w.u16(v));
                w.u32(link.block);
                Ok(())
            })
        })
    }
}

impl ShoreMesh {
    pub(super) fn parse(cur: &mut Cursor) -> Result<Self> {
        cur.expect("shore mesh version", 0)?;
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

    pub(super) fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(0);
        w.list("points", &self.points, |w, p| {
            w.f32s(p);
            Ok(())
        })?;
        w.u32s("corners", &self.corners)?;
        w.list("tiles", &self.tiles, |w, t| {
            [t.x, t.y, t.first_corner, t.last_corner]
                .iter()
                .for_each(|&v| w.u32(v));
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: PartialEq + std::fmt::Debug>(
        value: &T,
        write: impl Fn(&T, &mut Writer) -> Result<()>,
        parse: impl Fn(&mut Cursor) -> Result<T>,
    ) {
        let mut w = Writer::default();
        write(value, &mut w).unwrap();
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
        AiMap { data: None }.write(&mut w).unwrap();
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
