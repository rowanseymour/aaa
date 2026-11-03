//! The ground's shape, from a map's height maps.

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
    pub fn new(hm: &HeightMap, mountain: &HeightMap) -> Self {
        let side = hm.side;
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
        Ground {
            side,
            heights,
            water,
            surface,
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

/// The vertex and its up to eight neighbours.
fn neighbours(side: usize, i: usize) -> impl Iterator<Item = usize> {
    let (x, y) = (i % side, i / side);
    (y.saturating_sub(1)..=(y + 1).min(side - 1)).flat_map(move |ny| {
        (x.saturating_sub(1)..=(x + 1).min(side - 1)).map(move |nx| ny * side + nx)
    })
}
