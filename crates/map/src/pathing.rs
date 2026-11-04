//! Where movers can go, a tile at a time: the terrain decides whether land
//! units or ships can cross a tile, and objects that block keep both off
//! the tiles they cover.

use std::collections::HashMap;

use aaa_data::{Id, Shape, Variation};
use tgck::map::Terrain;

pub use tgck::map::CELL_SIZE;

/// What a mover travels on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Land,
    Naval,
}

const LAND: u8 = 1;
const NAVAL: u8 = 2;
const BLOCKED: u8 = 4;

/// An object that keeps movers off the ground under it.
pub struct Obstacle {
    /// In world units.
    pub position: [f32; 2],
    /// The direction its model's -y axis points, a unit vector.
    pub facing: [f32; 2],
    pub shape: Shape,
}

#[derive(Clone)]
pub struct Pathing {
    /// Tiles per side.
    pub side: usize,
    /// Row-major bits per tile.
    tiles: Vec<u8>,
}

impl Pathing {
    /// Tiles whose terrain isn't among `variations` count as land.
    pub fn new(
        terrain: &Terrain,
        variations: &HashMap<Id, Variation>,
        obstacles: impl IntoIterator<Item = Obstacle>,
    ) -> Self {
        let by_index: Vec<u8> = terrain
            .names
            .iter()
            .map(|n| match variations.get(&Id::from(n.as_str())) {
                Some(v) => (u8::from(v.land) * LAND) | (u8::from(v.naval) * NAVAL),
                None => LAND,
            })
            .collect();
        let tiles = terrain
            .tiles
            .iter()
            .map(|t| by_index.get(t.terrain as usize).copied().unwrap_or(LAND))
            .collect();
        let mut pathing = Pathing {
            side: terrain.side,
            tiles,
        };
        for o in obstacles {
            pathing.block(&o);
        }
        pathing
    }

    /// A map of one kind of tile, for tests and tools.
    pub fn uniform(side: usize, domain: Domain) -> Self {
        let bits = match domain {
            Domain::Land => LAND,
            Domain::Naval => NAVAL,
        };
        Pathing {
            side,
            tiles: vec![bits; side * side],
        }
    }

    /// Whether a mover on `domain` can enter the tile; off the map it can't.
    pub fn passable(&self, x: i32, y: i32, domain: Domain) -> bool {
        let Some(&bits) = self.tile(x, y) else {
            return false;
        };
        let wanted = match domain {
            Domain::Land => LAND,
            Domain::Naval => NAVAL,
        };
        bits & BLOCKED == 0 && bits & wanted != 0
    }

    /// Keeps movers off one tile.
    pub fn set_blocked(&mut self, x: i32, y: i32) {
        if let Some(i) = self.index(x, y) {
            self.tiles[i] |= BLOCKED;
        }
    }

    fn tile(&self, x: i32, y: i32) -> Option<&u8> {
        self.tiles.get(self.index(x, y)?)
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let side = self.side as i32;
        ((0..side).contains(&x) && (0..side).contains(&y)).then(|| (y * side + x) as usize)
    }

    /// Blocks the tiles whose centres lie within half a tile's width of the
    /// obstacle's shape, as the game does: so shapes reaching no further
    /// than that from their middle block nothing.
    fn block(&mut self, o: &Obstacle) {
        const MARGIN: f32 = CELL_SIZE / 4.0;
        let [px, py] = o.position;
        let reach = match o.shape {
            Shape::Point => 0.0,
            Shape::Circle { radius } => radius,
            Shape::Rect { width, height } => width.hypot(height) / 2.0,
        };
        if reach <= MARGIN {
            return;
        }
        // The shape's y axis runs along the facing.
        let [fx, fy] = o.facing;
        let covers = |wx: f32, wy: f32| {
            let (dx, dy) = (wx - px, wy - py);
            match o.shape {
                Shape::Point => false,
                Shape::Circle { radius } => dx.hypot(dy) <= radius + MARGIN,
                Shape::Rect { width, height } => {
                    let across = (dx * fy - dy * fx).abs() - width / 2.0;
                    let along = (dx * fx + dy * fy).abs() - height / 2.0;
                    across.max(0.0).hypot(along.max(0.0)) < MARGIN
                }
            }
        };
        let tile = |w: f32| (w / CELL_SIZE).floor() as i32;
        let extent = reach + MARGIN;
        for y in tile(py - extent)..=tile(py + extent) {
            for x in tile(px - extent)..=tile(px + extent) {
                let centre = |t: i32| (t as f32 + 0.5) * CELL_SIZE;
                if covers(centre(x), centre(y)) {
                    self.set_blocked(x, y);
                }
            }
        }
    }
}

/// The tile a world position lies in.
pub fn tile_at([x, y]: [f32; 2]) -> (i32, i32) {
    (
        (x / CELL_SIZE).floor() as i32,
        (y / CELL_SIZE).floor() as i32,
    )
}

/// The centre of a tile in world units.
pub fn tile_centre(x: i32, y: i32) -> [f32; 2] {
    [(x as f32 + 0.5) * CELL_SIZE, (y as f32 + 0.5) * CELL_SIZE]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocked(shape: Shape, facing: [f32; 2]) -> Vec<(i32, i32)> {
        let mut p = Pathing::uniform(10, Domain::Land);
        p.block(&Obstacle {
            position: [11.0, 11.0],
            facing,
            shape,
        });
        let mut out = Vec::new();
        for y in 0..10 {
            for x in 0..10 {
                if !p.passable(x, y, Domain::Land) {
                    out.push((x, y));
                }
            }
        }
        out
    }

    #[test]
    fn small_obstacles_block_nothing() {
        assert_eq!(blocked(Shape::Circle { radius: 0.5 }, [0.0, -1.0]), []);
        assert_eq!(blocked(Shape::Point, [0.0, -1.0]), []);
        assert_eq!(
            blocked(Shape::Circle { radius: 0.6 }, [0.0, -1.0]),
            [(5, 5)]
        );
    }

    #[test]
    fn shapes_reach_half_a_tile_beyond_their_edges() {
        // 1.5 short of the next tiles' centres, and then 1.6.
        assert_eq!(
            blocked(Shape::Circle { radius: 1.4 }, [0.0, -1.0]),
            [(5, 5)]
        );
        assert_eq!(
            blocked(Shape::Circle { radius: 1.6 }, [0.0, -1.0]),
            [(5, 4), (4, 5), (5, 5), (6, 5), (5, 6)]
        );
        let rect = Shape::Rect {
            width: 7.2,
            height: 1.0,
        };
        assert_eq!(
            blocked(rect, [0.0, -1.0]),
            [(3, 5), (4, 5), (5, 5), (6, 5), (7, 5)]
        );
    }

    #[test]
    fn rectangles_turn_with_their_facing() {
        // Three tiles across and one deep.
        let rect = Shape::Rect {
            width: 6.0,
            height: 1.0,
        };
        assert_eq!(blocked(rect, [0.0, -1.0]), [(4, 5), (5, 5), (6, 5)]);
        assert_eq!(blocked(rect, [1.0, 0.0]), [(5, 4), (5, 5), (5, 6)]);
    }

    #[test]
    fn domains() {
        let mut p = Pathing::uniform(2, Domain::Naval);
        assert!(p.passable(0, 0, Domain::Naval));
        assert!(!p.passable(0, 0, Domain::Land));
        assert!(!p.passable(2, 0, Domain::Naval));
        p.set_blocked(0, 0);
        assert!(!p.passable(0, 0, Domain::Naval));
    }
}
