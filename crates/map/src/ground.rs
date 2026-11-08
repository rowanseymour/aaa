//! The ground's shape, from a map's height maps.

use aaa_data::Shape;
use tgck::map::{CELL_SIZE, HeightMap};

/// Ground heights with the water bed below the water. The height map stores
/// the water surface at water vertices, and the mountain height map how far
/// the bed lies below it.
pub struct Ground {
    pub side: usize,
    pub heights: Vec<f32>,
    pub water: Vec<bool>,
    /// Water level at water vertices and the land beside them.
    pub surface: Vec<f32>,
}

impl Ground {
    /// The ground of two height maps the same size, of at least one cell.
    pub fn new(hm: &HeightMap, mountain: &HeightMap) -> Result<Self, String> {
        let side = hm.side;
        if side < 2 || mountain.side != side {
            return Err(format!(
                "height maps of {side} and {} vertices a side don't make ground",
                mountain.side
            ));
        }
        let water: Vec<bool> = hm.layer.iter().map(|&l| l != 0).collect();
        // Off water the mountain heights are leftovers the game ignores:
        // objects there sit on the height map alone.
        let heights = hm
            .heights
            .iter()
            .zip(&mountain.heights)
            .zip(&water)
            .map(|((&h, &d), &w)| if w { h + d } else { h })
            .collect();
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
        Ok(Ground {
            side,
            heights,
            water,
            surface,
        })
    }

    /// Height at a vertex.
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.heights[y * self.side + x]
    }

    /// Width and depth in world units.
    pub fn size(&self) -> f32 {
        (self.side - 1) as f32 * CELL_SIZE
    }

    /// Ground height at a world position, interpolated.
    pub fn height_at(&self, wx: f32, wy: f32) -> f32 {
        let max = (self.side - 1) as f32;
        let fx = (wx / CELL_SIZE).clamp(0.0, max);
        let fy = (wy / CELL_SIZE).clamp(0.0, max);
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.side - 1), (y0 + 1).min(self.side - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let top = self.at(x0, y0) * (1.0 - tx) + self.at(x1, y0) * tx;
        let bottom = self.at(x0, y1) * (1.0 - tx) + self.at(x1, y1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
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
        Ground {
            side,
            heights,
            water: vec![false; side * side],
            surface: vec![0.0; side * side],
        }
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

    #[test]
    fn height_maps_must_match_and_hold_a_cell() {
        let hm = |side: usize| HeightMap {
            version: 1,
            min: 0.0,
            max: 0.0,
            default: 0.0,
            unknown: 0,
            side,
            heights: vec![1.0; side * side],
            layer: vec![0; side * side],
        };
        assert!(Ground::new(&hm(5), &hm(4)).is_err());
        assert!(Ground::new(&hm(1), &hm(1)).is_err());
        assert!(Ground::new(&hm(0), &hm(0)).is_err());
        let ground = Ground::new(&hm(5), &hm(5)).unwrap();
        assert_eq!(ground.height_at(8.0, 8.0), 1.0);
    }

    #[test]
    fn a_point_stays_upright() {
        let slope = ground(|x, _| x);
        let tilt = slope.tilt([8.0, 8.0], [0.0, 1.0], Shape::Point);
        assert_eq!((tilt.height, tilt.up), (8.0, [0.0, 0.0, 1.0]));
    }
}
