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
    /// Its blocking shape.
    pub shape: Shape,
    /// Its soft shape, which bounds the tiles it blocks.
    pub soft: Shape,
}

/// How far short of a box's high edges the tiles it spans stop, so that an
/// edge lying on a tile's edge doesn't take in the tile beyond.
const EDGE: f32 = 0.0001;

/// Whether shapes that only touch are apart or together.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Contact {
    Separates,
    Joins,
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
            (Shape::Rect { .. }, Shape::Rect { .. }) => !self.apart(other, Contact::Separates),
            (Shape::Rect { .. }, _) => other.overlaps(self),
            (Shape::Circle { radius }, _) => other.distance(self.position) < radius,
            (Shape::Point, _) => other.distance(self.position) == 0.0,
        }
    }

    /// Like [`overlaps`](Self::overlaps), except that two rectangles only
    /// touching meet.
    pub fn meets(&self, other: &Obstacle) -> bool {
        match (self.shape, other.shape) {
            (Shape::Rect { .. }, Shape::Rect { .. }) => !self.apart(other, Contact::Joins),
            _ => self.overlaps(other),
        }
    }

    /// Whether an axis of either rectangle keeps the other clear of it.
    fn apart(&self, other: &Obstacle, contact: Contact) -> bool {
        self.separated_from(other, contact) || other.separated_from(self, contact)
    }

    /// Whether one of this rectangle's axes keeps `other`'s corners clear
    /// of it.
    fn separated_from(&self, other: &Obstacle, contact: Contact) -> bool {
        let (Shape::Rect { width, height }, Some(corners)) = (self.shape, other.corners()) else {
            return false;
        };
        let local = corners.map(|c| crate::to_local(self.position, self.facing, c));
        let beyond = |c: f32, edge: f32| c > edge || (contact == Contact::Separates && c == edge);
        let clear = |axis: usize, half: f32| {
            local.iter().all(|c| beyond(c[axis], half))
                || local.iter().all(|c| beyond(-half, c[axis]))
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

    /// The low and high corners of the box `shape` takes up standing as it
    /// does.
    pub(crate) fn bounds(&self, shape: Shape) -> ([f32; 2], [f32; 2]) {
        let [x, y] = self.position;
        let (half_x, half_y) = match shape {
            Shape::Point => (0.0, 0.0),
            Shape::Circle { radius } => (radius, radius),
            Shape::Rect { width, height } => {
                let [fx, fy] = self.facing;
                let (w, h) = (width / 2.0, height / 2.0);
                (w * fy.abs() + h * fx.abs(), w * fx.abs() + h * fy.abs())
            }
        };
        ([x - half_x, y - half_y], [x + half_x, y + half_y])
    }
}

/// The tiles from the one under a box's low corner to the one just short
/// of its high corner, which may be off the map.
pub(crate) fn spanned((lo, hi): ([f32; 2], [f32; 2])) -> impl Iterator<Item = (i32, i32)> {
    let (x0, y0) = tile_at(lo);
    let (x1, y1) = tile_at([hi[0] - EDGE, hi[1] - EDGE]);
    let (x1, y1) = (x1.max(x0), y1.max(y0));
    (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| (x, y)))
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
    pub fn new(
        terrain: &Terrain,
        variations: &HashMap<Id, Variation>,
        obstacles: impl IntoIterator<Item = Obstacle>,
    ) -> Self {
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
        self.index(x, y)
            .is_some_and(|i| self.blockers[i] == 0 && self.tiles[i] & bits(domain) != 0)
    }

    /// Whether the tile's terrain is `domain`'s, whatever stands on it.
    fn crossable(&self, x: i32, y: i32, domain: Domain) -> bool {
        self.index(x, y)
            .is_some_and(|i| self.tiles[i] & bits(domain) != 0)
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
            let (x, y) = tile_at(o.position);
            return self.crossable(x, y, domain);
        }
        // A rectangle square to the map meets every tile in its box.
        let square_to_map = matches!(o.shape, Shape::Rect { .. }) && o.facing.contains(&0.0);
        spanned(o.bounds(o.shape))
            .filter(|&(x, y)| square_to_map || o.meets(&tile_square(x, y)))
            .all(|(x, y)| self.crossable(x, y, domain))
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

    /// Keeps movers off the tiles an obstacle covers.
    pub fn block(&mut self, o: &Obstacle) {
        for (x, y) in covered(o) {
            self.set_blocked(x, y);
        }
    }
}

/// The tiles an obstacle blocks: of those under its soft shape's box, the
/// ones whose centres lie within a quarter of a tile of its blocking shape.
/// So a blocking shape reaching no further than that from its middle blocks
/// none, and one reaching beyond its soft shape's box blocks nothing there.
fn covered(o: &Obstacle) -> Vec<(i32, i32)> {
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
        .filter(|&(x, y)| covers(tile_centre(x, y)))
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
fn tile_square(x: i32, y: i32) -> Obstacle {
    let side = Shape::Rect {
        width: CELL_SIZE,
        height: CELL_SIZE,
    };
    Obstacle {
        position: tile_centre(x, y),
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
        blocked_within(shape, Shape::Circle { radius: 5.0 }, facing)
    }

    fn blocked_within(shape: Shape, soft: Shape, facing: [f32; 2]) -> Vec<(i32, i32)> {
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
                if !p.passable(x, y, Domain::Land) {
                    out.push((x, y));
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
            unknown: 0,
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
        };
        let variations = HashMap::from([(Id::from("grass"), grass)]);
        let p = Pathing::new(&terrain, &variations, []);
        let at = |x, y| {
            (
                p.passable(x, y, Domain::Land),
                p.passable(x, y, Domain::Naval),
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
    fn only_tiles_under_the_soft_shapes_box_are_blocked() {
        let wide = Shape::Circle { radius: 1.6 };
        // A box edge on a tile's edge doesn't take in the tile beyond.
        let soft = Shape::Circle { radius: 1.0 };
        assert_eq!(blocked_within(wide, soft, [0.0, -1.0]), [(5, 5)]);
        // A rectangle's box turns with it.
        let soft = Shape::Rect {
            width: 4.0,
            height: 2.0,
        };
        let turned = blocked_within(wide, soft, [1.0, 0.0]);
        assert_eq!(turned, [(5, 4), (5, 5), (5, 6)]);
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
        assert!(!p.passable(1, 2, Domain::Land));
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
        let water = |tiles: &[(usize, usize)]| {
            let mut p = Pathing::uniform(10, Domain::Land);
            for &(x, y) in tiles {
                p.tiles[y * 10 + x] = NAVAL;
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
        let corners = [(3, 3), (6, 3), (3, 6), (6, 6)];
        assert!(water(&corners).room_for(&turned, Domain::Land, []));
        // One of its corners just reaches into (5, 3).
        assert!(!water(&[(5, 3)]).room_for(&turned, Domain::Land, []));
        // A circle of radius 2.5 likewise misses its box's corners, but not
        // the tiles beside them.
        let circle = at([0.0, -1.0], Shape::Circle { radius: 2.5 });
        assert!(water(&corners).room_for(&circle, Domain::Land, []));
        assert!(!water(&[(4, 3)]).room_for(&circle, Domain::Land, []));
    }

    #[test]
    fn room_on_tile_lines() {
        let at = |position, facing, shape| Obstacle {
            position,
            facing,
            shape,
            soft: shape,
        };
        let water = |tiles: &[(usize, usize)]| {
            let mut p = Pathing::uniform(10, Domain::Land);
            for &(x, y) in tiles {
                p.tiles[y * 10 + x] = NAVAL;
            }
            p
        };
        // A circle just reaching a tile's corner (10, 8) leaves it out.
        let circle = at([13.0, 12.0], [0.0, -1.0], Shape::Circle { radius: 5.0 });
        assert!(water(&[(4, 3)]).room_for(&circle, Domain::Land, []));
        assert!(!water(&[(4, 4)]).room_for(&circle, Domain::Land, []));
        // A turned rectangle whose edge runs through that corner takes the
        // tile in, though it only touches it.
        let side = Shape::Rect {
            width: 5.0,
            height: 5.0,
        };
        let turned = at([11.5, 10.0], [0.6, 0.8], side);
        assert!(water(&[]).room_for(&turned, Domain::Land, []));
        assert!(!water(&[(4, 3)]).room_for(&turned, Domain::Land, []));
        // It misses (7, 3) in its box's corner.
        assert!(water(&[(7, 3)]).room_for(&turned, Domain::Land, []));
        // A point, or a shape hardly bigger, on a tile corner has only the
        // tile with the larger x and y.
        let others = [(1, 1), (1, 2), (2, 1)];
        for shape in [Shape::Point, Shape::Circle { radius: 0.00005 }] {
            let o = at([4.0, 4.0], [0.0, -1.0], shape);
            assert!(water(&others).ground_for(&o, Domain::Land));
            assert!(!water(&[(2, 2)]).ground_for(&o, Domain::Land));
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
            soft: shape,
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
