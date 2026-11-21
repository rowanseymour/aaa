//! Simple national emblems a game's program can give its nations, which
//! our interface draws faintly behind its object display.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Shapes in flat colours, each painted over those before it, all centred
/// in a circle of radius 1.
#[derive(Clone, Copy, Debug)]
pub struct Emblem(pub &'static [Shape]);

/// One of an [`Emblem`]'s shapes, its sizes against the emblem's radius.
#[derive(Clone, Copy, Debug)]
pub enum Shape {
    /// A disc of a radius.
    Disc(f32, Color),
    /// A five-pointed star, point up, its points that far out.
    Star(f32, Color),
    /// An upright cross whose arms reach that far out, each as wide as the
    /// second figure.
    Cross(f32, f32, Color),
}

/// How far a star's inner corners are from its centre, against its points:
/// that of a regular pentagram.
const STAR_WAIST: f32 = 0.382;

/// Samples taken across and down each texel, to smooth the edges.
const SAMPLES: u32 = 4;

impl Shape {
    /// Its colour at `p`, if it covers it.
    fn at(&self, p: Vec2) -> Option<Color> {
        match *self {
            Shape::Disc(radius, color) => (p.length() <= radius).then_some(color),
            Shape::Star(radius, color) => in_star(p / radius).then_some(color),
            Shape::Cross(reach, width, color) => {
                let [x, y] = p.abs().to_array();
                let arm = |along: f32, across: f32| along <= reach && across <= width / 2.0;
                (arm(x, y) || arm(y, x)).then_some(color)
            }
        }
    }
}

impl Emblem {
    /// Its colour at `p`: that of the last shape covering it.
    fn at(&self, p: Vec2) -> Option<Color> {
        self.0.iter().rev().find_map(|shape| shape.at(p))
    }

    /// The emblem drawn `size` texels across and down, `across` of its
    /// height wide, centred and with every colour's alpha scaled by `alpha`.
    pub fn image(&self, size: UVec2, across: f32, alpha: f32) -> Image {
        let centre = size.as_vec2() / 2.0;
        let radius = size.y as f32 * across / 2.0;
        let mut data = Vec::with_capacity((size.x * size.y * 4) as usize);
        for y in 0..size.y {
            for x in 0..size.x {
                let mut sum = Vec4::ZERO;
                for j in 0..SAMPLES {
                    for i in 0..SAMPLES {
                        let offset = (Vec2::new(i as f32, j as f32) + 0.5) / SAMPLES as f32;
                        let texel = Vec2::new(x as f32, y as f32) + offset;
                        // Down the image is down the emblem.
                        let p = (texel - centre) / radius * Vec2::new(1.0, -1.0);
                        if let Some(color) = self.at(p) {
                            let c = color.to_linear();
                            sum += Vec4::new(c.red, c.green, c.blue, 1.0) * c.alpha;
                        }
                    }
                }
                let covered = sum / (SAMPLES * SAMPLES) as f32;
                let texel = match covered.w {
                    0.0 => [0; 4],
                    a => {
                        let c = LinearRgba::new(covered.x / a, covered.y / a, covered.z / a, a);
                        let mut srgb = Srgba::from(c).to_u8_array();
                        srgb[3] = (a * alpha * 255.0).round() as u8;
                        srgb
                    }
                };
                data.extend_from_slice(&texel);
            }
        }
        Image::new(
            Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        )
    }
}

/// Whether `p` is inside a point-up star whose points touch the unit circle.
fn in_star(p: Vec2) -> bool {
    let corner = |k: usize| {
        let radius = if k.is_multiple_of(2) { 1.0 } else { STAR_WAIST };
        Vec2::from_angle(FRAC_PI_2 + TAU * k as f32 / 10.0) * radius
    };
    // Crossings of a ray to the right, over the outline's ten edges.
    let mut inside = false;
    for k in 0..10 {
        let (a, b) = (corner(k), corner(k + 1));
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color::srgb(1.0, 0.0, 0.0);
    const BLUE: Color = Color::srgb(0.0, 0.0, 1.0);

    #[test]
    fn a_star_has_its_centre_and_points_but_not_between_them() {
        assert!(in_star(Vec2::ZERO));
        assert!(in_star(Vec2::new(0.0, 0.95)));
        // Between the top point and the next, past the waist.
        assert!(!in_star(Vec2::from_angle(FRAC_PI_2 + TAU / 10.0) * 0.6));
        assert!(!in_star(Vec2::new(0.0, -0.9)));
    }

    #[test]
    fn a_cross_covers_its_arms_but_not_between_them() {
        let cross = Shape::Cross(1.0, 0.4, RED);
        assert_eq!(cross.at(Vec2::new(0.9, 0.1)), Some(RED));
        assert_eq!(cross.at(Vec2::new(-0.1, -0.9)), Some(RED));
        assert_eq!(cross.at(Vec2::new(0.5, 0.5)), None);
        assert_eq!(cross.at(Vec2::new(1.1, 0.0)), None);
    }

    #[test]
    fn later_shapes_are_painted_over_earlier_ones() {
        let emblem = Emblem(&[Shape::Disc(1.0, BLUE), Shape::Star(0.9, RED)]);
        assert_eq!(emblem.at(Vec2::ZERO), Some(RED));
        assert_eq!(emblem.at(Vec2::new(0.0, -0.95)), Some(BLUE));
        assert_eq!(emblem.at(Vec2::new(1.1, 0.0)), None);
    }

    #[test]
    fn an_image_is_faint_where_drawn_and_clear_elsewhere() {
        let image = Emblem(&[Shape::Star(1.0, RED)]).image(UVec2::new(40, 20), 0.8, 0.5);
        let data = image.data.unwrap();
        let texel = |x: u32, y: u32| &data[((y * 40 + x) * 4) as usize..][..4];
        assert_eq!(texel(20, 10), [255, 0, 0, 128]);
        assert_eq!(texel(0, 0)[3], 0);
    }
}
