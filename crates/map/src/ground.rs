//! The ground's shape, from a map's height maps and its tiles' terrain.

use std::collections::HashMap;

use aaa_data::{Footing, Id, Shape, TerrainKind, Variation};
use tgck::map::{CELL_SIZE, HeightMap, RESOLUTION_BITS, Terrain};

/// The ground: the height map, with the mountain height map added around
/// mountains and water (where it lays the water's bed), and mountains'
/// tiles each raised a little more.
pub struct Ground {
    /// Vertices per side.
    pub side: usize,
    /// The ground at each vertex as the tile it's the corner of nearest the
    /// origin has it, row-major, for drawing: one height a vertex can't
    /// show the steps between tiles that [`Ground::height_at`] has.
    pub heights: Vec<f32>,
    pub water: Vec<bool>,
    /// Water level at water vertices and the land beside them.
    pub surface: Vec<f32>,
    /// The two height maps, row-major by vertex.
    base: Vec<f32>,
    mountain: Vec<f32>,
    /// Per tile, row-major: how far it's raised on top of the mountain
    /// height map, or `None` where that doesn't count.
    relief: Vec<Option<f32>>,
}

impl Ground {
    /// The ground of two height maps the same size, of at least one cell,
    /// and the terrain of the tiles between their vertices.
    pub fn new(
        hm: &HeightMap,
        mountain: &HeightMap,
        terrain: &Terrain,
        variations: &HashMap<Id, Variation>,
    ) -> Result<Self, String> {
        let resolutions = [
            hm.resolution_bits,
            mountain.resolution_bits,
            terrain.resolution_bits,
        ];
        if resolutions != [RESOLUTION_BITS; 3] {
            return Err(format!(
                "map made at resolutions {resolutions:?}, not {RESOLUTION_BITS}"
            ));
        }
        let side = hm.side;
        if side < 2 || mountain.side != side {
            return Err(format!(
                "height maps of {side} and {} vertices a side don't make ground",
                mountain.side
            ));
        }
        let tiles = terrain.side;
        if tiles + 1 != side || terrain.tiles.len() != tiles * tiles {
            return Err(format!(
                "{tiles} tiles a side don't fit height maps of {side} vertices"
            ));
        }
        let kinds: Vec<TerrainKind> = terrain
            .tiles
            .iter()
            .map(|t| {
                let name = terrain.names.get(usize::from(t.terrain));
                match name.and_then(|n| variations.get(&Id::from(n.as_str()))) {
                    Some(v) => v.kind,
                    None if crate::pathing::undefined_is_water(t) => TerrainKind::Water,
                    None => TerrainKind::Land,
                }
            })
            .collect();
        let relief = (0..tiles * tiles)
            .map(|k| {
                let rugged = |j: usize| !matches!(kinds[j], TerrainKind::Land);
                neighbours(tiles, k).any(rugged).then(|| match kinds[k] {
                    TerrainKind::Mountain { noise: [low, high] } => {
                        low + (high - low) * tile_share(k)
                    }
                    _ => 0.0,
                })
            })
            .collect();
        let water: Vec<bool> = hm.layer.iter().map(|&l| l != 0).collect();
        let surface = (0..side * side)
            .map(|i| {
                if water[i] {
                    return hm.heights[i];
                }
                let levels: Vec<f32> = neighbours(side, i)
                    .filter(|&j| water[j])
                    .map(|j| hm.heights[j])
                    .collect();
                match levels.len() {
                    0 => hm.heights[i],
                    n => levels.iter().sum::<f32>() / n as f32,
                }
            })
            .collect();
        let mut ground = Ground {
            side,
            heights: Vec::new(),
            water,
            surface,
            base: hm.heights.clone(),
            mountain: mountain.heights.clone(),
            relief,
        };
        ground.heights = (0..side * side)
            .map(|i| {
                let (x, y) = ((i % side) as f32, (i / side) as f32);
                ground.height_at(x * CELL_SIZE, y * CELL_SIZE)
            })
            .collect();
        Ok(ground)
    }

    /// A ground of plain `heights`, row-major by vertex over a square of at
    /// least one cell, with no water or mountains, for tests and tools.
    pub fn from_heights(heights: Vec<f32>) -> Self {
        let side = heights.len().isqrt();
        assert!(
            side >= 2 && side * side == heights.len(),
            "{} heights aren't a square of at least one cell",
            heights.len()
        );
        let tiles = side.saturating_sub(1);
        Ground {
            side,
            water: vec![false; heights.len()],
            surface: heights.clone(),
            base: heights.clone(),
            mountain: vec![0.0; heights.len()],
            relief: vec![None; tiles * tiles],
            heights,
        }
    }

    /// Height at a vertex.
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.heights[y * self.side + x]
    }

    /// Width and depth in world units.
    pub fn size(&self) -> f32 {
        (self.side - 1) as f32 * CELL_SIZE
    }

    /// Ground height at a world position.
    pub fn height_at(&self, wx: f32, wy: f32) -> f32 {
        let tiles = self.side - 1;
        let tile = |w: f32| ((w / CELL_SIZE).floor().max(0.0) as usize).min(tiles - 1);
        let relief = self.relief[tile(wy) * tiles + tile(wx)];
        let base = self.sample(&self.base, wx, wy);
        relief.map_or(base, |r| base + self.sample(&self.mountain, wx, wy) + r)
    }

    /// The height a floating thing keeps at a world position: on the water,
    /// which lies half a unit under the height map, where that's above the
    /// ground.
    pub fn float_at(&self, wx: f32, wy: f32) -> f32 {
        let surface = self.sample(&self.base, wx, wy) - WATERLINE;
        self.height_at(wx, wy).max(surface)
    }

    /// The height a thing of `footing` stands at at a world position.
    /// Flying things have a height of their own; without one they're on the
    /// ground.
    pub fn stand_at(&self, footing: Footing, wx: f32, wy: f32) -> f32 {
        match footing {
            Footing::Water => self.float_at(wx, wy),
            Footing::Ground | Footing::Air => self.height_at(wx, wy),
        }
    }

    /// A height map's height at a world position: bilinear within a cell,
    /// but the mean of the two vertices either side of it on a grid line,
    /// and held at the edge beyond the map.
    fn sample(&self, grid: &[f32], wx: f32, wy: f32) -> f32 {
        let last = self.side - 1;
        let at = |x: f32, y: f32| {
            let index = |v: f32| (v.max(0.0) as usize).min(last);
            grid[index(y) * self.side + index(x)]
        };
        let (fx, fy) = (wx / CELL_SIZE, wy / CELL_SIZE);
        let (x0, y0) = (fx.floor(), fy.floor());
        let on_line = |f: f32| (f - f.round()).abs() * CELL_SIZE < 1e-4;
        match (on_line(fx), on_line(fy)) {
            (true, true) => at(fx.round(), fy.round()),
            (true, false) => (at(fx.round(), y0) + at(fx.round(), y0 + 1.0)) / 2.0,
            (false, true) => (at(x0, fy.round()) + at(x0 + 1.0, fy.round())) / 2.0,
            (false, false) => {
                let (tx, ty) = (fx - x0, fy - y0);
                let top = at(x0, y0) * (1.0 - tx) + at(x0 + 1.0, y0) * tx;
                let bottom = at(x0, y0 + 1.0) * (1.0 - tx) + at(x0 + 1.0, y0 + 1.0) * tx;
                top * (1.0 - ty) + bottom * ty
            }
        }
    }
}

/// How far the water's surface lies under the height map.
const WATERLINE: f32 = 0.5;

/// Where in its terrain's range a mountain tile's extra height lies, from 0
/// to just under 1, fixed by the tile's row-major index.
fn tile_share(k: usize) -> f32 {
    let r = (k as u64 * 4723 + 6323) % 6997 % 1000;
    r as f32 / 1000.0
}

/// How an object that follows the ground sits on it, in world axes with z up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tilt {
    pub height: f32,
    /// Unit length, along the ground ahead.
    pub forward: [f32; 3],
    /// Unit length; not quite square to `forward` where the ground bends.
    pub up: [f32; 3],
}

impl Ground {
    /// How an object at `position` facing `facing`, a unit vector, rests on
    /// the ground under `footprint`: on a plane through the ground at the
    /// footprint's corners and centre, raised to clear all of them so it
    /// doesn't sink into a hill, and never below the ground at its centre.
    pub fn tilt(&self, position: [f32; 2], facing: [f32; 2], footprint: Shape) -> Tilt {
        let [x, y] = position;
        let [fx, fy] = facing;
        let ground = |right: f32, ahead: f32| {
            let [x, y] = crate::from_local(position, facing, [right, ahead]);
            [x, y, self.height_at(x, y)]
        };
        let (half_w, half_h) = match footprint {
            Shape::Point => (0.0, 0.0),
            Shape::Circle { radius } => (radius, radius),
            Shape::Rect { width, height } => (width / 2.0, height / 2.0),
        };
        let back_left = ground(-half_w, -half_h);
        let back_right = ground(half_w, -half_h);
        let front_right = ground(half_w, half_h);
        let front_left = ground(-half_w, half_h);
        let centre = ground(0.0, 0.0);
        // One normal for each side's triangle with the centre, averaged.
        let left = cross(sub(centre, back_left), sub(front_left, centre));
        let right = cross(sub(front_right, back_right), sub(centre, front_right));
        let up = normalize(add(normalize(left), normalize(right)));
        if up[2] <= 0.0 {
            return Tilt {
                height: centre[2],
                forward: [fx, fy, 0.0],
                up: [0.0, 0.0, 1.0],
            };
        }
        let lift = [back_left, back_right, front_right, front_left]
            .iter()
            .fold(dot(up, centre), |d, &c| d.max(dot(up, c)));
        let plane = |px: f32, py: f32| (lift - up[0] * px - up[1] * py) / up[2];
        let height = plane(x, y).max(centre[2]);
        Tilt {
            height,
            forward: normalize([fx, fy, plane(x + fx, y + fy) - height]),
            up,
        }
    }
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Zero for a vector too short to have a direction.
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt();
    if len < 1e-6 {
        return [0.0; 3];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

/// The vertex and its up to eight neighbours.
fn neighbours(side: usize, i: usize) -> impl Iterator<Item = usize> {
    let (x, y) = (i % side, i / side);
    (y.saturating_sub(1)..=(y + 1).min(side - 1)).flat_map(move |ny| {
        (x.saturating_sub(1)..=(x + 1).min(side - 1)).map(move |nx| ny * side + nx)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ground whose height at each vertex is `height` of its position.
    fn ground(height: impl Fn(f32, f32) -> f32) -> Ground {
        let side = 9;
        let heights = (0..side * side)
            .map(|i| height((i % side) as f32 * CELL_SIZE, (i / side) as f32 * CELL_SIZE))
            .collect();
        Ground::from_heights(heights)
    }

    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
    }

    const TANK: Shape = Shape::Rect {
        width: 1.5,
        height: 2.5,
    };

    #[test]
    fn tilts_up_a_slope_ahead_and_across_one_beside() {
        let slope = ground(|_, y| y * 0.5);
        let n = 1.25_f32.sqrt();
        let up_slope = [0.0, -0.5 / n, 1.0 / n];
        let climbing = slope.tilt([8.0, 8.0], [0.0, 1.0], TANK);
        assert!((climbing.height - 4.0).abs() < 1e-5);
        assert!(near(climbing.forward, [0.0, 1.0 / n, 0.5 / n]));
        assert!(near(climbing.up, up_slope));
        let across = slope.tilt([8.0, 8.0], [1.0, 0.0], TANK);
        assert!(near(across.forward, [1.0, 0.0, 0.0]));
        assert!(near(across.up, up_slope));
    }

    #[test]
    fn rests_on_the_sides_of_a_dip() {
        let valley = ground(|x, _| (x - 8.0).abs());
        let tilt = valley.tilt([8.0, 8.0], [0.0, 1.0], TANK);
        assert!((tilt.height - 0.75).abs() < 1e-5);
        assert!(near(tilt.up, [0.0, 0.0, 1.0]));
    }

    fn height_map(side: usize, height: f32) -> HeightMap {
        HeightMap {
            version: 1,
            min: 0.0,
            max: 0.0,
            default: 0.0,
            resolution_bits: RESOLUTION_BITS,
            side,
            heights: vec![height; side * side],
            layer: vec![0; side * side],
        }
    }

    /// `tiles` a side of grass, but for a mountain at 1,1 and sea at 3,0,
    /// and the terrains they name.
    fn terrain(tiles: usize) -> (Terrain, HashMap<Id, Variation>) {
        let tile = |terrain| tgck::map::Tile {
            terrain,
            water_body: 0,
            flags: 0,
            seen_by: 0,
            padding: 0,
            unused: 0,
        };
        let mut grid = vec![tile(0); tiles * tiles];
        if tiles >= 4 {
            grid[tiles + 1] = tile(1);
            grid[3] = tile(2);
        }
        let terrain = Terrain {
            version: 7,
            resolution_bits: RESOLUTION_BITS,
            biome: String::new(),
            names: vec!["grass".into(), "rock".into(), "sea".into()],
            side: tiles,
            tiles: grid,
            ai_map: None,
            shore_mesh: None,
        };
        let variation = |kind| Variation {
            texture: String::new(),
            repeat: [8.0; 2],
            color: [0.5; 3],
            water: None,
            animation: None,
            priority: 0,
            land: true,
            naval: false,
            kind,
            group: None,
        };
        let variations = HashMap::from([
            (Id::from("grass"), variation(TerrainKind::Land)),
            (
                Id::from("rock"),
                variation(TerrainKind::Mountain { noise: [1.0, 2.0] }),
            ),
            (Id::from("sea"), variation(TerrainKind::Water)),
        ]);
        (terrain, variations)
    }

    #[test]
    fn height_maps_must_match_and_hold_a_cell() {
        let (t4, v) = terrain(4);
        let new = |a, b, t: &Terrain| Ground::new(&height_map(a, 1.0), &height_map(b, 1.0), t, &v);
        assert!(new(5, 4, &t4).is_err());
        assert!(new(1, 1, &terrain(0).0).is_err());
        assert!(new(0, 0, &terrain(0).0).is_err());
        // Tiles must sit between the vertices.
        assert!(new(6, 6, &t4).is_err());
        assert!(new(5, 5, &t4).is_ok());
    }

    #[test]
    fn maps_made_at_another_resolution_are_refused() {
        let (mut t4, v) = terrain(4);
        let (hm, mut mountain) = (height_map(5, 1.0), height_map(5, 1.0));
        mountain.resolution_bits = 2;
        assert!(Ground::new(&hm, &mountain, &t4, &v).is_err());
        t4.resolution_bits = 0;
        assert!(Ground::new(&hm, &height_map(5, 1.0), &t4, &v).is_err());
    }

    #[test]
    fn the_mountain_map_raises_mountains_water_and_the_land_beside_them() {
        let (terrain, variations) = terrain(4);
        let ground = Ground::new(
            &height_map(5, 1.0),
            &height_map(5, 5.0),
            &terrain,
            &variations,
        )
        .unwrap();
        // The mountain's own tile, 5 in row-major order, is raised 1.95.
        assert!((ground.height_at(3.0, 3.0) - 7.95).abs() < 1e-5);
        // Beside it, and beside the sea, the mountain map counts alone.
        assert_eq!(ground.height_at(5.0, 5.0), 6.0);
        assert_eq!(ground.height_at(5.0, 1.0), 6.0);
        // As it does on the sea itself.
        assert_eq!(ground.height_at(7.0, 1.0), 6.0);
        // Away from both it doesn't.
        assert_eq!(ground.height_at(7.0, 7.0), 1.0);
        // Vertices take their ground from the tile they're the corner of
        // nearest the origin.
        assert!((ground.at(1, 1) - 7.95).abs() < 1e-5);
    }

    #[test]
    fn floating_things_keep_to_the_water_or_the_ground_above_it() {
        let (terrain, variations) = terrain(4);
        let ground = |mountain| {
            Ground::new(
                &height_map(5, 1.0),
                &height_map(5, mountain),
                &terrain,
                &variations,
            )
            .unwrap()
        };
        let sea = ground(-5.0);
        // On the sea and the shore beside it, the bed is under the water.
        assert_eq!(sea.height_at(7.0, 1.0), -4.0);
        assert_eq!(sea.float_at(7.0, 1.0), 0.5);
        assert_eq!(sea.float_at(5.0, 1.0), 0.5);
        assert_eq!(sea.stand_at(Footing::Water, 7.0, 1.0), 0.5);
        assert_eq!(sea.stand_at(Footing::Ground, 7.0, 1.0), -4.0);
        assert_eq!(sea.stand_at(Footing::Air, 7.0, 1.0), -4.0);
        // Away from water, and where the bed rises above it, on the ground.
        assert_eq!(sea.float_at(7.0, 7.0), 1.0);
        assert_eq!(ground(5.0).float_at(7.0, 1.0), 6.0);
    }

    #[test]
    #[should_panic(expected = "aren't a square")]
    fn plain_ground_needs_a_cell() {
        Ground::from_heights(vec![1.0]);
    }

    #[test]
    fn grid_lines_take_the_mean_of_the_vertices_either_side() {
        // The height of each vertex is its column.
        let ground = ground(|x, _| x / CELL_SIZE);
        assert!((ground.height_at(2.5, 3.0) - 1.25).abs() < 1e-5);
        // On a row the mean of the vertices either side, wherever along it.
        assert_eq!(ground.height_at(2.5, 2.0), 1.5);
        assert_eq!(ground.height_at(2.0, 3.0), 1.0);
        assert_eq!(ground.height_at(4.0, 4.0), 2.0);
        // Beyond the map, held at the edge.
        assert_eq!(ground.height_at(100.0, 3.0), 8.0);
        assert_eq!(ground.height_at(-5.0, 3.0), 0.0);
    }

    #[test]
    fn a_point_stays_upright() {
        let slope = ground(|x, _| x);
        let tilt = slope.tilt([8.0, 8.0], [0.0, 1.0], Shape::Point);
        assert_eq!((tilt.height, tilt.up), (8.0, [0.0, 0.0, 1.0]));
    }
}
