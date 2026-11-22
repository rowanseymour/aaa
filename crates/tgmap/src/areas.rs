//! The regions a map's triggers test things against.

use tgck::map::{Detail, Thing};
use tgdata::Shape;

use crate::pathing::{reach, span};
use crate::{Obstacle, Tile};

/// A region placed in the map, by the `area` record of its object.
#[derive(Debug, Clone, PartialEq)]
pub struct Area {
    /// Its object's id, which triggers name it by.
    pub id: u32,
    pub name: String,
    pub position: [f32; 2],
    /// A unit vector, which turns its box for [`Area::touches`].
    pub facing: [f32; 2],
    pub outline: AreaShape,
    /// For a box, half its diagonal.
    pub radius: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AreaShape {
    /// `radius` around its position.
    Circle,
    /// Its low and high corners, measured from its position: x across its
    /// facing and y along it.
    Box { low: [f32; 2], high: [f32; 2] },
}

/// The record's numbers for each shape; the shipped maps use no others.
const CIRCLE: u32 = 1;
const BOX: u32 = 3;

impl Area {
    /// The area a map object is, if it's one of a shape this knows.
    pub fn of(thing: &Thing) -> Option<Area> {
        let Detail::Area(area) = &thing.detail else {
            return None;
        };
        let [lx, ly, hx, hy] = area.bounds;
        let outline = match area.shape {
            CIRCLE => AreaShape::Circle,
            BOX => AreaShape::Box {
                low: [lx, ly],
                high: [hx, hy],
            },
            _ => return None,
        };
        let [x, y, _] = thing.position;
        let [fx, fy, _] = thing.facing;
        let length = libm::hypotf(fx, fy);
        Some(Area {
            id: thing.id,
            name: area.name.clone(),
            position: [x, y],
            facing: match length > 0.0 {
                true => [fx / length, fy / length],
                false => [0.0, -1.0],
            },
            outline,
            radius: area.radius,
        })
    }

    /// Whether a thing whose soft shape stands as `thing`'s does is in the
    /// area by the test that keeps its list of who's in it. That's looser
    /// than its outline: the thing's centre within the area's radius of it,
    /// grown by the thing's, and the thing's box of tiles sharing one with
    /// the area's.
    pub fn holds(&self, thing: &Obstacle) -> bool {
        self.first_tile(thing).is_some()
    }

    /// The first of the tiles shared by the area and a thing it
    /// [holds](Self::holds), going row by row from the lowest y and, in a
    /// row, the lowest x.
    pub fn first_tile(&self, thing: &Obstacle) -> Option<Tile> {
        let reach = self.radius + reach(thing.soft);
        let [dx, dy] = [0, 1].map(|i| thing.position[i] - self.position[i]);
        if libm::hypotf(dx, dy) > reach {
            return None;
        }
        let (low, high) = span(thing.bounds(thing.soft));
        let (area_low, area_high) = span(self.square());
        let shared = low.x <= area_high.x
            && area_low.x <= high.x
            && low.y <= area_high.y
            && area_low.y <= high.y;
        shared.then(|| Tile::new(low.x.max(area_low.x), low.y.max(area_low.y)))
    }

    /// The box its tiles are taken under, square to the map whichever way
    /// it faces.
    fn square(&self) -> ([f32; 2], [f32; 2]) {
        let [x, y] = self.position;
        match self.outline {
            AreaShape::Circle => {
                let r = self.radius;
                ([x - r, y - r], [x + r, y + r])
            }
            AreaShape::Box { low, high } => ([x + low[0], y + low[1]], [x + high[0], y + high[1]]),
        }
    }

    /// Whether `thing`'s soft shape, standing as it does, overlaps the
    /// area's outline, turned to its facing; touching counts.
    pub fn touches(&self, thing: &Obstacle) -> bool {
        let outline = self.obstacle();
        match (thing.soft, outline.shape) {
            (Shape::Point, _) => outline.distance(thing.position) == 0.0,
            (Shape::Circle { radius }, _) => outline.distance(thing.position) <= radius,
            (Shape::Rect { .. }, Shape::Rect { .. }) => {
                let thing = Obstacle {
                    shape: thing.soft,
                    ..thing.clone()
                };
                thing.meets(&outline)
            }
            (Shape::Rect { .. }, _) => {
                let thing = Obstacle {
                    shape: thing.soft,
                    ..thing.clone()
                };
                thing.distance(self.position) <= self.radius
            }
        }
    }

    /// Its outline as an obstacle's shape.
    fn obstacle(&self) -> Obstacle {
        let (position, shape) = match self.outline {
            AreaShape::Circle => (
                self.position,
                Shape::Circle {
                    radius: self.radius,
                },
            ),
            AreaShape::Box { low, high } => {
                let middle = [(low[0] + high[0]) / 2.0, (low[1] + high[1]) / 2.0];
                let shape = Shape::Rect {
                    width: high[0] - low[0],
                    height: high[1] - low[1],
                };
                (crate::from_local(self.position, self.facing, middle), shape)
            }
        };
        Obstacle {
            position,
            facing: self.facing,
            shape,
            soft: shape,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(position: [f32; 2], soft: Shape) -> Obstacle {
        Obstacle {
            position,
            facing: [0.0, -1.0],
            shape: soft,
            soft,
        }
    }

    /// A box 20 long and 2 deep, turned to lie along y.
    fn wall() -> Area {
        Area {
            id: 1,
            name: "wall".into(),
            position: [50.0, 50.0],
            facing: [1.0, 0.0],
            outline: AreaShape::Box {
                low: [-10.0, -1.0],
                high: [10.0, 1.0],
            },
            radius: libm::hypotf(10.0, 1.0),
        }
    }

    #[test]
    fn a_circle_holds_things_reaching_its_radius() {
        let area = Area {
            outline: AreaShape::Circle,
            radius: 4.0,
            ..wall()
        };
        let regiment = Shape::Circle { radius: 1.0 };
        assert!(area.holds(&at([54.5, 50.0], regiment)));
        assert!(!area.holds(&at([55.5, 50.0], regiment)));
        // Just outside its outline still counts as held, not as touching.
        assert!(!area.touches(&at([55.5, 50.0], Shape::Point)));
        assert!(area.touches(&at([54.5, 50.0], regiment)));
    }

    #[test]
    fn a_box_holds_by_its_tiles_square_to_the_map_but_touches_turned() {
        let area = wall();
        let small = Shape::Circle { radius: 0.5 };
        // Its tiles run along x, whichever way it faces.
        assert!(area.holds(&at([58.0, 50.0], small)));
        assert!(!area.holds(&at([50.0, 58.0], small)));
        // Its outline is turned to lie along y.
        assert!(!area.touches(&at([58.0, 50.0], small)));
        assert!(area.touches(&at([50.0, 58.0], small)));
        assert!(area.touches(&at([51.5, 50.0], small)));
        assert!(!area.touches(&at([51.6, 50.0], small)));
    }

    #[test]
    fn only_circles_and_boxes_are_areas() {
        let mut thing = Thing {
            template: "circle_area".into(),
            class: tgck::map::AREA,
            id: 7,
            name: String::new(),
            position: [3.0, 4.0, 1.0],
            facing: [0.0, -2.0, 0.0],
            flags: 0,
            scale: 1.0,
            variation: -1,
            seen_by: 0,
            detail: Detail::Area(tgck::map::Area {
                version: 1,
                id: None,
                name: "Here".into(),
                shape: CIRCLE,
                radius: 5.0,
                bounds: [0.0; 4],
                inside: Vec::new(),
            }),
        };
        let area = Area::of(&thing).unwrap();
        assert_eq!((area.id, area.name.as_str()), (7, "Here"));
        assert_eq!((area.position, area.facing), ([3.0, 4.0], [0.0, -1.0]));
        if let Detail::Area(a) = &mut thing.detail {
            a.shape = 0;
        }
        assert_eq!(Area::of(&thing), None);
    }
}
