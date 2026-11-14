//! Where movers can go, a tile at a time: the terrain decides whether land
//! units or ships can cross a tile, and objects that block keep both off
//! the tiles they cover until they're gone.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::collections::HashMap;

use aaa_data::{Id, Shape, Variation};
use tgck::map::Terrain;

pub use tgck::map::CELL_SIZE;

use crate::obstacle::Obstacle;
use crate::tile::Tile;

/// What a mover travels on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Land,
    Naval,
}

const LAND: u8 = 1;
const NAVAL: u8 = 2;

/// How far short of a box's high edges the tiles it spans stop, so that an
/// edge lying on a tile's edge doesn't take in the tile beyond.
const EDGE: f32 = 0.0001;

/// The tiles from the one under a box's low corner to the one just short
/// of its high corner, which may be off the map.
pub(crate) fn spanned((lo, hi): ([f32; 2], [f32; 2])) -> impl Iterator<Item = Tile> {
    let low = Tile::at(lo);
    let high = Tile::at([hi[0] - EDGE, hi[1] - EDGE]);
    let (x1, y1) = (high.x.max(low.x), high.y.max(low.y));
    (low.y..=y1).flat_map(move |y| (low.x..=x1).map(move |x| Tile::new(x, y)))
}

/// What a diagonal step between tiles costs, in steps straight across.
const DIAGONAL: f32 = std::f32::consts::SQRT_2;

/// The whole tiles near something, as [`Pathing::zone`] finds them.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    /// The tile its box of tiles starts at, and how many it is a side.
    low: Tile,
    side: usize,
    tiles: Vec<bool>,
}

impl Zone {
    /// A zone of just these tiles, each within 16 of the origin.
    #[cfg(test)]
    pub(crate) fn of(tiles: &[Tile]) -> Zone {
        let side = 16;
        let mut zone = Zone {
            low: Tile::new(0, 0),
            side,
            tiles: vec![false; side * side],
        };
        for t in tiles {
            zone.tiles[t.index(side).expect("a tile within 16 of the origin")] = true;
        }
        zone
    }

    pub fn has(&self, tile: Tile) -> bool {
        let within = tile.offset(-self.low.x, -self.low.y);
        within.index(self.side).is_some_and(|i| self.tiles[i])
    }

    /// Whether what stands at `at` with no ground of its own is in it: by
    /// the tile under it.
    pub fn holds(&self, at: [f32; 2]) -> bool {
        self.has(Tile::at(at))
    }

    pub(crate) fn tiles(&self) -> impl Iterator<Item = Tile> + '_ {
        let held = self.tiles.iter().enumerate().filter(|(_, held)| **held);
        held.map(move |(i, _)| Tile::of_index(i, self.side).offset(self.low.x, self.low.y))
    }

    /// Whether something that blocks tiles is in it: by any tile under its
    /// soft shape's box.
    pub fn meets(&self, o: &Obstacle) -> bool {
        spanned(o.bounds(o.soft)).any(|tile| self.has(tile))
    }
}

#[derive(Clone)]
pub struct Pathing {
    /// Tiles per side.
    pub side: usize,
    /// Row-major bits per tile.
    pub(crate) tiles: Vec<u8>,
    /// Row-major, how many obstacles cover each tile, so that one going
    /// leaves the others' tiles blocked.
    blockers: Vec<u16>,
}

impl Pathing {
    /// Tiles whose terrain isn't among `variations` count as water or land
    /// by [`undefined_is_water`].
    pub fn new(terrain: &Terrain, variations: &HashMap<Id, Variation>) -> Self {
        let by_index: Vec<Option<u8>> = terrain
            .names
            .iter()
            .map(|n| {
                let v = variations.get(&Id::from(n.as_str()))?;
                Some((u8::from(v.land) * LAND) | (u8::from(v.naval) * NAVAL))
            })
            .collect();
        let tiles: Vec<u8> = terrain
            .tiles
            .iter()
            .map(|t| {
                let defined = by_index.get(usize::from(t.terrain)).copied().flatten();
                defined.unwrap_or(if undefined_is_water(t) { NAVAL } else { LAND })
            })
            .collect();
        Pathing {
            side: terrain.side,
            blockers: vec![0; tiles.len()],
            tiles,
        }
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
    pub fn passable(&self, tile: Tile, domain: Domain) -> bool {
        tile.index(self.side)
            .is_some_and(|i| self.blockers[i] == 0 && self.tiles[i] & bits(domain) != 0)
    }

    /// Whether the tile's terrain is `domain`'s, whatever stands on it.
    fn crossable(&self, tile: Tile, domain: Domain) -> bool {
        tile.index(self.side)
            .is_some_and(|i| self.tiles[i] & bits(domain) != 0)
    }

    /// Keeps movers off one tile.
    pub fn set_blocked(&mut self, tile: Tile) {
        if let Some(i) = tile.index(self.side) {
            self.blockers[i] += 1;
        }
    }

    /// Gives back the tiles an obstacle that's gone covered.
    pub fn unblock(&mut self, o: &Obstacle) {
        for tile in covered(o) {
            if let Some(i) = tile.index(self.side) {
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

    /// The length of the map's sides, in world units.
    pub fn size(&self) -> f32 {
        self.side as f32 * CELL_SIZE
    }

    /// The point nearest `at` that's a tile in from the map's low edges
    /// and just under one in from its high ones.
    pub fn inside(&self, at: [f32; 2]) -> [f32; 2] {
        let high = (self.size() - CELL_SIZE).next_down().max(CELL_SIZE);
        at.map(|c| c.clamp(CELL_SIZE, high))
    }

    /// Whether `o`'s shape lies wholly on the map.
    pub fn on_map(&self, o: &Obstacle) -> bool {
        let size = self.side as f32 * CELL_SIZE;
        let (lo, hi) = o.bounds(o.shape);
        lo.iter().chain(&hi).all(|&c| (0.0..size).contains(&c))
    }

    /// Whether every tile `o`'s shape overlaps is `domain`'s, whatever
    /// stands on it. Tiles in the corners of a turned rectangle's or a
    /// circle's box that the shape doesn't reach are no matter; within its
    /// box a rectangle takes in the tiles it only touches, a circle doesn't.
    pub fn ground_for(&self, o: &Obstacle, domain: Domain) -> bool {
        if reach(o.shape) < EDGE {
            return self.crossable(Tile::at(o.position), domain);
        }
        // A rectangle square to the map meets every tile in its box.
        let square_to_map = matches!(o.shape, Shape::Rect { .. }) && o.facing.contains(&0.0);
        spanned(o.bounds(o.shape))
            .filter(|&tile| square_to_map || o.meets(&tile_square(tile)))
            .all(|tile| self.crossable(tile, domain))
    }

    /// Whether something blocking that takes up `o`'s shape could stand
    /// there among the blocking things `others`: its shape is on the map,
    /// its ground is `domain`'s (see [`ground_for`](Self::ground_for)), and
    /// it overlaps none of them. Its
    /// size makes no difference, so a shape too small to block a tile still
    /// needs these.
    pub fn room_for<'a>(
        &self,
        o: &Obstacle,
        domain: Domain,
        others: impl IntoIterator<Item = &'a Obstacle>,
    ) -> bool {
        self.on_map(o)
            && self.ground_for(o, domain)
            && others.into_iter().all(|other| !other.overlaps(o))
    }

    /// The zone of `radius` around `centre`: the tiles a walk from the one
    /// under it reaches for less than that, tile to tile, a diagonal step
    /// costing as much further. The walk keeps to land if it starts on
    /// land, and off it otherwise, whatever stands on the tiles; a centre
    /// nothing can stand on has no zone.
    pub fn zone(&self, centre: [f32; 2], radius: f32) -> Zone {
        let centre = Tile::at(centre);
        let reach = (radius / CELL_SIZE).ceil().max(0.0) as i32;
        let side = (2 * reach + 1) as usize;
        let mut zone = Zone {
            low: centre.offset(-reach, -reach),
            side,
            tiles: vec![false; side * side],
        };
        let Some(start) = centre.index(self.side).filter(|&i| self.tiles[i] != 0) else {
            return zone;
        };
        let land = self.tiles[start] & LAND != 0;
        let low = zone.low;
        let slot = |t: Tile| ((t.y - low.y) as usize) * side + (t.x - low.x) as usize;
        let mut cost = vec![f32::INFINITY; side * side];
        cost[slot(centre)] = 0.0;
        // Nearest first: costs are never negative, so their bits sort as
        // they do.
        let mut open = BinaryHeap::from([Reverse((0, centre))]);
        while let Some(Reverse((bits, tile))) = open.pop() {
            let so_far = cost[slot(tile)];
            if so_far.to_bits() < bits {
                continue;
            }
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let next = tile.offset(dx, dy);
                    let inside =
                        (next.x - centre.x).abs() <= reach && (next.y - centre.y).abs() <= reach;
                    let same = next.index(self.side).is_some()
                        && self.crossable(next, Domain::Land) == land;
                    if (dx, dy) == (0, 0) || !inside || !same {
                        continue;
                    }
                    let step = match dx != 0 && dy != 0 {
                        true => CELL_SIZE * DIAGONAL,
                        false => CELL_SIZE,
                    };
                    let next_cost = so_far + step;
                    if next_cost < radius && next_cost < cost[slot(next)] {
                        cost[slot(next)] = next_cost;
                        open.push(Reverse((next_cost.to_bits(), next)));
                    }
                }
            }
        }
        for (tile, cost) in zone.tiles.iter_mut().zip(&cost) {
            *tile = *cost < radius;
        }
        zone
    }

    /// Keeps movers off the tiles an obstacle covers.
    pub fn block(&mut self, o: &Obstacle) {
        for tile in covered(o) {
            self.set_blocked(tile);
        }
    }
}

/// The tiles an obstacle blocks: of those under its soft shape's box, the
/// ones whose centres lie within a quarter of a tile of its blocking shape.
/// So a blocking shape reaching no further than that from its middle blocks
/// none, and one reaching beyond its soft shape's box blocks nothing there.
pub(crate) fn covered(o: &Obstacle) -> Vec<Tile> {
    const MARGIN: f32 = CELL_SIZE / 4.0;
    if reach(o.shape) <= MARGIN {
        return Vec::new();
    }
    // A centre exactly a quarter tile from a circle counts, but not from a
    // rectangle.
    let covers = |centre: [f32; 2]| match o.shape {
        Shape::Point => false,
        Shape::Circle { .. } => o.distance(centre) <= MARGIN + EDGE,
        Shape::Rect { .. } => o.distance(centre) < MARGIN,
    };
    spanned(o.bounds(o.soft))
        .filter(|tile| covers(tile.centre()))
        .collect()
}

/// How far a shape reaches from its centre at most.
fn reach(shape: Shape) -> f32 {
    match shape {
        Shape::Point => 0.0,
        Shape::Circle { radius } => radius,
        Shape::Rect { width, height } => libm::hypotf(width, height) / 2.0,
    }
}

/// A tile's square, to test what overlaps it.
fn tile_square(tile: Tile) -> Obstacle {
    let side = Shape::Rect {
        width: CELL_SIZE,
        height: CELL_SIZE,
    };
    Obstacle {
        position: tile.centre(),
        facing: [0.0, 1.0],
        shape: side,
        soft: side,
    }
}

/// Whether a tile whose terrain the data doesn't define is water: where the
/// map has it in a body of water, as the game's own water terrains almost
/// all are.
pub(crate) fn undefined_is_water(tile: &tgck::map::Tile) -> bool {
    tile.water_body != 0
}

/// The tile bit a mover on `domain` needs.
fn bits(domain: Domain) -> u8 {
    match domain {
        Domain::Land => LAND,
        Domain::Naval => NAVAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_are_octagons_of_tiles_on_open_ground() {
        let p = Pathing::uniform(40, Domain::Land);
        // 8 tiles straight out and 6 along a diagonal, for 18 on 2-unit
        // tiles.
        let zone = p.zone([41.0, 41.0], 18.0);
        assert!(zone.has(Tile::new(28, 20)) && !zone.has(Tile::new(29, 20)));
        assert!(zone.has(Tile::new(26, 26)) && !zone.has(Tile::new(27, 27)));
        assert!(zone.has(Tile::new(28, 22)) && !zone.has(Tile::new(28, 23)));
        assert!(zone.holds([40.5, 25.9]) && !zone.holds([40.5, 23.9]));
    }

    #[test]
    fn zones_stop_at_shores_but_not_at_obstacles() {
        let mut p = Pathing::uniform(40, Domain::Land);
        // Water down column 22 but for a ford at row 30.
        for y in (0..40).filter(|&y| y != 30) {
            let i = Tile::new(22, y).index(p.side).unwrap();
            p.tiles[i] = NAVAL;
        }
        p.set_blocked(Tile::new(21, 20));
        let zone = p.zone([41.0, 41.0], 18.0);
        assert!(zone.has(Tile::new(21, 20)));
        assert!(!zone.has(Tile::new(22, 20)) && !zone.has(Tile::new(23, 20)));
        // Round by the ford is too far for most of the far bank.
        assert!(!zone.has(Tile::new(23, 29)));
        let wide = p.zone([41.0, 41.0], 40.0);
        assert!(wide.has(Tile::new(23, 29)) && !wide.has(Tile::new(22, 20)));
        // From the water it's the water alone.
        let wet = p.zone([45.0, 41.0], 18.0);
        assert!(
            wet.has(Tile::new(22, 15))
                && !wet.has(Tile::new(21, 20))
                && !wet.has(Tile::new(22, 30))
        );
        // And nothing from ground nothing stands on.
        let i = Tile::new(5, 5).index(p.side).unwrap();
        p.tiles[i] = 0;
        assert!(!p.zone([11.0, 11.0], 18.0).has(Tile::new(5, 5)));
    }

    #[test]
    fn blocking_things_are_in_a_zone_by_any_tile_under_them() {
        let p = Pathing::uniform(40, Domain::Land);
        let zone = p.zone([41.0, 41.0], 18.0);
        let rect = Shape::Rect {
            width: 6.0,
            height: 6.0,
        };
        let house = |x: f32| Obstacle {
            position: [x, 41.0],
            facing: [0.0, 1.0],
            shape: rect,
            soft: rect,
        };
        // Tiles 29 to 31 across, and the zone ends at 28.
        assert!(!zone.meets(&house(61.0)));
        assert!(zone.meets(&house(59.0)));
    }

    fn blocked(shape: Shape, facing: [f32; 2]) -> Vec<Tile> {
        blocked_within(shape, Shape::Circle { radius: 5.0 }, facing)
    }

    fn blocked_within(shape: Shape, soft: Shape, facing: [f32; 2]) -> Vec<Tile> {
        let mut p = Pathing::uniform(10, Domain::Land);
        p.block(&Obstacle {
            position: [11.0, 11.0],
            facing,
            shape,
            soft,
        });
        let mut out = Vec::new();
        for y in 0..10 {
            for x in 0..10 {
                if !p.passable(Tile::new(x, y), Domain::Land) {
                    out.push(Tile::new(x, y));
                }
            }
        }
        out
    }

    #[test]
    fn undefined_terrain_is_water_where_the_map_has_water() {
        let tile = |terrain, water_body| tgck::map::Tile {
            terrain,
            water_body,
            flags: 0,
            seen_by: 0,
            padding: 0,
            unused: 0,
        };
        let terrain = Terrain {
            version: 7,
            resolution_bits: 0,
            biome: String::new(),
            names: vec!["grass".into(), "undefined".into()],
            side: 2,
            // A defined land tile under water stays land.
            tiles: vec![tile(0, 0), tile(1, 0), tile(1, 2), tile(0, 2)],
            ai_map: None,
            shore_mesh: None,
        };
        let grass = Variation {
            texture: String::new(),
            repeat: [8.0; 2],
            color: [0.5; 3],
            water: None,
            animation: None,
            priority: 0,
            land: true,
            naval: false,
            kind: aaa_data::TerrainKind::Land,
            group: None,
            properties: Vec::new(),
        };
        let variations = HashMap::from([(Id::from("grass"), grass)]);
        let p = Pathing::new(&terrain, &variations);
        let at = |x, y| {
            (
                p.passable(Tile::new(x, y), Domain::Land),
                p.passable(Tile::new(x, y), Domain::Naval),
            )
        };
        assert_eq!(at(0, 0), (true, false));
        assert_eq!(at(1, 0), (true, false));
        assert_eq!(at(0, 1), (false, true));
        assert_eq!(at(1, 1), (true, false));
    }

    #[test]
    fn small_obstacles_block_nothing() {
        assert_eq!(blocked(Shape::Circle { radius: 0.5 }, [0.0, -1.0]), []);
        assert_eq!(blocked(Shape::Point, [0.0, -1.0]), []);
        assert_eq!(
            blocked(Shape::Circle { radius: 0.6 }, [0.0, -1.0]),
            [Tile::new(5, 5)]
        );
    }

    #[test]
    fn shapes_reach_a_quarter_tile_beyond_their_edges() {
        // Radii either side of 1.5, which leaves the next tiles' centres a
        // quarter tile out.
        assert_eq!(
            blocked(Shape::Circle { radius: 1.4 }, [0.0, -1.0]),
            [Tile::new(5, 5)]
        );
        assert_eq!(
            blocked(Shape::Circle { radius: 1.6 }, [0.0, -1.0]),
            [
                Tile::new(5, 4),
                Tile::new(4, 5),
                Tile::new(5, 5),
                Tile::new(6, 5),
                Tile::new(5, 6)
            ]
        );
        let rect = Shape::Rect {
            width: 7.2,
            height: 1.0,
        };
        assert_eq!(
            blocked(rect, [0.0, -1.0]),
            [
                Tile::new(3, 5),
                Tile::new(4, 5),
                Tile::new(5, 5),
                Tile::new(6, 5),
                Tile::new(7, 5)
            ]
        );
    }

    #[test]
    fn rectangles_turn_with_their_facing() {
        // Three tiles across and one deep.
        let rect = Shape::Rect {
            width: 6.0,
            height: 1.0,
        };
        assert_eq!(
            blocked(rect, [0.0, -1.0]),
            [Tile::new(4, 5), Tile::new(5, 5), Tile::new(6, 5)]
        );
        assert_eq!(
            blocked(rect, [1.0, 0.0]),
            [Tile::new(5, 4), Tile::new(5, 5), Tile::new(5, 6)]
        );
    }

    #[test]
    fn only_tiles_under_the_soft_shapes_box_are_blocked() {
        let wide = Shape::Circle { radius: 1.6 };
        // A box edge on a tile's edge doesn't take in the tile beyond.
        let soft = Shape::Circle { radius: 1.0 };
        assert_eq!(blocked_within(wide, soft, [0.0, -1.0]), [Tile::new(5, 5)]);
        // A rectangle's box turns with it.
        let soft = Shape::Rect {
            width: 4.0,
            height: 2.0,
        };
        let turned = blocked_within(wide, soft, [1.0, 0.0]);
        assert_eq!(turned, [Tile::new(5, 4), Tile::new(5, 5), Tile::new(5, 6)]);
    }

    #[test]
    fn room_takes_the_terrain_under_the_shape_and_clear_of_other_blockers() {
        let mut p = Pathing::uniform(4, Domain::Land);
        p.tiles[3] = NAVAL;
        let at = |position, shape| Obstacle {
            position,
            facing: [0.0, -1.0],
            shape,
            soft: shape,
        };
        let small = Shape::Circle { radius: 0.2 };
        assert!(p.room_for(&at([1.0, 1.0], small), Domain::Land, []));
        // Too small to block a tile, it still needs its own to be land.
        assert!(!p.room_for(&at([7.0, 1.0], small), Domain::Land, []));
        assert!(p.room_for(&at([7.0, 1.0], small), Domain::Naval, []));
        // And to be on the map.
        assert!(!p.room_for(&at([0.1, 1.0], small), Domain::Land, []));
        assert!(!p.room_for(&at([7.9, 7.0], small), Domain::Naval, []));
        // A blocked tile is no matter, only overlapping what blocks it.
        let hut = at([3.0, 5.0], Shape::Circle { radius: 1.0 });
        p.block(&hut);
        assert!(!p.passable(Tile::new(1, 2), Domain::Land));
        assert!(p.room_for(&at([2.5, 3.5], small), Domain::Land, [&hut]));
        assert!(!p.room_for(&at([3.0, 4.5], small), Domain::Land, [&hut]));
    }

    #[test]
    fn room_takes_only_the_tiles_the_shape_overlaps() {
        let at = |facing, shape| Obstacle {
            position: [10.0, 10.0],
            facing,
            shape,
            soft: shape,
        };
        let water = |tiles: &[Tile]| {
            let mut p = Pathing::uniform(10, Domain::Land);
            for t in tiles {
                p.tiles[t.index(10).unwrap()] = NAVAL;
            }
            p
        };
        // A 4 by 2 rectangle turned 45° spans tiles 3 to 6 each way but
        // reaches none of the four in its box's corners.
        let turned = at(
            [0.70710677, 0.70710677],
            Shape::Rect {
                width: 4.0,
                height: 2.0,
            },
        );
        let corners = [(3, 3), (6, 3), (3, 6), (6, 6)].map(|(x, y)| Tile::new(x, y));
        assert!(water(&corners).room_for(&turned, Domain::Land, []));
        // One of its corners just reaches into (5, 3).
        assert!(!water(&[Tile::new(5, 3)]).room_for(&turned, Domain::Land, []));
        // A circle of radius 2.5 likewise misses its box's corners, but not
        // the tiles beside them.
        let circle = at([0.0, -1.0], Shape::Circle { radius: 2.5 });
        assert!(water(&corners).room_for(&circle, Domain::Land, []));
        assert!(!water(&[Tile::new(4, 3)]).room_for(&circle, Domain::Land, []));
    }

    #[test]
    fn room_on_tile_lines() {
        let at = |position, facing, shape| Obstacle {
            position,
            facing,
            shape,
            soft: shape,
        };
        let water = |tiles: &[Tile]| {
            let mut p = Pathing::uniform(10, Domain::Land);
            for t in tiles {
                p.tiles[t.index(10).unwrap()] = NAVAL;
            }
            p
        };
        // A circle just reaching a tile's corner (10, 8) leaves it out.
        let circle = at([13.0, 12.0], [0.0, -1.0], Shape::Circle { radius: 5.0 });
        assert!(water(&[Tile::new(4, 3)]).room_for(&circle, Domain::Land, []));
        assert!(!water(&[Tile::new(4, 4)]).room_for(&circle, Domain::Land, []));
        // A turned rectangle whose edge runs through that corner takes the
        // tile in, though it only touches it.
        let side = Shape::Rect {
            width: 5.0,
            height: 5.0,
        };
        let turned = at([11.5, 10.0], [0.6, 0.8], side);
        assert!(water(&[]).room_for(&turned, Domain::Land, []));
        assert!(!water(&[Tile::new(4, 3)]).room_for(&turned, Domain::Land, []));
        // It misses (7, 3) in its box's corner.
        assert!(water(&[Tile::new(7, 3)]).room_for(&turned, Domain::Land, []));
        // A point, or a shape hardly bigger, on a tile corner has only the
        // tile with the larger x and y.
        let others = [(1, 1), (1, 2), (2, 1)].map(|(x, y)| Tile::new(x, y));
        for shape in [Shape::Point, Shape::Circle { radius: 0.00005 }] {
            let o = at([4.0, 4.0], [0.0, -1.0], shape);
            assert!(water(&others).ground_for(&o, Domain::Land));
            assert!(!water(&[Tile::new(2, 2)]).ground_for(&o, Domain::Land));
        }
    }

    #[test]
    fn tiles_stay_blocked_until_everything_on_them_is_gone() {
        let o = |x| Obstacle {
            position: [x, 11.0],
            facing: [0.0, -1.0],
            shape: Shape::Circle { radius: 1.0 },
            soft: Shape::Circle { radius: 1.0 },
        };
        let mut p = Pathing::uniform(10, Domain::Land);
        let (a, b) = (o(11.0), o(11.5));
        p.block(&a);
        p.block(&b);
        p.unblock(&a);
        assert!(!p.passable(Tile::new(5, 5), Domain::Land));
        p.unblock(&b);
        assert!(p.passable(Tile::new(5, 5), Domain::Land));
    }

    #[test]
    fn domains() {
        let mut p = Pathing::uniform(2, Domain::Naval);
        assert!(p.passable(Tile::new(0, 0), Domain::Naval));
        assert!(!p.passable(Tile::new(0, 0), Domain::Land));
        assert!(!p.passable(Tile::new(2, 0), Domain::Naval));
        p.set_blocked(Tile::new(0, 0));
        assert!(!p.passable(Tile::new(0, 0), Domain::Naval));
    }
}
