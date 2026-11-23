//! The display panels: windows over the match, each opened by a button
//! of either interface's or by its key, one at a time. The match
//! holds still while one's up, unless it was already. A panel opens only
//! while the interface has spawned its window.

use std::collections::HashMap;

use bevy::image::{CompressedImageFormats, TRANSPARENT_IMAGE_HANDLE};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::ScrollArea;
use tgdata::GameData;
use tgrender::textures;

use crate::intent::{Asked, Asks, Intent};
use crate::ui::Blocks;
use crate::ui::{self, Font, Ink, Surface, label};
use crate::{Orders, Screen};
use tgsim::Action;

/// What the key that closes whichever panel is up is bound to.
pub(crate) const CLOSE_ALL: &str = "CloseSideOrDisplayPanel";

pub(crate) const PANELS: [Panel; 4] = [
    Panel::Objectives,
    Panel::Technologies,
    Panel::MessageLog,
    Panel::Politics,
];

/// The layout the windows' places and sizes are given on.
const LAYOUT: Vec2 = Vec2::new(1024.0, 768.0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Objectives,
    Technologies,
    MessageLog,
    Politics,
}

impl Panel {
    /// What the keys that open it, and close it while it's up, are bound
    /// to.
    pub const fn keys(self) -> (&'static str, &'static str) {
        match self {
            Panel::Objectives => ("ButtonObjectives", "ObjectivesDisplayClose"),
            Panel::Technologies => ("ButtonTechnology", "TechnologyDisplayClose"),
            Panel::MessageLog => ("ButtonMessageLog", "MessageLogToggle"),
            // Its button's all the hotkey files have to close it.
            Panel::Politics => (crate::politics::BUTTON, crate::politics::BUTTON),
        }
    }

    /// What either interface's button that opens it is
    /// [`called`](crate::ui::called), so a script works with both.
    pub const fn called(self) -> &'static str {
        match self {
            Panel::Objectives => "panel.objectives",
            Panel::Technologies => "panel.technologies",
            Panel::MessageLog => "panel.messages",
            Panel::Politics => crate::politics::CALLED,
        }
    }

    /// The window the original shows it in, where the hotkey files bind
    /// its keys.
    fn window(self) -> &'static str {
        match self {
            Panel::Objectives => "ObjectivesDisplay",
            Panel::Technologies => "TechnologyDisplay",
            Panel::MessageLog => "SideMessageLogPanel",
            Panel::Politics => crate::politics::WIDGET,
        }
    }
}

/// Which panel's up, and whether opening it paused the match.
#[derive(Resource, Default)]
pub struct Displays {
    open: Option<Panel>,
    paused_it: bool,
}

impl Displays {
    pub fn is_open(&self, panel: Panel) -> bool {
        self.open == Some(panel)
    }

    /// The window of the panel that's up, whose keys are heard while it
    /// is.
    pub fn window(&self) -> Option<&'static str> {
        self.open.map(Panel::window)
    }

    /// Whether a key pressing `widget` is heard while a panel's up: only
    /// the panels' own buttons, which put another in its place.
    pub fn passes(&self, widget: &str) -> bool {
        (PANELS.iter()).any(|p| p.keys().0.eq_ignore_ascii_case(widget))
    }

    /// Keeps the match paused when the panel closes, as a pause asked for
    /// while it's up should.
    pub fn keep_paused(&mut self) {
        self.paused_it = false;
    }
}

/// Whether no panel's up, as none is before a match sets them up.
pub fn closed(displays: Option<Res<Displays>>) -> bool {
    displays.is_none_or(|d| d.open.is_none())
}

/// A panel's window.
#[derive(Component)]
pub struct PanelWindow(Panel);

/// Whether the interface has a window for `panel`, so it's one of these
/// panels rather than drawn some other way.
pub fn windowed(panel: Panel) -> impl Fn(Query<&PanelWindow>) -> bool {
    move |windows| windows.iter().any(|w| w.0 == panel)
}

/// The box a panel's window shows its contents in.
#[derive(Component)]
pub struct Contents(pub Panel);

/// A window's place and look.
pub struct Window {
    pub panel: Panel,
    pub title: String,
    /// Its place and size on the game's 1024 by 768 layout.
    pub at: Vec2,
    pub size: Vec2,
    /// Whether its contents scroll in a darker pane, rather than laying
    /// out panes of their own.
    pub pane: bool,
}

pub struct DisplaysPlugin;

impl Plugin for DisplaysPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Displays>()
            .init_resource::<Icons>()
            .add_systems(OnEnter(Screen::Match), reset)
            .add_systems(PostUpdate, draw_icons.after(bevy::ui::UiSystems::Layout))
            .add_systems(
                Update,
                keys.in_set(Keys)
                    .after(crate::pause::Toggle)
                    .after(crate::keys::send)
                    .run_if(in_state(Screen::Match))
                    .run_if(loaded),
            );
    }
}

/// Where the panels' keys are read, for what shows them to go after.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Keys;

/// Whether the loading screen has gone, so the match is up to look at.
pub fn loaded(loading: Query<(), With<crate::loading::LoadingScreen>>) -> bool {
    loading.is_empty()
}

fn reset(mut displays: ResMut<Displays>) {
    *displays = Displays::default();
}

/// Spawns a panel's window, hidden: its title, the box it shows its
/// contents in, and a Close button. Returns the window and the box.
pub fn spawn_window(commands: &mut Commands, window: Window) -> (Entity, Entity) {
    let at = |v: f32, of: f32| percent(100.0 * v / of);
    let panel = window.panel;
    let mut contents = Entity::PLACEHOLDER;
    let spawned = commands
        .spawn((
            PanelWindow(panel),
            Blocks,
            RelativeCursorPosition::default(),
            ui::panel(
                Surface::Hud,
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    left: at(window.at.x, LAYOUT.x),
                    top: at(window.at.y, LAYOUT.y),
                    width: at(window.size.x, LAYOUT.x),
                    height: at(window.size.y, LAYOUT.y),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(12)),
                    row_gap: px(8),
                    ..default()
                },
            ),
            GlobalZIndex(ui::layer::DIALOG),
            DespawnOnExit(Screen::Match),
        ))
        .with_children(|parent| {
            // Only the Close button below, without the game's second one at
            // the top.
            parent.spawn(label(window.title, Font::HudTitle, Ink::Body));
            let node = Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                flex_grow: 1.0,
                min_height: px(0),
                ..default()
            };
            let mut spawned = parent.spawn(Contents(panel));
            match window.pane {
                true => spawned.insert((
                    pane(Node {
                        overflow: Overflow::scroll_y(),
                        ..node
                    }),
                    ScrollArea,
                    ScrollPosition::default(),
                )),
                false => spawned.insert(node),
            };
            contents = spawned.id();
            parent
                .spawn(Node {
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        ui::button("Close", Font::Hud),
                        ui::called("panel.close"),
                        // Its own close, which leaves the politics panel up.
                        Asks::new(panel.keys().1),
                    ));
                });
        })
        .id();
    (spawned, contents)
}

/// A darker pane in a window, for a list to go in.
pub fn pane(node: Node) -> impl Bundle {
    ui::panel(
        Surface::Slot,
        Node {
            padding: UiRect::all(px(8)),
            ..node
        },
    )
}

/// What opening and closing the panels reads and changes.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Toggling<'w, 's> {
    displays: ResMut<'w, Displays>,
    orders: ResMut<'w, Orders>,
    windows: Query<'w, 's, (&'static PanelWindow, &'static mut Node)>,
}

impl Toggling<'_, '_> {
    /// Opens `panel`, or closes it if it's up.
    pub fn toggle(&mut self, panel: Panel) {
        match self.displays.is_open(panel) {
            true => self.close(),
            false => self.open(panel),
        }
    }

    /// Opens `panel` in place of whichever is up, holding the match still
    /// if it wasn't already.
    pub fn open(&mut self, panel: Panel) {
        let displays = &mut *self.displays;
        if displays.open.is_none() {
            // A multiplayer match, even one hosted here, is other people's too,
            // and plays on.
            displays.paused_it = !self.orders.pausing() && !self.orders.multiplayer();
            if !self.orders.multiplayer() {
                self.orders.act(Action::Pause(true));
            }
        }
        displays.open = Some(panel);
        self.show();
    }

    /// Closes `panel` if it's up.
    pub fn close_panel(&mut self, panel: Panel) {
        if self.displays.is_open(panel) {
            self.close();
        }
    }

    /// Closes whichever panel's up, letting the match go on if opening it
    /// held it.
    pub fn close(&mut self) {
        let displays = &mut *self.displays;
        if displays.open.take().is_none() {
            return;
        }
        if std::mem::take(&mut displays.paused_it) {
            self.orders.act(Action::Pause(false));
        }
        self.show();
    }

    fn show(&mut self) {
        for (PanelWindow(panel), mut node) in &mut self.windows {
            crate::ui::show(&mut node, self.displays.is_open(*panel));
        }
    }
}

/// Opens or closes the panels as asked: a panel's own close, or closing
/// whatever's up, closes it, and a panel's button opens it or, up, closes
/// it.
fn keys(mut intents: MessageReader<Intent>, mut toggling: Toggling) {
    let asked = Asked::read(&mut intents);
    let windowed = |panel: Panel| (toggling.windows.iter()).any(|(w, _)| w.0 == panel);
    let wants = |name: &str| asked.wants(name);
    if let Some(open) = toggling.displays.open
        && (wants(open.keys().1) || wants(CLOSE_ALL))
    {
        toggling.close();
        return;
    }
    let panel = (PANELS.iter()).find(|&&p| wants(p.keys().0) && windowed(p));
    if let Some(&panel) = panel {
        toggling.toggle(panel);
    }
}

/// The path of our interface's icon `name`, white on clear for a button to
/// tint.
pub fn icon(name: &str) -> String {
    format!("UI/Modern/Icons/{name}.svg")
}

/// An image node showing the icon at a path in the data, an SVG drawn
/// again whenever its size on screen changes, so it's never scaled.
#[derive(Component)]
#[require(ImageNode)]
pub struct Icon {
    path: String,
    /// The height in physical pixels it was last drawn at.
    drawn: u32,
}

impl Icon {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            drawn: 0,
        }
    }
}

/// The icons drawn so far, by path, each at the height it was last drawn
/// at, kept from one match to the next. Only the last is kept, so scaling
/// the interface through many sizes doesn't keep a drawing at each.
#[derive(Resource, Default)]
pub struct Icons(HashMap<String, (u32, Handle<Image>)>);

impl Icons {
    /// The image at `path` in `game`, an SVG drawn `height` texels down
    /// and anything else at its own size, blank if it isn't there or
    /// won't decode.
    pub fn get(
        &mut self,
        game: &GameData,
        path: &str,
        height: u32,
        images: &mut Assets<Image>,
        formats: CompressedImageFormats,
    ) -> Handle<Image> {
        let svg = path.to_ascii_lowercase().ends_with(".svg");
        let height = if svg { height } else { 0 };
        if let Some((drawn, image)) = self.0.get(path)
            && *drawn == height
        {
            return image.clone();
        }
        let image = match game.read(path) {
            None => {
                warn!("no {path} in the data");
                None
            }
            Some(bytes) if svg => (textures::svg(&bytes, Some(height), true))
                .map_err(|e| warn!("{path}: {e}"))
                .ok(),
            Some(bytes) => textures::decode(&bytes, path, formats, true),
        };
        let image = image.map_or(TRANSPARENT_IMAGE_HANDLE, |i| images.add(i));
        self.0.insert(path.to_string(), (height, image.clone()));
        image
    }
}

/// Draws each [`Icon`] at its size on screen, once it's laid out and
/// again whenever that changes, as when the interface is scaled.
fn draw_icons(
    loaded: Option<Res<crate::Loaded>>,
    formats: Res<bevy::image::CompressedImageFormatSupport>,
    mut cache: ResMut<Icons>,
    mut images: ResMut<Assets<Image>>,
    mut icons: Query<(&mut Icon, &ComputedNode, &mut ImageNode), Changed<ComputedNode>>,
) {
    let Some(loaded) = loaded else {
        return;
    };
    for (mut icon, node, mut image) in &mut icons {
        let height = node.size().y.round() as u32;
        if height == 0 || height == icon.drawn {
            continue;
        }
        icon.drawn = height;
        image.image = cache.get(&loaded.game, &icon.path, height, &mut images, formats.0);
    }
}

#[cfg(test)]
mod tests {

    use bevy::state::app::StatesPlugin;
    use tgdata::Hotkey;

    use super::*;
    use crate::keys::Bindings;
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
            hotkey("f2", "ButtonTechnology", None),
            hotkey("f3", "ButtonMessageLog", None),
            hotkey("f2", "TechnologyDisplay", Some("TechnologyDisplayClose")),
            hotkey("f3", "SideMessageLogPanel", Some("MessageLogToggle")),
            hotkey("esc", "TechnologyDisplay", Some("TechnologyDisplayClose")),
            hotkey("esc", pause::MENU, None),
            hotkey("backspace", "TechnologyDisplay|1", Some(CLOSE_ALL)),
            hotkey("m", "game|action|verify", Some(MOVE)),
        ];
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin, PausePlugin))
            .insert_state(Screen::Match)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::console::Console>()
            .init_resource::<crate::actions::Page>()
            .init_resource::<crate::special_ops::Armed>()
            .init_resource::<crate::Focus>()
            .init_resource::<Displays>()
            .init_resource::<Orders>()
            .insert_resource(Bindings::new(&hotkeys, &tgdata::Strings::default()))
            .add_message::<Intent>()
            .init_resource::<Sent>()
            .add_observer(crate::intent::send)
            .add_systems(
                Update,
                (
                    crate::keys::send.run_if(crate::keys_heard),
                    (keys, record).chain().after(pause::Toggle),
                )
                    .chain(),
            );
        for panel in [Panel::Technologies, Panel::MessageLog] {
            app.world_mut().spawn((PanelWindow(panel), Node::default()));
        }
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

    fn open(app: &App) -> Option<Panel> {
        app.world().resource::<Displays>().open
    }

    const MOVE: &str = "ActorCommand move";

    /// Every intent sent so far.
    #[derive(Resource, Default)]
    struct Sent(Vec<String>);

    fn record(mut intents: MessageReader<Intent>, mut sent: ResMut<Sent>) {
        sent.0.extend(intents.read().map(|i| i.0.clone()));
    }

    fn sent(app: &App, name: &str) -> bool {
        app.world().resource::<Sent>().0.iter().any(|s| s == name)
    }

    #[test]
    fn a_panel_keeps_the_match_keys_to_itself() {
        let mut app = app();
        press(&mut app, KeyCode::F2);
        press(&mut app, KeyCode::KeyM);
        assert!(!sent(&app, MOVE));
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::KeyM);
        assert!(sent(&app, MOVE));
    }

    #[test]
    fn a_panels_own_keys_go_unheard_while_it_is_down() {
        let mut app = app();
        press(&mut app, KeyCode::Backspace);
        assert!(!sent(&app, CLOSE_ALL));
    }

    #[test]
    fn the_pause_menu_keeps_the_keys() {
        let mut app = app();
        press(&mut app, KeyCode::Escape);
        assert!(!app.world_mut().run_system_cached(pause::closed).unwrap());
        press(&mut app, KeyCode::KeyM);
        press(&mut app, KeyCode::F2);
        assert!(!sent(&app, MOVE) && open(&app).is_none());
    }

    #[test]
    fn its_button_asks_as_its_key_does() {
        let mut app = app();
        let button = app.world_mut().spawn(Asks::new("ButtonTechnology")).id();
        app.world_mut()
            .trigger(bevy::ui_widgets::Activate { entity: button });
        app.update();
        assert!(open(&app) == Some(Panel::Technologies));
    }

    #[test]
    fn politics_is_a_panel_only_where_it_has_a_window() {
        let mut app = app();
        let press = |app: &mut App| {
            let button = app.world_mut().spawn(Asks::new(crate::politics::BUTTON));
            let entity = button.id();
            app.world_mut()
                .trigger(bevy::ui_widgets::Activate { entity });
            app.update();
        };
        press(&mut app);
        assert!(open(&app).is_none());
        let window = (PanelWindow(Panel::Politics), Node::default());
        app.world_mut().spawn(window);
        press(&mut app);
        assert!(open(&app) == Some(Panel::Politics) && paused(&app));
        press(&mut app);
        assert!(open(&app).is_none() && !paused(&app));
        // Esc closes it though the hotkey files give it no key.
        press(&mut app);
        super::tests::press(&mut app, KeyCode::Escape);
        assert!(open(&app).is_none() && !paused(&app));
        let menu = app.world_mut().run_system_cached(pause::closed);
        assert!(menu.unwrap());
    }

    fn paused(app: &App) -> bool {
        app.world().resource::<Orders>().pausing()
    }

    #[test]
    fn its_key_opens_and_closes_it_pausing_the_match() {
        let mut app = app();
        press(&mut app, KeyCode::F2);
        assert!(open(&app) == Some(Panel::Technologies) && paused(&app));
        press(&mut app, KeyCode::F2);
        assert!(open(&app).is_none() && !paused(&app));
        press(&mut app, KeyCode::F2);
        press(&mut app, KeyCode::Backspace);
        assert!(open(&app).is_none() && !paused(&app));
    }

    #[test]
    fn another_panels_key_opens_it_in_its_place() {
        let mut app = app();
        press(&mut app, KeyCode::F2);
        press(&mut app, KeyCode::F3);
        assert!(open(&app) == Some(Panel::MessageLog) && paused(&app));
        press(&mut app, KeyCode::F3);
        assert!(open(&app).is_none() && !paused(&app));
    }

    #[test]
    fn escape_closes_it_without_opening_the_menu() {
        let mut app = app();
        press(&mut app, KeyCode::F2);
        press(&mut app, KeyCode::Escape);
        assert!(open(&app).is_none() && !paused(&app));
        let menu = app.world_mut().run_system_cached(pause::closed);
        assert!(menu.unwrap());
    }

    #[test]
    fn it_leaves_a_multiplayer_match_playing() {
        let mut app = app();
        let (_host, orders, _) = crate::orders::testing::joined(&[1]);
        app.insert_resource(orders);
        press(&mut app, KeyCode::F2);
        assert!(open(&app) == Some(Panel::Technologies) && !paused(&app));
        press(&mut app, KeyCode::F2);
        assert!(open(&app).is_none() && !paused(&app));
    }

    #[test]
    fn closing_it_leaves_a_match_paused_before_it_opened() {
        let mut app = app();
        (app.world_mut().resource_mut::<Orders>()).act(Action::Pause(true));
        press(&mut app, KeyCode::F2);
        press(&mut app, KeyCode::F2);
        assert!(open(&app).is_none() && paused(&app));
    }

    /// Asks for `action` as the pause key or button would.
    fn ask(app: &mut App, action: Action) {
        let world = app.world_mut();
        world.resource_scope(|world, mut orders: Mut<Orders>| {
            let mut displays = world.resource_mut::<Displays>();
            crate::clock::ask(&mut orders, Some(&mut displays), vec![action]);
        });
    }

    #[test]
    fn a_pause_asked_while_it_was_up_holds_once_it_closes() {
        let mut app = app();
        press(&mut app, KeyCode::F2);
        ask(&mut app, Action::Pause(false));
        ask(&mut app, Action::Pause(true));
        press(&mut app, KeyCode::F2);
        assert!(open(&app).is_none() && paused(&app));
    }

    #[test]
    fn an_unpause_asked_while_it_was_up_holds_once_it_closes() {
        let mut app = app();
        press(&mut app, KeyCode::F2);
        ask(&mut app, Action::Pause(false));
        press(&mut app, KeyCode::F2);
        assert!(open(&app).is_none() && !paused(&app));
    }

    #[test]
    fn closing_it_leaves_a_match_paused_while_it_was_up() {
        let mut app = app();
        press(&mut app, KeyCode::F2);
        app.world_mut().resource_mut::<Displays>().keep_paused();
        press(&mut app, KeyCode::F2);
        assert!(open(&app).is_none() && paused(&app));
    }
}
