//! Which terrain lies under each tile, for what it does to those on it.

use std::collections::HashMap;

use aaa_data::{Id, TerrainKind, Variation};
use tgck::map::Terrain;

use crate::pathing::tile_at;

/// A terrain, as far as it bears on those standing on it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GroundKind {
    /// The properties it gives, in the order it lists them.
    pub properties: Vec<Id>,
    pub mountain: bool,
}

/// The terrain under each tile.
#[derive(Debug, Clone)]
pub struct Grounds {
    /// Tiles per side.
    side: usize,
    /// Row-major index into `kinds` per tile.
    tiles: Vec<u16>,
    /// The first is what a tile off the map or of no known terrain has:
    /// nothing.
    kinds: Vec<GroundKind>,
}

impl Default for Grounds {
    fn default() -> Self {
        Grounds::uniform(0, GroundKind::default())
    }
}

impl Grounds {
    pub fn new(terrain: &Terrain, variations: &HashMap<Id, Variation>) -> Self {
        let named = terrain.names.iter().map(|n| {
            let v = variations.get(&Id::from(n.as_str()));
            v.map_or_else(GroundKind::default, |v| GroundKind {
                properties: v.properties.clone(),
                mountain: matches!(v.kind, TerrainKind::Mountain { .. }),
            })
        });
        let kinds: Vec<GroundKind> = std::iter::once(GroundKind::default())
            .chain(named)
            .collect();
        let index = |t: &tgck::map::Tile| match usize::from(t.terrain) + 1 {
            i if i < kinds.len() => i as u16,
            _ => 0,
        };
        Grounds {
            side: terrain.side,
            tiles: terrain.tiles.iter().map(index).collect(),
            kinds,
        }
    }

    /// A map of one terrain, for tests and tools.
    pub fn uniform(side: usize, kind: GroundKind) -> Self {
        Grounds {
            side,
            tiles: vec![1; side * side],
            kinds: vec![GroundKind::default(), kind],
        }
    }

    /// Makes one tile's terrain `kind`.
    pub fn set(&mut self, (x, y): (i32, i32), kind: GroundKind) {
        let found = self.kinds.iter().position(|k| *k == kind);
        let i = found.unwrap_or_else(|| {
            self.kinds.push(kind);
            self.kinds.len() - 1
        });
        if let Some(t) = self.index(x, y) {
            self.tiles[t] = i as u16;
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let side = self.side as i32;
        ((0..side).contains(&x) && (0..side).contains(&y)).then(|| (y * side + x) as usize)
    }

    /// The terrain of the tile a world position lies in, as taken by
    /// [`Grounds::kind`].
    pub fn at(&self, position: [f32; 2]) -> u16 {
        let (x, y) = tile_at(position);
        self.index(x, y).map_or(0, |i| self.tiles[i])
    }

    pub fn kind(&self, i: u16) -> &GroundKind {
        &self.kinds[usize::from(i)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pathing::tile_centre;

    #[test]
    fn tiles_have_their_own_terrain_and_none_off_the_map() {
        let forest = GroundKind {
            properties: vec![Id::from("forest")],
            mountain: false,
        };
        let mut grounds = Grounds::uniform(4, GroundKind::default());
        grounds.set((2, 1), forest.clone());
        assert_eq!(*grounds.kind(grounds.at(tile_centre(2, 1))), forest);
        assert_eq!(
            *grounds.kind(grounds.at(tile_centre(1, 1))),
            GroundKind::default()
        );
        assert_eq!(grounds.at(tile_centre(4, 1)), 0);
        assert_eq!(grounds.at(tile_centre(-1, 0)), 0);
    }
}
