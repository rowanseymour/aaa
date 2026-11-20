//! The original's object display: what's selected, drawn from the layout
//! of the display for its kind (`SimpleActorDisplay`, `CompanyDisplay` or
//! `GroupDisplay`, within `ObjectDisplay`) as
//! [`Showing`](crate::object_display::Showing) says. Its parts carry the
//! same markers as ours, such as [`Fill`] and [`Says`], so the systems
//! that keep ours up to date keep these too; only the drawing is its own.

use std::collections::HashMap;

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::Font as Face;
use bevy::ui_widgets::Button;
use tgdata::Id;
use tgdata::widgets::{RESTING, Rect, SCREEN, ViewElement, Widget};
use tgrender::overlay::BannerImages;
use tgrender::portrait::Stage;
use tgsim::{Target, World};

use super::Pictures;
use super::lettering::{Letters, Measured, PLAIN, Sliced};
use crate::banners::Badges;
use crate::object_display::{
    self as shown, Actor, Armed, Defends, Detail, EfficiencyTarget, Fill, Flown, Looks, Mode, Pick,
    Place, Queued, Says, Shield, Showing, Status, Strikes, UnitPicture,
};
use crate::ui::{self, Hint, Shade};
use crate::{Drawn, Leaders, Loaded, Local, Sim};

/// The displays, one for each kind of thing shown.
const SIMPLE: &str = "SimpleActorDisplay";
const COMPANY: &str = "CompanyDisplay";
const GROUP: &str = "GroupDisplay";

/// The formation buttons' panel, the button it repeats, and the part of
/// that the formation's picture is drawn in.
const FORMATIONS: &str = "FormationButtons";
const FORMATION_BUTTON: &str = "FormationButton";
const FORMATION_DECAL: &str = "FormationButtonDecal";

/// The layouts the display is drawn from, with their pictures.
#[derive(Resource)]
pub struct Layout {
    /// Where the display is on the layouts' screen.
    at: Rect,
    displays: HashMap<&'static str, Widget>,
    images: HashMap<String, Handle<Image>>,
    /// Each strip of an image repeated between two ends, by its ends' and
    /// middle's paths, made into one image to slice, with how wide each
    /// end is.
    strips: HashMap<(String, String), (Handle<Image>, f32)>,
}

impl Layout {
    /// Reads the displays within `display`, which is `at` on the screen,
    /// and the pictures and fonts they're drawn with.
    pub(super) fn read(
        display: &Widget,
        at: Rect,
        pictures: &mut Pictures,
        (letters, faces): (&mut Letters, &mut Assets<Face>),
    ) -> Self {
        let mut layout = Layout {
            at,
            displays: HashMap::new(),
            images: HashMap::new(),
            strips: HashMap::new(),
        };
        for name in [SIMPLE, COMPANY, GROUP] {
            let Some(widget) = display.children.iter().find(|c| c.name == name) else {
                continue;
            };
            layout.load(widget, pictures, letters, faces);
            layout.displays.insert(name, widget.clone());
        }
        layout
    }

    fn load(
        &mut self,
        widget: &Widget,
        pictures: &mut Pictures,
        letters: &mut Letters,
        faces: &mut Assets<Face>,
    ) {
        let elements = widget.view.iter().chain(&widget.parts);
        for element in elements {
            self.element(element, pictures, letters, faces);
        }
        for child in &widget.children {
            self.load(child, pictures, letters, faces);
        }
    }

    fn element(
        &mut self,
        element: &ViewElement,
        pictures: &mut Pictures,
        letters: &mut Letters,
        faces: &mut Assets<Face>,
    ) {
        if let Some(path) = &element.texture
            && let Some(image) = pictures.get(path)
        {
            self.images.insert(path.clone(), image);
        }
        if let Some(ends) = strip(element) {
            let key = (ends.0.clone(), ends.1.clone());
            if !self.strips.contains_key(&key)
                && let Some(made) = joined(pictures, &ends.0, &ends.1)
            {
                self.strips.insert(key, made);
            }
        }
        if element.kind.as_deref() == Some("LABEL") {
            let font = element.font.as_deref().unwrap_or(PLAIN);
            letters.read(pictures.game, faces, font);
        }
        for child in &element.children {
            self.element(child, pictures, letters, faces);
        }
    }
}

/// The ends' and middle's images of a strip repeated between two ends, if
/// `element` is one.
fn strip(element: &ViewElement) -> Option<(String, String)> {
    let directional = element.kind.as_deref() == Some("DECAL_TILED_DIRECTIONAL");
    let across = (element.value("direction")).is_none_or(|d| d.starts_with("horizont"));
    (directional && across).then_some((
        element.image("ends_texture")?,
        element.image("center_texture")?,
    ))
}

/// The image of a strip, for slicing into three: the left half of the
/// ends' image, the middle's, then the right half of the ends'. Also how
/// wide each end is.
fn joined(pictures: &mut Pictures, ends: &str, middle: &str) -> Option<(Handle<Image>, f32)> {
    let (ends, middle) = (pictures.rgba(ends)?, pictures.rgba(middle)?);
    let (half, height) = (ends.0.x / 2, ends.0.y);
    if half == 0 || height == 0 {
        return None;
    }
    let width = 2 * half + middle.0.x;
    let mut out = vec![0u8; (width * height * 4) as usize];
    // Copies columns `from..from + w` of `image` to those from `to`.
    let mut copy = |(size, data): &(UVec2, Vec<u8>), from: u32, w: u32, to: u32| {
        for y in 0..height {
            for x in 0..w {
                let (sx, sy) = ((from + x).min(size.x - 1), y.min(size.y - 1));
                let src = ((sy * size.x + sx) * 4) as usize;
                let dst = ((y * width + to + x) * 4) as usize;
                if let Some(px) = data.get(src..src + 4) {
                    out[dst..dst + 4].copy_from_slice(px);
                }
            }
        }
    };
    copy(&ends, 0, half, 0);
    copy(&middle, 0, middle.0.x, half);
    copy(&ends, ends.0.x - half, half, half + middle.0.x);
    let image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        out,
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    Some((pictures.images.add(image), half as f32))
}

/// The whole of the layouts' screen.
const SCREEN_RECT: Rect = Rect {
    left: 0.0,
    top: 0.0,
    right: SCREEN[0],
    bottom: SCREEN[1],
};

/// `at` moved `x` across and `y` down.
fn moved(at: Rect, x: f32, y: f32) -> Rect {
    Rect {
        left: at.left + x,
        top: at.top + y,
        right: at.right + x,
        bottom: at.bottom + y,
    }
}

/// What the display's parts are drawn in, over the whole layouts' screen.
#[derive(Component)]
pub struct Holder;

impl Holder {
    pub(super) fn bundle() -> impl Bundle {
        (
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            Holder,
            Pickable::IGNORE,
        )
    }
}

/// A status bar, filled as its [`Fill`] says, in the colour its layout
/// gives for how full it is: `start` empty, `mid` at `mid_at` and `end`
/// full, blended between them.
#[derive(Component, Clone, Copy)]
pub struct Shaded {
    start: [f32; 3],
    mid: [f32; 3],
    end: [f32; 3],
    mid_at: f32,
    across: bool,
}

impl Shaded {
    fn of(element: &ViewElement) -> Self {
        let number = |key| {
            element
                .value(key)
                .and_then(|v| v.trim().parse::<f32>().ok())
        };
        Shaded {
            start: element.colour_of("start_color").unwrap_or([1.0; 3]),
            mid: element.colour_of("mid_color").unwrap_or([0.5; 3]),
            end: element.colour_of("end_color").unwrap_or([0.0; 3]),
            mid_at: number("mid_percentage").unwrap_or(0.5),
            across: element
                .value("horizontal")
                .is_none_or(|h| !h.eq_ignore_ascii_case("false")),
        }
    }

    fn colour(&self, share: f32) -> Color {
        let blend = |a: [f32; 3], b: [f32; 3], t: f32| {
            let t = t.clamp(0.0, 1.0);
            Color::srgb(
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            )
        };
        match share < self.mid_at {
            true => blend(self.start, self.mid, share / self.mid_at),
            false if self.mid_at >= 1.0 => blend(self.mid, self.end, 1.0),
            false => blend(
                self.mid,
                self.end,
                (share - self.mid_at) / (1.0 - self.mid_at),
            ),
        }
    }
}

/// The part of a [`Shaded`] bar that's filled.
#[derive(Component)]
pub struct Filled;

/// The economy line: what its [`Status`]'s thing counts for in its side's
/// economy, each resource's glyph and figure, as last drawn.
#[derive(Component)]
pub struct Economy {
    of: Status,
    font: String,
    shown: Vec<(Id, String)>,
}

/// Where the portrait shows.
#[derive(Component)]
pub struct Portrait;

/// Where each part drawn of a widget was put, by its path from the
/// widget, e.g. `StatusBar` or `Portrait/Model`; the widget's own node is
/// at the empty path.
type Parts = HashMap<String, (Entity, Rect)>;

/// Draws widgets as their layouts have them, within `frame`: the
/// rectangle of the layouts' screen that the node they're drawn in covers.
#[derive(Clone, Copy)]
struct Painter<'a> {
    layout: &'a Layout,
    letters: &'a Letters,
    frame: Rect,
}

impl Painter<'_> {
    /// The node covering `at`, a rectangle of the layouts' screen, within
    /// the [`frame`](Painter::frame).
    fn place(&self, at: Rect) -> Node {
        let f = self.frame;
        let percent = |v: f32, of: f32| Val::Percent(100.0 * v / of.max(f32::EPSILON));
        Node {
            position_type: PositionType::Absolute,
            left: percent(at.left - f.left, f.width()),
            top: percent(at.top - f.top, f.height()),
            width: percent(at.width().max(0.0), f.width()),
            height: percent(at.height().max(0.0), f.height()),
            ..default()
        }
    }

    /// Draws `widget`, `within` its parent's bounds, and all within it but
    /// the widgets at the paths in `skip`.
    fn paint(
        &self,
        s: &mut ChildSpawnerCommands,
        widget: &Widget,
        within: Rect,
        skip: &[&str],
    ) -> Parts {
        let mut parts = Parts::new();
        self.widget(s, widget, within, "", skip, &mut parts);
        parts
    }

    fn widget(
        &self,
        s: &mut ChildSpawnerCommands,
        widget: &Widget,
        within: Rect,
        path: &str,
        skip: &[&str],
        parts: &mut Parts,
    ) {
        if !widget.visible || skip.contains(&path) {
            return;
        }
        let at = widget.bounds(within);
        let own = s.spawn((self.place(at), Pickable::IGNORE)).id();
        parts.insert(path.to_string(), (own, at));
        let join = |name: &str| match path {
            "" => name.to_string(),
            _ => format!("{path}/{name}"),
        };
        if let Some(view) = &widget.view {
            self.element(s, view, within, &join(&view.name), parts);
        }
        for part in &widget.parts {
            self.element(s, part, at, &join(&part.name), parts);
        }
        for child in &widget.children {
            self.widget(s, child, at, &join(&child.name), skip, parts);
        }
    }

    fn element(
        &self,
        s: &mut ChildSpawnerCommands,
        element: &ViewElement,
        within: Rect,
        path: &str,
        parts: &mut Parts,
    ) {
        if !element.shown(&RESTING) {
            return;
        }
        let at = element.bounds.place(within);
        let node = self.place(at);
        let entity = match element.kind.as_deref() {
            Some("LABEL") => {
                let font = element.font.as_deref().unwrap_or(PLAIN);
                let [r, g, b] = element.colour;
                let (justify, side) = match element.justify.as_deref().map(str::to_ascii_lowercase)
                {
                    Some(j) if j == "right" => (Justify::Right, JustifyContent::FlexEnd),
                    Some(j) if j == "center" => (Justify::Center, JustifyContent::Center),
                    _ => (Justify::Left, JustifyContent::FlexStart),
                };
                let wraps = element
                    .value("multiline")
                    .is_some_and(|m| m.eq_ignore_ascii_case("true"));
                let lines = match wraps {
                    true => LineBreak::WordBoundary,
                    false => LineBreak::NoWrap,
                };
                let text = self.letters.text(font, "", Color::srgb(r, g, b));
                // Lined up within its bounds by what holds it, as a line
                // is only as wide as its words; wrapped across them.
                let holder = Node {
                    justify_content: side,
                    ..node
                };
                let width = match wraps {
                    true => percent(100),
                    false => Val::Auto,
                };
                let words = (
                    Node { width, ..default() },
                    text,
                    TextLayout::new(justify, lines),
                );
                let mut label = None;
                s.spawn((holder, Pickable::IGNORE))
                    .with_children(|holder| label = Some(holder.spawn(words).id()));
                label
            }
            Some("STATUS_BAR") => {
                let shaded = Shaded::of(element);
                let fill = match shaded.across {
                    true => Node {
                        width: percent(0),
                        height: percent(100),
                        ..default()
                    },
                    false => Node {
                        position_type: PositionType::Absolute,
                        bottom: px(0),
                        width: percent(100),
                        height: percent(0),
                        ..default()
                    },
                };
                let mut bar = s.spawn((node, shaded, Pickable::IGNORE));
                Some(
                    bar.with_child((fill, Filled, BackgroundColor(Color::NONE)))
                        .id(),
                )
            }
            Some("MODEL") => {
                let [r, g, b] = element.colour;
                let backdrop = BackgroundColor(Color::srgb(r, g, b));
                let model = Node {
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    overflow: Overflow::clip(),
                    ..node
                };
                Some(s.spawn((model, backdrop, Pickable::IGNORE)).id())
            }
            _ => self.picture(s, element, node),
        };
        if let Some(entity) = entity {
            parts.insert(path.to_string(), (entity, at));
        }
        for child in &element.children {
            let path = format!("{path}/{}", child.name);
            self.element(s, child, at, &path, parts);
        }
    }

    /// A decal: its image, a strip of one, or where it has neither, a
    /// colour of its own as a backdrop, or nothing for code to fill.
    fn picture(
        &self,
        s: &mut ChildSpawnerCommands,
        element: &ViewElement,
        node: Node,
    ) -> Option<Entity> {
        let image = element
            .texture
            .as_ref()
            .and_then(|t| self.layout.images.get(t));
        if let Some(image) = image {
            let image = ImageNode::new(image.clone()).with_mode(NodeImageMode::Stretch);
            return Some(s.spawn((node, image, Pickable::IGNORE)).id());
        }
        if let Some(ends) = strip(element)
            && let Some((image, end)) = self.layout.strips.get(&ends)
        {
            let slicer = TextureSlicer {
                border: BorderRect {
                    min_inset: Vec2::new(*end, 0.0),
                    max_inset: Vec2::new(*end, 0.0),
                },
                center_scale_mode: SliceScaleMode::Tile { stretch_value: 1.0 },
                sides_scale_mode: SliceScaleMode::Stretch,
                max_corner_scale: 1.0,
            };
            let image = ImageNode::new(image.clone()).with_mode(NodeImageMode::Sliced(slicer));
            return Some(s.spawn((node, image, Sliced, Pickable::IGNORE)).id());
        }
        let decal = element.kind.as_deref() == Some("DECAL");
        if decal && element.colour != [1.0; 3] {
            let [r, g, b] = element.colour;
            let backdrop = BackgroundColor(Color::srgb(r, g, b));
            return Some(s.spawn((node, backdrop, Pickable::IGNORE)).id());
        }
        decal.then(|| {
            s.spawn((
                node,
                ImageNode::default(),
                Visibility::Hidden,
                Pickable::IGNORE,
            ))
            .id()
        })
    }
}

/// Lays the display out afresh when what it should show changes.
#[allow(clippy::too_many_arguments)]
pub fn lay_out(
    mut commands: Commands,
    showing: Res<Showing>,
    // None where the layouts have no object display.
    layout: Option<Res<Layout>>,
    letters: Res<Letters>,
    sim: Res<Sim>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    gem: Res<ui::Gem>,
    drawn: Res<Drawn>,
    badges: Res<Badges>,
    loaded: Res<Loaded>,
    mut looks: ResMut<Looks>,
    mut banners: ResMut<BannerImages>,
    mut images: ResMut<Assets<Image>>,
    holder: Single<Entity, With<Holder>>,
) {
    // It draws no properties' icons, so a change only to those is none.
    let Some(layout) = layout else {
        return;
    };
    if !showing.is_changed() || showing.icons_only {
        return;
    }
    let holder = *holder;
    commands.entity(holder).despawn_related::<Children>();
    let Some((mode, detail)) = &showing.now else {
        return;
    };
    let name = match mode {
        Mode::SimpleActor(_) => SIMPLE,
        Mode::Company(_) => COMPANY,
        Mode::Group(_) => GROUP,
        Mode::Settlement => return,
    };
    let Some(display) = layout.displays.get(name) else {
        return;
    };
    commands.entity(holder).with_children(|s| {
        let mut draw = Draw {
            s,
            painter: Painter {
                layout: &layout,
                letters: &letters,
                frame: SCREEN_RECT,
            },
            display,
            area: display.bounds(layout.at),
            sim: &sim.0,
            local: local.0,
            leaders: &leaders,
            gem: &gem,
            drawn: &drawn,
            badges: &badges,
            detail,
            game: &loaded.game,
            looks: &mut looks,
            banners: &mut banners,
            images: &mut images,
        };
        match mode {
            Mode::SimpleActor(actor) => draw.simple_actor(actor),
            Mode::Company(actor) => draw.company(actor),
            Mode::Group(rs) => draw.group(rs),
            Mode::Settlement => {}
        }
        if matches!(mode, Mode::Company(Actor::Organization(_)) | Mode::Group(_)) {
            draw.formation_buttons();
        }
    });
}

/// What laying the display out reads, and where it's drawn.
struct Draw<'a, 'w> {
    s: &'a mut ChildSpawnerCommands<'w>,
    painter: Painter<'a>,
    /// The layout of the display for what's shown, and where it is.
    display: &'a Widget,
    area: Rect,
    sim: &'a World,
    local: Option<u32>,
    leaders: &'a Leaders,
    gem: &'a ui::Gem,
    drawn: &'a Drawn,
    badges: &'a Badges,
    detail: &'a Detail,
    game: &'a tgdata::GameData,
    looks: &'a mut Looks,
    banners: &'a mut BannerImages,
    images: &'a mut Assets<Image>,
}

impl Draw<'_, '_> {
    /// The formation buttons, down the layout's panel for them from its
    /// top, each its height over [`MOST`](crate::formations::MOST) apart,
    /// drawn in the states they're in, with a place for the formation's
    /// picture that [`show`](crate::formations::show) fills.
    fn formation_buttons(&mut self) {
        let Some(panel) = self.display.children.iter().find(|c| c.name == FORMATIONS) else {
            return;
        };
        let Some(button) = panel.children.iter().find(|c| c.name == FORMATION_BUTTON) else {
            return;
        };
        let within = panel.bounds(self.area);
        let row = (within.height() / crate::formations::MOST as f32).floor();
        let size = button.bounds(Rect {
            left: 0.0,
            top: 0.0,
            right: within.width(),
            bottom: within.height(),
        });
        let decal = button.children.iter().find(|c| c.name == FORMATION_DECAL);
        for n in 0..crate::formations::MOST {
            let top = within.top + row * n as f32;
            let at = Rect {
                left: within.left,
                top,
                right: within.left + size.width(),
                bottom: top + size.height(),
            };
            // Placed within the button, as a share of its size.
            let share = |r: Rect| Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(100.0 * (r.left - at.left) / at.width().max(1.0)),
                top: Val::Percent(100.0 * (r.top - at.top) / at.height().max(1.0)),
                width: Val::Percent(100.0 * r.width() / at.width().max(1.0)),
                height: Val::Percent(100.0 * r.height() / at.height().max(1.0)),
                ..default()
            };
            let faces: Vec<(Node, super::Drawn)> = (button.view.iter())
                .flat_map(|v| &v.children)
                .filter_map(|e| {
                    let image = self
                        .painter
                        .layout
                        .images
                        .get(e.texture.as_deref()?)?
                        .clone();
                    let element = ViewElement {
                        children: Vec::new(),
                        ..e.clone()
                    };
                    let drawn = super::Drawn {
                        when: vec![element],
                        image,
                        greyed: None,
                    };
                    Some((share(e.bounds.place(at)), drawn))
                })
                .collect();
            let icon = decal.map_or(at, |d| d.bounds(at));
            self.s
                .spawn((
                    super::Pressable::default(),
                    crate::formations::FormationButton(n),
                    ui::called(format!("formation.{}", n + 1)),
                    self.painter.place(at),
                    Button,
                    Pickable::default(),
                    Visibility::Hidden,
                ))
                .with_children(|b| {
                    for (node, drawn) in faces {
                        let image = ImageNode::new(drawn.image.clone())
                            .with_mode(bevy::ui::widget::NodeImageMode::Stretch);
                        b.spawn((node, image, Visibility::Hidden, drawn, Pickable::IGNORE));
                    }
                    b.spawn((
                        share(icon),
                        ImageNode::default(),
                        crate::formations::FormationIcon,
                        Pickable::IGNORE,
                    ));
                });
        }
    }

    /// The display's widget called `name`, drawn but for `skip`.
    fn part(&mut self, name: &str, skip: &[&str]) -> Parts {
        let Some(widget) = self.display.children.iter().find(|c| c.name == name) else {
            return Parts::new();
        };
        self.painter.paint(self.s, widget, self.area, skip)
    }

    fn entity(&mut self, parts: &Parts, path: &str) -> Option<EntityCommands<'_>> {
        let &(entity, _) = parts.get(path)?;
        Some(self.s.commands_mut().entity(entity))
    }

    fn picture(&mut self, template: &str) -> Option<Handle<Image>> {
        shown::picture(self.drawn, self.looks, self.game, self.images, template)
    }

    /// Shows `image` in the decal at `path`, if there's one to show.
    fn show(&mut self, parts: &Parts, path: &str, image: Option<Handle<Image>>) {
        if let Some(image) = image
            && let Some(mut decal) = self.entity(parts, path)
        {
            decal.insert((ImageNode::new(image), Visibility::Inherited));
        }
    }

    /// Fills the bar at `path` as `fill` says.
    fn fill(&mut self, parts: &Parts, path: &str, fill: Fill) {
        if let Some(mut bar) = self.entity(parts, path) {
            bar.insert(fill);
        }
    }

    /// Has the widget at `path` give `of`'s tooltip.
    fn says(&mut self, parts: &Parts, path: &str, of: Says) {
        if let Some(mut part) = self.entity(parts, path) {
            part.insert((shown::says(of), Pickable::default()));
        }
    }

    /// The name and owner lines.
    fn title(&mut self, name: String, owner: u32) {
        let parts = self.part("Name", &[]);
        if let Some(mut label) = self.entity(&parts, "Label") {
            label.insert(Text::new(name));
        }
        let parts = self.part("Owner", &[]);
        let Some(&(label, at)) = parts.get("Label") else {
            return;
        };
        let Some(player) = self.sim.sides.player(owner) else {
            return;
        };
        let colour = shown::owner_colour(self.sim, self.local, owner);
        let name = self.leaders.owner(player, &self.sim.sides, &self.looks.map);
        // The owner's gem, as tall as the line, then its name.
        let line = Node {
            align_items: AlignItems::Center,
            column_gap: percent(100.0 * 4.0 / at.width().max(1.0)),
            ..self.painter.place(at)
        };
        let font = self.font("Owner");
        let mark =
            (!player.independent).then(|| ui::side_mark(self.gem, player.rgb, Node::default()));
        let words = self.painter.letters.text(&font, &name, colour);
        let gem_size = Vec2::new(at.height() * 0.5, at.height());
        let s = &mut *self.s;
        s.commands_mut().entity(label).despawn();
        s.spawn((line, Pickable::IGNORE)).with_children(|line| {
            if let Some(mark) = mark {
                line.spawn((
                    mark,
                    Measured {
                        size: Some(gem_size),
                        ..default()
                    },
                ));
            }
            line.spawn(words);
        });
    }

    /// The font of the display's label widget called `name`.
    fn font(&self, name: &str) -> String {
        let widget = self.display.children.iter().find(|c| c.name == name);
        let label =
            widget.and_then(|w| w.parts.iter().find(|p| p.kind.as_deref() == Some("LABEL")));
        label.and_then(|l| l.font.clone()).unwrap_or(PLAIN.into())
    }

    /// The portrait's box and frame, showing `template`'s picture until
    /// there's a portrait drawn, over a health bar filled as `fill` says,
    /// both giving `of`'s tooltip.
    fn portrait(&mut self, template: &str, fill: Fill, of: Says) {
        let skip = ["ModelPortrait", "Icon"];
        let parts = self.part("PortraitButton", &skip);
        let picture = self.picture(template);
        if let Some(&(model, _)) = parts.get("Portrait/Model") {
            let face = (
                Node {
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                ImageNode::new(picture.unwrap_or_default()).with_mode(NodeImageMode::Stretch),
                Portrait,
                Pickable::IGNORE,
            );
            self.s.commands_mut().entity(model).with_child(face);
        }
        self.says(&parts, "", of);
        let parts = self.part("Health", &["TargetPercentage"]);
        self.fill(&parts, "StatusBar", fill);
        self.says(&parts, "", of);
    }

    /// The line saying what `status`'s thing counts for in its side's
    /// economy, and the one saying what it's doing.
    fn status(&mut self, status: Status) {
        let parts = self.part("Economy", &[]);
        if let Some(&(label, at)) = parts.get("Label") {
            // Its figures, each after its resource's glyph, in its place.
            let font = self.font("Economy");
            self.s.commands_mut().entity(label).despawn();
            let row = Node {
                align_items: AlignItems::FlexStart,
                ..self.painter.place(at)
            };
            let line = Economy {
                of: status,
                font,
                shown: Vec::new(),
            };
            self.s.spawn((row, line, Pickable::IGNORE));
        }
        let parts = self.part("State", &[]);
        if let Some(mut label) = self.entity(&parts, "Label") {
            label.insert(status);
        }
    }

    /// The shield, with `target`'s defence by it, for one with a body,
    /// whose `template` gives the icon.
    fn defense(&mut self, target: Target, template: &Id) {
        if self.sim.defense(target).is_none() {
            return;
        }
        let Some(values) = self.display.children.iter().find(|c| c.name == "Values") else {
            return;
        };
        let Some(widget) = values.children.iter().find(|c| c.name == "DefenseValues") else {
            return;
        };
        let within = values.bounds(self.area);
        let parts = self.painter.paint(self.s, widget, within, &[]);
        let icon = (self.looks.listings.get(template)).and_then(|l| l.defense_icon.clone());
        let icon = self.looks.image(self.game, self.images, icon.as_deref());
        self.show(&parts, "Icon/Decal", icon);
        if let Some(mut label) = self.entity(&parts, "Label/Label") {
            label.insert(Defends(target, Some(Color::WHITE)));
        }
        if let Some(mut panel) = self.entity(&parts, "") {
            panel.insert((Shield(target), Hint(String::new()), Pickable::default()));
        }
    }

    /// `target`'s weapons, each its icon with its main damage by it, a row
    /// each down from the top of the values.
    fn attacks(&mut self, target: Target) {
        let Some(values) = self.display.children.iter().find(|c| c.name == "Values") else {
            return;
        };
        let Some(widget) = values.children.iter().find(|c| c.name == "AttackValues") else {
            return;
        };
        let most = (values.value("max_widgets_values")).and_then(|v| v.parse().ok());
        let most = most.unwrap_or(shown::MOST_WEAPONS).min(shown::MOST_WEAPONS);
        let apart = (values.value("display_values_separation")).and_then(|v| v.parse().ok());
        let apart: f32 = apart.unwrap_or(0.0);
        let within = values.bounds(self.area);
        let row = widget.bounds(within).height() + apart;
        let icons: Vec<Option<String>> = (self.sim.weapons(target).iter())
            .take(most)
            .map(|w| w.shown.icon.clone())
            .collect();
        for (i, icon) in icons.into_iter().enumerate() {
            let down = row * i as f32;
            let within = Rect {
                top: within.top + down,
                bottom: within.bottom + down,
                ..within
            };
            let parts = self.painter.paint(self.s, widget, within, &[]);
            let icon = self.looks.image(self.game, self.images, icon.as_deref());
            self.show(&parts, "Icon/Decal", icon);
            if let Some(mut label) = self.entity(&parts, "Label/Label") {
                label.insert(Strikes(target, i, Some(Color::WHITE)));
            }
            if let Some(mut panel) = self.entity(&parts, "") {
                panel.insert((Armed(target, i), Hint(String::new()), Pickable::default()));
            }
        }
    }

    /// The box of what a lone thing's template says of it; none for one
    /// that says nothing.
    fn description(&mut self, says: Option<String>) {
        let Some(says) = says else {
            return;
        };
        let parts = self.part("DescriptionLabel", &[]);
        if let Some(mut label) = self.entity(&parts, "Label") {
            label.insert(Text::new(says));
        }
    }

    fn described(&self, template: &Id) -> Option<String> {
        self.looks.listings.get(template)?.description.clone()
    }

    /// The simple actor display: a building with what it's commissioning,
    /// or a unit on its own.
    fn simple_actor(&mut self, actor: &Actor) {
        match *actor {
            Actor::Organization(r) => {
                let organization = &self.sim.organizations[r];
                let name = self.organization_name(r);
                self.title(name, organization.owner);
                let captain = shown::captain(self.sim, self.looks, r);
                let unit = self.sim.units[captain].template.clone();
                self.portrait(unit.as_str(), Fill::Unit(captain), Says::Unit(captain));
                self.attacks(Target::Unit(captain));
                self.defense(Target::Unit(captain), &unit);
                let says = [&organization.template, &unit];
                let says = says.into_iter().find_map(|t| self.described(t));
                self.description(says);
                self.status(Status::Organization(r));
            }
            Actor::Building(b, ref queue) => {
                self.structure(b);
                let template = self.sim.buildings[b].template.clone();
                self.attacks(Target::Building(b));
                self.defense(Target::Building(b), &template);
                // What it's commissioning shows where its description would.
                match queue.is_empty() {
                    true => self.description(self.described(&template)),
                    false => self.queue(b),
                }
                self.status(Status::Building(b));
            }
        }
    }

    fn organization_name(&self, r: usize) -> String {
        let looks = &self.looks;
        shown::organization_name(self.sim, &looks.names, &looks.strings, &looks.map, r)
    }

    /// A building's name and owner, and its picture over its health and,
    /// for a relay, its stockpile.
    fn structure(&mut self, b: usize) {
        let building = &self.sim.buildings[b];
        let (template, owner) = (building.template.clone(), building.owner);
        let looks = &self.looks;
        let name = shown::building_name(self.sim, &looks.names, &looks.strings, &looks.map, b);
        self.title(name, owner);
        let of = Says::Building(b);
        self.portrait(template.as_str(), Fill::Building(b), of);
        if self.sim.supply_of(b).is_some_and(|s| s.relay) {
            let parts = self.part("SupplyStockpile", &["TargetPercentage"]);
            self.fill(&parts, "StatusBar", Fill::Stockpile(b));
            self.says(&parts, "", of);
        }
    }

    /// What a building is commissioning: the first in the big place with
    /// its progress, those waiting in the small ones.
    fn queue(&mut self, b: usize) {
        let Some(panel) = self
            .display
            .children
            .iter()
            .find(|c| c.name == "QueuePanel")
        else {
            return;
        };
        let number = |key: &str| {
            panel
                .value(key)
                .and_then(|v| v.trim().parse::<usize>().ok())
        };
        let most = number("max_queue_widgets").unwrap_or(6);
        let across = number("max_queue_widgets_horizontal").unwrap_or(3).max(1);
        let at = panel.bounds(self.area);
        let queue: Vec<String> = (self.sim.buildings[b].queue.iter())
            .map(|c| c.template().to_string())
            .collect();
        let child = |name: &str| panel.children.iter().find(|c| c.name == name);
        let (Some(current), Some(progress), Some(waiting)) =
            (child("CurrentButton"), child("Progress"), child("Queue"))
        else {
            return;
        };
        let held = |place| Says::Queued {
            building: b,
            place,
            held: false,
        };
        let parts = self.painter.paint(self.s, current, at, &[]);
        let picture = queue.first().and_then(|t| self.picture(t));
        self.show(&parts, "UnitButtonDecal/Decal", picture);
        self.button(&parts, Queued(0), held(0), "object.queued.1");
        let parts = self
            .painter
            .paint(self.s, progress, at, &["TargetPercentage"]);
        self.fill(&parts, "StatusBar", Fill::Progress(b));
        self.says(&parts, "", held(0));
        let Some(button) = waiting.children.iter().find(|c| c.name == "QueueButton") else {
            return;
        };
        let row = waiting.bounds(at);
        let size = button.bounds(row);
        let (w, h) = (size.width() + 2.0, size.height() + 2.0);
        for (n, template) in queue.iter().take(most).skip(1).enumerate() {
            let place = n + 1;
            let (x, y) = ((n % across) as f32 * w, (n / across) as f32 * h);
            let within = moved(row, x, y);
            let parts = self.painter.paint(self.s, button, within, &[]);
            let picture = self.picture(template);
            self.show(&parts, "UnitButtonDecal/Decal", picture);
            let id = format!("object.queued.{}", place + 1);
            self.button(&parts, Queued(place), held(place), &id);
        }
        if queue.len() > most
            && let Some(more) = waiting
                .children
                .iter()
                .find(|c| c.name == "QueueMoreIndicator")
        {
            self.painter.paint(self.s, more, row, &[]);
        }
    }

    /// Makes the widget of `parts` a button that calls off what's at
    /// `place` in the queue.
    fn button(&mut self, parts: &Parts, place: Queued, of: Says, id: &str) {
        if let Some(mut button) = self.entity(parts, "") {
            button
                .insert((
                    Button,
                    ui::called(id),
                    place,
                    shown::says(of),
                    Pickable::default(),
                ))
                .observe(shown::cancel);
        }
    }

    /// The company display: an organization with its units, or an HQ with
    /// its places.
    fn company(&mut self, actor: &Actor) {
        match *actor {
            Actor::Organization(r) => self.organization(r),
            Actor::Building(b, _) => {
                self.structure(b);
                self.places(Some(b));
                self.status(Status::Building(b));
            }
        }
    }

    fn organization(&mut self, r: usize) {
        let organization = &self.sim.organizations[r];
        let name = self.organization_name(r);
        self.title(name, organization.owner);
        let captain = shown::captain(self.sim, self.looks, r);
        let template = self.sim.units[captain].template.clone();
        self.portrait(template.as_str(), Fill::Unit(captain), Says::Unit(captain));
        let lone = shown::lone(self.sim, r);
        if !lone && self.detail.slots.is_empty() {
            self.units(r, captain);
        }
        if !lone {
            self.flag(r);
        }
        self.places(None);
        self.status(Status::Organization(r));
    }

    /// A card for each of the organization at `r`'s units but an officer
    /// `captain`, who has the portrait: its picture over its health, in a
    /// block for each kind of slot.
    fn units(&mut self, r: usize, captain: usize) {
        let Some(panel) = self
            .display
            .children
            .iter()
            .find(|c| c.name == "CompanyUnits")
        else {
            return;
        };
        let Some(card) = panel.children.iter().find(|c| c.name == "CompanyUnit") else {
            return;
        };
        let number = |key: &str| panel.value(key).and_then(|v| v.trim().parse::<f32>().ok());
        let gap = Vec2::new(
            number("horizontal_unit_separation").unwrap_or(2.0),
            number("vertical_unit_separation").unwrap_or(2.0),
        );
        let separation = number("definition_separation").unwrap_or(0.0);
        let at = panel.bounds(self.area);
        let size = card.bounds(at);
        let size = Vec2::new(size.width(), size.height());
        let groups = shown::card_groups(self.sim, self.looks, r, captain);
        let places = shown::card_places(
            &groups,
            size,
            gap,
            separation,
            Vec2::new(at.width(), at.height()),
        );
        for (u, place) in places {
            let within = moved(at, place.x, place.y);
            let parts = self.painter.paint(
                self.s,
                card,
                within,
                &["Properties", "Health/TargetPercentage"],
            );
            let picture = self.picture(self.sim.units[u].template.as_str());
            self.show(&parts, "UnitButton/UnitButtonDecal/Decal", picture);
            if let Some(mut decal) = self.entity(&parts, "UnitButton/UnitButtonDecal/Decal") {
                decal.insert((UnitPicture(u), Shade::Full));
            }
            self.fill(&parts, "Health/StatusBar", Fill::Unit(u));
            self.says(&parts, "", Says::Unit(u));
        }
    }

    /// The organization at `r`'s banner, between its efficiency and its
    /// morale.
    fn flag(&mut self, r: usize) {
        let parts = self.part("BannerButton", &[]);
        self.banner(&parts, "Banner/Model", r);
        self.says(&parts, "", Says::Organization(r));
        let parts = self.part("AttackEfficiency", &[]);
        self.efficiency(&parts, "", r);
        let parts = self.part("Morale", &["TargetPercentage"]);
        self.fill(&parts, "StatusBar", Fill::Morale(r));
        self.says(&parts, "", Says::Organization(r));
    }

    /// Fills the efficiency bar at `path` of `parts` for the organization
    /// at `r`, its marker at what that's easing towards.
    fn efficiency(&mut self, parts: &Parts, path: &str, r: usize) {
        let join = |name: &str| match path {
            "" => name.to_string(),
            _ => format!("{path}/{name}"),
        };
        let (status, target) = (join("StatusBar"), join("TargetPercentage"));
        let decal = format!("{target}/Decal");
        self.fill(parts, &status, Fill::Efficiency(r));
        self.says(parts, path, Says::Organization(r));
        let (Some(&(bar, at)), Some(&(marker, mark))) = (parts.get(&status), parts.get(&decal))
        else {
            return;
        };
        // Within the bar, its right edge at the share.
        let size = |v: f32, of: f32| percent(100.0 * v / of.max(1.0));
        let (width, height) = (
            size(mark.width(), at.width()),
            size(mark.height(), at.height()),
        );
        let Val::Percent(w) = width else {
            return;
        };
        let commands = self.s.commands_mut();
        commands.entity(marker).insert((
            Node {
                position_type: PositionType::Absolute,
                top: px(0),
                left: shown::towards(self.sim, r),
                width,
                height,
                margin: UiRect::left(percent(-w)),
                ..default()
            },
            EfficiencyTarget(r),
        ));
        commands.entity(bar).add_child(marker);
        for (_, &(e, _)) in parts
            .iter()
            .filter(|(p, _)| p.starts_with(&target) && **p != decal)
        {
            commands.entity(e).despawn();
        }
    }

    /// The organization at `r`'s banner in the model at `path`.
    fn banner(&mut self, parts: &Parts, path: &str, r: usize) {
        let Some(&(model, _)) = parts.get(path) else {
            return;
        };
        let layers = Flown {
            sim: self.sim,
            detail: self.detail,
            leaders: self.leaders,
            badges: self.badges,
            game: self.game,
        }
        .layers(self.banners, self.images, r);
        let commands = self.s.commands_mut();
        for (image, colour) in layers {
            let Some(image) = image else {
                continue;
            };
            let layer = (
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                ImageNode {
                    image,
                    color: colour,
                    ..default()
                },
                Pickable::IGNORE,
            );
            commands.entity(model).with_child(layer);
        }
    }

    /// An HQ's places: each with the banner, health and morale of the
    /// organization in it, which a click selects; the picture and progress
    /// of what `building`'s queue holds it for, which a click calls off;
    /// or empty.
    fn places(&mut self, building: Option<usize>) {
        let count = self.detail.slots.len();
        if count == 0 {
            return;
        }
        let Some(panel) = self
            .display
            .children
            .iter()
            .find(|c| c.name == "HQAttached")
        else {
            return;
        };
        let Some(place) = panel
            .children
            .iter()
            .find(|c| c.name == "HQAttachedOrganization")
        else {
            return;
        };
        let at = panel.bounds(self.area);
        let size = place.bounds(at);
        let area = Vec2::new(at.width(), at.height());
        let slot = Vec2::new(size.width(), size.height());
        let empty = self.looks.said(shown::EMPTY_SLOT);
        for (i, held) in self.detail.slots.clone().into_iter().enumerate() {
            let step = shown::spread(i, count, area, slot);
            let within = moved(at, step.x, step.y);
            // A banner where there's an organization and a picture where
            // there isn't, with the bars of what's there.
            let (shows, bars): (&str, &[&str]) = match held {
                Place::Organization(_) => ("Button/ButtonModel", &["Health", "Morale"]),
                Place::Queued(_) => ("Button/ButtonDecal", &["RecruitProgress"]),
                Place::Flown(_) => ("Button/ButtonDecal", &["HealthTransport"]),
                Place::Empty => ("Button/ButtonDecal", &[]),
            };
            let all = ["Health", "Morale", "HealthTransport", "RecruitProgress"];
            let markers = all.map(|b| format!("{b}/TargetPercentage"));
            let mut skip: Vec<&str> = markers.iter().map(String::as_str).collect();
            let pictures = ["Button/ButtonDecal", "Button/ButtonModel"];
            skip.extend(pictures.into_iter().filter(|p| *p != shows));
            skip.extend(all.into_iter().filter(|b| !bars.contains(b)));
            let parts = self.painter.paint(self.s, place, within, &skip);
            let id = ui::called(format!("object.slot.{}", i + 1));
            let fill = |this: &mut Self, name: &str, fill: Option<Fill>| {
                if let Some(fill) = fill {
                    this.fill(&parts, &format!("{name}/StatusBar"), fill);
                }
            };
            match (held, building) {
                (Place::Organization(r), _) => {
                    self.banner(&parts, "Button/ButtonModel/Model", r);
                    fill(self, "Health", Some(Fill::Organization(r)));
                    fill(self, "Morale", Some(Fill::Morale(r)));
                    if let Some(mut card) = self.entity(&parts, "") {
                        let of = shown::says(Says::Organization(r));
                        card.insert((Button, id, Pick(r), of, Pickable::default()))
                            .observe(shown::pick);
                    }
                }
                (Place::Queued(n), Some(b)) => {
                    let queue = &self.sim.buildings[b].queue;
                    let template = queue.get(n).map(|c| c.template().to_string());
                    let picture = template.and_then(|t| self.picture(&t));
                    self.show(&parts, "Button/ButtonDecal/Decal", picture);
                    // Only the first is under way.
                    fill(
                        self,
                        "RecruitProgress",
                        (n == 0).then_some(Fill::Progress(b)),
                    );
                    let of = Says::Queued {
                        building: b,
                        place: n,
                        held: true,
                    };
                    if let Some(mut card) = self.entity(&parts, "") {
                        card.insert((Button, id, Queued(n), shown::says(of), Pickable::default()))
                            .observe(shown::cancel);
                    }
                }
                (Place::Flown(d), _) => {
                    let template = self.sim.deliveries[d].template.to_string();
                    let picture = self.picture(&template);
                    self.show(&parts, "Button/ButtonDecal/Decal", picture);
                    fill(self, "HealthTransport", Some(Fill::Flight));
                    let name = shown::template_name(&self.looks.names, &template);
                    if let Some(mut card) = self.entity(&parts, "") {
                        card.insert((Hint(name), Pickable::default()));
                    }
                }
                _ => {
                    if let Some(mut card) = self.entity(&parts, "") {
                        card.insert((Hint(empty.clone()), Pickable::default()));
                    }
                }
            }
        }
    }

    /// The group display: the first's name and owner, and a card for
    /// each.
    fn group(&mut self, rs: &[usize]) {
        let first = rs[0];
        let name = self.organization_name(first);
        self.title(name, self.sim.organizations[first].owner);
        let offset = (self.display.value("mini_display_offset"))
            .and_then(|v| v.trim().parse::<f32>().ok())
            .unwrap_or(0.0);
        // Each card after the last, whatever its kind and so its width.
        let mut left = 0.0;
        for r in shown::newest_first(rs) {
            let Some(card) = card_widget(self.display, shown::lone(self.sim, r)) else {
                continue;
            };
            let size = card.bounds(self.area);
            let at = moved(size, left, 0.0);
            left += size.width() + offset;
            let holder = self
                .s
                .spawn((self.painter.place(at), Pickable::IGNORE))
                .id();
            let commands = self.s.commands_mut();
            let mut holder = commands.entity(holder);
            holder.with_children(|s| {
                let mut draw = Draw {
                    s,
                    painter: Painter {
                        frame: card.bounds(SCREEN_RECT),
                        ..self.painter
                    },
                    display: self.display,
                    area: SCREEN_RECT,
                    sim: self.sim,
                    local: self.local,
                    leaders: self.leaders,
                    gem: self.gem,
                    drawn: self.drawn,
                    badges: self.badges,
                    detail: self.detail,
                    game: self.game,
                    looks: &mut *self.looks,
                    banners: &mut *self.banners,
                    images: &mut *self.images,
                };
                draw.card(card, r);
            });
        }
    }

    /// The group display's card for the organization at `r`, drawn from
    /// `card`'s layout to fill the node it's drawn in: a lone thing's
    /// picture and health, or a regiment's banner over its efficiency,
    /// morale, captain's health and a small bar for each of the rest.
    fn card(&mut self, card: &Widget, r: usize) {
        let lone = shown::lone(self.sim, r);
        let skip = [
            "PortraitButton/Portrait",
            "PortraitButton/ModelPortrait",
            "PortraitButton/Icon",
            "DescriptionLabel",
            "PrimaryName",
            "HealthBars",
            "RetreatRoutButton",
            "Health/TargetPercentage",
            "PrimaryHealth/TargetPercentage",
            "Morale/TargetPercentage",
        ];
        let parts = self.painter.paint(self.s, card, SCREEN_RECT, &skip);
        let captain = shown::captain(self.sim, self.looks, r);
        let of = match lone {
            true => Says::Unit(captain),
            false => Says::Organization(r),
        };
        let id = format!("object.organization.{}", self.sim.organizations[r].id);
        if let Some(mut node) = self.entity(&parts, "") {
            node.insert((
                Button,
                ui::called(id),
                Pick(r),
                shown::says(of),
                Pickable::default(),
            ))
            .observe(shown::pick);
        }
        let Some(portrait) = card.children.iter().find(|c| c.name == "PortraitButton") else {
            return;
        };
        let bounds = card.bounds(SCREEN_RECT);
        let at = portrait.bounds(bounds);
        let inner = |child: &str| portrait.children.iter().find(|c| c.name == child);
        match lone {
            true => {
                if let Some(icon) = inner("Icon") {
                    let parts = self.painter.paint(self.s, icon, at, &[]);
                    let template = self.sim.units[captain].template.clone();
                    let picture = self.picture(template.as_str());
                    self.show(&parts, "Decal", picture);
                }
                self.fill(&parts, "Health/StatusBar", Fill::Unit(captain));
            }
            false => {
                if let Some(model) = inner("ModelPortrait") {
                    let model = self.painter.paint(self.s, model, at, &[]);
                    self.banner(&model, "Model", r);
                }
                self.fill(&parts, "PrimaryHealth/StatusBar", Fill::Unit(captain));
                self.says(&parts, "PrimaryHealth", Says::Unit(captain));
                self.fill(&parts, "Morale/StatusBar", Fill::Morale(r));
                self.efficiency(&parts, "AttackEfficiency", r);
                self.health_bars(card, bounds, r);
            }
        }
    }

    /// The small bars on a group display's card at `at` for the
    /// organization at `r`, two to a row, each over its own plate.
    fn health_bars(&mut self, card: &Widget, at: Rect, r: usize) {
        let Some(panel) = card.children.iter().find(|c| c.name == "HealthBars") else {
            return;
        };
        let Some(bar) = panel.children.iter().find(|c| c.name == "Health") else {
            return;
        };
        let plate = panel.children.iter().find(|c| c.name == "HealthBarBorder");
        let number = |key: &str| panel.value(key).and_then(|v| v.trim().parse::<f32>().ok());
        let gap = number("bar_separation").unwrap_or(5.0);
        let border = Vec2::new(
            number("horizontal_border_separation").unwrap_or(3.0),
            number("vertical_border_separation").unwrap_or(2.0),
        );
        let area = panel.bounds(at);
        let size = bar.bounds(area);
        let size = Vec2::new(size.width(), size.height());
        for (n, u) in shown::small_bars(self.sim, self.looks, r)
            .into_iter()
            .enumerate()
        {
            let step = Vec2::new((n % 2) as f32, (n / 2) as f32) * (size + gap);
            let place = border + step;
            // Cut off at the panel's edge, as one starting past it would be.
            if place.y >= area.height() {
                break;
            }
            if let Some(plate) = plate {
                // As wide as the bar and the border either side.
                let corner = (place - border).max(Vec2::ZERO);
                let within = moved(area, corner.x, corner.y);
                let within = Rect {
                    right: within.left + size.x + 2.0 * border.x,
                    ..within
                };
                self.painter.paint(self.s, plate, within, &[]);
            }
            let within = moved(area, place.x, place.y);
            let parts = self
                .painter
                .paint(self.s, bar, within, &["TargetPercentage"]);
            self.fill(&parts, "StatusBar", Fill::Unit(u));
            self.says(&parts, "", Says::Unit(u));
        }
    }
}

/// The group display's widget for a card: a lone thing's or an
/// organization's.
fn card_widget(display: &Widget, lone: bool) -> Option<&Widget> {
    let name = match lone {
        true => "SimpleActorMini",
        false => "CompanyMini",
    };
    display.children.iter().find(|c| c.name == name)
}

/// What the group display's cards are drawn with, wherever they're shown:
/// the original's layout, or this project's own interface, which embeds
/// them.
#[derive(Resource)]
pub struct Cards {
    pub(super) layout: Layout,
    pub(super) letters: Letters,
}

/// The things a card shows of the match, as its interface has them.
pub(crate) struct CardShows<'a> {
    pub sim: &'a World,
    pub local: Option<u32>,
    pub leaders: &'a Leaders,
    pub gem: &'a ui::Gem,
    pub drawn: &'a Drawn,
    pub badges: &'a Badges,
    pub detail: &'a Detail,
    pub game: &'a tgdata::GameData,
    pub looks: &'a mut Looks,
    pub banners: &'a mut BannerImages,
    pub images: &'a mut Assets<Image>,
}

impl Cards {
    /// How big the card for a thing on its own, or an organization, is, in
    /// the layouts' units; none where the layouts have no such card.
    pub(crate) fn size(&self, lone: bool) -> Option<Vec2> {
        let at = card_widget(self.layout.displays.get(GROUP)?, lone)?.bounds(SCREEN_RECT);
        Some(Vec2::new(at.width(), at.height()))
    }

    /// How far down a card for a thing on its own, or an organization, its
    /// picture starts: what's above is for its name, which this project's
    /// interface shows elsewhere.
    pub(crate) fn top(&self, lone: bool) -> Option<f32> {
        let card = card_widget(self.layout.displays.get(GROUP)?, lone)?;
        let at = card.bounds(SCREEN_RECT);
        let portrait = card.children.iter().find(|c| c.name == "PortraitButton")?;
        Some(portrait.bounds(at).top - at.top)
    }

    /// Draws the card for the organization at `r` to fill the node `s`
    /// spawns into, as the group display shows it.
    pub(crate) fn draw(&self, s: &mut ChildSpawnerCommands, shows: CardShows, r: usize) {
        let Some(display) = self.layout.displays.get(GROUP) else {
            return;
        };
        let Some(card) = card_widget(display, shown::lone(shows.sim, r)) else {
            return;
        };
        let mut draw = Draw {
            s,
            painter: Painter {
                layout: &self.layout,
                letters: &self.letters,
                frame: card.bounds(SCREEN_RECT),
            },
            display,
            area: SCREEN_RECT,
            sim: shows.sim,
            local: shows.local,
            leaders: shows.leaders,
            gem: shows.gem,
            drawn: shows.drawn,
            badges: shows.badges,
            detail: shows.detail,
            game: shows.game,
            looks: shows.looks,
            banners: shows.banners,
            images: shows.images,
        };
        draw.card(card, r);
    }
}

/// Keeps each [`Shaded`] bar filled as its [`Fill`] says.
pub fn fill_bars(
    sim: Res<Sim>,
    bars: Query<(&Fill, &Shaded, &Children)>,
    mut fills: Query<(&mut Node, &mut BackgroundColor), With<Filled>>,
) {
    for (fill, shaded, children) in &bars {
        let share = shown::share(&sim.0, *fill);
        for &child in children {
            let Ok((mut node, mut colour)) = fills.get_mut(child) else {
                continue;
            };
            let size = percent(100.0 * share);
            match shaded.across {
                true if node.width != size => node.width = size,
                false if node.height != size => node.height = size,
                _ => {}
            }
            colour.set_if_neq(BackgroundColor(shaded.colour(share)));
        }
    }
}

/// Keeps each economy line's figures to what its thing counts for.
pub fn economy(
    mut commands: Commands,
    sim: Res<Sim>,
    local: Res<Local>,
    looks: Res<Looks>,
    letters: Res<Letters>,
    images: Res<Assets<Image>>,
    mut lines: Query<(Entity, &mut Economy)>,
) {
    let sim = &sim.0;
    let resources = &sim.rules().economy.resources;
    for (row, mut line) in &mut lines {
        let now = (shown::counted(sim, local.0, line.of))
            .map_or_else(Vec::new, |l| shown::economy(&l, resources));
        if line.shown == now {
            continue;
        }
        commands.entity(row).despawn_related::<Children>();
        let font = line.font.clone();
        commands.entity(row).with_children(|row| {
            for (kind, figure) in &now {
                if let Some(glyph) = looks.glyphs.get(kind) {
                    let size = images.get(glyph).map(Image::size_f32);
                    row.spawn((
                        ImageNode::new(glyph.clone()),
                        Node::default(),
                        Measured { size, ..default() },
                        Pickable::IGNORE,
                    ));
                }
                row.spawn(letters.text(&font, &format!("{figure} "), Color::WHITE));
            }
        });
        line.shown = now;
    }
}

/// Shows the portrait, once it's drawn, in the portrait's box: a face
/// filling it, a fitted model keeping its shape across it.
pub fn portray(stage: Res<Stage>, mut boxes: Query<(&mut ImageNode, &mut Node), With<Portrait>>) {
    let Some((portrait, fitted)) = stage.picture() else {
        return;
    };
    for (mut shown, mut node) in &mut boxes {
        if shown.image == portrait {
            continue;
        }
        shown.image = portrait.clone();
        shown.image_mode = NodeImageMode::Stretch;
        let (height, aspect) = match fitted {
            true => (Val::Auto, Some(tgrender::portrait::FIT_ASPECT)),
            false => (percent(100), None),
        };
        node.height = height;
        node.aspect_ratio = aspect;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_blends_through_its_middle_colour() {
        let health = Shaded {
            start: [1.0, 0.0, 0.0],
            mid: [1.0, 1.0, 0.0],
            end: [0.0, 1.0, 0.0],
            mid_at: 0.5,
            across: true,
        };
        assert_eq!(health.colour(0.0), Color::srgb(1.0, 0.0, 0.0));
        assert_eq!(health.colour(0.25), Color::srgb(1.0, 0.5, 0.0));
        assert_eq!(health.colour(0.5), Color::srgb(1.0, 1.0, 0.0));
        assert_eq!(health.colour(1.0), Color::srgb(0.0, 1.0, 0.0));
        // One whose middle is at its start is the middle's colour throughout.
        let morale = Shaded {
            start: [1.0; 3],
            mid: [0.3, 0.3, 0.8],
            end: [0.3, 0.3, 0.8],
            mid_at: 0.0,
            ..health
        };
        assert_eq!(morale.colour(0.0), Color::srgb(0.3, 0.3, 0.8));
        assert_eq!(morale.colour(0.7), Color::srgb(0.3, 0.3, 0.8));
    }

    #[test]
    fn a_strip_is_read_only_where_it_runs_across() {
        let strip = |direction: &str| ViewElement {
            name: "View".into(),
            kind: Some("DECAL_TILED_DIRECTIONAL".into()),
            bounds: Default::default(),
            when: Vec::new(),
            texture: None,
            colour: [1.0; 3],
            text: None,
            font: None,
            justify: None,
            tiles: None,
            children: Vec::new(),
            fields: vec![
                ("ends_texture".into(), "/UI/%s/Game/Ends.tga".into()),
                ("center_texture".into(), "/UI/%s/Game/Center.tga".into()),
                ("direction".into(), direction.into()),
            ],
        };
        assert_eq!(
            super::strip(&strip("horizontal")),
            Some(("UI/Game/Ends.tga".into(), "UI/Game/Center.tga".into()))
        );
        assert_eq!(super::strip(&strip("vertical")), None);
    }
}
