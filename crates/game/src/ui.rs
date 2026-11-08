//! The interface's look, and the pieces screens are built from. Screens say
//! what part each colour plays, as a [`Surface`], [`Ink`] or [`Edge`], and
//! [`Theme`] says what that looks like, so changing the theme restyles the
//! whole interface, even while it's up. [`Shade`] and [`Meter`] do the same
//! for pictures and bars.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use crate::sound::Sounds;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Theme>()
            .add_systems(Update, highlight)
            // Before layout, which a meter's fill width feeds.
            .add_systems(
                PostUpdate,
                (paint, ink, edge, shade, fill).before(bevy::ui::UiSystems::Prepare),
            )
            .add_observer(click);
    }
}

/// What the interface looks like.
#[derive(Resource, Clone, Debug)]
pub struct Theme {
    /// What every surface is shaded from.
    pub tint: Color,
    /// What stands out from the surfaces: a chosen button, a frame to read
    /// inside.
    pub accent: Color,
    /// What marks something as switched on, such as the order being aimed.
    pub lit: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            tint: Color::BLACK,
            accent: Color::srgb(0.75, 0.6, 0.3),
            lit: crate::OWN,
        }
    }
}

impl Theme {
    pub fn surface(&self, surface: Surface) -> Color {
        let see_through = |alpha: f32| self.tint.with_alpha(alpha);
        let lit = |by: f32| mix(self.tint, Color::WHITE, by);
        match surface {
            Surface::Solid => self.tint,
            Surface::Hud => see_through(0.85),
            Surface::Popup => see_through(0.7),
            Surface::Slot => see_through(0.6),
            Surface::Scrim => see_through(0.4),
            Surface::Panel => lit(0.07),
            Surface::Track => lit(0.12),
            Surface::Button => lit(0.15),
            Surface::Hovered => lit(0.3),
            Surface::Chosen => mix(self.tint, self.accent, 1.0 / 3.0),
            Surface::Inert => lit(0.15).with_alpha(0.5),
        }
    }

    pub fn ink(&self, ink: Ink) -> Color {
        match ink {
            Ink::Body => Color::WHITE,
            Ink::Help => Color::srgb(0.8, 0.8, 0.8),
            Ink::Muted => Color::srgb(0.45, 0.45, 0.45),
            Ink::Warn => Color::srgb(1.0, 0.75, 0.3),
        }
    }

    pub fn edge(&self, edge: Edge) -> Color {
        match edge {
            Edge::Border => mix(self.tint, Color::WHITE, 0.45),
            Edge::Accent => self.accent,
            Edge::Clear => Color::NONE,
            Edge::Hovered => Color::WHITE,
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
}

/// What a node's border is for.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[require(BorderColor)]
pub enum Edge {
    /// Frames a box, such as a card.
    Border,
    /// Frames what's there to be read.
    Accent,
    /// Leaves room for a frame, but draws none.
    Clear,
    /// Frames what's under the pointer.
    Hovered,
    /// Frames what's switched on.
    Lit,
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
            Font::Large => 20.0,
            Font::Title => 22.0,
        }
    }
}

pub fn font(font: Font) -> TextFont {
    TextFont {
        font_size: FontSize::Px(font.px()),
        ..default()
    }
}

/// Text that the pointer passes through, coloured by what's added to it.
fn words(words: impl Into<String>, size: Font) -> impl Bundle {
    (Text::new(words), font(size), Pickable::IGNORE)
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

/// A list that drops below what opens it, over what's there, as wide as
/// it.
pub fn dropdown() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            top: percent(100),
            width: percent(100),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(px(1)),
            ..default()
        },
        Edge::Border,
        Surface::Panel,
    )
}

/// Dims the whole screen under a [`dialog`] on top of it.
pub fn scrim() -> impl Bundle {
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
        GlobalZIndex(10),
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
/// widget templates give each kind of widget.
#[derive(Component)]
pub struct Click(&'static str);

impl Click {
    /// The game's plain button press.
    pub const PUSH: Click = Click(PUSH);
    /// The game's press of a button that opens a list.
    pub const OPEN: Click = Click(OPEN);
}

const PUSH: &str = "push_button";
const OPEN: &str = "tiled_button";

/// Every [`Click`], for [`Sounds`] to have ready.
pub const CLICKS: [&str; 2] = [PUSH, OPEN];

/// A button laid out as `node`, that does something when
/// [`Activate`](bevy::ui_widgets::Activate)d, given an observer.
pub fn clickable(node: Node) -> impl Bundle {
    sounding(node, Click::PUSH)
}

/// A [`clickable`] that opens a list to choose from.
pub fn opener(node: Node) -> impl Bundle {
    sounding(node, Click::OPEN)
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

pub fn button(words: impl Into<String>, size: Font) -> impl Bundle {
    (clickable(padded()), children![text(words, size)])
}

pub fn disabled(words: impl Into<String>, size: Font) -> impl Bundle {
    (inert(padded()), children![label(words, size, Ink::Muted)])
}

fn highlight(mut buttons: Query<(&Hovered, &Chosen, &mut Surface), With<MenuButton>>) {
    for (hovered, chosen, mut surface) in &mut buttons {
        surface.set_if_neq(match (hovered.get(), chosen.0) {
            (true, _) => Surface::Hovered,
            (false, true) => Surface::Chosen,
            (false, false) => Surface::Button,
        });
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
    if press.button != PointerButton::Primary {
        return;
    }
    if let Ok(Click(audio)) = buttons.get(press.entity) {
        sounds.interface(&time, audio);
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
    fn a_black_tint_keeps_the_greys() {
        let theme = Theme::default();
        let grey = |s| theme.surface(s).to_srgba();
        assert!((grey(Surface::Panel).red - 0.07).abs() < 1e-5);
        assert!((grey(Surface::Hovered).green - 0.3).abs() < 1e-5);
        assert_eq!(grey(Surface::Hud).alpha, 0.85);
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
