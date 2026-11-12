//! The technology display: a window listing the technologies the player's
//! side holds, opened by a button beside the minimap or its key. The match
//! holds still while it's up, unless it was already.

use std::collections::HashMap;

use aaa_data::{Id, PropertyLook};
use aaa_render::textures;
use bevy::asset::RenderAssetUsages;
use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{Activate, Button, ScrollArea};

use crate::actions::Blocks;
use crate::keys::Bindings;
use crate::ui::{self, Click, Edge, Font, Hint, Ink, Marked, Surface, Theme, label, panel};
use crate::{Loaded, Local, Screen, Sim, minimap};

/// What the keys that open, close and close every display panel are
/// bound to.
pub const OPEN: &str = "ButtonTechnology";
const CLOSE: &str = "TechnologyDisplayClose";
const CLOSE_ALL: &str = "CloseSideOrDisplayPanel";

const GEAR_SIZE: f32 = 22.0;
/// The gear's texels across, more than it's drawn at so it stays crisp
/// once the interface is scaled up.
const GEAR_TEXELS: u32 = 64;
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
    /// The button's name, to mark its key in.
    name: String,
}

struct Line {
    name: String,
    icon: String,
    image: Option<Option<Handle<Image>>>,
    /// Its tooltip under its name: what it does, then its description.
    hint: String,
}

/// The button at the minimap's corner that opens the window: a gear in a
/// ring like the supply border's button, lit while the window's up.
#[derive(Component)]
struct TechButton;

/// The gear in the [`TechButton`].
#[derive(Component)]
struct Gear;

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
                (keys, refresh)
                    .chain()
                    .after(crate::pause::Toggle)
                    .run_if(in_state(Screen::Match))
                    .run_if(crate::console::closed)
                    .run_if(crate::pause::closed)
                    .run_if(loaded),
            )
            .add_systems(
                Update,
                light_button.after(keys).run_if(in_state(Screen::Match)),
            )
            .add_systems(
                Update,
                mark_button
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
    mut gear_image: bevy::prelude::Local<Option<Handle<Image>>>,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    bindings: Res<Bindings>,
) {
    let game = &mut loaded.game;
    let strings = game.strings();
    let said = |key: &str, or: &str| strings.get(key).map_or(or, String::as_str).to_string();
    let gear = (gear_image.get_or_insert_with(|| images.add(gear(GEAR_TEXELS)))).clone();
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
    let name = said(NAME, "Display Technologies");
    commands.insert_resource(TechDisplay::default());
    commands
        .spawn((
            TechButton,
            Blocks,
            Button,
            Click::PUSH,
            RelativeCursorPosition::default(),
            bindings.marked(OPEN, &name),
            Hint(said(HELP, "Displays a list of all your technologies.")),
            // At the minimap square's other top corner from the supply
            // border's button.
            Node {
                position_type: PositionType::Absolute,
                left: px(crate::MARGIN + minimap::DISC - minimap::CORNER_BUTTON),
                bottom: px(crate::MARGIN + minimap::DISC - minimap::CORNER_BUTTON),
                width: px(minimap::CORNER_BUTTON),
                height: px(minimap::CORNER_BUTTON),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Surface::Hud,
            Edge::Border,
            GlobalZIndex(2),
            DespawnOnExit(Screen::Match),
            children![(
                Gear,
                ImageNode::new(gear),
                Node {
                    width: px(GEAR_SIZE),
                    height: px(GEAR_SIZE),
                    ..default()
                },
                Pickable::IGNORE,
            )],
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
                ScrollArea,
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
        name,
    });
}

/// Marks the button's key afresh once the hotkeys change.
fn mark_button(
    bindings: Res<Bindings>,
    looks: Res<Looks>,
    mut buttons: Query<&mut Marked, With<TechButton>>,
) {
    for mut button in &mut buttons {
        button.set_if_neq(bindings.marked(OPEN, &looks.name));
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

impl TechDisplay {
    /// Keeps the match paused when the window closes, as a pause asked for
    /// while it's up should.
    pub fn keep_paused(&mut self) {
        self.paused_it = false;
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

/// What [`light_button`] reads and sets on the button.
type ButtonLook = (
    Ref<'static, Hovered>,
    Has<bevy::ui::Pressed>,
    &'static mut Edge,
    &'static mut Surface,
);

/// Lights the button while the window's up, its edge while it's hovered,
/// and its face while it's held down.
fn light_button(
    display: Option<Res<TechDisplay>>,
    theme: Res<Theme>,
    mut buttons: Query<ButtonLook, With<TechButton>>,
    mut gears: Query<&mut ImageNode, With<Gear>>,
) {
    let Some(display) = display else { return };
    let restyle = display.is_changed() || theme.is_changed();
    for (hovered, pressed, mut edge, mut surface) in &mut buttons {
        if restyle || hovered.is_changed() {
            edge.set_if_neq(Edge::lit(display.open, hovered.get(), Edge::Border));
        }
        surface.set_if_neq(match pressed {
            true => Surface::Hovered,
            false => Surface::Hud,
        });
    }
    if !restyle {
        return;
    }
    let colour = match display.open {
        true => theme.edge(Edge::Lit),
        false => theme.ink(Ink::Help),
    };
    for mut gear in &mut gears {
        if gear.color != colour {
            gear.color = colour;
        }
    }
}

/// A white gear `texels` across on a clear ground, to be tinted.
fn gear(texels: u32) -> Image {
    const TEETH: f32 = 8.0;
    const OUTER: f32 = 1.0;
    const ROOT: f32 = 0.7;
    const HOLE: f32 = 0.32;
    // Each texel's coverage from a grid of samples across it.
    const SAMPLES: u32 = 4;
    let half = texels as f32 / 2.0;
    let inside = |x: f32, y: f32| {
        let (x, y) = ((x - half) / half, (y - half) / half);
        let d = (x * x + y * y).sqrt();
        let turn = (y.atan2(x) / std::f32::consts::TAU * TEETH).rem_euclid(1.0);
        let rim = match (0.25..0.75).contains(&turn) {
            true => OUTER,
            false => ROOT,
        };
        d > HOLE && d < rim
    };
    let mut data = Vec::with_capacity((texels * texels * 4) as usize);
    for v in 0..texels {
        for u in 0..texels {
            let mut hits = 0;
            for j in 0..SAMPLES {
                for i in 0..SAMPLES {
                    let at = |t: u32, s: u32| t as f32 + (s as f32 + 0.5) / SAMPLES as f32;
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

    #[test]
    fn closing_it_leaves_a_match_paused_while_it_was_up() {
        let mut app = app();
        press(&mut app, KeyCode::F2);
        app.world_mut().resource_mut::<TechDisplay>().keep_paused();
        press(&mut app, KeyCode::F2);
        assert!(!open(&app) && paused(&app));
    }
}
