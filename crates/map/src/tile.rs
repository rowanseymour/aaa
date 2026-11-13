//! The squares the map is cut into, a [`CELL_SIZE`] a side.

use tgck::map::CELL_SIZE;

/// A tile, counted from the one at the world's origin. It may be off the
/// map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
}

impl Tile {
    pub const fn new(x: i32, y: i32) -> Tile {
        Tile { x, y }
    }

    /// The tile a world position lies in.
    pub fn at([x, y]: [f32; 2]) -> Tile {
        Tile::new(
            (x / CELL_SIZE).floor() as i32,
            (y / CELL_SIZE).floor() as i32,
        )
    }

    /// Its centre in world units.
    pub fn centre(self) -> [f32; 2] {
        [
            (self.x as f32 + 0.5) * CELL_SIZE,
            (self.y as f32 + 0.5) * CELL_SIZE,
        ]
    }

    /// The tile `dx` and `dy` tiles on from it.
    pub const fn offset(self, dx: i32, dy: i32) -> Tile {
        Tile::new(self.x + dx, self.y + dy)
    }

    /// Its row-major index on a map `side` tiles a side, if it's on it.
    pub fn index(self, side: usize) -> Option<usize> {
        let on = |c: i32| usize::try_from(c).ok().filter(|&c| c < side);
        Some(on(self.y)? * side + on(self.x)?)
    }

    /// The tile at a row-major index on a map `side` tiles a side.
    pub fn of_index(i: usize, side: usize) -> Tile {
        Tile::new((i % side) as i32, (i / side) as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_under_positions_floor_towards_the_low_side() {
        assert_eq!(Tile::at([0.0, 0.0]), Tile::new(0, 0));
        assert_eq!(Tile::at([CELL_SIZE - 0.01, CELL_SIZE]), Tile::new(0, 1));
        assert_eq!(Tile::at([-0.01, -CELL_SIZE]), Tile::new(-1, -1));
        assert_eq!(Tile::at(Tile::new(-3, 7).centre()), Tile::new(-3, 7));
    }

    #[test]
    fn only_tiles_on_the_map_have_an_index() {
        assert_eq!(Tile::new(0, 0).index(4), Some(0));
        assert_eq!(Tile::new(3, 2).index(4), Some(11));
        for off in [(-1, 0), (0, -1), (4, 0), (0, 4)] {
            assert_eq!(Tile::new(off.0, off.1).index(4), None, "{off:?}");
        }
        assert_eq!(Tile::new(0, 0).index(0), None);
        for i in 0..16 {
            assert_eq!(Tile::of_index(i, 4).index(4), Some(i));
        }
    }
}
