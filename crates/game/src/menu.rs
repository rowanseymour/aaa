//! The menus before a match: the main menu and the kinds of new game.

use aaa_render::textures;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::Screen;
use crate::ui::{Font, Surface, backed, button, disabled};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::MainMenu), main_menu)
            .add_systems(OnEnter(Screen::NewGame), new_game);
    }
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
    commands
        .spawn((
            ChildOf(root),
            backed(
                Surface::Panel,
                Node {
                    width: px(320),
                    height: percent(100),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(32)),
                    row_gap: px(12),
                    ..default()
                },
            ),
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

/// Our own emblem, crowning the main menu's column where the game has its
/// logo.
const LOGO: &[u8] = include_bytes!("../../../docs/logo/logo.png");

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
        .spawn((button("New Game", Font::Large), ChildOf(column)))
        .observe(go(Screen::NewGame));
    commands
        .spawn((button("Options", Font::Large), ChildOf(column)))
        .observe(go(Screen::Options));
    commands.spawn((spacer(), ChildOf(column)));
    commands
        .spawn((button("Exit", Font::Large), ChildOf(column)))
        .observe(quit);
}

fn new_game(mut commands: Commands) {
    let column = column(&mut commands, Screen::NewGame);
    commands.spawn((disabled("Campaign", Font::Large), ChildOf(column)));
    commands
        .spawn((button("Custom", Font::Large), ChildOf(column)))
        .observe(go(Screen::Lobby));
    commands.spawn((spacer(), ChildOf(column)));
    commands
        .spawn((button("Back", Font::Large), ChildOf(column)))
        .observe(go(Screen::MainMenu));
}
