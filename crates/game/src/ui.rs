//! The interface's look, and the pieces screens are built from. Screens say
//! what part each colour plays, as a [`Surface`], [`Ink`] or [`Edge`], and
//! [`Theme`] says what that looks like, so changing the theme restyles the
//! whole interface, even while it's up. [`Shade`] and [`Meter`] do the same
//! for pictures and bars.

use bevy::asset::uuid_handle;
use bevy::ecs::observer::IntoEntityObserver;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{Font as Face, FontSource};
use bevy::ui_widgets::{
    Button, Slider, SliderRange, SliderStep, SliderThumb, SliderValue, TrackClick,
};

use crate::sound::Sounds;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Theme>()
            .add_systems(Startup, (faces, spawn_hint))
            .add_systems(Update, (highlight, hint))
            // Before layout, which a meter's fill width and a slider's
            // thumb feed, so that what's spawned in Update is drawn right
            // from its first frame.
            .add_systems(
                PostUpdate,
                (paint, ink, edge, shade, fill, place_thumbs).before(bevy::ui::UiSystems::Prepare),
            )
            .add_observer(click);
    }
}

/// What the interface looks like.
#[derive(Resource, Clone, Debug)]
pub struct Theme {
    /// What every surface is shaded from.
    pub tint: Color,
    /// What surfaces are lit toward, and the colour of plain text.
    pub light: Color,
    /// What stands out from the surfaces: a chosen button, a frame to read
    /// inside.
    pub accent: Color,
    /// What marks something as switched on, such as the order being aimed.
    pub lit: Color,
    /// The data's colour for a figure that's gone short; [`Ink::Warn`]'s
    /// without one.
    pub short: Option<Color>,
    /// The data's colour for the key that gives what a name names.
    pub key: Option<Color>,
}

impl Default for Theme {
    fn default() -> Self {
        // A dark olive and a cream, to go with the logo.
        Theme {
            tint: Color::srgb_u8(0x0f, 0x14, 0x0c),
            light: Color::srgb_u8(0xdc, 0xd9, 0xc6),
            accent: Color::srgb(0.75, 0.6, 0.3),
            lit: crate::OWN,
            short: None,
            key: None,
        }
    }
}

impl Theme {
    pub fn surface(&self, surface: Surface) -> Color {
        let see_through = |alpha: f32| self.tint.with_alpha(alpha);
        let lit = |by: f32| mix(self.tint, self.light, by);
        match surface {
            Surface::Solid => self.tint,
            Surface::Hud => see_through(0.85),
            Surface::Popup => see_through(0.7),
            Surface::Slot => see_through(0.6),
            Surface::Scrim => see_through(0.4),
            Surface::Panel => lit(0.07),
            Surface::Track => lit(0.1),
            Surface::Button => lit(0.12),
            Surface::Hovered => lit(0.3),
            Surface::Chosen => mix(self.tint, self.accent, 1.0 / 3.0),
            Surface::Inert => lit(0.12).with_alpha(0.5),
            Surface::Clear => Color::NONE,
        }
    }

    pub fn ink(&self, ink: Ink) -> Color {
        match ink {
            Ink::Body => self.light,
            Ink::Help => mix(self.tint, self.light, 0.8),
            Ink::Muted => mix(self.tint, self.light, 0.45),
            Ink::Warn => Color::srgb(1.0, 0.75, 0.3),
            Ink::Short => self.short.unwrap_or_else(|| self.ink(Ink::Warn)),
            Ink::Key => self.key.unwrap_or(Color::srgb(1.0, 1.0, 0.0)),
        }
    }

    pub fn edge(&self, edge: Edge) -> Color {
        match edge {
            Edge::Border => mix(self.tint, self.light, 0.45),
            Edge::Card => mix(self.tint, self.light, 0.25),
            Edge::Accent => self.accent,
            Edge::Clear => Color::NONE,
            Edge::Hovered => self.light,
            Edge::Lit => self.lit,
        }
    }

    pub fn shade(&self, shade: Shade) -> Color {
        match shade {
            Shade::Full => Color::WHITE,
            Shade::Dimmed => Color::srgb(0.35, 0.35, 0.35),
            Shade::Dark => Color::srgb(0.2, 0.2, 0.2),
        }
    }

    /// A [`Meter`] `share` full.
    pub fn gauge(&self, gauge: Gauge, share: f32) -> Color {
        match (gauge, share) {
            (Gauge::Progress, _) => Color::srgb(0.95, 0.45, 0.1),
            (Gauge::Stockpile, _) => Color::srgb(0.1, 0.85, 0.9),
            (Gauge::Recharge, _) => Color::srgb(1.0, 0.0, 0.0),
            (Gauge::Health, s) if s > 0.66 => Color::srgb(0.1, 0.85, 0.1),
            (Gauge::Health, s) if s > 0.33 => Color::srgb(0.9, 0.85, 0.1),
            (Gauge::Health, _) => Color::srgb(0.9, 0.15, 0.1),
        }
    }
}

/// Part way from `a` to `b`, in sRGB as the theme's greys are given.
fn mix(a: Color, b: Color, by: f32) -> Color {
    a.to_srgba().mix(&b.to_srgba(), by).into()
}

/// What a node's background is for.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[require(BackgroundColor)]
pub enum Surface {
    /// Hides what's behind it, such as the loading screen.
    Solid,
    /// Behind the interface over the map, letting a little of it show.
    Hud,
    /// Behind text that comes and goes over the map, such as a tooltip.
    Popup,
    /// Darkens a place in the [`Surface::Hud`] for something to go.
    Slot,
    /// Dims what's under a menu opened over it.
    Scrim,
    /// Behind the menus' contents.
    Panel,
    /// The empty part of a bar.
    Track,
    /// A button, idle.
    Button,
    /// A button under the pointer.
    Hovered,
    /// A button that's the current choice.
    Chosen,
    /// Laid out like a button, but for something not there yet.
    Inert,
    /// Nothing, over a backdrop drawn with what's around it so the two
    /// don't darken each other.
    Clear,
}

/// What a run of text is for.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[require(TextColor)]
pub enum Ink {
    Body,
    /// Explains what's above it.
    Help,
    /// What can't be changed or chosen.
    Muted,
    /// What's in the way, or needs the player's attention.
    Warn,
    /// A figure that's gone short, such as a negative rate.
    Short,
    /// The key for what a name names, marked in it.
    Key,
}

/// What a node's border is for.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[require(BorderColor)]
pub enum Edge {
    /// Frames a box, such as a card.
    Border,
    /// Frames a [`Card`] button, idle: fainter than a [`Edge::Border`], as
    /// it sits in a panel rather than over the map.
    Card,
    /// Frames what's there to be read.
    Accent,
    /// Leaves room for a frame, but draws none.
    Clear,
    /// Frames what's under the pointer.
    Hovered,
    /// Frames what's switched on.
    Lit,
}

impl Edge {
    /// Frames a button that switches something on: lit while it's on, else
    /// hovered while it's under the pointer, else `idle`.
    pub fn lit(on: bool, hovered: bool, idle: Edge) -> Edge {
        match (on, hovered) {
            (true, _) => Edge::Lit,
            (false, true) => Edge::Hovered,
            (false, false) => idle,
        }
    }
}

/// How brightly a picture is drawn.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[require(ImageNode)]
pub enum Shade {
    Full,
    /// What can't be used yet.
    Dimmed,
    /// What's gone, such as a dead unit.
    Dark,
}

/// A bar on a [`Surface::Track`], filled to a share of its width in a
/// colour that says how that share reads.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
#[require(Surface = Surface::Track)]
pub struct Meter {
    pub gauge: Gauge,
    /// From 0, empty, to 1, full.
    pub share: f32,
}

/// What a [`Meter`] measures.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gauge {
    /// Green when full, then yellow and red as it drops.
    Health,
    Progress,
    /// A relay's supply stockpile.
    Stockpile,
    /// How far something used has come back.
    Recharge,
}

#[derive(Component)]
struct MeterFill;

/// How big a run of text is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Font {
    /// The small print in the interface over the map.
    Small,
    /// The interface over the map.
    Hud,
    /// What heads the interface over the map.
    HudTitle,
    /// The menus.
    Body,
    /// What heads a part of a menu.
    Heading,
    /// A question a dialog asks.
    Question,
    /// The menus' main buttons.
    Large,
    /// What heads a whole screen.
    Title,
}

impl Font {
    pub const fn px(self) -> f32 {
        match self {
            Font::Small => 13.0,
            Font::Hud => 14.0,
            Font::HudTitle => 15.0,
            Font::Body => 16.0,
            Font::Heading => 18.0,
            Font::Question => 20.0,
            Font::Large => 24.0,
            Font::Title => 26.0,
        }
    }

    /// The face it's set in: the logo's lettering, its stencil where it's
    /// big enough to keep its letters whole, and the same family's plain cut,
    /// drawn for small sizes, elsewhere.
    fn source(self) -> FontSource {
        match self {
            Font::Heading | Font::Large | Font::Title => FontSource::Handle(STENCIL),
            _ => FontSource::Handle(PLAIN),
        }
    }
}

const STENCIL: Handle<Face> = uuid_handle!("8ce28202-5c4b-41aa-b4eb-656a8714c917");
const PLAIN: Handle<Face> = uuid_handle!("f5c773eb-4ca7-4c04-80e4-2f691862ea4e");

fn faces(mut fonts: ResMut<Assets<Face>>) {
    let faces = [
        (
            STENCIL,
            &include_bytes!("../../../assets/fonts/BigShouldersStencilDisplay-800.ttf")[..],
        ),
        (
            PLAIN,
            &include_bytes!("../../../assets/fonts/BigShouldersText-600.ttf")[..],
        ),
    ];
    for (handle, bytes) in faces {
        // Never fails for a fixed handle.
        let _ = fonts.insert(handle.id(), Face::from_bytes(bytes.to_vec()));
    }
}

pub fn font(font: Font) -> TextFont {
    TextFont {
        font: font.source(),
        font_size: FontSize::Px(font.px()),
        ..default()
    }
}

/// Text that the pointer passes through, coloured by what's added to it.
fn words(words: impl Into<String>, size: Font) -> impl Bundle {
    (Text::new(words), font(size), Pickable::IGNORE)
}

/// Looks up the game's text by key, falling back to the given words where
/// its strings lack that key.
pub fn said(strings: &aaa_data::Strings) -> impl Fn(&str, &str) -> String + Copy + '_ {
    |key, or| strings.get(key).map_or(or, String::as_str).to_string()
}

/// Text in one of the theme's [`Ink`]s.
pub fn label(words: impl Into<String>, size: Font, ink: Ink) -> impl Bundle {
    (self::words(words, size), ink)
}

/// Text in a colour of its own, which the theme leaves alone, such as a
/// side's.
pub fn coloured(words: impl Into<String>, size: Font, colour: Color) -> impl Bundle {
    (self::words(words, size), TextColor(colour))
}

/// A name with the key that gives what it names marked in it: what
/// comes before the key, the key, and what comes after.
#[derive(Component, Clone, PartialEq, Eq, Default, Debug)]
pub struct Marked {
    pub before: String,
    pub key: String,
    pub after: String,
}

impl Marked {
    /// A name with no key to mark.
    pub fn plain(name: impl Into<String>) -> Self {
        Marked {
            before: name.into(),
            ..default()
        }
    }
}

/// A [`label`] of a name with its key marked.
pub fn marked(parent: &mut ChildSpawnerCommands, name: &Marked, size: Font, ink: Ink) {
    let line = label(name.before.clone(), size, ink);
    parent.spawn(line).with_children(|line| {
        line.spawn((TextSpan::new(name.key.clone()), font(size), Ink::Key));
        line.spawn((TextSpan::new(name.after.clone()), font(size), ink));
    });
}

/// A [`label`] in [`Ink::Body`].
pub fn text(words: impl Into<String>, size: Font) -> impl Bundle {
    label(words, size, Ink::Body)
}

/// A [`panel`]'s corners.
const CORNER: f32 = 12.0;

/// The box the interface's boxes are built on: a background for what it's
/// for, laid out and shaped as `node`, such as the minimap's disc or a strip
/// along the screen's edge.
pub fn backed(surface: Surface, node: Node) -> impl Bundle {
    (node, surface)
}

/// A [`backed`] box with the interface's rounded corners.
pub fn panel(surface: Surface, node: Node) -> impl Bundle {
    backed(
        surface,
        Node {
            border_radius: BorderRadius::all(px(CORNER)),
            ..node
        },
    )
}

/// A [`panel`] of text over the map, which the pointer passes through.
pub fn tooltip(node: Node) -> impl Bundle {
    (
        panel(
            Surface::Popup,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                padding: UiRect::all(px(CORNER)),
                ..node
            },
        ),
        Pickable::IGNORE,
    )
}

/// What's said of the part of the interface the pointer's on; under its
/// name, for a part that also has a [`Marked`] one. It's said in the box
/// its pane [`Docks`] hints in, or else in one by the pointer.
#[derive(Component, Clone, PartialEq, Debug)]
#[require(Hovered, Pickable)]
pub struct Hint(pub String);

/// On a pane, the [`hint_box`] that the [`Hint`]s inside it are said in.
#[derive(Component)]
pub struct Docks(pub Entity);

/// The widest a hint by the pointer is before its words wrap.
const HINT_WIDTH: f32 = 320.0;

/// A box [`Hint`]s are said in.
#[derive(Component)]
struct HintBox;

/// The box for the hints of what no pane docks.
#[derive(Component)]
struct ByPointer;

/// The parts of the hint's text after the first: a name's key, then the
/// rest.
#[derive(Component)]
struct HintSpan(usize);

/// A box, laid out as `node`, that shows while a [`Hint`] is said in it.
pub fn hint_box(node: Node) -> impl Bundle {
    let node = Node {
        display: Display::None,
        ..node
    };
    let span = |n: usize, ink: Ink| (HintSpan(n), TextSpan::default(), font(Font::Small), ink);
    (
        HintBox,
        tooltip(node),
        children![(
            label("", Font::Small, Ink::Body),
            children![span(0, Ink::Key), span(1, Ink::Body)],
        )],
    )
}

fn spawn_hint(mut commands: Commands) {
    let node = Node {
        position_type: PositionType::Absolute,
        max_width: px(HINT_WIDTH),
        ..default()
    };
    commands.spawn((ByPointer, hint_box(node), GlobalZIndex(layer::HINT)));
}

/// Shows the hint under the pointer: in its pane's box, or beside the
/// pointer, on the side with more room.
#[allow(clippy::too_many_arguments)]
fn hint(
    hints: Query<(
        Entity,
        &Hint,
        Option<&Marked>,
        &Hovered,
        &InheritedVisibility,
    )>,
    parents: Query<&ChildOf>,
    docks: Query<&Docks>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    scale: Res<UiScale>,
    mut boxes: Query<(Entity, &mut Node, &Children, Has<ByPointer>), With<HintBox>>,
    mut texts: Query<(&mut Text, &Children)>,
    mut spans: Query<(&HintSpan, &mut TextSpan)>,
) {
    // A part counts as hovered with everything it's in, so of those with
    // something to say, the innermost.
    let hovered = hints
        .iter()
        .filter(|(_, h, name, on, shown)| {
            on.get() && shown.get() && !(h.0.is_empty() && name.is_none())
        })
        .max_by_key(|(entity, ..)| parents.iter_ancestors(*entity).count());
    let dock = hovered.and_then(|(entity, ..)| {
        let mut panes = parents.iter_ancestors(entity);
        panes.find_map(|pane| Some(docks.get(pane).ok()?.0))
    });
    let at = windows
        .single()
        .ok()
        .and_then(|w| Some((w.cursor_position()?, w.size())));
    for (entity, mut node, children, by_pointer) in &mut boxes {
        let here = match dock {
            Some(dock) => dock == entity,
            None => by_pointer && at.is_some(),
        };
        let (Some((_, hint, name, ..)), true) = (hovered, here) else {
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        };
        if node.display != Display::Flex {
            node.display = Display::Flex;
        }
        if let (true, Some((cursor, window))) = (by_pointer, at) {
            const GAP: f32 = 16.0;
            // Each way, on the side of the pointer with more of the window.
            let beside = |at: f32, across: f32| match at > across / 2.0 {
                true => (Val::Auto, px((across - at + GAP) / scale.0)),
                false => (px((at + GAP) / scale.0), Val::Auto),
            };
            (node.left, node.right) = beside(cursor.x, window.x);
            (node.top, node.bottom) = beside(cursor.y, window.y);
        }
        let parts = match name {
            Some(name) => [
                name.before.clone(),
                name.key.clone(),
                format!("{}\n{}", name.after, hint.0),
            ],
            None => [hint.0.clone(), String::new(), String::new()],
        };
        for child in children.iter() {
            let Ok((mut text, parts_after)) = texts.get_mut(child) else {
                continue;
            };
            if text.0 != parts[0] {
                text.0.clone_from(&parts[0]);
            }
            for part in parts_after.iter() {
                if let Ok((span, mut text)) = spans.get_mut(part)
                    && text.0 != parts[span.0 + 1]
                {
                    text.0.clone_from(&parts[span.0 + 1]);
                }
            }
        }
    }
}

/// A solid box with a thin border, such as a card.
pub fn framed(node: Node) -> impl Bundle {
    (
        Node {
            border: UiRect::all(px(1)),
            ..node
        },
        Edge::Border,
        Surface::Solid,
    )
}

/// A [`Meter`] laid out as `node`, empty until it's given a share.
pub fn meter(node: Node, gauge: Gauge) -> impl Bundle {
    (
        node,
        Meter { gauge, share: 0.0 },
        Pickable::IGNORE,
        children![(
            MeterFill,
            Node {
                height: percent(100),
                width: percent(0),
                ..default()
            },
            BackgroundColor(Color::NONE),
            Pickable::IGNORE,
        )],
    )
}

/// How wide a [`slider`]'s thumb is.
const THUMB: f32 = 12.0;

/// A bar to drag along, laid out as `node`, choosing a share from 0 to 1
/// that starts at `value`. What it chooses is told by
/// [`ValueChange`](bevy::ui_widgets::ValueChange), and only shown once
/// it's put back as its [`SliderValue`].
pub fn slider(node: Node, value: f32) -> impl Bundle {
    let fill = |node: Node| Node {
        position_type: PositionType::Absolute,
        left: px(0),
        ..node
    };
    (
        Slider {
            track_click: TrackClick::Snap,
            ..default()
        },
        SliderValue(value),
        SliderRange::new(0.0, 1.0),
        SliderStep(0.01),
        Node {
            height: px(20),
            align_items: AlignItems::Center,
            ..node
        },
        children![
            (
                fill(Node {
                    right: px(0),
                    height: px(4),
                    border_radius: BorderRadius::all(px(2)),
                    ..default()
                }),
                Surface::Track,
            ),
            // The thumb travels the width less its own.
            (
                fill(Node {
                    right: px(THUMB),
                    height: percent(100),
                    ..default()
                }),
                children![(
                    SliderThumb,
                    fill(Node {
                        width: px(THUMB),
                        height: percent(100),
                        border_radius: BorderRadius::all(px(3)),
                        ..default()
                    }),
                    Surface::Chosen,
                )],
            ),
        ],
    )
}

/// Puts each slider's thumb where its value is.
fn place_thumbs(
    sliders: Query<(Entity, &SliderValue, &SliderRange), Changed<SliderValue>>,
    children: Query<&Children>,
    mut thumbs: Query<&mut Node, With<SliderThumb>>,
) {
    for (slider, value, range) in &sliders {
        let at = percent(range.thumb_position(value.0) * 100.0);
        for e in children.iter_descendants(slider) {
            if let Ok(mut thumb) = thumbs.get_mut(e) {
                thumb.left = at;
            }
        }
    }
}

/// A list that drops below what opens it, over what's there, as wide as
/// it.
pub fn dropdown() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            top: percent(100),
            // As wide as its opener, or its longest choice where that's
            // wider.
            min_width: percent(100),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(px(1)),
            ..default()
        },
        Edge::Border,
        Surface::Panel,
    )
}

/// Where what's drawn over the rest of the interface stacks, lowest first.
pub mod layer {
    /// A menu over the match, on a [`scrim`](super::scrim).
    pub const DIALOG: i32 = 10;
    /// A page over the whole screen, such as the options.
    pub const PAGE: i32 = 15;
    /// An open [`Dropdown`](super::Dropdown)'s list.
    pub const LIST: i32 = 20;
    /// A question that wants an answer before anything else.
    pub const QUESTION: i32 = 25;
    /// The box that says what the cursor is over.
    pub const HINT: i32 = 30;
    /// The console.
    pub const CONSOLE: i32 = 100;
}

/// Dims the whole screen under a [`dialog`] on top of it, drawn at
/// `layer`.
pub fn scrim(layer: i32) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Surface::Scrim,
        GlobalZIndex(layer),
    )
}

/// A column of buttons in a [`panel`], such as a menu over the match.
pub fn dialog() -> impl Bundle {
    panel(
        Surface::Panel,
        Node {
            width: px(320),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(32)),
            row_gap: px(12),
            ..default()
        },
    )
}

/// A button, lit while hovered or [`Chosen`].
#[derive(Component)]
pub struct MenuButton;

/// Marks a [`MenuButton`] as the current choice.
#[derive(Component, Default, PartialEq)]
pub struct Chosen(pub bool);

/// The interface audio a button plays as it's pressed, as the game's
/// widget templates give each kind of widget, and whether a right press
/// sounds it as well as a left.
#[derive(Component)]
pub struct Click {
    audio: &'static str,
    right: bool,
}

impl Click {
    /// The game's plain button press.
    pub const PUSH: Click = Click {
        audio: PUSH,
        right: true,
    };
    /// The game's press of a button that opens a list.
    pub const OPEN: Click = Click {
        audio: OPEN,
        right: false,
    };
    /// The game's press of a button that stays on or off.
    pub const TOGGLE: Click = Click {
        audio: TOGGLE,
        right: false,
    };
    /// The command panel's: toggle buttons that are pressed like push
    /// buttons.
    pub const COMMAND: Click = Click {
        audio: TOGGLE,
        right: true,
    };

    pub fn play(&self, sounds: &mut Sounds, time: &Time) {
        sounds.interface(time, self.audio);
    }
}

const PUSH: &str = "push_button";
const OPEN: &str = "tiled_button";
const TOGGLE: &str = "toggle_button";

/// Every [`Click`], for [`Sounds`] to have ready.
pub const CLICKS: [&str; 3] = [PUSH, OPEN, TOGGLE];

/// A button laid out as `node`, that does something when
/// [`Activate`](bevy::ui_widgets::Activate)d, given an observer.
pub fn clickable(node: Node) -> impl Bundle {
    sounding(node, Click::PUSH)
}

/// A [`clickable`] that opens a list to choose from.
pub fn opener(node: Node) -> impl Bundle {
    sounding(node, Click::OPEN)
}

/// A [`clickable`] that's switched on or off, alone or as one of a group
/// to choose from.
pub fn switch(node: Node) -> impl Bundle {
    sounding(node, Click::TOGGLE)
}

fn sounding(node: Node, click: Click) -> impl Bundle {
    (
        MenuButton,
        click,
        Button,
        Hovered::default(),
        Chosen::default(),
        node,
        Surface::Button,
    )
}

/// Laid out like a button, but for something not there yet.
pub fn inert(node: Node) -> impl Bundle {
    (node, Surface::Inert)
}

/// A button's padding around its label.
pub fn padded() -> Node {
    Node {
        padding: UiRect::axes(px(12), px(6)),
        justify_content: JustifyContent::Center,
        ..default()
    }
}

/// A button that stands alone, framed and rounded like a card, rather than
/// one of a list or table's cells; its frame lights as it's hovered or
/// chosen.
#[derive(Component, Default)]
#[require(Edge = Edge::Card)]
pub struct Card;

/// The shape of a [`Card`] laid out as `node`.
pub fn card(node: Node) -> Node {
    Node {
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(8)),
        ..node
    }
}

pub fn button(words: impl Into<String>, size: Font) -> impl Bundle {
    shaped_button(words, size, padded())
}

/// A [`button`] laid out as `node`, which should keep [`padded`]'s padding.
pub fn shaped_button(words: impl Into<String>, size: Font, node: Node) -> impl Bundle {
    (clickable(card(node)), Card, children![text(words, size)])
}

/// A [`switch`] with words on it.
pub fn toggle(words: impl Into<String>, size: Font) -> impl Bundle {
    (switch(card(padded())), Card, children![text(words, size)])
}

/// Laid out like a [`button`], but for something not there yet.
pub fn disabled(words: impl Into<String>, size: Font) -> impl Bundle {
    (
        inert(card(padded())),
        Card,
        children![label(words, size, Ink::Muted)],
    )
}

/// A button in a row saying what's chosen, over the list of choices while
/// it's open, which is drawn over everything else.
pub struct Dropdown {
    pub row: Entity,
    pub width: Val,
    pub label: Shown,
    pub open: bool,
}

/// What a [`Dropdown`] shows for a choice: words, after a picture if it has
/// one, and a [`Hint`] if it has one.
#[derive(Default)]
pub struct Shown {
    pub icon: Option<Handle<Image>>,
    pub words: String,
    pub hint: Option<String>,
}

impl From<String> for Shown {
    fn from(words: String) -> Self {
        Shown { words, ..default() }
    }
}

impl From<&str> for Shown {
    fn from(words: &str) -> Self {
        words.to_string().into()
    }
}

impl Shown {
    pub fn spawn(self, commands: &mut Commands, cell: Entity) {
        if let Some(hint) = self.hint {
            commands.entity(cell).insert(Hint(hint));
        }
        if let Some(icon) = self.icon {
            let gap = match self.words.is_empty() {
                true => 0.0,
                false => 6.0,
            };
            let size = Node {
                margin: UiRect::right(px(gap)),
                ..default()
            };
            commands.spawn((ChildOf(cell), size, ImageNode::new(icon)));
        }
        if !self.words.is_empty() {
            // Unwrapped, so a list as wide as its longest choice is as tall
            // as its choices.
            let words = (
                text(self.words, Font::Body),
                TextLayout::new(Justify::Left, LineBreak::NoWrap),
            );
            commands.spawn((ChildOf(cell), words));
        }
    }
}

impl Dropdown {
    pub fn spawn<B: Bundle, C: Bundle, S: Into<Shown>, M1, M2>(
        self,
        commands: &mut Commands,
        opener_marker: B,
        on_open: impl IntoEntityObserver<M1>,
        choices: impl Iterator<Item = (C, S)>,
        on_choose: impl IntoEntityObserver<M2> + Copy,
    ) {
        let cell = Node {
            width: percent(100),
            justify_content: JustifyContent::Start,
            align_items: AlignItems::Center,
            ..padded()
        };
        let holder = Node {
            width: self.width,
            ..default()
        };
        let holder = commands.spawn((ChildOf(self.row), holder)).id();
        let label = commands
            .spawn((ChildOf(holder), opener(cell.clone()), opener_marker))
            .observe(on_open)
            .id();
        self.label.spawn(commands, label);
        if !self.open {
            return;
        }
        let list = commands
            .spawn((ChildOf(holder), dropdown(), GlobalZIndex(layer::LIST)))
            .id();
        for (choice, shown) in choices {
            let item = commands
                .spawn((ChildOf(list), clickable(cell.clone()), choice))
                .observe(on_choose)
                .id();
            shown.into().spawn(commands, item);
        }
    }
}

type Lighting<'a> = (
    &'a Hovered,
    &'a Chosen,
    &'a mut Surface,
    Option<&'a mut Edge>,
    Has<Card>,
);

/// Lights buttons, and the frames of [`Card`]s only, leaving any other
/// button's frame to whatever sets it.
fn highlight(mut buttons: Query<Lighting, With<MenuButton>>) {
    for (hovered, chosen, mut surface, edge, card) in &mut buttons {
        let (fill, frame) = match (hovered.get(), chosen.0) {
            (true, _) => (Surface::Hovered, Edge::Hovered),
            (false, true) => (Surface::Chosen, Edge::Accent),
            (false, false) => (Surface::Button, Edge::Card),
        };
        surface.set_if_neq(fill);
        if let (Some(mut edge), true) = (edge, card) {
            edge.set_if_neq(frame);
        }
    }
}

/// Plays a button's [`Click`] as it's pressed, not once it's let go and
/// activates.
fn click(
    press: On<PointerPress>,
    buttons: Query<&Click>,
    mut sounds: ResMut<Sounds>,
    time: Res<Time>,
) {
    let Ok(click) = buttons.get(press.entity) else {
        return;
    };
    let sounds_it = match press.button {
        PointerButton::Primary => true,
        PointerButton::Secondary => click.right,
        PointerButton::Middle => false,
    };
    if sounds_it {
        click.play(&mut sounds, &time);
    }
}

fn paint(theme: Res<Theme>, mut nodes: Query<(Ref<Surface>, &mut BackgroundColor)>) {
    for (surface, mut colour) in &mut nodes {
        if theme.is_changed() || surface.is_changed() {
            colour.set_if_neq(BackgroundColor(theme.surface(*surface)));
        }
    }
}

fn ink(theme: Res<Theme>, mut texts: Query<(Ref<Ink>, &mut TextColor)>) {
    for (ink, mut colour) in &mut texts {
        if theme.is_changed() || ink.is_changed() {
            colour.set_if_neq(TextColor(theme.ink(*ink)));
        }
    }
}

fn edge(theme: Res<Theme>, mut nodes: Query<(Ref<Edge>, &mut BorderColor)>) {
    for (edge, mut colour) in &mut nodes {
        if theme.is_changed() || edge.is_changed() {
            colour.set_if_neq(BorderColor::all(theme.edge(*edge)));
        }
    }
}

fn shade(theme: Res<Theme>, mut pictures: Query<(Ref<Shade>, &mut ImageNode)>) {
    for (shade, mut image) in &mut pictures {
        let colour = theme.shade(*shade);
        if (theme.is_changed() || shade.is_changed()) && image.color != colour {
            image.color = colour;
        }
    }
}

fn fill(
    theme: Res<Theme>,
    meters: Query<(Ref<Meter>, &Children)>,
    mut fills: Query<(&mut Node, &mut BackgroundColor), With<MeterFill>>,
) {
    for (meter, children) in &meters {
        if !theme.is_changed() && !meter.is_changed() {
            continue;
        }
        let share = meter.share.clamp(0.0, 1.0);
        for &child in children {
            let Ok((mut node, mut colour)) = fills.get_mut(child) else {
                continue;
            };
            let width = percent(share * 100.0);
            if node.width != width {
                node.width = width;
            }
            colour.set_if_neq(BackgroundColor(theme.gauge(meter.gauge, share)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_and_white_give_greys() {
        let theme = Theme {
            tint: Color::BLACK,
            light: Color::WHITE,
            ..default()
        };
        let grey = |s| theme.surface(s).to_srgba();
        assert!((grey(Surface::Panel).red - 0.07).abs() < 1e-5);
        assert!((grey(Surface::Hovered).green - 0.3).abs() < 1e-5);
        assert_eq!(grey(Surface::Hud).alpha, 0.85);
    }

    #[test]
    fn the_default_panel_is_the_logos_olive() {
        let [r, g, b, _] = Theme::default()
            .surface(Surface::Panel)
            .to_srgba()
            .to_u8_array();
        // The logo's ground, 0x1d2219, give or take rounding.
        assert!(r.abs_diff(0x1d) <= 1 && g.abs_diff(0x22) <= 1 && b.abs_diff(0x19) <= 1);
    }

    #[test]
    fn a_hint_is_said_in_the_box_its_pane_docks() {
        let mut app = App::new();
        app.init_resource::<UiScale>()
            .add_systems(Startup, spawn_hint)
            .add_systems(Update, hint);
        let world = app.world_mut();
        let docked = world.spawn(hint_box(Node::default())).id();
        let other = world.spawn(hint_box(Node::default())).id();
        let pane = world.spawn(Docks(docked)).id();
        world.spawn(Docks(other));
        let on = (Hint("Entrenched".into()), Hovered(true));
        world.spawn((on, InheritedVisibility::VISIBLE, ChildOf(pane)));
        app.update();
        let world = app.world_mut();
        let by_pointer = world
            .query_filtered::<Entity, With<ByPointer>>()
            .single(world)
            .unwrap();
        let shown = |e: Entity| world.get::<Node>(e).map(|n| n.display);
        assert_eq!(shown(docked), Some(Display::Flex));
        assert_eq!(shown(other), Some(Display::None));
        assert_eq!(shown(by_pointer), Some(Display::None));
        let text = world.get::<Children>(docked).unwrap()[0];
        assert_eq!(world.get::<Text>(text).unwrap().0, "Entrenched");
    }

    fn lit(hovered: bool, chosen: bool, card: bool) -> Edge {
        let mut app = App::new();
        app.add_systems(Update, highlight);
        let mut button = app.world_mut().spawn((
            MenuButton,
            Hovered(hovered),
            Chosen(chosen),
            Surface::Button,
            Edge::Lit,
        ));
        if card {
            button.insert(Card);
        }
        let button = button.id();
        app.update();
        *app.world().get::<Edge>(button).unwrap()
    }

    #[test]
    fn a_cards_frame_lights_with_it() {
        assert_eq!(lit(true, false, true), Edge::Hovered);
        assert_eq!(lit(false, true, true), Edge::Accent);
        assert_eq!(lit(false, false, true), Edge::Card);
    }

    #[test]
    fn other_buttons_keep_their_own_frames() {
        assert_eq!(lit(true, false, false), Edge::Lit);
    }

    #[test]
    fn a_meter_fills_to_its_share() {
        let mut app = App::new();
        app.init_resource::<Theme>().add_systems(Update, fill);
        let bar = app
            .world_mut()
            .spawn(meter(Node::default(), Gauge::Health))
            .id();
        app.world_mut().get_mut::<Meter>(bar).unwrap().share = 0.25;
        app.update();
        let child = app.world().get::<Children>(bar).unwrap()[0];
        let node = app.world().get::<Node>(child).unwrap();
        assert_eq!(node.width, percent(25));
        let colour = app.world().get::<BackgroundColor>(child).unwrap().0;
        assert_eq!(colour, Theme::default().gauge(Gauge::Health, 0.25));
    }

    #[test]
    fn retinting_repaints_what_is_up() {
        let mut app = App::new();
        app.init_resource::<Theme>()
            .add_systems(Update, (paint, ink, edge));
        let card = app.world_mut().spawn(framed(Node::default())).id();
        app.update();
        let tint = Color::srgb(0.0, 0.1, 0.2);
        app.world_mut().resource_mut::<Theme>().tint = tint;
        app.update();
        let background = app.world().get::<BackgroundColor>(card).unwrap();
        assert_eq!(background.0, tint);
    }
}
