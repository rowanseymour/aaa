//! The options, a page over the whole screen that the main menu and the
//! menu over a match both open, and what's chosen in them.

use std::collections::HashMap;

use aaa_data::{GameData, HotkeyConfiguration, Id};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use bevy::window::{Monitor, PrimaryMonitor, PrimaryWindow};

use crate::Screen;
use crate::supply::Border;
use crate::ui::{
    Chosen, Dropdown, Font, Ink, Surface, clickable, label, padded, switch, text, toggle,
};

const HOTKEYS: &str = "options_dialog_Child_text_6";
const BORDER: &str = "options_dialog_Child_text_15";
const RESOLUTION: &str = "options_dialog_Child_text_18";

/// What the player has chosen, and what there is to choose from.
#[derive(Resource)]
pub struct Options {
    /// The sets of hotkeys the game has, as it lists them.
    configurations: Vec<HotkeyConfiguration>,
    /// The id of the set in use.
    hotkeys: Option<Id>,
    /// What the game calls the choice of hotkeys.
    hotkeys_label: String,
    /// What the game calls the choice to show the supply border.
    border_label: String,
    /// What the game calls the choice of the window's size.
    resolution_label: String,
}

impl Options {
    /// The game's own choices.
    pub fn new(game: &mut GameData) -> Self {
        let strings = game.strings();
        let configurations = game.hotkey_configurations(&strings);
        let named = game.default_hotkey_configuration();
        Options::of(configurations, named, &strings)
    }

    /// With `named` the set of hotkeys in use, or the first where there's
    /// no such set.
    fn of(
        configurations: Vec<HotkeyConfiguration>,
        named: Option<Id>,
        strings: &HashMap<String, String>,
    ) -> Self {
        let known = named.filter(|id| configurations.iter().any(|c| c.id == *id));
        let said = |key: &str, or: &str| strings.get(key).map_or(or, String::as_str).to_string();
        Options {
            hotkeys: known.or_else(|| configurations.first().map(|c| c.id.clone())),
            configurations,
            hotkeys_label: said(HOTKEYS, "Hotkeys"),
            border_label: said(BORDER, "Supply Border"),
            resolution_label: said(RESOLUTION, "Resolution"),
        }
    }

    /// The set of hotkeys in use, if the game has any.
    pub fn hotkeys(&self) -> Option<&HotkeyConfiguration> {
        let chosen = self.hotkeys.as_ref()?;
        self.configurations.iter().find(|c| c.id == *chosen)
    }
}

/// Which list of choices is open.
#[derive(Resource, Default, PartialEq, Clone, Copy, Debug)]
enum Open {
    #[default]
    Nothing,
    Hotkeys,
    Resolution,
}

/// The options shown: those of the game, or of how it's drawn.
#[derive(Resource, Default, PartialEq, Clone, Copy)]
enum Tab {
    #[default]
    Game,
    Video,
}

impl Tab {
    fn name(self) -> &'static str {
        match self {
            Tab::Game => "Game",
            Tab::Video => "Video",
        }
    }
}

/// The options, over the whole screen.
#[derive(Component)]
struct Page;

/// A button showing a [`Tab`]'s options.
#[derive(Component)]
struct TabButton(Tab);

/// Where the shown tab's options go.
#[derive(Component)]
struct Body;

/// Where the choice of hotkeys goes.
#[derive(Component)]
struct Hotkeys;

/// A button choosing a set of hotkeys, by its id.
#[derive(Component)]
struct HotkeysChoice(Id);

/// Where the choice of the window's size goes.
#[derive(Component)]
struct Resolution;

/// A button choosing a size for the window.
#[derive(Component)]
struct ResolutionChoice(UVec2);

/// A button that opens a list of choices, by the list.
#[derive(Component)]
struct Opener(Open);

/// A button of a list of choices: what opens it, or a choice.
#[derive(Component)]
struct InList;

/// The button that shows or hides the supply border.
#[derive(Component)]
struct BorderToggle;

/// The sizes offered for the window, of those the screen has room for.
const RESOLUTIONS: [(u32, u32); 9] = [
    (1024, 768),
    (1280, 720),
    (1280, 800),
    (1366, 768),
    (1440, 900),
    (1600, 900),
    (1920, 1080),
    (2560, 1440),
    (3840, 2160),
];

pub struct OptionsPlugin;

impl Plugin for OptionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Open>()
            .init_resource::<Tab>()
            .add_systems(OnEnter(Screen::Options), screen)
            .add_systems(
                Update,
                (
                    close_list,
                    show_body,
                    show_hotkeys,
                    show_resolution,
                    mark_chosen,
                    show_border,
                )
                    .chain(),
            );
    }
}

fn screen(mut commands: Commands) {
    commands.spawn((Camera2d, DespawnOnExit(Screen::Options)));
    let page = page(&mut commands);
    commands.entity(page).insert(DespawnOnExit(Screen::Options));
}

/// Spawns the options over the whole screen, on top of whatever's there,
/// until their button to leave them is pressed.
pub fn page(commands: &mut Commands) -> Entity {
    commands.insert_resource(Open::Nothing);
    commands.insert_resource(Tab::Game);
    let page = Node {
        position_type: PositionType::Absolute,
        width: percent(100),
        height: percent(100),
        ..default()
    };
    let page = commands
        .spawn((Page, page, Surface::Panel, GlobalZIndex(15)))
        .id();
    let column = Node {
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(32)),
        row_gap: px(12),
        ..default()
    };
    let tabs = Node {
        width: px(240),
        ..column.clone()
    };
    let tabs = commands.spawn((ChildOf(page), tabs)).id();
    commands.spawn((text("Options", Font::Title), ChildOf(tabs)));
    for tab in [Tab::Game, Tab::Video] {
        commands
            .spawn((
                TabButton(tab),
                toggle(tab.name(), Font::Large),
                ChildOf(tabs),
            ))
            .observe(move |_: On<Activate>, mut shown: ResMut<Tab>| {
                shown.set_if_neq(tab);
            });
    }
    let main = Node {
        flex_grow: 1.0,
        ..column.clone()
    };
    let main = commands.spawn((ChildOf(page), main)).id();
    let body = Node {
        flex_grow: 1.0,
        flex_direction: FlexDirection::Column,
        row_gap: px(24),
        ..default()
    };
    commands.spawn((Body, ChildOf(main), body));
    let footer = Node {
        justify_content: JustifyContent::End,
        ..default()
    };
    let footer = commands.spawn((ChildOf(main), footer)).id();
    let ok = Node {
        width: px(160),
        ..padded()
    };
    commands
        .spawn((
            ChildOf(footer),
            clickable(ok),
            children![text("Ok", Font::Large)],
        ))
        .observe(leave);
    page
}

/// Leaves the options: for the main menu from its own screen, else for
/// what they were spawned over.
fn leave(
    _: On<Activate>,
    mut commands: Commands,
    screen: Res<State<Screen>>,
    mut next: ResMut<NextState<Screen>>,
    pages: Query<Entity, With<Page>>,
) {
    match screen.get() {
        Screen::Options => next.set(Screen::MainMenu),
        _ => pages.iter().for_each(|p| commands.entity(p).despawn()),
    }
}

/// A group of options under a `title`, for a [`Body`].
fn section(commands: &mut Commands, body: Entity, title: &str) -> Entity {
    let section = Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(8),
        ..default()
    };
    let section = commands.spawn((ChildOf(body), section)).id();
    commands.spawn((text(title, Font::Heading), ChildOf(section)));
    section
}

/// An option's `name` in a `section`, beside whatever's put in the place
/// marked `holder` to choose it by.
fn choice(commands: &mut Commands, section: Entity, name: &str, holder: impl Bundle) {
    let row = Node {
        align_items: AlignItems::Center,
        column_gap: px(12),
        ..default()
    };
    let name = (
        Node {
            width: px(160),
            ..default()
        },
        children![label(name, Font::Body, Ink::Muted)],
    );
    commands.spawn((
        ChildOf(section),
        row,
        children![name, (holder, Node::default())],
    ));
}

/// Shows the options of the tab chosen.
fn show_body(
    mut commands: Commands,
    options: Res<Options>,
    tab: Res<Tab>,
    body: Single<(Entity, Ref<Body>)>,
) {
    let (body, added) = *body;
    if !tab.is_changed() && !added.is_added() {
        return;
    }
    commands.entity(body).despawn_children();
    commands.insert_resource(Open::Nothing);
    match *tab {
        Tab::Game => {
            let game = section(&mut commands, body, "Game");
            choice(&mut commands, game, &options.hotkeys_label, Hotkeys);
            let extras = section(&mut commands, body, "Auxiliaries");
            let width = Node {
                width: px(412),
                justify_content: JustifyContent::Start,
                ..padded()
            };
            commands
                .spawn((
                    BorderToggle,
                    ChildOf(extras),
                    switch(width),
                    children![text("", Font::Body)],
                ))
                .observe(|_: On<Activate>, mut border: ResMut<Border>| border.0 = !border.0);
        }
        Tab::Video => {
            let basic = section(&mut commands, body, "Basic");
            choice(&mut commands, basic, &options.resolution_label, Resolution);
        }
    }
}

/// Opens a list of choices, or closes it if it's the one open.
fn open_list(activate: On<Activate>, openers: Query<&Opener>, mut open: ResMut<Open>) {
    if let Ok(Opener(list)) = openers.get(activate.entity) {
        *open = match *open == *list {
            true => Open::Nothing,
            false => *list,
        };
    }
}

/// How wide the lists of choices are.
const LIST_WIDTH: f32 = 240.0;

/// Shows the hotkeys in use, and the others to choose from while picking.
fn show_hotkeys(
    mut commands: Commands,
    options: Res<Options>,
    open: Res<Open>,
    holder: Single<(Entity, Ref<Hotkeys>)>,
) {
    let (holder, added) = *holder;
    if !options.is_changed() && !open.is_changed() && !added.is_added() {
        return;
    }
    commands.entity(holder).despawn_children();
    let list = Dropdown {
        row: holder,
        width: px(LIST_WIDTH),
        label: options.hotkeys().map_or("", |c| c.name.as_str()).into(),
        open: *open == Open::Hotkeys,
    };
    let choices = options.configurations.iter();
    let choices = choices.map(|c| ((HotkeysChoice(c.id.clone()), InList), c.name.clone()));
    list.spawn(
        &mut commands,
        (Opener(Open::Hotkeys), InList),
        open_list,
        choices,
        choose_hotkeys,
    );
}

fn choose_hotkeys(
    activate: On<Activate>,
    choices: Query<&HotkeysChoice>,
    mut options: ResMut<Options>,
    mut open: ResMut<Open>,
) {
    if let Ok(choice) = choices.get(activate.entity) {
        options.hotkeys = Some(choice.0.clone());
        *open = Open::Nothing;
    }
}

/// A window's size as the options show it, e.g. `1280x720`.
fn size_of(window: &Window) -> UVec2 {
    window.size().round().as_uvec2()
}

/// Shows the window's size, and the others to choose from while picking:
/// those the screen it's on has room for.
fn show_resolution(
    mut commands: Commands,
    open: Res<Open>,
    holder: Single<(Entity, Ref<Resolution>)>,
    window: Query<&Window, With<PrimaryWindow>>,
    monitor: Query<&Monitor, With<PrimaryMonitor>>,
    mut shown: Local<Option<UVec2>>,
) {
    let (holder, added) = *holder;
    let size = window.single().ok().map(size_of);
    if *shown == size && !open.is_changed() && !added.is_added() {
        return;
    }
    *shown = size;
    commands.entity(holder).despawn_children();
    let list = Dropdown {
        row: holder,
        width: px(LIST_WIDTH),
        label: size.map_or(String::new(), |s| format!("{}x{}", s.x, s.y)),
        open: *open == Open::Resolution,
    };
    let room = monitor.single().ok().map(|m| {
        let size = UVec2::new(m.physical_width, m.physical_height).as_dvec2();
        (size / m.scale_factor).as_uvec2()
    });
    let sizes = RESOLUTIONS.iter().map(|&(w, h)| UVec2::new(w, h));
    let sizes = sizes.filter(|s| room.is_none_or(|room| s.cmple(room).all()));
    let choices = sizes.map(|s| ((ResolutionChoice(s), InList), format!("{}x{}", s.x, s.y)));
    list.spawn(
        &mut commands,
        (Opener(Open::Resolution), InList),
        open_list,
        choices,
        choose_resolution,
    );
}

fn choose_resolution(
    activate: On<Activate>,
    choices: Query<&ResolutionChoice>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
    mut open: ResMut<Open>,
) {
    let Ok(choice) = choices.get(activate.entity) else {
        return;
    };
    if let Ok(mut window) = window.single_mut() {
        let size = choice.0.as_vec2();
        window.resolution.set(size.x, size.y);
    }
    *open = Open::Nothing;
}

/// A button and what it chooses.
type Choice<'a> = (
    &'a mut Chosen,
    Option<&'a TabButton>,
    Option<&'a HotkeysChoice>,
    Option<&'a ResolutionChoice>,
);

fn mark_chosen(
    options: Res<Options>,
    tab: Res<Tab>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut buttons: Query<Choice>,
) {
    let size = window.single().ok().map(size_of);
    for (mut chosen, shown, hotkeys, resolution) in &mut buttons {
        let now = shown.is_some_and(|t| t.0 == *tab)
            || hotkeys.is_some_and(|h| options.hotkeys.as_ref() == Some(&h.0))
            || resolution.is_some_and(|r| size == Some(r.0));
        chosen.set_if_neq(Chosen(now));
    }
}

/// Closes whichever list is open on a press anywhere but on it.
fn close_list(
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut open: ResMut<Open>,
    list: Query<&Hovered, With<InList>>,
) {
    let pressed = mouse.is_some_and(|m| m.just_pressed(MouseButton::Left));
    if pressed && !list.iter().any(|h| h.get()) {
        open.set_if_neq(Open::Nothing);
    }
}

/// Says whether the supply border is showing.
fn show_border(
    options: Res<Options>,
    border: Res<Border>,
    toggles: Query<(&Children, Ref<BorderToggle>)>,
    mut texts: Query<&mut Text>,
) {
    for (children, added) in &toggles {
        if !border.is_changed() && !added.is_added() {
            continue;
        }
        let state = match border.0 {
            true => "On",
            false => "Off",
        };
        let mut texts = texts.iter_many_mut(children);
        if let Some(Ok(mut text)) = texts.fetch_next() {
            text.0 = format!("{}: {state}", options.border_label);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::state::app::StatesPlugin;

    use super::*;

    fn configuration(id: &str, name: &str) -> HotkeyConfiguration {
        HotkeyConfiguration {
            id: id.into(),
            name: name.into(),
            file: None,
        }
    }

    fn options(named: Option<&str>) -> Options {
        let configurations = vec![
            configuration("classic", "Standard"),
            configuration("visual", "Visual"),
        ];
        Options::of(configurations, named.map(Id::from), &HashMap::new())
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin, OptionsPlugin))
            .insert_state(Screen::Options)
            .insert_resource(options(Some("classic")))
            .init_resource::<Border>();
        app.update();
        app
    }

    fn activate<C: Component>(app: &mut App, which: impl Fn(&C) -> bool) {
        let world = app.world_mut();
        let mut buttons = world.query::<(Entity, &C)>();
        let found = buttons.iter(world).find(|(_, c)| which(c));
        let (entity, _) = found.expect("no such button");
        world.trigger(Activate { entity });
        app.update();
    }

    fn choices(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query::<&HotkeysChoice>().iter(world).count()
    }

    #[test]
    fn starts_on_the_set_named_or_the_first() {
        let chosen = |named| options(named).hotkeys().map(|c| c.name.clone());
        assert_eq!(chosen(Some("visual")).as_deref(), Some("Visual"));
        assert_eq!(chosen(Some("missing")).as_deref(), Some("Standard"));
        assert_eq!(chosen(None).as_deref(), Some("Standard"));
        let none = Options::of(Vec::new(), Some("classic".into()), &HashMap::new());
        assert!(none.hotkeys().is_none());
    }

    #[test]
    fn the_list_opens_and_a_choice_from_it_is_kept() {
        let mut app = app();
        assert_eq!(choices(&mut app), 0);
        activate::<Opener>(&mut app, |o| o.0 == Open::Hotkeys);
        assert_eq!(choices(&mut app), 2);
        activate::<HotkeysChoice>(&mut app, |c| c.0.as_str() == "visual");
        assert_eq!(choices(&mut app), 0);
        let options = app.world().resource::<Options>();
        assert_eq!(options.hotkeys().map(|c| c.id.as_str()), Some("visual"));
    }

    #[test]
    fn a_resolution_chosen_sizes_the_window() {
        let mut app = app();
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        activate::<TabButton>(&mut app, |t| t.0 == Tab::Video);
        activate::<Opener>(&mut app, |o| o.0 == Open::Resolution);
        activate::<ResolutionChoice>(&mut app, |r| r.0 == UVec2::new(1600, 900));
        let world = app.world_mut();
        let window = world.query::<&Window>().single(world).unwrap();
        assert_eq!(window.size(), Vec2::new(1600.0, 900.0));
        assert_eq!(*world.resource::<Open>(), Open::Nothing);
    }

    #[test]
    fn the_border_toggle_says_which_it_is() {
        let mut app = app();
        let said = |app: &mut App| {
            let world = app.world_mut();
            let mut toggles = world.query_filtered::<&Children, With<BorderToggle>>();
            let child = toggles.single(world).unwrap()[0];
            world.get::<Text>(child).unwrap().0.clone()
        };
        let before = said(&mut app);
        activate::<BorderToggle>(&mut app, |_| true);
        assert_ne!(said(&mut app), before);
        assert!(said(&mut app).starts_with("Supply Border: "));
    }
}
