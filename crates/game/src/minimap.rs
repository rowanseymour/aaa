//! The minimap: the map drawn as a diamond on a black disc in the corner,
//! turned the way the game camera sees it, with units and buildings in
//! their owner's colour and the ground the camera sees outlined in white.
//! Clicking or dragging on it moves the camera there.

use aaa_data::Id;
use aaa_map::{Ground, Map};
use aaa_render::camera::RtsCamera;
use aaa_render::terrain::world;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::RelativeCursorPosition;

use crate::{Loaded, Sim};
use aaa_sim::State;

/// Width of the disc on screen, in logical pixels.
const DISC: f32 = 260.0;
/// Space between the disc's edge and the diamond's corners.
const INSET: f32 = 12.0;
/// Pixels across the minimap's image, enough for a sharp diamond on a
/// high-density screen.
const PIXELS: usize = 512;

const SHALLOW: [f32; 3] = [0.34, 0.48, 0.68];
const DEEP: [f32; 3] = [0.17, 0.28, 0.5];
/// How deep water is before it takes the deep colour.
const DEEP_AT: f32 = 3.0;

#[derive(Resource)]
pub struct Minimap {
    image: Handle<Image>,
    /// The ground alone, which each frame's markers are drawn over.
    base: Vec<u8>,
    /// The map's width, in world units.
    size: f32,
}

/// The node showing the minimap's image.
#[derive(Component)]
pub struct MinimapView;

/// The game camera sees the map corner-on, with its north-west corner
/// furthest away, so that corner is the diamond's top and north-east its
/// right. Takes a world position to where it falls across the image, 0 to
/// 1 each way, `v` down.
fn to_image(size: f32, [x, y]: [f32; 2]) -> Vec2 {
    let (a, b) = (x / size, y / size);
    Vec2::new(0.5 + (a + b - 1.0) / 2.0, 0.5 + (a - b) / 2.0)
}

/// The world position at a point across the image, which lies off the map
/// outside the diamond.
fn from_image(size: f32, p: Vec2) -> [f32; 2] {
    [(p.x + p.y - 0.5) * size, (p.x - p.y + 0.5) * size]
}

pub fn spawn(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
) {
    let Loaded { game, map } = &mut *loaded;
    let variations = game.terrain_variations();
    let colours: Vec<[f32; 3]> = map
        .terrain
        .names
        .iter()
        .map(|n| {
            variations
                .get(&Id::from(n.as_str()))
                .map_or([0.5; 3], |v| v.color)
        })
        .collect();
    let base = draw_ground(map, &colours);
    let mut image = Image::new_fill(
        Extent3d {
            width: PIXELS as u32,
            height: PIXELS as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.data = Some(base.clone());
    let image = images.add(image);
    commands.insert_resource(Minimap {
        image: image.clone(),
        base,
        size: map.ground.size(),
    });
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            bottom: px(12),
            width: px(DISC),
            height: px(DISC),
            padding: UiRect::all(px(INSET)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(Color::BLACK),
        children![(
            MinimapView,
            RelativeCursorPosition::default(),
            ImageNode::new(image),
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
        )],
    ));
}

/// The ground as the image's pixels: each tile in its terrain's colour, or
/// blue by depth under water, shaded by slope as if lit from the top, and
/// clear outside the diamond.
fn draw_ground(map: &Map, colours: &[[f32; 3]]) -> Vec<u8> {
    let (ground, terrain) = (&map.ground, &map.terrain);
    let size = ground.size();
    let tile = size / terrain.side as f32;
    let mut pixels = vec![0; PIXELS * PIXELS * 4];
    for row in 0..PIXELS {
        for col in 0..PIXELS {
            let centre = (Vec2::new(col as f32, row as f32) + 0.5) / PIXELS as f32;
            let [x, y] = from_image(size, centre);
            if !(0.0..size).contains(&x) || !(0.0..size).contains(&y) {
                continue;
            }
            let height = ground.height_at(x, y);
            let depth = water_depth(ground, x, y, height);
            let rgb = match depth {
                Some(d) => {
                    let t = (d / DEEP_AT).clamp(0.0, 1.0);
                    [0, 1, 2].map(|i| SHALLOW[i] + (DEEP[i] - SHALLOW[i]) * t)
                }
                None => {
                    let (tx, ty) = ((x / tile) as usize, (y / tile) as usize);
                    let kind = terrain.at(tx, ty).terrain as usize;
                    let c = colours.get(kind).copied().unwrap_or([0.5; 3]);
                    // Brighter where the ground rises towards the bottom
                    // corner, so it faces the top one.
                    let d = tile;
                    let rise = ground.height_at(x + d, y - d) - ground.height_at(x - d, y + d);
                    let shade = (1.0 + rise / d * 0.4).clamp(0.6, 1.4);
                    c.map(|c| c * shade)
                }
            };
            let at = (row * PIXELS + col) * 4;
            let [r, g, b, _] = Color::srgb(rgb[0], rgb[1], rgb[2]).to_srgba().to_u8_array();
            pixels[at..at + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
    pixels
}

/// How far below the water's surface the ground is, where there is water.
fn water_depth(ground: &Ground, x: f32, y: f32, height: f32) -> Option<f32> {
    let cell = ground.size() / (ground.side - 1) as f32;
    let (vx, vy) = ((x / cell).round() as usize, (y / cell).round() as usize);
    let i = vy.min(ground.side - 1) * ground.side + vx.min(ground.side - 1);
    let depth = ground.surface[i] - height;
    (ground.water[i] && depth > 0.0).then_some(depth)
}

/// Redraws the markers and the camera's view over the ground.
pub fn update(
    minimap: Res<Minimap>,
    sim: Res<Sim>,
    cameras: Query<(&Camera, &RtsCamera)>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(mut image) = images.get_mut(&minimap.image) else {
        return;
    };
    let Some(pixels) = image.data.as_mut() else {
        return;
    };
    pixels.copy_from_slice(&minimap.base);
    let mut canvas = Canvas(pixels);
    let to_px = |p: [f32; 2]| to_image(minimap.size, p) * PIXELS as f32;
    let colour = |owner: u32| {
        let [r, g, b] = sim.0.sides.player(owner).map_or([1.0; 3], |p| p.rgb);
        let [r, g, b, _] = Color::srgb(r, g, b).to_srgba().to_u8_array();
        [r, g, b]
    };
    for building in &sim.0.buildings {
        if matches!(building.state, State::Destroyed(_)) {
            continue;
        }
        // Like the game, leave out the independents' houses and sheds,
        // but not the cities to be taken from them.
        let independent = sim
            .0
            .sides
            .player(building.owner)
            .is_none_or(|p| p.independent);
        let rgb = match building.state {
            _ if independent && building.zone.is_none() => continue,
            _ if independent => [255; 3],
            State::Disabled => [100; 3],
            _ => colour(building.owner),
        };
        canvas.square(to_px(building.position), 4.0, [0; 3]);
        canvas.square(to_px(building.position), 3.0, rgb);
    }
    for unit in sim.0.units.iter().filter(|u| u.alive()) {
        let owner = sim.0.regiments[unit.regiment].owner;
        canvas.square(to_px(unit.position), 2.0, colour(owner));
    }
    if let Ok((camera, rts)) = cameras.single() {
        // From where the camera is this frame rather than where it was
        // last drawn.
        let transform = GlobalTransform::from(rts.transform());
        let corners = view_corners(camera, &transform, rts.focus.y);
        if let Some(corners) = corners {
            let corners = corners.map(to_px);
            for i in 0..4 {
                canvas.line(corners[i], corners[(i + 1) % 4], [255; 3]);
            }
        }
    }
}

/// Where the corners of the camera's view meet the ground, taken as flat
/// at the height it looks at, clockwise from the top left. Corners looking
/// above the horizon are cut off a long way out.
fn view_corners(
    camera: &Camera,
    transform: &GlobalTransform,
    height: f32,
) -> Option<[[f32; 2]; 4]> {
    const FAR: f32 = 2000.0;
    let size = camera.logical_viewport_size()?;
    let screen = [
        Vec2::ZERO,
        Vec2::new(size.x, 0.0),
        size,
        Vec2::new(0.0, size.y),
    ];
    let mut out = [[0.0; 2]; 4];
    for (corner, out) in screen.into_iter().zip(&mut out) {
        let ray = camera.viewport_to_world(transform, corner).ok()?;
        let t = match ray.direction.y < -1e-4 {
            true => ((height - ray.origin.y) / ray.direction.y).min(FAR),
            false => FAR,
        };
        let p = ray.get_point(t);
        *out = [p.x, -p.z];
    }
    Some(out)
}

/// The image's pixels, to draw on.
struct Canvas<'a>(&'a mut [u8]);

impl Canvas<'_> {
    fn set(&mut self, x: i32, y: i32, [r, g, b]: [u8; 3]) {
        let n = PIXELS as i32;
        if (0..n).contains(&x) && (0..n).contains(&y) {
            let at = (y * n + x) as usize * 4;
            self.0[at..at + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }

    /// A square `half` pixels each way from a centre.
    fn square(&mut self, centre: Vec2, half: f32, rgb: [u8; 3]) {
        let (lo, hi) = ((centre - half).round(), (centre + half).round());
        for y in lo.y as i32..hi.y as i32 {
            for x in lo.x as i32..hi.x as i32 {
                self.set(x, y, rgb);
            }
        }
    }

    /// A line two pixels thick, clipped to the image.
    fn line(&mut self, a: Vec2, b: Vec2, rgb: [u8; 3]) {
        let Some((a, b)) = clip(a, b, PIXELS as f32) else {
            return;
        };
        let steps = (b - a).abs().max_element().ceil().max(1.0) as i32;
        for i in 0..=steps {
            let p = a.lerp(b, i as f32 / steps as f32);
            let (x, y) = (p.x.floor() as i32, p.y.floor() as i32);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                self.set(x + dx, y + dy, rgb);
            }
        }
    }
}

/// The part of a line inside a square from 0 to `n` each way, if any.
fn clip(a: Vec2, b: Vec2, n: f32) -> Option<(Vec2, Vec2)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-d.x, a.x), (d.x, n - a.x), (-d.y, a.y), (d.y, n - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            t0 = t0.max(r);
        } else {
            t1 = t1.min(r);
        }
    }
    (t0 <= t1).then(|| (a + d * t0, a + d * t1))
}

/// Whether the pointer is off the minimap's disc, so clicks go to the map.
pub fn pointer_away(views: Query<&RelativeCursorPosition, With<MinimapView>>) -> bool {
    views.iter().all(|v| match v.normalized {
        // The disc reaches a little past the image on every side.
        Some(p) => p.distance(Vec2::ZERO) > 0.5 * DISC / (DISC - 2.0 * INSET),
        None => true,
    })
}

/// Clicking or dragging on the diamond centres the camera there.
pub fn steer(
    buttons: Res<ButtonInput<MouseButton>>,
    mut dragging: Local<bool>,
    minimap: Res<Minimap>,
    loaded: Res<Loaded>,
    views: Query<&RelativeCursorPosition, With<MinimapView>>,
    mut cameras: Query<&mut RtsCamera>,
) {
    let over = views
        .iter()
        .find_map(|v| v.cursor_over.then_some(v.normalized?));
    // Only a press that starts on the minimap drags across it.
    if buttons.just_pressed(MouseButton::Left) {
        *dragging = over.is_some();
    }
    if !buttons.pressed(MouseButton::Left) {
        *dragging = false;
    }
    let (true, Some(p)) = (*dragging, over) else {
        return;
    };
    // Normalized from the node's centre, -0.5 to 0.5.
    let [x, y] = from_image(minimap.size, p + 0.5);
    if !(0.0..=minimap.size).contains(&x) || !(0.0..=minimap.size).contains(&y) {
        return;
    }
    for mut cam in &mut cameras {
        cam.focus = world(x, y, loaded.map.ground.height_at(x, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners() {
        let at = |p| to_image(10.0, p);
        assert_eq!(at([0.0, 10.0]), Vec2::new(0.5, 0.0));
        assert_eq!(at([10.0, 10.0]), Vec2::new(1.0, 0.5));
        assert_eq!(at([10.0, 0.0]), Vec2::new(0.5, 1.0));
        assert_eq!(at([0.0, 0.0]), Vec2::new(0.0, 0.5));
        assert_eq!(from_image(10.0, Vec2::new(0.75, 0.25)), [5.0, 10.0]);
    }

    #[test]
    fn clipping() {
        let (a, b) = clip(Vec2::new(-10.0, 5.0), Vec2::new(20.0, 5.0), 10.0).unwrap();
        assert_eq!((a, b), (Vec2::new(0.0, 5.0), Vec2::new(10.0, 5.0)));
        assert!(clip(Vec2::new(-10.0, -1.0), Vec2::new(20.0, -1.0), 10.0).is_none());
    }
}
