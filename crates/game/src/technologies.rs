//! The technology display: a window listing the technologies the player's
//! side holds, opened by a tab beside the minimap or its key. The match
//! holds still while it's up, unless it was already.

use std::collections::HashMap;

use aaa_data::{Id, PropertyLook};
use aaa_render::textures;
use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{Activate, Button};

use crate::actions::Blocks;
use crate::keys::Bindings;
use crate::ui::{self, Click, Font, Hint, Ink, Marked, Surface, label, panel};
use crate::{Loaded, Local, Screen, Sim, minimap};

/// What the keys that open, close and close every display panel are
/// bound to.
pub const OPEN: &str = "ButtonTechnology";
const CLOSE: &str = "TechnologyDisplayClose";
const CLOSE_ALL: &str = "CloseSideOrDisplayPanel";

const TAB: &str = "UI/Game/ControlPanel/SidePanelTabTechnologyUnselected.tga";
const TAB_HELD: &str = "UI/Game/ControlPanel/SidePanelTabTechnologySelected.tga";
const TAB_SIZE: Vec2 = Vec2::new(40.0, 52.0);
const NAME: &str = "control_panel_ButtonTechnology_ToolTip";
const HELP: &str = "control_panel_ButtonTechnology_tooltip_2";
const TITLE: &str = "technology_display_Child_text";

/// The window's place and size on the game's 1024 by 768 layout.
const LAYOUT: Vec2 = Vec2::new(1024.0, 768.0);
const WINDOW_AT: Vec2 = Vec2::new(321.0, 53.0);
const WINDOW_SIZE: Vec2 = Vec2::new(385.0, 513.0);
/// A line is at least this tall, and its icon this wide.
const LINE: f32 = 20.0;
const ICON: f32 = 16.0;

/// Whether the window's up, whether opening it paused the match, and when
/// its list was last brought up to date.
#[derive(Resource, Default)]
pub struct TechDisplay {
    open: bool,
    paused_it: bool,
    refreshed: Option<f64>,
    /// What the list shows, to rebuild it only when that changes.
    shown: Vec<Id>,
}

/// How each technology shows in the list, by id.
#[derive(Resource)]
struct Looks {
    lines: HashMap<Id, Line>,
    formats: bevy::image::CompressedImageFormats,
    tab: Option<Handle<Image>>,
    tab_held: Option<Handle<Image>>,
    /// The tab's name, to mark its key in.
    name: String,
}

struct Line {
    name: String,
    icon: String,
    image: Option<Option<Handle<Image>>>,
    /// Its tooltip under its name: what it does, then its description.
    hint: String,
}

#[derive(Component)]
struct Tab;

#[derive(Component)]
struct TechWindow;

#[derive(Component)]
struct List;

pub struct TechnologyPlugin;

impl Plugin for TechnologyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TechDisplay>()
            .add_systems(OnEnter(Screen::Match), spawn)
            .add_systems(
                Update,
                (keys, press_tab, refresh)
                    .chain()
                    .after(crate::pause::Toggle)
                    .run_if(in_state(Screen::Match))
                    .run_if(crate::console::closed)
                    .run_if(crate::pause::closed)
                    .run_if(loaded),
            )
            .add_systems(
                Update,
                mark_tab
                    .run_if(resource_exists_and_changed::<Bindings>)
                    .run_if(resource_exists::<Looks>)
                    .run_if(in_state(Screen::Match)),
            );
    }
}

/// Whether the loading screen has gone, so the match is up to look at.
fn loaded(loading: Query<(), With<crate::LoadingScreen>>) -> bool {
    loading.is_empty()
}

/// Whether the window's down, as it is before a match sets it up.
pub fn closed(display: Option<Res<TechDisplay>>) -> bool {
    display.is_none_or(|d| !d.open)
}

fn spawn(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    bindings: Res<Bindings>,
) {
    let game = &mut loaded.game;
    let strings = game.strings();
    let said = |key: &str, or: &str| strings.get(key).map_or(or, String::as_str).to_string();
    let mut image = |path: &str| {
        let image = textures::decode(&game.read(path)?, path, formats.0, true)?;
        Some(images.add(image))
    };
    let (tab, tab_held) = (image(TAB), image(TAB_HELD));
    let looks: HashMap<Id, PropertyLook> = game.property_looks();
    let lines = (game.technologies().into_iter())
        .filter_map(|(id, t)| {
            let look = looks.get(&id)?;
            let mut hint = look.lines.join("\n");
            if let Some(d) = t.description {
                hint = match hint.is_empty() {
                    true => d,
                    false => format!("{hint}\n\n{d}"),
                };
            }
            let line = Line {
                name: t.name,
                icon: look.icon.clone(),
                image: None,
                hint,
            };
            Some((id, line))
        })
        .collect();
    let name = said(NAME, "TechDisplay Technologies");
    commands.insert_resource(TechDisplay::default());
    commands
        .spawn((
            Tab,
            Blocks,
            Button,
            Click::PUSH,
            RelativeCursorPosition::default(),
            marked(&bindings, &name),
            Hint(said(HELP, "Displays a list of all your technologies.")),
            ImageNode::new(tab.clone().unwrap_or_default()),
            // At the minimap square's other top corner from the supply
            // border's button.
            Node {
                position_type: PositionType::Absolute,
                left: px(crate::MARGIN + minimap::DISC - TAB_SIZE.x),
                bottom: px(crate::MARGIN + minimap::DISC - TAB_SIZE.y),
                width: px(TAB_SIZE.x),
                height: px(TAB_SIZE.y),
                ..default()
            },
            GlobalZIndex(2),
            DespawnOnExit(Screen::Match),
        ))
        .observe(|_: On<Activate>, mut toggling: Toggling| toggling.toggle());
    let at = |v: f32, of: f32| percent(100.0 * v / of);
    commands
        .spawn((
            TechWindow,
            Blocks,
            RelativeCursorPosition::default(),
            panel(
                Surface::Panel,
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    left: at(WINDOW_AT.x, LAYOUT.x),
                    top: at(WINDOW_AT.y, LAYOUT.y),
                    width: at(WINDOW_SIZE.x, LAYOUT.x),
                    height: at(WINDOW_SIZE.y, LAYOUT.y),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(12)),
                    row_gap: px(8),
                    ..default()
                },
            ),
            GlobalZIndex(ui::layer::DIALOG),
            DespawnOnExit(Screen::Match),
        ))
        .with_children(|window| {
            // Only the Close button below, without the game's second one at
            // the top.
            window.spawn(label(
                said(TITLE, "Technology Panel"),
                Font::HudTitle,
                Ink::Body,
            ));
            window.spawn((
                List,
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    min_height: px(0),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ScrollPosition::default(),
            ));
            window
                .spawn(Node {
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_child(ui::button("Close", Font::Hud))
                .observe(|_: On<Activate>, mut toggling: Toggling| toggling.close());
        });
    commands.insert_resource(Looks {
        lines,
        formats: formats.0,
        tab,
        tab_held,
        name,
    });
}

/// The tab's name with its key marked.
fn marked(bindings: &Bindings, name: &str) -> Marked {
    match bindings.key(OPEN) {
        Some(key) => key.mark(name),
        None => Marked::plain(name),
    }
}

/// Marks the tab's key afresh once the hotkeys change.
fn mark_tab(bindings: Res<Bindings>, looks: Res<Looks>, mut tabs: Query<&mut Marked, With<Tab>>) {
    for mut tab in &mut tabs {
        tab.set_if_neq(marked(&bindings, &looks.name));
    }
}

/// What opening and closing the window reads and changes.
#[derive(bevy::ecs::system::SystemParam)]
struct Toggling<'w, 's> {
    display: ResMut<'w, TechDisplay>,
    time: ResMut<'w, Time<Virtual>>,
    window: Single<'w, 's, &'static mut Node, With<TechWindow>>,
}

impl Toggling<'_, '_> {
    fn toggle(&mut self) {
        match self.display.open {
            true => self.close(),
            false => self.open(),
        }
    }

    /// Opens the window, holding the match still if it wasn't already.
    fn open(&mut self) {
        let display = &mut *self.display;
        display.open = true;
        display.paused_it = !self.time.is_paused();
        display.refreshed = None;
        self.time.pause();
        self.window.display = bevy::ui::Display::Flex;
    }

    /// Closes the window, letting the match go on if opening it held it.
    fn close(&mut self) {
        let display = &mut *self.display;
        if !display.open {
            return;
        }
        display.open = false;
        if std::mem::take(&mut display.paused_it) {
            self.time.unpause();
        }
        self.window.display = bevy::ui::Display::None;
    }
}

/// Opens or closes the window on its keys.
fn keys(keys: Res<ButtonInput<KeyCode>>, bindings: Res<Bindings>, mut toggling: Toggling) {
    if toggling.display.open {
        let close = [CLOSE, CLOSE_ALL];
        if close.iter().any(|action| bindings.pressed(&keys, action)) {
            toggling.close();
        }
    } else if bindings.pressed(&keys, OPEN) {
        toggling.open();
    }
}

/// Shows the tab held down while it's pressed.
fn press_tab(
    looks: Res<Looks>,
    mut tabs: Query<(Has<bevy::ui::Pressed>, &mut ImageNode), With<Tab>>,
) {
    for (pressed, mut image) in &mut tabs {
        let want = match pressed {
            true => &looks.tab_held,
            false => &looks.tab,
        };
        if let Some(want) = want
            && image.image != *want
        {
            image.image = want.clone();
        }
    }
}

/// Brings the open window's list up to date: at once on opening, then once
/// a second.
#[allow(clippy::too_many_arguments)]
fn refresh(
    mut commands: Commands,
    mut display: ResMut<TechDisplay>,
    mut looks: ResMut<Looks>,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    real: Res<Time<Real>>,
    sim: Res<Sim>,
    local: Res<Local>,
    list: Single<Entity, With<List>>,
) {
    let now = real.elapsed_secs_f64();
    if !display.open || display.refreshed.is_some_and(|then| now - then < 1.0) {
        return;
    }
    display.refreshed = Some(now);
    let held = local.0.and_then(|p| sim.0.technologies.get(&p));
    let shown: Vec<Id> = (held.into_iter().flatten())
        .filter(|t| looks.lines.contains_key(*t))
        .cloned()
        .collect();
    if shown == display.shown {
        return;
    }
    let formats = looks.formats;
    let mut entries = Vec::new();
    for id in &shown {
        let Some(line) = looks.lines.get_mut(id) else {
            continue;
        };
        let image = line.image.get_or_insert_with(|| {
            let path = &line.icon;
            let decoded = textures::decode(&loaded.game.read(path)?, path, formats, true)?;
            Some(images.add(decoded))
        });
        entries.push((line.name.clone(), image.clone(), line.hint.clone()));
    }
    let mut list = commands.entity(*list);
    list.despawn_children();
    list.with_children(|list| {
        for (name, image, hint) in entries {
            list.spawn((
                Node {
                    min_height: px(LINE),
                    column_gap: px(4),
                    align_items: AlignItems::Center,
                    ..default()
                },
                Hovered::default(),
                Marked::plain(name.as_str()),
                Hint(hint),
            ))
            .with_children(|line| {
                line.spawn((
                    ImageNode::new(image.unwrap_or_default()),
                    Node {
                        width: px(ICON),
                        height: px(ICON),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                line.spawn(label(name, Font::Small, Ink::Body));
            });
        }
    });
    display.shown = shown;
}

#[cfg(test)]
mod tests {
    use aaa_data::Hotkey;
    use bevy::state::app::StatesPlugin;

    use super::*;
    use crate::pause::{self, PausePlugin};

    fn hotkey(chord: &str, target: &str, command: Option<&str>) -> Hotkey {
        Hotkey {
            chord: chord.into(),
            target: target.into(),
            command: command.map(str::to_string),
        }
    }

    fn app() -> App {
        let hotkeys = [
            hotkey("f2", OPEN, None),
            hotkey("f2", "TechnologyDisplay", Some(CLOSE)),
            hotkey("esc", "TechnologyDisplay", Some(CLOSE)),
            hotkey("backspace", "TechnologyDisplay|1", Some(CLOSE_ALL)),
        ];
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin, PausePlugin))
            .insert_state(Screen::Match)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::console::Console>()
            .init_resource::<crate::actions::Page>()
            .init_resource::<crate::Focus>()
            .init_resource::<TechDisplay>()
            .insert_resource(Bindings::new(&hotkeys, &HashMap::new()))
            .add_systems(Update, keys.after(pause::Toggle).run_if(pause::closed));
        app.world_mut().spawn((TechWindow, Node::default()));
        app.update();
        app
    }

    fn press(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(key);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(key);
        keys.clear();
    }

    fn open(app: &App) -> bool {
        app.world().resource::<TechDisplay>().open
    }

    fn paused(app: &App) -> bool {
        app.world().resource::<Time<Virtual>>().is_paused()
    }

    #[test]
    fn its_key_opens_and_closes_it_pausing_the_match() {
        let mut app = app();
        press(&mut app, KeyCode::F2);
        assert!(open(&app) && paused(&app));
        press(&mut app, KeyCode::F2);
        assert!(!open(&app) && !paused(&app));
        press(&mut app, KeyCode::F2);
        press(&mut app, KeyCode::Backspace);
        assert!(!open(&app) && !paused(&app));
    }

    #[test]
    fn escape_closes_it_without_opening_the_menu() {
        let mut app = app();
        press(&mut app, KeyCode::F2);
        press(&mut app, KeyCode::Escape);
        assert!(!open(&app) && !paused(&app));
        let menu = app.world_mut().run_system_cached(pause::closed);
        assert!(menu.unwrap());
    }

    #[test]
    fn closing_it_leaves_a_match_paused_before_it_opened() {
        let mut app = app();
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        press(&mut app, KeyCode::F2);
        press(&mut app, KeyCode::F2);
        assert!(!open(&app) && paused(&app));
    }
}
