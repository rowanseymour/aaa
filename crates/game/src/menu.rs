//! The menus before a match: the main menu and the kinds of new game.

use aaa_render::textures;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::ui::{Font, Surface, backed, button, called, disabled};
use crate::{Screen, confirm};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::MainMenu), main_menu)
            .add_systems(OnEnter(Screen::NewGame), new_game);
    }
}

/// Asks whether to exit the game, and does if so.
fn ask_quit(_: On<Activate>, mut commands: Commands, words: Res<confirm::Words>) {
    confirm::confirm(&mut commands, &words, Screen::MainMenu, "Quit?", quit);
}

/// An observer that exits the game.
pub fn quit(_: On<Activate>, mut exit: MessageWriter<AppExit>) {
    exit.write(AppExit::Success);
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
    commands.spawn((ChildOf(root), side())).id()
}

/// A [`column`]'s panel, for other pages to have one like it.
pub fn side() -> impl Bundle {
    backed(
        Surface::Panel,
        Node {
            width: px(320),
            height: percent(100),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(32)),
            row_gap: px(12),
            ..default()
        },
    )
}

/// Pushes the buttons after it to the bottom of a [`column`].
pub fn spacer() -> impl Bundle {
    Node {
        flex_grow: 1.0,
        ..default()
    }
}

/// Our own emblem, crowning the main menu's column where the game has its
/// logo.
const LOGO: &[u8] = include_bytes!("../../../assets/logo/logo.png");

fn main_menu(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    // Decoded once and kept, for each return to the menu.
    mut logo: Local<Option<Option<Handle<Image>>>>,
) {
    let column = column(&mut commands, Screen::MainMenu);
    let logo = logo.get_or_insert_with(|| {
        textures::decode(LOGO, "logo.png", formats.0, true).map(|i| images.add(i))
    });
    if let Some(logo) = logo {
        commands.spawn((
            ImageNode::new(logo.clone()),
            Node {
                width: percent(100),
                aspect_ratio: Some(1.0),
                margin: UiRect::bottom(px(12)),
                ..default()
            },
            ChildOf(column),
        ));
    }
    commands
        .spawn((
            button("New Game", Font::Large),
            called("menu.new-game"),
            ChildOf(column),
        ))
        .observe(go(Screen::NewGame));
    commands
        .spawn((
            button("Options", Font::Large),
            called("menu.options"),
            ChildOf(column),
        ))
        .observe(go(Screen::Options));
    commands.spawn((spacer(), ChildOf(column)));
    commands
        .spawn((
            button("Exit", Font::Large),
            called("menu.exit"),
            ChildOf(column),
        ))
        .observe(ask_quit);
}

fn new_game(mut commands: Commands) {
    let column = column(&mut commands, Screen::NewGame);
    commands.spawn((disabled("Campaign", Font::Large), ChildOf(column)));
    commands
        .spawn((
            button("Custom", Font::Large),
            called("menu.custom"),
            ChildOf(column),
        ))
        .observe(go(Screen::Lobby));
    commands.spawn((spacer(), ChildOf(column)));
    commands
        .spawn((
            button("Back", Font::Large),
            called("menu.back"),
            ChildOf(column),
        ))
        .observe(go(Screen::MainMenu));
}
