//! The menus before a match: the main menu, the kinds of new game, and the
//! buttons they and the [`lobby`](crate::lobby) are made of.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use crate::Screen;
use crate::sound::Sounds;

const IDLE: Color = Color::srgb(0.15, 0.15, 0.15);
const HOVER: Color = Color::srgb(0.3, 0.3, 0.3);
const CHOSEN: Color = Color::srgb(0.25, 0.22, 0.12);
pub const PANEL: Color = Color::srgb(0.07, 0.07, 0.07);
pub const DIM: Color = Color::srgb(0.45, 0.45, 0.45);

/// A menu button, lit while hovered or [`Chosen`].
#[derive(Component)]
pub struct MenuButton;

/// The interface audio a menu button plays as it's pressed, as the game's
/// widget templates give each kind of widget.
#[derive(Component)]
pub struct Click(&'static str);

const PUSH: Click = Click("push_button");
const OPEN: Click = Click("tiled_button");

/// Marks a menu button as the current choice.
#[derive(Component, Default, PartialEq)]
pub struct Chosen(pub bool);

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::MainMenu), main_menu)
            .add_systems(OnEnter(Screen::NewGame), new_game)
            .add_systems(Update, highlight)
            .add_observer(click);
    }
}

pub fn text(label: impl Into<String>, size: f32) -> impl Bundle {
    (
        Text::new(label),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
    )
}

/// A button laid out as `node`, that does something when [`Activate`]d,
/// given an observer.
pub fn clickable(node: Node) -> impl Bundle {
    sounding(node, PUSH)
}

/// A [`clickable`] that opens a list to choose from.
pub fn opener(node: Node) -> impl Bundle {
    sounding(node, OPEN)
}

fn sounding(node: Node, click: Click) -> impl Bundle {
    (
        MenuButton,
        click,
        Button,
        Hovered::default(),
        Chosen::default(),
        node,
        BackgroundColor(IDLE),
    )
}

/// Laid out like a button, but for something not there yet.
pub fn inert(node: Node) -> impl Bundle {
    (node, BackgroundColor(IDLE.with_alpha(0.5)))
}

/// A button's padding around its label.
pub fn padded() -> Node {
    Node {
        padding: UiRect::axes(px(12), px(6)),
        justify_content: JustifyContent::Center,
        ..default()
    }
}

pub fn button(label: impl Into<String>, size: f32) -> impl Bundle {
    (clickable(padded()), children![text(label, size)])
}

pub fn disabled(label: impl Into<String>, size: f32) -> impl Bundle {
    (
        inert(padded()),
        children![(text(label, size), TextColor(DIM))],
    )
}

/// An observer that moves to another screen.
pub fn go(to: Screen) -> impl Fn(On<Activate>, ResMut<NextState<Screen>>) {
    move |_, mut next| next.set(to)
}

/// The column of buttons down the right of the menus, as the game has it.
fn column(commands: &mut Commands, screen: Screen) -> Entity {
    commands.spawn((Camera2d, DespawnOnExit(screen)));
    let root = commands
        .spawn((
            DespawnOnExit(screen),
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::End,
                ..default()
            },
        ))
        .id();
    commands
        .spawn((
            ChildOf(root),
            Node {
                width: px(320),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(32)),
                row_gap: px(12),
                ..default()
            },
            BackgroundColor(PANEL),
        ))
        .id()
}

/// Pushes the buttons after it to the bottom of a [`column`].
fn spacer() -> impl Bundle {
    Node {
        flex_grow: 1.0,
        ..default()
    }
}

const SIZE: f32 = 20.0;

fn main_menu(mut commands: Commands) {
    let column = column(&mut commands, Screen::MainMenu);
    commands
        .spawn((button("New Game", SIZE), ChildOf(column)))
        .observe(go(Screen::NewGame));
    commands.spawn((disabled("Options", SIZE), ChildOf(column)));
    commands.spawn((spacer(), ChildOf(column)));
    commands
        .spawn((button("Exit", SIZE), ChildOf(column)))
        .observe(|_: On<Activate>, mut exit: MessageWriter<AppExit>| {
            exit.write(AppExit::Success);
        });
}

fn new_game(mut commands: Commands) {
    let column = column(&mut commands, Screen::NewGame);
    commands.spawn((disabled("Campaign", SIZE), ChildOf(column)));
    commands
        .spawn((button("Custom", SIZE), ChildOf(column)))
        .observe(go(Screen::Lobby));
    commands.spawn((spacer(), ChildOf(column)));
    commands
        .spawn((button("Back", SIZE), ChildOf(column)))
        .observe(go(Screen::MainMenu));
}

fn highlight(mut buttons: Query<(&Hovered, &Chosen, &mut BackgroundColor), With<MenuButton>>) {
    for (hovered, chosen, mut background) in &mut buttons {
        let color = match (hovered.get(), chosen.0) {
            (true, _) => HOVER,
            (false, true) => CHOSEN,
            (false, false) => IDLE,
        };
        if background.0 != color {
            background.0 = color;
        }
    }
}

/// Plays a button's [`Click`] as it's pressed, not once it's let go and
/// [`Activate`]s.
fn click(
    press: On<PointerPress>,
    buttons: Query<&Click>,
    sounds: Option<ResMut<Sounds>>,
    time: Res<Time>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    if let (Ok(Click(audio)), Some(mut sounds)) = (buttons.get(press.entity), sounds) {
        sounds.interface(&time, audio);
    }
}
