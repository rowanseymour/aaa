//! The minimap: the map drawn as a diamond, for either interface to place
//! a [view](Minimap::view) of, turned the way the game camera sees it,
//! with units and buildings coloured by how their owner stands toward the
//! player, buildings such as HQs marked by their own icons, and the ground
//! the camera sees outlined in white. Each layer over the ground is
//! redrawn on its own clock, set by the game's settings.
//! Clicking or dragging on it moves the camera there, unless an order is
//! being aimed, which a click on it gives there instead.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::RelativeCursorPosition;
use tgdata::{GameData, Id};
use tgmap::{Map, Tile};
use tgrender::camera::RtsCamera;
use tgrender::space::{map_point, world};

use crate::{Loaded, Sim, TINTS};
use tgsim::{Relation, State};

/// Pixels across the minimap's image, enough for a sharp diamond on a
/// high-density screen.
const PIXELS: usize = 512;

/// How much of the map's side one texel of an icon covers, going by the
/// game's Corps HQ icon on its Berlin map.
const ICON_TEXEL: f32 = 0.0058;

#[derive(Resource)]
pub struct Minimap {
    image: Handle<Image>,
    /// The ground alone.
    base: Vec<u8>,
    /// The ground under the fog of war, which each frame's markers are
    /// drawn over.
    fogged: Vec<u8>,
    /// Where each pixel on the map takes its fog from.
    samples: Vec<Sample>,
    /// The fog `fogged` was drawn under.
    fog: Vec<tgsim::Seen>,
    /// The map's width, in world units.
    size: f32,
    /// The icons buildings are marked with, by template id.
    icons: HashMap<Id, Icon>,
    colours: Colours,
    /// When each layer over the ground was last redrawn, and what it drew:
    /// the fog, whether it was lifted, the supply border's lines and the
    /// things' marks.
    fog_clock: Clock,
    revealed: bool,
    supply_clock: Clock,
    supply: Vec<(Vec2, Vec2)>,
    marks_clock: Clock,
    marks: Marks,
}

/// A layer's redraws, every `period` seconds of the wall clock from the
/// first.
struct Clock {
    period: f32,
    last: Option<f32>,
}

impl Clock {
    /// The shortest period kept, so that a setting of nothing or less
    /// still leaves a blink a first half to show in.
    const SHORTEST: f32 = 1.0 / 60.0;

    fn new(period: f32) -> Self {
        Clock {
            period: period.max(Self::SHORTEST),
            last: None,
        }
    }

    /// Whether the layer is to be redrawn now, counting it as redrawn. The
    /// redraws keep to whole periods from the first, however late a frame
    /// comes, unless one is missed altogether.
    fn due(&mut self, now: f32) -> bool {
        let next = match self.last {
            Some(last) if now - last < self.period => return false,
            Some(last) if now - last < 2.0 * self.period => last + self.period,
            _ => now,
        };
        self.last = Some(next);
        true
    }

    /// Whether it's the first half of the time since the last redraw.
    fn first_half(&self, now: f32) -> bool {
        self.last.is_none_or(|last| now - last < self.period / 2.0)
    }
}

/// A thing's mark: its colour, and the colour it blinks to for the first
/// half of each redraw while the player's own thing is in trouble.
#[derive(Clone, Copy)]
struct Ink {
    rgb: [u8; 3],
    warning: Option<[u8; 3]>,
}

impl Ink {
    fn now(self, warn: bool) -> [u8; 3] {
        match self.warning {
            Some(rgb) if warn => rgb,
            _ => self.rgb,
        }
    }
}

/// The things' marks as last taken: units' dots, with whether each is the
/// player's selected, drawn over the rest; buildings without an icon as
/// squares; and buildings' icons, with whether each is selected.
#[derive(Default)]
struct Marks {
    dots: Vec<(Vec2, bool, Ink)>,
    squares: Vec<(Vec2, Ink)>,
    icons: Vec<(bool, Id, [f32; 2], Ink)>,
}

/// An icon's pixels, RGBA, rows running along the map's y axis.
#[derive(Clone)]
struct Icon {
    width: usize,
    height: usize,
    rgba: Arc<[u8]>,
}

const UNDER_SIEGE: [f32; 3] = [1.0, 0.0, 0.0];
const REQUIRES_SUPPLY: [f32; 3] = [0.0, 1.0, 0.0];
const OWNED_SELECTED: [f32; 3] = [1.0; 3];

/// What things are marked in, buildings and units, by [`Colours::of`].
struct Colours {
    buildings: [[u8; 3]; 4],
    units: [[u8; 3]; 4],
    /// What the player's own things blink in while they're being fought,
    /// and while they're out of the supply they need.
    under_siege: [u8; 3],
    requires_supply: [u8; 3],
    /// What the player's own selected things are marked in.
    selected: [u8; 3],
    /// What the supply border is drawn in.
    border: [u8; 3],
}

impl Colours {
    fn load(game: &GameData) -> Self {
        let rgb = |[r, g, b]: [f32; 3]| {
            let [r, g, b, _] = Color::srgb(r, g, b).to_srgba().to_u8_array();
            [r, g, b]
        };
        let border = rgb(game.supply_border().colour);
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
                under_siege: rgb(m.color_under_siege.unwrap_or(UNDER_SIEGE)),
                requires_supply: rgb(m.color_requires_supply.unwrap_or(REQUIRES_SUPPLY)),
                selected: rgb(m.color_owned_selected.unwrap_or(OWNED_SELECTED)),
                border,
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
                    under_siege: rgb(UNDER_SIEGE),
                    requires_supply: rgb(REQUIRES_SUPPLY),
                    selected: rgb(OWNED_SELECTED),
                    border,
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

/// Draws the map's ground and loads its icons, for either interface to
/// place a [`Minimap::view`] of.
pub fn setup(
    mut commands: Commands,
    loaded: Res<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let Loaded { game, map } = &*loaded;
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
    let periods = game.minimap_periods();
    let minimap = Minimap {
        image: images.add(image),
        fogged: base.clone(),
        samples: samples(map.ground.size()),
        fog: Vec::new(),
        base,
        size: map.ground.size(),
        icons,
        colours: palette,
        fog_clock: Clock::new(periods.fog),
        revealed: false,
        supply_clock: Clock::new(periods.supply),
        supply: Vec::new(),
        marks_clock: Clock::new(periods.objects),
        marks: Marks::default(),
    };
    commands.insert_resource(minimap);
}

fn decode_icon(
    game: &GameData,
    path: &str,
    formats: bevy::image::CompressedImageFormats,
) -> Option<Icon> {
    let image = crate::ui::rgba8(&crate::ui::read_image(game, path, formats)?)?;
    Some(Icon {
        width: image.width() as usize,
        height: image.height() as usize,
        rgba: image.data?.into(),
    })
}

/// The ground as the image's pixels: each tile flat in its terrain's
/// colour, water included, and clear outside the diamond.
fn draw_ground(map: &Map, colours: &[[f32; 3]]) -> Vec<u8> {
    let terrain = &map.terrain;
    let size = map.ground.size();
    let tile = size / terrain.side as f32;
    let mut pixels = vec![0; PIXELS * PIXELS * 4];
    for row in 0..PIXELS {
        for col in 0..PIXELS {
            let centre = (Vec2::new(col as f32, row as f32) + 0.5) / PIXELS as f32;
            let [x, y] = from_image(size, centre);
            if !(0.0..size).contains(&x) || !(0.0..size).contains(&y) {
                continue;
            }
            let (tx, ty) = ((x / tile) as usize, (y / tile) as usize);
            let kind = terrain.at(tx, ty).terrain as usize;
            let [r, g, b] = colours.get(kind).copied().unwrap_or([0.5; 3]);
            let at = (row * PIXELS + col) * 4;
            let [r, g, b, _] = Color::srgb(r, g, b).to_srgba().to_u8_array();
            pixels[at..at + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
    pixels
}

/// A pixel on the map and the tiles its fog blends between: those whose
/// centres are to its south-west and beyond, and how far it lies across
/// and up from the first's centre to the others'.
struct Sample {
    pixel: usize,
    tile: Tile,
    across: f32,
    up: f32,
}

fn samples(size: f32) -> Vec<Sample> {
    let tile = tgck::map::CELL_SIZE;
    let mut samples = Vec::new();
    for row in 0..PIXELS {
        for col in 0..PIXELS {
            let centre = (Vec2::new(col as f32, row as f32) + 0.5) / PIXELS as f32;
            let [x, y] = from_image(size, centre);
            if !(0.0..size).contains(&x) || !(0.0..size).contains(&y) {
                continue;
            }
            let (fx, fy) = (x / tile - 0.5, y / tile - 0.5);
            let (x0, y0) = (fx.floor(), fy.floor());
            samples.push(Sample {
                pixel: (row * PIXELS + col) * 4,
                tile: Tile::new(x0 as i32, y0 as i32),
                across: fx - x0,
                up: fy - y0,
            });
        }
    }
    samples
}

/// How bright the minimap's ground is under the fog, of its full
/// brightness, with a shade of its own for explored ground.
fn brightness(seen: tgsim::Seen) -> f32 {
    const EXPLORED: f32 = 0.7;
    match seen {
        tgsim::Seen::Explored => EXPLORED,
        seen => crate::fog::brightness(seen),
    }
}

/// Darkens the ground into `fogged` as the fog of war darkens it, blended
/// between tiles, once the fog has changed.
fn fog_ground(minimap: &mut Minimap, fog: &crate::fog::Fog) {
    let (tiles, side) = fog.tiles();
    if minimap.fog == tiles {
        return;
    }
    minimap.fog = tiles.to_vec();
    minimap.fogged.copy_from_slice(&minimap.base);
    if tiles.is_empty() {
        return;
    }
    let light: Vec<f32> = tiles.iter().map(|&s| brightness(s)).collect();
    let off_map = brightness(tgsim::Seen::Unexplored);
    let at = |t: Tile| t.index(side).map_or(off_map, |i| light[i]);
    for s in &minimap.samples {
        let t = s.tile;
        let south = at(t) * (1.0 - s.across) + at(t.offset(1, 0)) * s.across;
        let north = at(t.offset(0, 1)) * (1.0 - s.across) + at(t.offset(1, 1)) * s.across;
        let l = south * (1.0 - s.up) + north * s.up;
        if l < 1.0 {
            for c in &mut minimap.fogged[s.pixel..s.pixel + 3] {
                *c = (*c as f32 * l) as u8;
            }
        }
    }
}

/// Redraws the minimap: each layer over the ground on its own clock, and
/// the camera's view every frame. While the player's side has its
/// minimap's fog lifted, as by Radar Scan, the ground has no fog and
/// everything is marked as it is.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut minimap: ResMut<Minimap>,
    fog: Res<crate::fog::Fog>,
    sim: Res<Sim>,
    local: Res<crate::Local>,
    (selected, selected_building): (Res<crate::Selected>, Res<crate::SelectedBuilding>),
    outline: Res<crate::supply::Outline>,
    time: Res<Time<Real>>,
    cameras: Query<(&Camera, &RtsCamera)>,
    mut images: ResMut<Assets<Image>>,
) {
    let now = time.elapsed_secs();
    let minimap = &mut *minimap;
    if minimap.fog_clock.due(now) {
        minimap.revealed = local.0.is_some_and(|p| sim.0.revealed(p));
        fog_ground(minimap, &fog);
    }
    if minimap.supply_clock.due(now) {
        minimap.supply = supply_lines(minimap.size, &outline.0);
    }
    if minimap.marks_clock.due(now) {
        minimap.marks = take_marks(
            minimap,
            &sim.0,
            &fog,
            local.0,
            &selected,
            &selected_building,
        );
    }
    let Some(mut image) = images.get_mut(&minimap.image) else {
        return;
    };
    let Some(pixels) = image.data.as_mut() else {
        return;
    };
    match minimap.revealed {
        true => pixels.copy_from_slice(&minimap.base),
        false => pixels.copy_from_slice(&minimap.fogged),
    }
    let mut canvas = Canvas(pixels);
    let border = minimap.colours.border;
    for &(from, to) in &minimap.supply {
        canvas.line(from, to, border);
    }
    let warn = minimap.marks_clock.first_half(now);
    draw_marks(&mut canvas, minimap, warn);
    if let Ok((camera, rts)) = cameras.single() {
        // From where the camera is this frame rather than where it was
        // last drawn.
        let transform = GlobalTransform::from(rts.transform());
        let corners = view_corners(camera, &transform, rts.focus.y);
        if let Some(corners) = corners {
            let to_px = |p: [f32; 2]| to_image(minimap.size, p) * PIXELS as f32;
            let corners = corners.map(to_px);
            for i in 0..4 {
                canvas.line(corners[i], corners[(i + 1) % 4], [255; 3]);
            }
        }
    }
}

/// The supply border's pieces across the image, but for where a zone just
/// runs off the map.
fn supply_lines(size: f32, paths: &[Vec<[f32; 2]>]) -> Vec<(Vec2, Vec2)> {
    let to_px = |p: [f32; 2]| to_image(size, p) * PIXELS as f32;
    let inside = tgck::map::CELL_SIZE / 2.0..=size - tgck::map::CELL_SIZE / 2.0;
    let on_map = |[x, y]: [f32; 2]| inside.contains(&x) && inside.contains(&y);
    let mut lines = Vec::new();
    for path in paths {
        for (i, &from) in path.iter().enumerate() {
            let to = path[(i + 1) % path.len()];
            if on_map(from) && on_map(to) {
                lines.push((to_px(from), to_px(to)));
            }
        }
    }
    lines
}

/// Where each thing the player can see is marked, and in what.
fn take_marks(
    minimap: &Minimap,
    sim: &tgsim::World,
    fog: &crate::fog::Fog,
    local: Option<u32>,
    selected: &crate::Selected,
    selected_building: &crate::SelectedBuilding,
) -> Marks {
    let revealed = minimap.revealed;
    let colours = &minimap.colours;
    let to_px = |p: [f32; 2]| to_image(minimap.size, p) * PIXELS as f32;
    // With nobody to play, there's no one to stand toward, so each side
    // keeps its own colour.
    let colour = |palette: &[[u8; 3]; 4], owner: u32| match local {
        Some(p) => palette[Colours::of(sim.sides.relation(p, owner))],
        None => {
            let [r, g, b] = sim.sides.player(owner).map_or([1.0; 3], |p| p.rgb);
            let [r, g, b, _] = Color::srgb(r, g, b).to_srgba().to_u8_array();
            [r, g, b]
        }
    };
    // Only the player's own things in trouble blink.
    let mine = |owner: u32| local == Some(owner);
    let neutral = colours.buildings[Colours::of(Relation::Neutral)];
    let mut marks = Marks::default();
    for (b, building) in sim.buildings.iter().enumerate() {
        // Marked as last seen, gone or not.
        let shown = match fog.building(b) {
            _ if revealed => !building.gone(),
            crate::fog::Look::Shown => !building.gone(),
            crate::fog::Look::Remembered => true,
            crate::fog::Look::Hidden => false,
        };
        if !shown {
            continue;
        }
        // Like the game, leave out the independents' houses and sheds,
        // but not the cities to be taken from them.
        let independent = sim
            .sides
            .player(building.owner)
            .is_none_or(|p| p.independent);
        let own = mine(building.owner);
        let picked = own && selected_building.0 == Some(b);
        let rgb = match building.state {
            _ if independent && building.zone.is_none() => continue,
            _ if independent => neutral,
            _ if picked => colours.selected,
            State::Disabled => [100; 3],
            _ => colour(&colours.buildings, building.owner),
        };
        let warning = match () {
            _ if !own || independent => None,
            _ if building.besieged.is_some() => Some(colours.under_siege),
            _ if sim.out_of_supply(b) => Some(colours.requires_supply),
            _ => None,
        };
        let ink = Ink { rgb, warning };
        match minimap.icons.contains_key(&building.template) {
            true => (marks.icons).push((picked, building.template.clone(), building.position, ink)),
            false => marks.squares.push((to_px(building.position), ink)),
        }
    }
    marks.icons.sort_by_key(|&(picked, ..)| picked);
    let shown =
        |(u, unit): &(usize, &tgsim::Unit)| unit.alive() && (revealed || fog.shows_unit(*u));
    for (u, unit) in sim.units.iter().enumerate().filter(shown) {
        let organization = &sim.organizations[unit.organization];
        let own = mine(organization.owner);
        // Its regiment's engagement, or its own: engaged or under siege.
        let states = sim.unit_states(u);
        let fighting = !organization.engaged.is_empty()
            || states.contains(&"mind_engaged")
            || unit.besieged.is_some();
        let picked = own && selected.0.contains(&unit.organization);
        let rgb = match picked {
            true => colours.selected,
            false => colour(&colours.units, organization.owner),
        };
        let warning = (own && fighting).then_some(colours.under_siege);
        marks
            .dots
            .push((to_px(unit.position), picked, Ink { rgb, warning }));
    }
    marks
}

/// Draws the marks from the lowest rank up: units' dots edged in black,
/// then buildings, then icons, then the player's selected units over all.
/// A selected unit blinking for trouble is drawn among the rest.
fn draw_marks(canvas: &mut Canvas, minimap: &Minimap, warn: bool) {
    let marks = &minimap.marks;
    let on_top =
        |&&(_, picked, ink): &&(Vec2, bool, Ink)| picked && !(warn && ink.warning.is_some());
    let (top, under): (Vec<_>, Vec<_>) = marks.dots.iter().partition(on_top);
    let inked = |dots: Vec<&(Vec2, bool, Ink)>| -> Vec<(Vec2, [u8; 3])> {
        dots.into_iter()
            .map(|&(at, _, ink)| (at, ink.now(warn)))
            .collect()
    };
    canvas.dots(&inked(under));
    for &(at, ink) in &marks.squares {
        canvas.square(at, 4.0, [0; 3]);
        canvas.square(at, 3.0, ink.now(warn));
    }
    for (_, template, at, ink) in &marks.icons {
        if let Some(icon) = minimap.icons.get(template) {
            canvas.icon(minimap.size, icon, *at, ink.now(warn));
        }
    }
    canvas.dots(&inked(top));
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
    let Rect { min, max } = camera.logical_viewport_rect()?;
    let screen = [min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)];
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
    /// [`ICON_TEXEL`] of the map's side apart along the map's axes. In pure
    /// white it's a white silhouette, its dark outlines included.
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
                let texel = match rgb {
                    [255, 255, 255] => rgb,
                    _ => [0, 1, 2].map(|i| ([r, g, b][i] * rgb[i] as u32 / 255) as u8),
                };
                if a > 0 {
                    self.set(col, row, texel);
                }
            }
        }
    }

    /// Units' dots, each edged in black where it doesn't cover another's
    /// dot.
    fn dots(&mut self, dots: &[(Vec2, [u8; 3])]) {
        for &(at, _) in dots {
            self.square(at, 3.0, [0; 3]);
        }
        for &(at, rgb) in dots {
            self.square(at, 2.0, rgb);
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
    /// The map drawn as it's shown, for a node to show it in.
    pub fn view(&self) -> impl Bundle {
        (
            MinimapView,
            crate::ui::OfMap,
            RelativeCursorPosition::default(),
            ImageNode::new(self.image.clone()),
        )
    }

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
/// order or special operation is aimed for the click to give.
pub fn steer(
    buttons: Res<ButtonInput<MouseButton>>,
    mut dragging: Local<bool>,
    minimap: Res<Minimap>,
    loaded: Res<Loaded>,
    (page, armed): (Res<crate::actions::Page>, Res<crate::special_ops::Armed>),
    views: Query<&RelativeCursorPosition, With<MinimapView>>,
    mut cameras: Query<&mut RtsCamera>,
) {
    let over = over(&views);
    // Only a press that starts on the minimap drags across it.
    if buttons.just_pressed(MouseButton::Left) {
        *dragging = on_disc(over).is_some() && !crate::actions::aiming(page) && armed.0.is_none();
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
        cam.focus = world(x, y, loaded.map.ground.height_at([x, y]));
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
    fn white_icons_are_flat() {
        // A white texel and a black outline one, both drawn white.
        let icon = Icon {
            width: 2,
            height: 1,
            rgba: Arc::from([255, 255, 255, 255, 0, 0, 0, 255].as_slice()),
        };
        let mut pixels = vec![9; PIXELS * PIXELS * 4];
        let size = 1.0 / ICON_TEXEL;
        let centre = [size / 2.0; 2];
        Canvas(&mut pixels).icon(size, &icon, centre, [255; 3]);
        for dx in [-0.5, 0.5] {
            let p = to_image(size, [centre[0] + dx, centre[1]]) * PIXELS as f32;
            let i = (p.y as usize * PIXELS + p.x as usize) * 4;
            assert_eq!(pixels[i..i + 3], [255; 3]);
        }
    }

    #[test]
    fn layers_redraw_on_their_own_clocks() {
        let mut clock = Clock::new(1.0);
        assert!(clock.due(0.25));
        assert!(!clock.due(0.5));
        assert!(clock.first_half(0.5));
        assert!(!clock.first_half(1.0));
        assert!(!clock.due(1.2));
        assert!(clock.due(1.3));
        assert!(clock.first_half(1.7));
        assert!(!clock.first_half(1.8));
        assert!(clock.due(5.0));
        assert!(!clock.due(5.9));
    }

    #[test]
    fn a_period_of_nothing_still_blinks() {
        let mut clock = Clock::new(0.0);
        assert!(clock.due(1.0));
        assert!(clock.first_half(1.0));
    }

    #[test]
    fn marks_blink_to_their_warning() {
        let calm = Ink {
            rgb: [1; 3],
            warning: None,
        };
        let fought = Ink {
            rgb: [1; 3],
            warning: Some([2; 3]),
        };
        assert_eq!(calm.now(true), [1; 3]);
        assert_eq!(fought.now(true), [2; 3]);
        assert_eq!(fought.now(false), [1; 3]);
    }

    #[test]
    fn clipping() {
        let (a, b) = clip(Vec2::new(-10.0, 5.0), Vec2::new(20.0, 5.0), 10.0).unwrap();
        assert_eq!((a, b), (Vec2::new(0.0, 5.0), Vec2::new(10.0, 5.0)));
        assert!(clip(Vec2::new(-10.0, -1.0), Vec2::new(20.0, -1.0), 10.0).is_none());
    }
}
