//! The menu Escape opens over a match, which holds the match still while
//! it's up.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::actions::Page;
use crate::intent::Intent;
use crate::loading::LoadingScreen;
use crate::marks::SelectionBox;
use crate::options::Options;
use crate::ui::{self, Font};
use crate::{Focus, Local, Orders, Screen, Sim, console};
use crate::{confirm, menu};
use tgsim::Action;

/// Whether the menu is up, and whether the match was already paused
/// before it went up, as the console can leave it.
#[derive(Resource, Default)]
pub struct PauseMenu {
    open: bool,
    was_paused: bool,
}

#[derive(Component)]
struct Overlay;

/// What the menu says that the game's data has words for.
#[derive(Resource)]
pub struct Words {
    connection: String,
    /// The entry that leaves for the debriefing: once the match is over,
    /// before then alone, and as an observer.
    end: String,
    abort: String,
    leave: String,
}

impl Words {
    pub fn of(strings: &tgdata::Strings) -> Self {
        let said = ui::said(strings);
        Words {
            connection: crate::connection::Words::menu_entry(strings),
            end: said("ui_game_menu_leave_game_host_game_over", "End Mission"),
            abort: said(
                "ui_game_menu_leave_game_host_game_not_over_only_ai_left",
                "Abort Mission",
            ),
            leave: said("game_menu_ButtonLeaveGame_Client_View_text", "Leave Game"),
        }
    }

    /// What the entry that leaves for the debriefing reads, if there's one:
    /// a player with others to play on against resigns instead.
    fn leaving(&self, over: bool, shared: bool, side: bool) -> Option<&str> {
        match (over, shared, side) {
            (true, _, _) => Some(&self.end),
            (false, false, _) => Some(&self.abort),
            (false, true, false) => Some(&self.leave),
            (false, true, true) => None,
        }
    }
}

impl Default for Words {
    fn default() -> Self {
        Words::of(&tgdata::Strings::default())
    }
}

/// The control panel's button that opens the menu, which the hotkey files
/// bind Escape to.
pub(crate) const MENU: &str = "ButtonMenu";

/// Opening and closing the menu on Escape, which what else Escape closes
/// goes after, so as not to have the same press open the menu.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Toggle;

pub struct PausePlugin;

impl Plugin for PausePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PauseMenu>()
            .init_resource::<Pressed>()
            .init_resource::<Words>()
            .init_resource::<crate::connection::Dialog>()
            .add_systems(OnEnter(Screen::Match), reset)
            .add_systems(
                Update,
                (
                    // Every frame, as what's asked while the menu can't
                    // open isn't for later; and after the keys, so that
                    // Escape's own asking comes with its press.
                    note_press.after(crate::keys::send),
                    toggle
                        .run_if(console::closed)
                        .run_if(crate::keys_free)
                        .run_if(crate::displays::closed)
                        // It goes over the menu, which would open unseen.
                        .run_if(crate::connection::closed),
                )
                    .chain()
                    .in_set(Toggle)
                    .before(crate::actions::back_out)
                    .run_if(in_state(Screen::Match)),
            );
    }
}

/// Whether the menu's button was pressed this frame.
#[derive(Resource, Default)]
struct Pressed(bool);

/// Notes whether the menu's button was pressed, rather than Escape, which
/// the hotkey files bind to it but which backs out of what's up first.
fn note_press(
    keys: Res<ButtonInput<KeyCode>>,
    mut intents: MessageReader<Intent>,
    mut pressed: ResMut<Pressed>,
) {
    let asked = crate::intent::asked(&mut intents, MENU);
    pressed.0 = asked && !keys.just_pressed(KeyCode::Escape);
}

impl PauseMenu {
    pub fn is_open(&self) -> bool {
        self.open
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
/// action page or an armed special operation, or the loading screen is
/// still up, and opens it when its button is pressed.
#[allow(clippy::too_many_arguments)]
fn toggle(
    words: Res<Words>,
    keys: Res<ButtonInput<KeyCode>>,
    button: Res<Pressed>,
    page: Res<Page>,
    mut armed: ResMut<crate::special_ops::Armed>,
    loading: Query<(), With<LoadingScreen>>,
    mut commands: Commands,
    menu: ResMut<PauseMenu>,
    orders: ResMut<Orders>,
    overlays: Query<Entity, With<Overlay>>,
    mut focus: ResMut<Focus>,
    mut boxes: Query<&mut Visibility, With<SelectionBox>>,
    sim: Option<Res<Sim>>,
    local: Option<Res<Local>>,
) {
    let escape = keys.just_pressed(KeyCode::Escape);
    let button = button.0;
    if !(escape || button) || !loading.is_empty() {
        return;
    }
    if menu.open {
        // The menu covers its button.
        if escape {
            close(&mut commands, menu, orders, &overlays);
        }
    } else if escape && armed.0.is_some() {
        armed.0 = None;
    } else if button || *page == Page::Orders {
        // What the map was showing of the cursor would otherwise stay up
        // under the menu, as nothing updates it while it's open.
        focus.set_if_neq(Focus::Nothing);
        boxes.iter_mut().for_each(|mut v| *v = Visibility::Hidden);
        // A side of the player's own that's still in a match going on.
        let playing = |&side: &u32| sim.as_ref().is_some_and(|s| !s.0.over && s.0.held(side));
        let side = local.and_then(|l| l.0).filter(playing);
        let over = sim.as_ref().is_some_and(|s| s.0.over);
        let shared = orders.players().is_some();
        let leaving = words.leaving(over, shared, side.is_some());
        let leaving = leaving.map(str::to_string);
        let side = side.filter(|&side| orders.may(&Action::Resign(side)));
        open(&mut commands, &words, menu, orders, side, leaving);
    }
}

/// Puts the menu up, offering to resign `side` if it's the player's to.
fn open(
    commands: &mut Commands,
    words: &Words,
    mut menu: ResMut<PauseMenu>,
    mut orders: ResMut<Orders>,
    side: Option<u32>,
    leaving: Option<String>,
) {
    menu.open = true;
    // A match hosted elsewhere is other people's too, and plays on.
    menu.was_paused = orders.pausing() || orders.joined_to();
    if !orders.joined_to() {
        orders.act(Action::Pause(true));
    }

    let overlay = commands
        .spawn((
            Overlay,
            DespawnOnExit(Screen::Match),
            ui::scrim(ui::layer::DIALOG),
        ))
        .id();
    let column = commands.spawn((ChildOf(overlay), ui::dialog())).id();
    commands
        .spawn((
            ui::button("Options", Font::Large),
            ui::called("pause.options"),
            ChildOf(column),
        ))
        .observe(options);
    let connection = words.connection.clone();
    // Greyed where there's nobody else to be connected to.
    if crate::connection::openable(&orders) {
        commands
            .spawn((
                ui::button(connection, Font::Large),
                ui::called("pause.connection"),
                ChildOf(column),
            ))
            .observe(connection_status);
    } else {
        commands.spawn((ui::disabled(connection, Font::Large), ChildOf(column)));
    }
    if let Some(side) = side {
        commands
            .spawn((
                ui::button("Resign", Font::Large),
                ui::called("pause.resign"),
                ChildOf(column),
            ))
            .observe(ask_resign(side));
    }
    if let Some(leaving) = leaving {
        commands
            .spawn((
                ui::button(leaving, Font::Large),
                ui::called("pause.leave"),
                ChildOf(column),
            ))
            .observe(crate::leaving::ask_end);
    }
    commands
        .spawn((
            ui::button("Quit to Desktop", Font::Large),
            ui::called("pause.quit"),
            ChildOf(column),
        ))
        .observe(ask_quit);
    commands
        .spawn((
            ui::button("Return to Game", Font::Large),
            ui::called("pause.return"),
            ChildOf(column),
        ))
        .observe(resume);
}

/// Opens the connection dialog over the menu.
fn connection_status(_: On<Activate>, mut dialog: ResMut<crate::connection::Dialog>) {
    crate::connection::open(&mut dialog);
}

/// Asks whether to resign `side`, and does if so, closing the menu.
fn ask_resign(side: u32) -> impl Fn(On<Activate>, Commands, Res<confirm::Words>) {
    move |_, mut commands, words| {
        let resign = move |_: On<Activate>,
                           mut commands: Commands,
                           menu: ResMut<PauseMenu>,
                           mut orders: ResMut<Orders>,
                           overlays: Query<Entity, With<Overlay>>| {
            orders.act(Action::Resign(side));
            close(&mut commands, menu, orders, &overlays);
        };
        let question = words.sure.clone();
        confirm::confirm(&mut commands, &words, Screen::Match, question, resign);
    }
}

/// Asks whether to exit the game, and does if so.
fn ask_quit(_: On<Activate>, mut commands: Commands, words: Res<confirm::Words>) {
    let question = words.sure.clone();
    confirm::confirm(&mut commands, &words, Screen::Match, question, menu::quit);
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
    mut orders: ResMut<Orders>,
    overlays: &Query<Entity, With<Overlay>>,
) {
    menu.open = false;
    if !menu.was_paused {
        orders.act(Action::Pause(false));
    }
    overlays.iter().for_each(|e| commands.entity(e).despawn());
}

fn resume(
    _: On<Activate>,
    mut commands: Commands,
    menu: ResMut<PauseMenu>,
    orders: ResMut<Orders>,
    overlays: Query<Entity, With<Overlay>>,
) {
    close(&mut commands, menu, orders, &overlays);
}

#[cfg(test)]
mod tests {

    use bevy::state::app::StatesPlugin;

    use super::*;

    #[test]
    fn the_menu_keeps_the_scale_setting_over_the_original_interface() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(Screen::Match)
            .insert_resource(crate::hud::Hud::Original)
            .insert_resource(crate::ui_scale::UiScaleSetting(1.0))
            .init_resource::<UiScale>()
            .init_resource::<PauseMenu>()
            .add_systems(Update, crate::ui_scale::fit_ui);
        crate::hud::add_states(&mut app);
        let scale = |app: &mut App, open| {
            app.world_mut().resource_mut::<PauseMenu>().open = open;
            app.update();
            app.world().resource::<UiScale>().0
        };
        let size = crate::shot::SIZE.as_vec2();
        let original = crate::hud::original::layout_scale(size);
        let setting = crate::ui_scale::fitted(1.0, size);
        assert_ne!(original, setting);
        assert_eq!(scale(&mut app, false), original);
        assert_eq!(scale(&mut app, true), setting);
        assert_eq!(scale(&mut app, false), original);
    }

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
        .add_message::<Intent>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<console::Console>()
        .init_resource::<Page>()
        .init_resource::<crate::special_ops::Armed>()
        .init_resource::<Focus>()
        .init_resource::<Orders>()
        .insert_resource(Options::of(Vec::new(), None, &tgdata::Strings::default()));
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
        app.world().resource::<Orders>().pausing()
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
        (app.world_mut().resource_mut::<Orders>()).act(Action::Pause(true));
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
    fn its_button_opens_it_from_an_action_page_and_escape_alone_does_not_twice() {
        let mut app = app();
        app.insert_resource(Page::Rally);
        app.world_mut().write_message(Intent::from(MENU));
        app.update();
        assert!(open(&app) && paused(&app));
        // Escape asks for the button as it closes the menu.
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::Escape);
        app.world_mut().write_message(Intent::from(MENU));
        app.update();
        assert!(!open(&app));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.update();
        assert!(!open(&app));
    }

    #[test]
    fn escape_disarms_a_special_operation_first() {
        let mut app = app();
        app.insert_resource(crate::special_ops::Armed(Some(1)));
        escape(&mut app);
        assert!(!open(&app));
        let armed = app.world().resource::<crate::special_ops::Armed>();
        assert_eq!(armed, &crate::special_ops::Armed(None));
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
        let world = app.world_mut();
        let mut texts = world.query::<&Text>();
        assert!(texts.iter(world).any(|t| t.0 == "Are you sure?"));
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

    /// [`app`] with a match hosted here, played as side 1 if `local`
    /// says so, past its first step.
    fn playing(local: Option<u32>) -> App {
        let mut app = app();
        let mut world = crate::orders::testing::fresh();
        let mut orders = Orders::hosting(Default::default(), world.clone(), local, "Player");
        // Until its first step the side goes by nobody's name, and so
        // isn't one a person can resign.
        orders.carry_out(&mut world);
        app.insert_resource(orders)
            .insert_resource(Sim(world))
            .insert_resource(Local(local));
        app
    }

    fn labels(app: &mut App) -> Vec<String> {
        let world = app.world_mut();
        let mut texts = world.query::<&Text>();
        texts.iter(world).map(|t| t.0.clone()).collect()
    }

    #[test]
    fn the_entry_that_leaves_suits_how_the_match_stands() {
        let words = Words::default();
        // Over, alone, shared: with a side, then without.
        assert_eq!(words.leaving(true, true, true), Some("End Mission"));
        assert_eq!(words.leaving(false, false, true), Some("Abort Mission"));
        assert_eq!(words.leaving(false, true, false), Some("Leave Game"));
        assert_eq!(words.leaving(false, true, true), None);
    }

    #[test]
    fn resigning_asks_first_then_resigns_the_player_s_side() {
        let mut app = playing(Some(1));
        escape(&mut app);
        press(&mut app, "Resign");
        assert!(asking(&mut app) && open(&app));
        press(&mut app, "Ok");
        assert!(!open(&app));
        let world = app.world_mut();
        let mut sim = world.remove_resource::<Sim>().unwrap();
        world.resource_mut::<Orders>().carry_out(&mut sim.0);
        let asked = crate::orders::testing::asked(world.resource::<Orders>().record());
        assert!(asked.contains(&Action::Resign(1)));
        assert!(sim.0.resigned(1));
        world.insert_resource(sim);
        // Once it has, there's nothing to resign.
        escape(&mut app);
        assert!(!labels(&mut app).contains(&"Resign".to_string()));
        // Nor for one who only watches.
        let mut app = playing(None);
        escape(&mut app);
        assert!(!labels(&mut app).contains(&"Resign".to_string()));
    }
}
