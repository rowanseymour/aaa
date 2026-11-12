//! The options, as the main menu and the menu over a match both show
//! them, and what's chosen in them.

use std::collections::HashMap;

use aaa_data::{GameData, HotkeyConfiguration, Id};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::Screen;
use crate::menu::{column, go, spacer};
use crate::supply::Border;
use crate::ui::{Chosen, Dropdown, Font, Ink, button, label, text, toggle};

const HOTKEYS: &str = "options_dialog_Child_text_6";
const BORDER: &str = "options_dialog_Child_text_15";

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
        }
    }

    /// The set of hotkeys in use, if the game has any.
    pub fn hotkeys(&self) -> Option<&HotkeyConfiguration> {
        let chosen = self.hotkeys.as_ref()?;
        self.configurations.iter().find(|c| c.id == *chosen)
    }
}

/// Whether the list of hotkeys is open.
#[derive(Resource, Default, PartialEq)]
struct Picking(bool);

/// Where the choice of hotkeys goes.
#[derive(Component)]
struct Hotkeys;

/// The button that opens the list of hotkeys.
#[derive(Component)]
struct HotkeysOpener;

/// A button choosing a set of hotkeys, by its id.
#[derive(Component)]
struct HotkeysChoice(Id);

/// The button that shows or hides the supply border.
#[derive(Component)]
struct BorderToggle;

pub struct OptionsPlugin;

impl Plugin for OptionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Picking>()
            .add_systems(OnEnter(Screen::Options), screen)
            .add_systems(
                Update,
                (close_list, show_hotkeys, mark_chosen, show_border).chain(),
            );
    }
}

fn screen(mut commands: Commands) {
    let column = column(&mut commands, Screen::Options);
    commands.spawn((text("Options", Font::Heading), ChildOf(column)));
    rows(&mut commands, column);
    commands.spawn((spacer(), ChildOf(column)));
    commands
        .spawn((button("Back", Font::Large), ChildOf(column)))
        .observe(go(Screen::MainMenu));
}

/// Puts the options in `parent`, a column.
pub fn rows(commands: &mut Commands, parent: Entity) {
    commands.insert_resource(Picking(false));
    commands.spawn((HotkeysLabel, ChildOf(parent), Node::default()));
    commands.spawn((Hotkeys, ChildOf(parent), Node::default()));
    commands
        .spawn((BorderToggle, toggle("", Font::Body), ChildOf(parent)))
        .observe(|_: On<Activate>, mut border: ResMut<Border>| border.0 = !border.0);
}

/// Where the name of the choice of hotkeys goes.
#[derive(Component)]
struct HotkeysLabel;

/// Shows the hotkeys in use, and the others to choose from while picking.
fn show_hotkeys(
    mut commands: Commands,
    options: Res<Options>,
    picking: Res<Picking>,
    holder: Single<(Entity, Ref<Hotkeys>)>,
    name: Single<Entity, With<HotkeysLabel>>,
) {
    let (holder, added) = *holder;
    if !options.is_changed() && !picking.is_changed() && !added.is_added() {
        return;
    }
    commands.entity(*name).despawn_children().with_child(label(
        options.hotkeys_label.clone(),
        Font::Body,
        Ink::Muted,
    ));
    commands.entity(holder).despawn_children();
    let list = Dropdown {
        row: holder,
        width: percent(100),
        label: options.hotkeys().map_or("", |c| c.name.as_str()).into(),
        open: picking.0,
    };
    let choices =
        (options.configurations.iter()).map(|c| (HotkeysChoice(c.id.clone()), c.name.clone()));
    list.spawn(
        &mut commands,
        HotkeysOpener,
        |_: On<Activate>, mut picking: ResMut<Picking>| picking.0 = !picking.0,
        choices,
        choose,
    );
}

fn choose(
    activate: On<Activate>,
    choices: Query<&HotkeysChoice>,
    mut options: ResMut<Options>,
    mut picking: ResMut<Picking>,
) {
    if let Ok(choice) = choices.get(activate.entity) {
        options.hotkeys = Some(choice.0.clone());
        picking.0 = false;
    }
}

fn mark_chosen(options: Res<Options>, mut buttons: Query<(&mut Chosen, &HotkeysChoice)>) {
    for (mut chosen, choice) in &mut buttons {
        chosen.set_if_neq(Chosen(options.hotkeys.as_ref() == Some(&choice.0)));
    }
}

/// A button of the list of hotkeys: what opens it, or a choice.
type InList = Or<(With<HotkeysOpener>, With<HotkeysChoice>)>;

/// Closes the list of hotkeys on a press anywhere but on it.
fn close_list(
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut picking: ResMut<Picking>,
    list: Query<&Hovered, InList>,
) {
    let pressed = mouse.is_some_and(|m| m.just_pressed(MouseButton::Left));
    if pressed && !list.iter().any(|h| h.get()) {
        picking.set_if_neq(Picking(false));
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
        activate::<HotkeysOpener>(&mut app, |_| true);
        assert_eq!(choices(&mut app), 2);
        activate::<HotkeysChoice>(&mut app, |c| c.0.as_str() == "visual");
        assert_eq!(choices(&mut app), 0);
        let options = app.world().resource::<Options>();
        assert_eq!(options.hotkeys().map(|c| c.id.as_str()), Some("visual"));
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
