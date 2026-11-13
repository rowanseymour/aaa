//! The display panels: windows over the match, each opened by a round
//! button on the minimap's rim or by its key, one at a time. The match holds still while one's up, unless it was already.

use bevy::asset::RenderAssetUsages;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{Activate, Button, ScrollArea};

use crate::actions::Blocks;
use crate::keys::Bindings;
use crate::ui::{self, Click, Edge, Font, Hint, Ink, Marked, Surface, Theme, label};
use crate::{Screen, minimap};

/// What the key that closes whichever panel is up is bound to.
const CLOSE_ALL: &str = "CloseSideOrDisplayPanel";

/// A glyph's size on its button, and its texels across, more than it's
/// drawn at so it stays crisp once the interface is scaled up.
const GLYPH_SIZE: f32 = 22.0;
const GLYPH_TEXELS: u32 = 64;

/// The layout the windows' places and sizes are given on.
const LAYOUT: Vec2 = Vec2::new(1024.0, 768.0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Objectives,
    Technologies,
    MessageLog,
}

impl Panel {
    /// What the keys that open it, and close it while it's up, are bound
    /// to.
    pub fn keys(self) -> (&'static str, &'static str) {
        match self {
            Panel::Objectives => ("ButtonObjectives", "ObjectivesDisplayClose"),
            Panel::Technologies => ("ButtonTechnology", "TechnologyDisplayClose"),
            Panel::MessageLog => ("ButtonMessageLog", "MessageLogToggle"),
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

/// The places on the minimap's rim its round buttons sit, between the
/// diamond's corners.
#[derive(Clone, Copy)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    /// Where a round button sits in it.
    pub fn node(self) -> Node {
        let centre = crate::MARGIN + minimap::DISC / 2.0 - minimap::CORNER_BUTTON / 2.0;
        let near = px(centre - minimap::BOLT);
        let far = px(centre + minimap::BOLT);
        let (left, bottom) = match self {
            Corner::TopLeft => (near, far),
            Corner::TopRight => (far, far),
            Corner::BottomLeft => (near, near),
            Corner::BottomRight => (far, near),
        };
        Node {
            position_type: PositionType::Absolute,
            left,
            bottom,
            width: px(minimap::CORNER_BUTTON),
            height: px(minimap::CORNER_BUTTON),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::MAX,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        }
    }
}

/// The button that opens a panel, with its name to mark its key in.
#[derive(Component)]
struct PanelButton {
    panel: Panel,
    name: String,
}

/// The glyph on a [`PanelButton`].
#[derive(Component)]
struct Glyph;

/// A panel's window.
#[derive(Component)]
struct PanelWindow(Panel);

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
            .add_systems(OnEnter(Screen::Match), reset)
            .add_systems(
                Update,
                keys.in_set(Keys)
                    .after(crate::pause::Toggle)
                    .run_if(in_state(Screen::Match))
                    .run_if(crate::console::closed)
                    .run_if(crate::pause::closed)
                    .run_if(loaded),
            )
            .add_systems(
                Update,
                light_buttons.after(Keys).run_if(in_state(Screen::Match)),
            )
            .add_systems(
                Update,
                mark_buttons
                    .run_if(resource_exists_and_changed::<Bindings>)
                    .run_if(in_state(Screen::Match)),
            );
    }
}

/// Where the panels' keys are read, for what shows them to go after.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Keys;

/// Whether the loading screen has gone, so the match is up to look at.
fn loaded(loading: Query<(), With<crate::LoadingScreen>>) -> bool {
    loading.is_empty()
}

fn reset(mut displays: ResMut<Displays>) {
    *displays = Displays::default();
}

/// Spawns the button at `corner` that opens `panel`, drawn with `glyph`.
pub fn spawn_button(
    commands: &mut Commands,
    bindings: &Bindings,
    panel: Panel,
    corner: Corner,
    glyph: Handle<Image>,
    name: String,
    help: String,
) {
    commands
        .spawn((
            Blocks,
            Button,
            Click::PUSH,
            RelativeCursorPosition::default(),
            bindings.marked(panel.keys().0, &name),
            Hint(help),
            PanelButton { panel, name },
            corner.node(),
            Surface::Clear,
            Edge::Border,
            GlobalZIndex(2),
            DespawnOnExit(Screen::Match),
            children![(
                Glyph,
                ImageNode::new(glyph),
                Node {
                    width: px(GLYPH_SIZE),
                    height: px(GLYPH_SIZE),
                    ..default()
                },
                Pickable::IGNORE,
            )],
        ))
        .observe(move |_: On<Activate>, mut toggling: Toggling| toggling.toggle(panel));
}

/// Spawns a panel's window, hidden: its title, the box it shows its
/// contents in, and a Close button. Returns the box.
pub fn spawn_window(commands: &mut Commands, window: Window) -> Entity {
    let at = |v: f32, of: f32| percent(100.0 * v / of);
    let panel = window.panel;
    let mut contents = Entity::PLACEHOLDER;
    commands
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
                    row.spawn(ui::button("Close", Font::Hud))
                        .observe(|_: On<Activate>, mut toggling: Toggling| toggling.close());
                });
        });
    contents
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
    time: ResMut<'w, Time<Virtual>>,
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
            displays.paused_it = !self.time.is_paused();
            self.time.pause();
        }
        displays.open = Some(panel);
        self.show();
    }

    /// Closes whichever panel's up, letting the match go on if opening it
    /// held it.
    pub fn close(&mut self) {
        let displays = &mut *self.displays;
        if displays.open.take().is_none() {
            return;
        }
        if std::mem::take(&mut displays.paused_it) {
            self.time.unpause();
        }
        self.show();
    }

    fn show(&mut self) {
        for (PanelWindow(panel), mut node) in &mut self.windows {
            let display = match self.displays.is_open(*panel) {
                true => Display::Flex,
                false => Display::None,
            };
            node.display = display;
        }
    }
}

/// Opens or closes the panels on their keys.
fn keys(keys: Res<ButtonInput<KeyCode>>, bindings: Res<Bindings>, mut toggling: Toggling) {
    if let Some(open) = toggling.displays.open {
        let close = [open.keys().1, CLOSE_ALL];
        if close.iter().any(|action| bindings.pressed(&keys, action)) {
            toggling.close();
            return;
        }
    }
    let panels = [Panel::Objectives, Panel::Technologies, Panel::MessageLog];
    let pressed = panels
        .into_iter()
        .find(|p| bindings.pressed(&keys, p.keys().0));
    if let Some(panel) = pressed.filter(|&p| !toggling.displays.is_open(p)) {
        toggling.open(panel);
    }
}

/// Marks the buttons' keys afresh once the hotkeys change.
fn mark_buttons(bindings: Res<Bindings>, mut buttons: Query<(&PanelButton, &mut Marked)>) {
    for (button, mut marked) in &mut buttons {
        marked.set_if_neq(bindings.marked(button.panel.keys().0, &button.name));
    }
}

/// What [`light_buttons`] reads and sets on a button.
type ButtonLook = (
    &'static PanelButton,
    Ref<'static, Hovered>,
    Has<bevy::ui::Pressed>,
    &'static mut Edge,
    &'static mut Surface,
    &'static Children,
);

/// Lights a button while its panel's up, its edge while it's hovered, and
/// its face while it's held down.
fn light_buttons(
    displays: Res<Displays>,
    theme: Res<Theme>,
    mut buttons: Query<ButtonLook>,
    mut glyphs: Query<&mut ImageNode, With<Glyph>>,
) {
    let restyle = displays.is_changed() || theme.is_changed();
    for (button, hovered, pressed, mut edge, mut surface, children) in &mut buttons {
        let open = displays.is_open(button.panel);
        if restyle || hovered.is_changed() {
            edge.set_if_neq(Edge::lit(open, hovered.get(), Edge::Border));
        }
        surface.set_if_neq(match pressed {
            true => Surface::Hovered,
            false => Surface::Clear,
        });
        if !restyle {
            continue;
        }
        let colour = match open {
            true => theme.edge(Edge::Lit),
            false => theme.ink(Ink::Help),
        };
        for &child in children {
            if let Ok(mut glyph) = glyphs.get_mut(child)
                && glyph.color != colour
            {
                glyph.color = colour;
            }
        }
    }
}

/// A white glyph on a clear ground, to be tinted: where `inside` holds of
/// the square from -1 to 1 each way, y down.
pub fn glyph(inside: impl Fn(f32, f32) -> bool) -> Image {
    silhouette(GLYPH_TEXELS, inside)
}

/// A [`glyph`] of any number of texels across.
pub fn silhouette(texels: u32, inside: impl Fn(f32, f32) -> bool) -> Image {
    // Each texel's coverage from a grid of samples across it.
    const SAMPLES: u32 = 4;
    let half = texels as f32 / 2.0;
    let mut data = Vec::with_capacity((texels * texels * 4) as usize);
    for v in 0..texels {
        for u in 0..texels {
            let mut hits = 0;
            for j in 0..SAMPLES {
                for i in 0..SAMPLES {
                    let at = |t: u32, s: u32| {
                        (t as f32 + (s as f32 + 0.5) / SAMPLES as f32 - half) / half
                    };
                    hits += inside(at(u, i), at(v, j)) as u32;
                }
            }
            let alpha = (255 * hits / (SAMPLES * SAMPLES)) as u8;
            data.extend_from_slice(&[255, 255, 255, alpha]);
        }
    }
    Image::new(
        Extent3d {
            width: texels,
            height: texels,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

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
            hotkey("f2", "ButtonTechnology", None),
            hotkey("f3", "ButtonMessageLog", None),
            hotkey("f2", "TechnologyDisplay", Some("TechnologyDisplayClose")),
            hotkey("f3", "SideMessageLogPanel", Some("MessageLogToggle")),
            hotkey("esc", "TechnologyDisplay", Some("TechnologyDisplayClose")),
            hotkey("backspace", "TechnologyDisplay|1", Some(CLOSE_ALL)),
        ];
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin, PausePlugin))
            .insert_state(Screen::Match)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::console::Console>()
            .init_resource::<crate::actions::Page>()
            .init_resource::<crate::Focus>()
            .init_resource::<Displays>()
            .insert_resource(Bindings::new(&hotkeys, &HashMap::new()))
            .add_systems(Update, keys.after(pause::Toggle).run_if(pause::closed));
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

    fn paused(app: &App) -> bool {
        app.world().resource::<Time<Virtual>>().is_paused()
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
    fn closing_it_leaves_a_match_paused_before_it_opened() {
        let mut app = app();
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        press(&mut app, KeyCode::F2);
        press(&mut app, KeyCode::F2);
        assert!(open(&app).is_none() && paused(&app));
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
