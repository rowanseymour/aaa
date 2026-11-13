//! The menu Escape opens over a match, which holds the match still while
//! it's up.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::actions::Page;
use crate::menu;
use crate::options::Options;
use crate::ui::{self, Font};
use crate::{Focus, LoadingScreen, Screen, SelectionBox, console};

/// Whether the menu is up, and whether the match was already paused
/// before it went up, as the console can leave it.
#[derive(Resource, Default)]
pub struct PauseMenu {
    open: bool,
    was_paused: bool,
}

#[derive(Component)]
struct Overlay;

/// Opening and closing the menu on Escape, which what else Escape closes
/// goes after, so as not to have the same press open the menu.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Toggle;

pub struct PausePlugin;

impl Plugin for PausePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PauseMenu>()
            .add_systems(OnEnter(Screen::Match), reset)
            .add_systems(
                Update,
                toggle
                    .in_set(Toggle)
                    .before(crate::actions::back_out)
                    .run_if(in_state(Screen::Match))
                    .run_if(console::closed)
                    .run_if(crate::technologies::closed),
            );
    }
}

/// Whether the menu is down, so the match takes input.
pub fn closed(menu: Res<PauseMenu>) -> bool {
    !menu.open
}

fn reset(mut menu: ResMut<PauseMenu>) {
    *menu = PauseMenu::default();
}

/// Opens or closes the menu on Escape, unless Escape is backing out of an
/// action page or the loading screen is still up.
#[allow(clippy::too_many_arguments)]
fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    page: Res<Page>,
    loading: Query<(), With<LoadingScreen>>,
    mut commands: Commands,
    menu: ResMut<PauseMenu>,
    time: ResMut<Time<Virtual>>,
    overlays: Query<Entity, With<Overlay>>,
    mut focus: ResMut<Focus>,
    mut boxes: Query<&mut Visibility, With<SelectionBox>>,
) {
    if !keys.just_pressed(KeyCode::Escape) || !loading.is_empty() {
        return;
    }
    if menu.open {
        close(&mut commands, menu, time, &overlays);
    } else if *page == Page::Orders {
        // What the map was showing of the cursor would otherwise stay up
        // under the menu, as nothing updates it while it's open.
        focus.set_if_neq(Focus::Nothing);
        boxes.iter_mut().for_each(|mut v| *v = Visibility::Hidden);
        open(&mut commands, menu, time);
    }
}

fn open(commands: &mut Commands, mut menu: ResMut<PauseMenu>, mut time: ResMut<Time<Virtual>>) {
    menu.open = true;
    menu.was_paused = time.is_paused();
    time.pause();

    let overlay = commands
        .spawn((
            Overlay,
            DespawnOnExit(Screen::Match),
            ui::scrim(ui::layer::DIALOG),
        ))
        .id();
    let column = commands.spawn((ChildOf(overlay), ui::dialog())).id();
    commands
        .spawn((ui::button("Options", Font::Large), ChildOf(column)))
        .observe(options);
    commands
        .spawn((ui::button("Quit to Desktop", Font::Large), ChildOf(column)))
        .observe(menu::ask_quit);
    commands
        .spawn((ui::button("Return to Game", Font::Large), ChildOf(column)))
        .observe(resume);
}

/// Shows the options over the menu, to go with it when it closes.
fn options(
    _: On<Activate>,
    mut commands: Commands,
    options: Res<Options>,
    overlay: Single<Entity, With<Overlay>>,
) {
    let page = crate::options::page(&mut commands, &options);
    commands.entity(page).insert(ChildOf(*overlay));
}

fn close(
    commands: &mut Commands,
    mut menu: ResMut<PauseMenu>,
    mut time: ResMut<Time<Virtual>>,
    overlays: &Query<Entity, With<Overlay>>,
) {
    menu.open = false;
    if !menu.was_paused {
        time.unpause();
    }
    overlays.iter().for_each(|e| commands.entity(e).despawn());
}

fn resume(
    _: On<Activate>,
    mut commands: Commands,
    menu: ResMut<PauseMenu>,
    time: ResMut<Time<Virtual>>,
    overlays: Query<Entity, With<Overlay>>,
) {
    close(&mut commands, menu, time, &overlays);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use bevy::state::app::StatesPlugin;

    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            PausePlugin,
            crate::confirm::ConfirmPlugin,
        ))
        .insert_state(Screen::Match)
        .insert_resource(crate::confirm::Words::default())
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<console::Console>()
        .init_resource::<Page>()
        .init_resource::<Focus>()
        .insert_resource(Options::of(Vec::new(), None, &HashMap::new()));
        app.update();
        app
    }

    /// Presses the button that says `words`.
    fn press(app: &mut App, words: &str) {
        let world = app.world_mut();
        let mut labels = world.query::<(&Text, &ChildOf)>();
        let found = labels.iter(world).find(|(text, _)| text.0 == words);
        let entity = found.expect("no such button").1.parent();
        world.trigger(Activate { entity });
        app.update();
    }

    fn pages(app: &mut App) -> usize {
        let world = app.world_mut();
        let mut pages = world.query::<&crate::options::Page>();
        pages.iter(world).count()
    }

    fn escape(app: &mut App) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::Escape);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::Escape);
        keys.clear();
    }

    fn open(app: &App) -> bool {
        app.world().resource::<PauseMenu>().open
    }

    fn paused(app: &App) -> bool {
        app.world().resource::<Time<Virtual>>().is_paused()
    }

    #[test]
    fn escape_opens_and_closes_it_pausing_the_match() {
        let mut app = app();
        escape(&mut app);
        assert!(open(&app) && paused(&app));
        escape(&mut app);
        assert!(!open(&app) && !paused(&app));
    }

    #[test]
    fn closing_it_leaves_a_match_paused_before_it_paused() {
        let mut app = app();
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        escape(&mut app);
        escape(&mut app);
        assert!(!open(&app) && paused(&app));
    }

    #[test]
    fn escape_backs_out_of_an_action_page_first() {
        let mut app = app();
        app.insert_resource(Page::Rally);
        escape(&mut app);
        assert!(!open(&app));
    }

    #[test]
    fn the_options_go_over_the_menu_and_leave_it_up() {
        let mut app = app();
        escape(&mut app);
        press(&mut app, "Options");
        assert_eq!(pages(&mut app), 1);
        press(&mut app, "Ok");
        assert_eq!(pages(&mut app), 0);
        assert!(open(&app));
    }

    fn asking(app: &mut App) -> bool {
        let world = app.world_mut();
        let mut root = world.query::<&GlobalZIndex>();
        root.iter(world).any(|z| z.0 == ui::layer::QUESTION)
    }

    #[test]
    fn quitting_asks_first_and_escape_cancels_only_the_question() {
        let mut app = app();
        escape(&mut app);
        press(&mut app, "Quit to Desktop");
        assert!(asking(&mut app));
        assert!(app.world_mut().resource::<Messages<AppExit>>().is_empty());
        escape(&mut app);
        assert!(!asking(&mut app));
        assert!(open(&app) && paused(&app));
    }

    #[test]
    fn escape_closes_the_options_with_the_menu() {
        let mut app = app();
        escape(&mut app);
        press(&mut app, "Options");
        escape(&mut app);
        assert_eq!(pages(&mut app), 0);
        assert!(!open(&app) && !paused(&app));
    }
}
