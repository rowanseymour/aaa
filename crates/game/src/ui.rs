//! The interface's look, and the pieces screens are built from. Screens say
//! what part each colour plays, as a [`Surface`], [`Ink`] or [`Edge`], and
//! [`Theme`] says what that looks like, so changing the theme restyles the
//! whole interface, even while it's up.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use crate::sound::Sounds;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Theme>()
            .add_systems(Update, highlight)
            .add_systems(PostUpdate, (paint, ink, edge))
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
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            tint: Color::BLACK,
            accent: Color::srgb(0.75, 0.6, 0.3),
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
}

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

/// Text that the pointer passes through.
pub fn label(words: impl Into<String>, size: Font, ink: Ink) -> impl Bundle {
    (Text::new(words), font(size), ink, Pickable::IGNORE)
}

/// A [`label`] in [`Ink::Body`].
pub fn text(words: impl Into<String>, size: Font) -> impl Bundle {
    label(words, size, Ink::Body)
}

pub fn panel(surface: Surface, node: Node) -> impl Bundle {
    (node, surface)
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
