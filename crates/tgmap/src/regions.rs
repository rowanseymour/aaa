//! The patches of open ground a map is cut into for computer players to
//! plan over, as the map stores them (`ai_map` in [`rmp.ksy`]), or cut
//! afresh for a map stored without them.
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use std::collections::{BinaryHeap, VecDeque};

use tgck::map::Terrain;

use crate::{Domain, Pathing, Tile};

/// Tiles a side of the squares a map stored without regions is cut into.
const FRESH_SQUARE: i32 = 24;

/// Marks a tile in no region.
const NONE: u32 = u32::MAX;

/// One patch of ground.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub centre: [f32; 2],
    /// The regions next to it, with the distance between the centres.
    pub neighbours: Vec<(usize, f32)>,
    /// Which land mass it's on, by index into [`Regions::islands`]; none
    /// for water.
    pub island: Option<usize>,
}

/// A map's regions, and which tile is in which.
#[derive(Debug, Clone, Default)]
pub struct Regions {
    pub regions: Vec<Region>,
    /// The regions of each land mass, largest first.
    pub islands: Vec<Vec<usize>>,
    side: usize,
    /// Row-major, each tile's region, or [`NONE`].
    tiles: Vec<u32>,
}

impl Regions {
    /// The regions `terrain` stores, or, without any, squares of the land
    /// `pathing` has.
    pub fn new(terrain: &Terrain, pathing: &Pathing) -> Regions {
        let stored = terrain.ai_map.as_ref().and_then(|m| m.data.as_ref());
        let (regions, tiles) = match stored {
            Some(data) if !data.regions.is_empty() => {
                let side = terrain.side;
                let mut tiles = vec![NONE; side * side];
                // A block naming no region there is leaves its tiles in none.
                let blocks = data
                    .blocks
                    .iter()
                    .filter(|b| (b.region as usize) < data.regions.len());
                for block in blocks {
                    let [top, right, bottom, left] = block.sides.each_ref().map(|s| s.bound);
                    for y in top..=bottom {
                        for x in left..=right {
                            if let Some(i) = Tile::new(x.into(), y.into()).index(side) {
                                tiles[i] = block.region;
                            }
                        }
                    }
                }
                let regions = (data.regions.iter())
                    .map(|r| Region {
                        centre: r.center,
                        neighbours: (r.neighbors.iter())
                            .filter(|(n, _)| (*n as usize) < data.regions.len())
                            .map(|&(n, d)| (n as usize, d))
                            .collect(),
                        island: None,
                    })
                    .collect();
                (regions, tiles)
            }
            _ => squares(pathing),
        };
        let mut out = Regions {
            regions,
            islands: Vec::new(),
            side: pathing.side,
            tiles,
        };
        out.find_islands(pathing);
        out
    }

    /// Regions laid out by hand, each tile in the region whose centre is
    /// nearest, for tests.
    pub fn laid_out(pathing: &Pathing, regions: Vec<Region>) -> Regions {
        let side = pathing.side;
        let tiles = (0..side * side)
            .map(|i| {
                let at = Tile::of_index(i, side).centre();
                let near = (regions.iter().enumerate())
                    .min_by(|(_, a), (_, b)| {
                        distance(a.centre, at).total_cmp(&distance(b.centre, at))
                    })
                    .map(|(r, _)| r as u32);
                near.unwrap_or(NONE)
            })
            .collect();
        let mut out = Regions {
            regions,
            islands: Vec::new(),
            side,
            tiles,
        };
        out.find_islands(pathing);
        out
    }

    /// Groups the land regions into land masses: those whose centres a land
    /// mover could get between.
    fn find_islands(&mut self, pathing: &Pathing) {
        // By the ground, whatever stands on it.
        let land = |r: &Region| pathing.crossable(Tile::at(r.centre), Domain::Land);
        let mut islands: Vec<Vec<usize>> = Vec::new();
        for r in 0..self.regions.len() {
            if !land(&self.regions[r]) {
                continue;
            }
            let found = (islands.iter_mut()).find(|i| {
                let other = self.regions[i[0]].centre;
                pathing.joined(Domain::Land, other, self.regions[r].centre)
            });
            match found {
                Some(island) => island.push(r),
                None => islands.push(vec![r]),
            }
        }
        islands.sort_by_key(|i| std::cmp::Reverse(i.len()));
        for (n, island) in islands.iter().enumerate() {
            for &r in island {
                self.regions[r].island = Some(n);
            }
        }
        self.islands = islands;
    }

    /// The region `at` is in, if any.
    pub fn at(&self, at: [f32; 2]) -> Option<usize> {
        let i = Tile::at(at).index(self.side)?;
        let r = *self.tiles.get(i)?;
        (r != NONE).then_some(r as usize)
    }

    /// The tiles in `region`.
    pub fn tiles(&self, region: usize) -> impl Iterator<Item = Tile> + '_ {
        let side = self.side;
        (self.tiles.iter().enumerate())
            .filter(move |&(_, &r)| r as usize == region && r != NONE)
            .map(move |(i, _)| Tile::new((i % side) as i32, (i / side) as i32))
    }

    /// The regions of the largest land mass.
    pub fn main_island(&self) -> &[usize] {
        self.islands.first().map_or(&[], Vec::as_slice)
    }

    /// How many steps from region to region each region is from `from`;
    /// `None` for those it can't reach.
    pub fn steps(&self, from: usize) -> Vec<Option<u32>> {
        let mut out = vec![None; self.regions.len()];
        let Some(first) = out.get_mut(from) else {
            return out;
        };
        *first = Some(0);
        let mut queue = VecDeque::from([from]);
        while let Some(r) = queue.pop_front() {
            let next = out[r].map_or(0, |s| s + 1);
            for &(n, _) in &self.regions[r].neighbours {
                if out[n].is_none() {
                    out[n] = Some(next);
                    queue.push_back(n);
                }
            }
        }
        out
    }

    /// How far each region is from `from`, summed from centre to centre
    /// along the shortest way; infinite for those it can't reach.
    pub fn travel(&self, from: usize) -> Vec<f32> {
        self.travel_through(from, |_| true)
    }

    /// As [`Regions::travel`], going on only through the regions that
    /// `open` lets in; those it doesn't are infinitely far.
    pub fn travel_through(&self, from: usize, open: impl Fn(usize) -> bool) -> Vec<f32> {
        let mut out = vec![f32::INFINITY; self.regions.len()];
        if from >= out.len() {
            return out;
        }
        out[from] = 0.0;
        let mut heap = BinaryHeap::from([Near(0.0, from)]);
        while let Some(Near(d, r)) = heap.pop() {
            if d > out[r] {
                continue;
            }
            for &(n, step) in &self.regions[r].neighbours {
                if !open(n) {
                    continue;
                }
                let further = d + step;
                if further < out[n] {
                    out[n] = further;
                    heap.push(Near(further, n));
                }
            }
        }
        out
    }
}

/// A region by how far it is, nearest first out of a [`BinaryHeap`].
struct Near(f32, usize);

impl PartialEq for Near {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Near {}

impl PartialOrd for Near {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Near {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.total_cmp(&self.0)
    }
}

/// Squares of [`FRESH_SQUARE`] tiles, each the land in it, its centre the
/// middle of that land, next to the squares beside it.
fn squares(pathing: &Pathing) -> (Vec<Region>, Vec<u32>) {
    let side = pathing.side as i32;
    let across = (side + FRESH_SQUARE - 1) / FRESH_SQUARE;
    let mut tiles = vec![NONE; pathing.side * pathing.side];
    let mut sums: Vec<([f32; 2], u32)> = vec![([0.0, 0.0], 0); (across * across) as usize];
    for y in 0..side {
        for x in 0..side {
            let tile = Tile::new(x, y);
            if !pathing.crossable(tile, Domain::Land) {
                continue;
            }
            let square = (y / FRESH_SQUARE * across + x / FRESH_SQUARE) as usize;
            let [cx, cy] = tile.centre();
            let (sum, n) = &mut sums[square];
            *sum = [sum[0] + cx, sum[1] + cy];
            *n += 1;
            if let Some(i) = tile.index(pathing.side) {
                tiles[i] = square as u32;
            }
        }
    }
    // Squares with no land are left out, so number the rest afresh.
    let mut numbers = vec![NONE; sums.len()];
    let mut regions = Vec::new();
    for (s, &([x, y], n)) in sums.iter().enumerate() {
        if n > 0 {
            numbers[s] = regions.len() as u32;
            let centre = [x / n as f32, y / n as f32];
            regions.push(Region {
                centre,
                neighbours: Vec::new(),
                island: None,
            });
        }
    }
    for t in &mut tiles {
        if *t != NONE {
            *t = numbers[*t as usize];
        }
    }
    for sy in 0..across {
        for sx in 0..across {
            let here = numbers[(sy * across + sx) as usize];
            if here == NONE {
                continue;
            }
            for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                let (nx, ny) = (sx + dx, sy + dy);
                if nx < 0 || ny < 0 || nx >= across || ny >= across {
                    continue;
                }
                let there = numbers[(ny * across + nx) as usize];
                if there != NONE {
                    let d = distance(
                        regions[here as usize].centre,
                        regions[there as usize].centre,
                    );
                    regions[here as usize].neighbours.push((there as usize, d));
                }
            }
        }
    }
    (regions, tiles)
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    let (x, y) = (a[0] - b[0], a[1] - b[1]);
    libm::sqrtf(x * x + y * y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_map_stored_without_regions_is_cut_into_squares_of_its_land() {
        let mut pathing = Pathing::uniform(48, Domain::Land);
        for y in 0..48 {
            pathing.set(Tile::new(47, y), Domain::Naval);
        }
        let (regions, tiles) = squares(&pathing);
        assert_eq!(regions.len(), 4);
        // The right-hand squares' land stops short of the water.
        assert!(regions[1].centre[0] < 36.0 * tgck::map::CELL_SIZE);
        assert_eq!(regions[0].neighbours.len(), 2);
        assert_eq!(tiles[47], NONE);
    }

    #[test]
    fn a_region_whose_centre_is_built_on_is_still_land() {
        let mut pathing = Pathing::uniform(16, Domain::Land);
        pathing.set_blocked(Tile::new(2, 2));
        let region = Region {
            centre: Tile::new(2, 2).centre(),
            neighbours: Vec::new(),
            island: None,
        };
        let regions = Regions::laid_out(&pathing, vec![region]);
        assert_eq!(regions.regions[0].island, Some(0));
    }

    #[test]
    fn regions_are_counted_out_in_steps_and_travel() {
        let pathing = Pathing::uniform(48, Domain::Land);
        let at = |x: f32| Region {
            centre: [x, 10.0],
            neighbours: Vec::new(),
            island: None,
        };
        let mut laid = vec![at(10.0), at(30.0), at(60.0)];
        laid[0].neighbours = vec![(1, 20.0)];
        laid[1].neighbours = vec![(0, 20.0), (2, 30.0)];
        laid[2].neighbours = vec![(1, 30.0)];
        let regions = Regions::laid_out(&pathing, laid);
        assert_eq!(regions.steps(0), [Some(0), Some(1), Some(2)]);
        assert_eq!(regions.travel(2), [50.0, 30.0, 0.0]);
        let blocked = regions.travel_through(2, |r| r != 1);
        assert_eq!(blocked, [f32::INFINITY, f32::INFINITY, 0.0]);
        assert_eq!(regions.at([58.0, 3.0]), Some(2));
        assert_eq!(regions.main_island(), [0, 1, 2]);
    }
}
