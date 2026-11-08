//! The minimap: the map drawn as a diamond on a black disc in the corner,
//! turned the way the game camera sees it, with units and buildings
//! coloured by how their owner stands toward the player, buildings such as
//! HQs marked by their own icons, and the ground the camera sees outlined
//! in white.
//! Clicking or dragging on it moves the camera there, unless an order is
//! being aimed, which a click on it gives there instead.

use std::collections::HashMap;
use std::sync::Arc;

use aaa_data::{GameData, Id};
use aaa_map::{Ground, Map};
use aaa_render::camera::RtsCamera;
use aaa_render::terrain::{map_point, world};
use aaa_render::textures;
use bevy::asset::RenderAssetUsages;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::RelativeCursorPosition;

use crate::{Loaded, Sim, TINTS};
use aaa_sim::{Relation, State};

/// Width of the disc on screen, in logical pixels, which the diamond's
/// corners touch.
pub const DISC: f32 = 236.0;
/// Pixels across the minimap's image, enough for a sharp diamond on a
/// high-density screen.
const PIXELS: usize = 512;

const SHALLOW: [f32; 3] = [0.34, 0.48, 0.68];
const DEEP: [f32; 3] = [0.17, 0.28, 0.5];
/// How deep water is before it takes the deep colour.
const DEEP_AT: f32 = 3.0;
/// How much of the map's side one texel of an icon covers, going by the
/// game's Corps HQ icon on its Berlin map.
const ICON_TEXEL: f32 = 0.0058;

#[derive(Resource)]
pub struct Minimap {
    image: Handle<Image>,
    /// The ground alone, which each frame's markers are drawn over.
    base: Vec<u8>,
    /// The map's width, in world units.
    size: f32,
    /// The icons buildings are marked with, by template id.
    icons: HashMap<Id, Icon>,
    colours: Colours,
}

/// An icon's pixels, RGBA, rows running along the map's y axis.
#[derive(Clone)]
struct Icon {
    width: usize,
    height: usize,
    rgba: Arc<[u8]>,
}

/// What things are marked in, buildings and units, by [`Colours::of`].
struct Colours {
    buildings: [[u8; 3]; 4],
    units: [[u8; 3]; 4],
}

impl Colours {
    fn load(game: &mut GameData) -> Self {
        let rgb = |[r, g, b]: [f32; 3]| {
            let [r, g, b, _] = Color::srgb(r, g, b).to_srgba().to_u8_array();
            [r, g, b]
        };
        match game.minimap() {
            Some(m) => Colours {
                buildings: [
                    m.color_owned_static,
                    m.color_allied_static,
                    m.color_enemy_static,
                    m.color_neutral_static,
                ]
                .map(rgb),
                units: [
                    m.color_owned_mobile,
                    m.color_allied_mobile,
                    m.color_enemy_mobile,
                    m.color_neutral_mobile,
                ]
                .map(rgb),
            },
            None => {
                warn!("no minimap colours; using the marks' tints");
                let tints = [TINTS[0], TINTS[1], TINTS[2], Color::WHITE].map(|c| {
                    let [r, g, b, _] = c.to_srgba().to_f32_array();
                    rgb([r, g, b])
                });
                Colours {
                    buildings: tints,
                    units: tints,
                }
            }
        }
    }

    /// Which of the colours suits how `relation` stands.
    fn of(relation: Relation) -> usize {
        match relation {
            Relation::Itself => 0,
            Relation::Ally => 1,
            Relation::Enemy => 2,
            Relation::Neutral => 3,
        }
    }
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
    formats: Res<CompressedImageFormatSupport>,
) {
    let Loaded { game, map } = &mut *loaded;
    let palette = Colours::load(game);
    let mut by_path: HashMap<String, Option<Icon>> = HashMap::new();
    let mut icons = HashMap::new();
    for (template, path) in game.minimap_icons() {
        let icon = by_path
            .entry(path.clone())
            .or_insert_with(|| decode_icon(game, &path, formats.0));
        if let Some(icon) = icon {
            icons.insert(template, icon.clone());
        }
    }
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
        icons,
        colours: palette,
    });
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(crate::MARGIN),
            bottom: px(crate::MARGIN),
            width: px(DISC),
            height: px(DISC),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(crate::CHROME),
        // Over the banners, which stack by where they are on screen.
        GlobalZIndex(1),
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

fn decode_icon(
    game: &mut GameData,
    path: &str,
    formats: bevy::image::CompressedImageFormats,
) -> Option<Icon> {
    let image = textures::decode(&game.read(path)?, path, formats, true)?;
    let image = match image.texture_descriptor.format {
        TextureFormat::Rgba8UnormSrgb => image,
        _ => image.convert(TextureFormat::Rgba8UnormSrgb)?,
    };
    Some(Icon {
        width: image.width() as usize,
        height: image.height() as usize,
        rgba: image.data?.into(),
    })
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
    local: Res<crate::Local>,
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
    // With nobody to play, there's no one to stand toward, so each side
    // keeps its own colour.
    let colour = |palette: &[[u8; 3]; 4], owner: u32| match local.0 {
        Some(p) => palette[Colours::of(sim.0.sides.relation(p, owner))],
        None => {
            let [r, g, b] = sim.0.sides.player(owner).map_or([1.0; 3], |p| p.rgb);
            let [r, g, b, _] = Color::srgb(r, g, b).to_srgba().to_u8_array();
            [r, g, b]
        }
    };
    let neutral = minimap.colours.buildings[Colours::of(Relation::Neutral)];
    let mut icons = Vec::new();
    for building in &sim.0.buildings {
        if building.gone() {
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
            _ if independent => neutral,
            State::Disabled => [100; 3],
            _ => colour(&minimap.colours.buildings, building.owner),
        };
        // Drawn last, over the units.
        if let Some(icon) = minimap.icons.get(&Id::from(building.template.as_str())) {
            icons.push((icon, building.position, rgb));
            continue;
        }
        canvas.square(to_px(building.position), 4.0, [0; 3]);
        canvas.square(to_px(building.position), 3.0, rgb);
    }
    for unit in sim.0.units.iter().filter(|u| u.alive()) {
        let owner = sim.0.regiments[unit.regiment].owner;
        canvas.square(
            to_px(unit.position),
            2.0,
            colour(&minimap.colours.units, owner),
        );
    }
    for (icon, at, rgb) in icons {
        canvas.icon(minimap.size, icon, at, rgb);
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
        *out = map_point(p);
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

    /// An icon tinted by `rgb`, centred on a world position, its texels
    /// [`ICON_TEXEL`] of the map's side apart along the map's axes.
    fn icon(&mut self, size: f32, icon: &Icon, [x, y]: [f32; 2], rgb: [u8; 3]) {
        let texel = size * ICON_TEXEL;
        let (w, h) = (icon.width as f32, icon.height as f32);
        let (hw, hh) = (w * texel / 2.0, h * texel / 2.0);
        let corners = [[-hw, -hh], [hw, -hh], [hw, hh], [-hw, hh]]
            .map(|[dx, dy]| to_image(size, [x + dx, y + dy]) * PIXELS as f32);
        let lo = corners.iter().fold(Vec2::MAX, |a, &c| a.min(c)).floor();
        let hi = corners.iter().fold(Vec2::MIN, |a, &c| a.max(c)).ceil();
        for row in lo.y as i32..hi.y as i32 {
            for col in lo.x as i32..hi.x as i32 {
                let centre = (Vec2::new(col as f32, row as f32) + 0.5) / PIXELS as f32;
                let [px, py] = from_image(size, centre);
                let u = ((px - x) / texel + w / 2.0).floor();
                let v = ((py - y) / texel + h / 2.0).floor();
                if !(0.0..w).contains(&u) || !(0.0..h).contains(&v) {
                    continue;
                }
                let at = (v as usize * icon.width + u as usize) * 4;
                let [r, g, b, a] = [0, 1, 2, 3].map(|i| icon.rgba[at + i] as u32);
                let tinted = [r, g, b]
                    .into_iter()
                    .zip(rgb)
                    .map(|(c, t)| (c * t as u32 / 255) as u8);
                self.blend(col, row, tinted, a as u8);
            }
        }
    }

    /// Lays a colour over a pixel with the given opacity.
    fn blend(&mut self, x: i32, y: i32, rgb: impl Iterator<Item = u8>, alpha: u8) {
        let n = PIXELS as i32;
        if alpha == 0 || !(0..n).contains(&x) || !(0..n).contains(&y) {
            return;
        }
        let at = (y * n + x) as usize * 4;
        let a = alpha as u32;
        for (i, c) in rgb.enumerate() {
            let under = self.0[at + i] as u32;
            self.0[at + i] = ((c as u32 * a + under * (255 - a)) / 255) as u8;
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
    on_disc(over(&views)).is_none()
}

/// Where the pointer is across the minimap's node, -0.5 to 0.5 from its
/// centre each way.
fn over(views: &Query<&RelativeCursorPosition, With<MinimapView>>) -> Option<Vec2> {
    views
        .iter()
        .find_map(|v| v.cursor_over.then_some(v.normalized?))
}

/// The point across the node, if it's on the disc rather than in the
/// node's corners.
fn on_disc(over: Option<Vec2>) -> Option<Vec2> {
    over.filter(|p| p.length() <= 0.5)
}

/// The world position at a point across the minimap's node, if it's on the
/// diamond.
fn on_map(size: f32, p: Vec2) -> Option<[f32; 2]> {
    let [x, y] = from_image(size, p + 0.5);
    let on = |v: f32| (0.0..=size).contains(&v);
    (on(x) && on(y)).then_some([x, y])
}

/// Where the pointer is on the minimap.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Spot {
    /// Off the disc, so on whatever's behind it.
    Off,
    /// On the disc, but off the diamond the map is drawn in.
    OffMap,
    /// On the map, at this world position.
    At([f32; 2]),
}

fn spot(size: f32, over: Option<Vec2>) -> Spot {
    match on_disc(over) {
        None => Spot::Off,
        Some(p) => on_map(size, p).map_or(Spot::OffMap, Spot::At),
    }
}

impl Minimap {
    pub fn spot(&self, views: &Query<&RelativeCursorPosition, With<MinimapView>>) -> Spot {
        spot(self.size, over(views))
    }

    /// How far from a click on the minimap a unit may be and still be
    /// clicked, in world units: a little past the edge of its mark.
    pub fn reach(&self) -> f32 {
        const PIXELS_AWAY: f32 = 6.0;
        // The diamond's diagonals span the image, so its sides are the
        // image's width over root two.
        PIXELS_AWAY * self.size * std::f32::consts::SQRT_2 / PIXELS as f32
    }
}

/// Clicking or dragging on the diamond centres the camera there, while no
/// order is aimed for the click to give.
pub fn steer(
    buttons: Res<ButtonInput<MouseButton>>,
    mut dragging: Local<bool>,
    minimap: Res<Minimap>,
    loaded: Res<Loaded>,
    page: Res<crate::actions::Page>,
    views: Query<&RelativeCursorPosition, With<MinimapView>>,
    mut cameras: Query<&mut RtsCamera>,
) {
    let over = over(&views);
    // Only a press that starts on the minimap drags across it.
    if buttons.just_pressed(MouseButton::Left) {
        *dragging = over.is_some() && !crate::actions::aiming(page);
    }
    if !buttons.pressed(MouseButton::Left) {
        *dragging = false;
    }
    let (true, Some(p)) = (*dragging, over) else {
        return;
    };
    let Some([x, y]) = on_map(minimap.size, p) else {
        return;
    };
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
    fn spots() {
        let at = |x, y| spot(10.0, Some(Vec2::new(x, y)));
        assert_eq!(at(0.0, 0.0), Spot::At([5.0, 5.0]));
        assert_eq!(at(0.25, -0.25), Spot::At([5.0, 10.0]));
        // The disc's edge between two of the diamond's corners.
        assert_eq!(at(0.35, 0.35), Spot::OffMap);
        // The node's corner, outside the disc.
        assert_eq!(at(0.45, 0.45), Spot::Off);
        assert_eq!(spot(10.0, None), Spot::Off);
    }

    #[test]
    fn icons_lie_along_the_map() {
        // Two texels, white then black, along the map's x axis.
        let icon = Icon {
            width: 2,
            height: 1,
            rgba: Arc::from([255, 255, 255, 255, 0, 0, 0, 255].as_slice()),
        };
        let mut pixels = vec![9; PIXELS * PIXELS * 4];
        let size = 1.0 / ICON_TEXEL;
        let centre = [size / 2.0; 2];
        Canvas(&mut pixels).icon(size, &icon, centre, [0, 255, 0]);
        let at = |[x, y]: [f32; 2]| {
            let p = to_image(size, [x, y]) * PIXELS as f32;
            let i = (p.y as usize * PIXELS + p.x as usize) * 4;
            pixels[i..i + 3].to_vec()
        };
        // Tinted where white, black where black, and untouched off it.
        assert_eq!(at([centre[0] - 0.5, centre[1]]), [0, 255, 0]);
        assert_eq!(at([centre[0] + 0.5, centre[1]]), [0, 0, 0]);
        assert_eq!(at([centre[0] - 0.5, centre[1] + 1.0]), [9; 3]);
    }

    #[test]
    fn clipping() {
        let (a, b) = clip(Vec2::new(-10.0, 5.0), Vec2::new(20.0, 5.0), 10.0).unwrap();
        assert_eq!((a, b), (Vec2::new(0.0, 5.0), Vec2::new(10.0, 5.0)));
        assert!(clip(Vec2::new(-10.0, -1.0), Vec2::new(20.0, -1.0), 10.0).is_none());
    }
}
