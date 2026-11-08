//! Where movers can go, a tile at a time: the terrain decides whether land
//! units or ships can cross a tile, and objects that block keep both off
//! the tiles they cover until they're gone.

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

/// An object that keeps movers off the ground under it.
#[derive(Debug, Clone)]
pub struct Obstacle {
    /// In world units.
    pub position: [f32; 2],
    /// The direction its model's -y axis points, a unit vector.
    pub facing: [f32; 2],
    pub shape: Shape,
}

impl Obstacle {
    /// How far a point is from the shape's edge; 0 within it.
    pub fn distance(&self, [x, y]: [f32; 2]) -> f32 {
        // The shape's y axis runs along the facing.
        let [across, along] = crate::to_local(self.position, self.facing, [x, y]);
        match self.shape {
            Shape::Point => libm::hypotf(across, along),
            Shape::Circle { radius } => (libm::hypotf(across, along) - radius).max(0.0),
            Shape::Rect { width, height } => {
                let across = across.abs() - width / 2.0;
                let along = along.abs() - height / 2.0;
                libm::hypotf(across.max(0.0), along.max(0.0))
            }
        }
    }

    /// Whether it and `other` share any ground; touching isn't enough.
    pub fn overlaps(&self, other: &Obstacle) -> bool {
        match (self.shape, other.shape) {
            (Shape::Rect { .. }, Shape::Rect { .. }) => {
                !self.separated_from(other) && !other.separated_from(self)
            }
            (Shape::Rect { .. }, _) => other.overlaps(self),
            (Shape::Circle { radius }, _) => other.distance(self.position) < radius,
            (Shape::Point, _) => other.distance(self.position) == 0.0,
        }
    }

    /// Whether one of this rectangle's axes keeps `other`'s corners clear
    /// of it.
    fn separated_from(&self, other: &Obstacle) -> bool {
        let (Shape::Rect { width, height }, Some(corners)) = (self.shape, other.corners()) else {
            return false;
        };
        let local = corners.map(|c| crate::to_local(self.position, self.facing, c));
        let clear = |axis: usize, half: f32| {
            local.iter().all(|c| c[axis] >= half) || local.iter().all(|c| c[axis] <= -half)
        };
        clear(0, width / 2.0) || clear(1, height / 2.0)
    }

    /// A rectangle's corners in world units.
    fn corners(&self) -> Option<[[f32; 2]; 4]> {
        let Shape::Rect { width, height } = self.shape else {
            return None;
        };
        let (w, h) = (width / 2.0, height / 2.0);
        Some(
            [[-w, -h], [w, -h], [w, h], [-w, h]]
                .map(|c| crate::from_local(self.position, self.facing, c)),
        )
    }

    /// Whether it keeps movers off a tile.
    pub fn covers(&self, tile: (i32, i32)) -> bool {
        covered(self).contains(&tile)
    }
}

#[derive(Clone)]
pub struct Pathing {
    /// Tiles per side.
    pub side: usize,
    /// Row-major bits per tile.
    tiles: Vec<u8>,
    /// Row-major, how many obstacles cover each tile, so that one going
    /// leaves the others' tiles blocked.
    blockers: Vec<u16>,
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
        let tiles: Vec<u8> = terrain
            .tiles
            .iter()
            .map(|t| by_index.get(t.terrain as usize).copied().unwrap_or(LAND))
            .collect();
        let mut pathing = Pathing {
            side: terrain.side,
            blockers: vec![0; tiles.len()],
            tiles,
        };
        for o in obstacles {
            pathing.block(&o);
        }
        pathing
    }

    /// A map of one kind of tile, for tests and tools.
    pub fn uniform(side: usize, domain: Domain) -> Self {
        Pathing {
            side,
            tiles: vec![bits(domain); side * side],
            blockers: vec![0; side * side],
        }
    }

    /// Whether a mover on `domain` can enter the tile; off the map it can't.
    pub fn passable(&self, x: i32, y: i32, domain: Domain) -> bool {
        let Some(i) = self.index(x, y) else {
            return false;
        };
        self.blockers[i] == 0 && self.tiles[i] & bits(domain) != 0
    }

    /// Keeps movers off one tile.
    pub fn set_blocked(&mut self, x: i32, y: i32) {
        if let Some(i) = self.index(x, y) {
            self.blockers[i] += 1;
        }
    }

    /// Gives back the tiles an obstacle that's gone covered.
    pub fn unblock(&mut self, o: &Obstacle) {
        for (x, y) in covered(o) {
            if let Some(i) = self.index(x, y) {
                self.blockers[i] = self.blockers[i].saturating_sub(1);
            }
        }
    }

    /// A copy with the tiles that `obstacles` cover given back, for a mover
    /// that passes through them.
    pub fn without<'a>(&self, obstacles: impl IntoIterator<Item = &'a Obstacle>) -> Pathing {
        let mut p = self.clone();
        for o in obstacles {
            p.unblock(o);
        }
        p
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let side = self.side as i32;
        ((0..side).contains(&x) && (0..side).contains(&y)).then(|| (y * side + x) as usize)
    }

    /// Whether an obstacle could stand there: every tile it would cover is
    /// on the map, open and crossable on `domain`.
    pub fn room_for(&self, o: &Obstacle, domain: Domain) -> bool {
        covered(o)
            .into_iter()
            .all(|(x, y)| self.passable(x, y, domain))
    }

    /// Keeps movers off the tiles an obstacle covers.
    pub fn block(&mut self, o: &Obstacle) {
        for (x, y) in covered(o) {
            self.set_blocked(x, y);
        }
    }
}

/// The tiles whose centres lie within a quarter of a tile's width of the
/// obstacle's shape, as the game blocks them: so shapes reaching no further
/// than that from their middle cover none.
fn covered(o: &Obstacle) -> Vec<(i32, i32)> {
    const MARGIN: f32 = CELL_SIZE / 4.0;
    let [px, py] = o.position;
    let reach = match o.shape {
        Shape::Point => 0.0,
        Shape::Circle { radius } => radius,
        Shape::Rect { width, height } => libm::hypotf(width, height) / 2.0,
    };
    if reach <= MARGIN {
        return Vec::new();
    }
    let covers = |wx: f32, wy: f32| match o.shape {
        Shape::Point => false,
        Shape::Circle { .. } => o.distance([wx, wy]) <= MARGIN,
        Shape::Rect { .. } => o.distance([wx, wy]) < MARGIN,
    };
    let extent = reach + MARGIN;
    let (x0, y0) = tile_at([px - extent, py - extent]);
    let (x1, y1) = tile_at([px + extent, py + extent]);
    let mut out = Vec::new();
    for y in y0..=y1 {
        for x in x0..=x1 {
            let [cx, cy] = tile_centre(x, y);
            if covers(cx, cy) {
                out.push((x, y));
            }
        }
    }
    out
}

/// The tile bit a mover on `domain` needs.
fn bits(domain: Domain) -> u8 {
    match domain {
        Domain::Land => LAND,
        Domain::Naval => NAVAL,
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
    fn shapes_reach_a_quarter_tile_beyond_their_edges() {
        // Radii either side of 1.5, which leaves the next tiles' centres a
        // quarter tile out.
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
    fn tiles_stay_blocked_until_everything_on_them_is_gone() {
        let o = |x| Obstacle {
            position: [x, 11.0],
            facing: [0.0, -1.0],
            shape: Shape::Circle { radius: 1.0 },
        };
        let mut p = Pathing::uniform(10, Domain::Land);
        let (a, b) = (o(11.0), o(11.5));
        p.block(&a);
        p.block(&b);
        p.unblock(&a);
        assert!(!p.passable(5, 5, Domain::Land));
        p.unblock(&b);
        assert!(p.passable(5, 5, Domain::Land));
    }

    #[test]
    fn overlaps() {
        let at = |position, facing, shape| Obstacle {
            position,
            facing,
            shape,
        };
        let square = Shape::Rect {
            width: 2.0,
            height: 2.0,
        };
        let a = at([0.0, 0.0], [0.0, 1.0], square);
        let circle = |x| at([x, 0.0], [0.0, 1.0], Shape::Circle { radius: 1.0 });
        assert!(a.overlaps(&circle(1.9)) && circle(1.9).overlaps(&a));
        assert!(!a.overlaps(&circle(2.0)));
        // A square turned 45° reaches √2 from its middle along the axes.
        let turned = |x| at([x, 0.0], [0.70710677, 0.70710677], square);
        assert!(a.overlaps(&turned(2.3)) && turned(2.3).overlaps(&a));
        assert!(!a.overlaps(&turned(2.5)));
        let point = |x| at([x, 0.0], [0.0, 1.0], Shape::Point);
        assert!(point(0.5).overlaps(&a) && a.overlaps(&point(0.5)));
        assert!(!point(1.5).overlaps(&a));
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
