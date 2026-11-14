//! What keeps movers off the ground under it: an object's shapes, where
//! it stands and the way it faces.

use aaa_data::Shape;

use crate::pathing::{covered, spanned};
use crate::tile::Tile;

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
    pub fn covers(&self, tile: Tile) -> bool {
        covered(self).contains(&tile)
    }

    /// The tiles under its soft shape's box, some perhaps off the map.
    pub fn footprint(&self) -> impl Iterator<Item = Tile> + use<> {
        spanned(self.bounds(self.soft))
    }

    /// The low and high corners of the box `shape` takes up standing as it
    /// does.
    pub fn bounds(&self, shape: Shape) -> ([f32; 2], [f32; 2]) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
