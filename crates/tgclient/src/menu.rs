//! The menus before a match: the main menu and the kinds of new game, which
//! open over the same screen, as the game has them.

use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use tgrender::textures;

use crate::ui::{Font, Ink, Surface, backed, button, called, disabled, label, panel};
use crate::{Look, Screen, confirm};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_sub_state::<MenuPage>()
            .add_systems(OnEnter(Screen::MainMenu), backdrop)
            .add_systems(OnEnter(MenuPage::Top), main_menu)
            .add_systems(OnEnter(MenuPage::NewGame), new_game)
            .add_systems(OnEnter(MenuPage::Multiplayer), multiplayer);
    }
}

/// Which of the main menu's columns is up.
#[derive(SubStates, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[source(Screen = Screen::MainMenu)]
pub(crate) enum MenuPage {
    #[default]
    Top,
    NewGame,
    Multiplayer,
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

/// An observer that opens one of the main menu's columns, from its own
/// screen or another.
pub fn open(
    page: MenuPage,
) -> impl Fn(On<Activate>, ResMut<NextState<Screen>>, ResMut<NextState<MenuPage>>) {
    move |_, mut screen, mut next| {
        screen.set_if_different(Screen::MainMenu);
        next.set(page);
    }
}

/// What stays on the main menu's screen whichever column is up.
/// The camera comes with [`crate::backdrop`].
fn backdrop(mut commands: Commands) {
    let version = concat!("Version ", env!("CARGO_PKG_VERSION"));
    commands.spawn((
        // Backed to be read over the map.
        panel(
            Surface::Hud,
            Node {
                position_type: PositionType::Absolute,
                left: px(16),
                bottom: px(12),
                padding: UiRect::axes(px(8), px(4)),
                ..default()
            },
        ),
        DespawnOnExit(Screen::MainMenu),
        children![label(version, Font::Body, Ink::Muted)],
    ));
}

/// The column of buttons down the right of the menus, as the game has it.
fn column(commands: &mut Commands, page: MenuPage) -> Entity {
    let root = commands
        .spawn((
            DespawnOnExit(page),
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

fn main_menu(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    look: Res<Look>,
    data: Res<crate::Data>,
    // Decoded once and kept, for each return to the menu.
    mut logo: Local<Option<Option<Handle<Image>>>>,
) {
    let column = column(&mut commands, MenuPage::Top);
    // Our own emblem, where the game has its logo.
    let logo = logo.get_or_insert_with(|| {
        let path = look.logo.as_deref()?;
        if !data.0.exists(path) {
            warn!("no logo {path} in the data");
        }
        textures::read(&data.0, path, formats.0, true).map(|i| images.add(i))
    });
    if logo.is_none() {
        let title = label(&look.title, Font::Title, Ink::Body);
        commands.spawn((title, ChildOf(column)));
    }
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
        .observe(open(MenuPage::NewGame));
    commands
        .spawn((
            button("Multiplayer", Font::Large),
            called("menu.multiplayer"),
            ChildOf(column),
        ))
        .observe(open(MenuPage::Multiplayer));
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
    let column = column(&mut commands, MenuPage::NewGame);
    commands
        .spawn((
            button("Campaign", Font::Large),
            called("menu.campaign"),
            ChildOf(column),
        ))
        .observe(go(Screen::Campaign));
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
        .observe(open(MenuPage::Top));
}

fn multiplayer(mut commands: Commands) {
    let column = column(&mut commands, MenuPage::Multiplayer);
    commands
        .spawn((
            button("LAN", Font::Large),
            called("menu.lan"),
            ChildOf(column),
        ))
        .observe(go(Screen::Lan));
    commands.spawn((disabled("Internet", Font::Large), ChildOf(column)));
    commands.spawn((spacer(), ChildOf(column)));
    commands
        .spawn((
            button("Back", Font::Large),
            called("menu.back"),
            ChildOf(column),
        ))
        .observe(open(MenuPage::Top));
}

#[cfg(test)]
mod tests {
    use bevy::state::app::StatesPlugin;

    use super::*;

    #[derive(Resource, Default)]
    struct Entered(u32);

    /// Presses a button that opens `page`.
    fn press(app: &mut App, page: MenuPage) {
        let world = app.world_mut();
        let entity = world.spawn_empty().observe(open(page)).id();
        world.trigger(Activate { entity });
        app.update();
    }

    #[test]
    fn columns_switch_without_leaving_the_screen() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(Screen::MainMenu)
            .add_sub_state::<MenuPage>()
            .init_resource::<Entered>()
            .add_systems(OnEnter(Screen::MainMenu), |mut e: ResMut<Entered>| e.0 += 1);
        app.update();
        press(&mut app, MenuPage::NewGame);
        let page = |app: &App| *app.world().resource::<State<MenuPage>>().get();
        assert_eq!(page(&app), MenuPage::NewGame);
        press(&mut app, MenuPage::Top);
        assert_eq!(page(&app), MenuPage::Top);
        assert_eq!(app.world().resource::<Entered>().0, 1);
    }
}
