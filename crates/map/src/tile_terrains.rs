//! Which terrain lies under each tile, for what it does to those on it.

use std::collections::HashMap;

use aaa_data::{Id, TerrainKind, Variation};
use tgck::map::Terrain;

use crate::pathing::tile_at;

/// A terrain, as far as it bears on those standing on it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TileTerrain {
    /// The properties it gives, in the order it lists them.
    pub properties: Vec<Id>,
    pub mountain: bool,
}

/// Which of a [`TileTerrains`]' terrains a tile has, as
/// [`TileTerrains::at`] finds it. The default is no terrain, which a tile
/// off the map has, or one whose terrain the map's list doesn't name. A
/// terrain the list names but the data doesn't define has an index of its
/// own, which gives nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerrainIndex(u16);

/// The terrain under each tile.
#[derive(Debug, Clone)]
pub struct TileTerrains {
    /// Tiles per side.
    side: usize,
    /// Row-major, per tile.
    tiles: Vec<TerrainIndex>,
    /// By index; the first is no terrain, which gives nothing.
    terrains: Vec<TileTerrain>,
}

impl Default for TileTerrains {
    fn default() -> Self {
        TileTerrains::uniform(0, TileTerrain::default())
    }
}

impl TileTerrains {
    pub fn new(terrain: &Terrain, variations: &HashMap<Id, Variation>) -> Self {
        let named = terrain.names.iter().map(|n| {
            let v = variations.get(&Id::from(n.as_str()));
            v.map_or_else(TileTerrain::default, |v| TileTerrain {
                properties: v.properties.clone(),
                mountain: matches!(v.kind, TerrainKind::Mountain { .. }),
            })
        });
        let terrains: Vec<TileTerrain> = std::iter::once(TileTerrain::default())
            .chain(named)
            .collect();
        let index = |t: &tgck::map::Tile| {
            let i = usize::from(t.terrain) + 1;
            let i = u16::try_from(i).ok().filter(|_| i < terrains.len());
            i.map_or_else(TerrainIndex::default, TerrainIndex)
        };
        TileTerrains {
            side: terrain.side,
            tiles: terrain.tiles.iter().map(index).collect(),
            terrains,
        }
    }

    /// A map of one terrain, for tests and tools.
    pub fn uniform(side: usize, terrain: TileTerrain) -> Self {
        TileTerrains {
            side,
            tiles: vec![TerrainIndex(1); side * side],
            terrains: vec![TileTerrain::default(), terrain],
        }
    }

    /// Makes one tile's terrain `terrain`.
    pub fn set(&mut self, (x, y): (i32, i32), terrain: TileTerrain) {
        let found = self.terrains.iter().position(|t| *t == terrain);
        let i = found.unwrap_or_else(|| {
            self.terrains.push(terrain);
            self.terrains.len() - 1
        });
        let i = u16::try_from(i).expect("no more terrains than a u16 counts");
        if let Some(t) = self.index(x, y) {
            self.tiles[t] = TerrainIndex(i);
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let side = self.side as i32;
        ((0..side).contains(&x) && (0..side).contains(&y)).then(|| (y * side + x) as usize)
    }

    /// The terrain of the tile a world position lies in.
    pub fn at(&self, position: [f32; 2]) -> TerrainIndex {
        let (x, y) = tile_at(position);
        self.index(x, y)
            .map_or_else(TerrainIndex::default, |i| self.tiles[i])
    }

    /// The terrain `i` names; no terrain for one from other tiles' terrains
    /// that these don't have.
    pub fn get(&self, TerrainIndex(i): TerrainIndex) -> &TileTerrain {
        let none = &self.terrains[0];
        self.terrains.get(usize::from(i)).unwrap_or(none)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pathing::tile_centre;

    #[test]
    fn tiles_have_their_own_terrain_and_none_off_the_map() {
        let forest = TileTerrain {
            properties: vec![Id::from("forest")],
            mountain: false,
        };
        let mut terrains = TileTerrains::uniform(4, TileTerrain::default());
        terrains.set((2, 1), forest.clone());
        assert_eq!(*terrains.get(terrains.at(tile_centre(2, 1))), forest);
        assert_eq!(
            *terrains.get(terrains.at(tile_centre(1, 1))),
            TileTerrain::default()
        );
        assert_eq!(terrains.at(tile_centre(4, 1)), TerrainIndex::default());
        assert_eq!(terrains.at(tile_centre(-1, 0)), TerrainIndex::default());
        // One of another map's terrains these don't have is none: forest is
        // a third terrain there, where these have two.
        let other = TileTerrains::uniform(4, forest.clone());
        let mut more = TileTerrains::uniform(4, TileTerrain::default());
        more.set((1, 0), forest);
        assert_eq!(
            *other.get(more.at(tile_centre(1, 0))),
            TileTerrain::default()
        );
    }
}
