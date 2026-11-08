//! The menus before a match: the main menu and the kinds of new game.

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

fn main_menu(mut commands: Commands) {
    let column = column(&mut commands, Screen::MainMenu);
    commands
        .spawn((button("New Game", Font::Large), ChildOf(column)))
        .observe(go(Screen::NewGame));
    commands.spawn((disabled("Options", Font::Large), ChildOf(column)));
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
