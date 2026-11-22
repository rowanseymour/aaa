//! The original's interface: its in-match panels, drawn from its layouts
//! (see [`tgdata::widgets`]), over a view of their 4:3 shape. They're
//! their pictures as they look at rest, the command grid's buttons, which
//! show what [`actions::Shown`](crate::actions::Shown) holds and ask for
//! their widget by name when pressed, other buttons that ask for what
//! their keys do, such as pausing, the resource bars, the object display,
//! and the one pane every tooltip is said in. What they show is shared
//! with ours; only the drawing is their own.

use std::collections::HashMap;

use bevy::camera::{Hdr, Viewport};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use tgdata::widgets::{Bounds, InterfaceFlags, RESTING, Rect, SCREEN, ViewElement, Widget};
use tgrender::textures;

use bevy::picking::hover::Hovered;
use bevy::ui_widgets::Button;

mod abilities;
mod bars;
mod experience;
pub(crate) mod lettering;
mod object;
mod politics;
mod shortage;
mod times;
mod tips;

use lettering::LayoutScale;
pub(crate) use object::{Cards, fill_bars};

use super::{Hud, InMatch};
use crate::actions::{COLUMNS, ROWS, Shown, Slot};
use crate::intent::Asks;
use crate::ui::Blocks;
use crate::{Loaded, Local, Sim};

/// While a match is played with the original's interface.
pub(crate) const DRAWN: InMatch = InMatch(Hud::Original);

pub struct OriginalHudPlugin;

impl Plugin for OriginalHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(DRAWN),
            (
                spawn,
                spawn_overlay,
                place_log.after(spawn),
                grey_icons,
                crate::object_display::redraw,
            ),
        )
        .add_systems(OnExit(DRAWN), leave)
        .add_systems(OnExit(crate::Screen::Match), |mut commands: Commands| {
            commands.remove_resource::<Greyed>();
        })
        .add_systems(
            PostUpdate,
            (
                match_textures,
                (fit_view, lettering::scale, lettering::share_face)
                    .chain()
                    .before(bevy::camera::CameraUpdateSystems)
                    .before(bevy::ui::UiSystems::Prepare),
            )
                .run_if(in_state(DRAWN)),
        )
        .add_systems(
            Update,
            (
                (
                    object::lay_out.in_set(super::LayOut),
                    (object::fill_bars, object::economy),
                )
                    .chain(),
                (
                    (select_toggles, show_pressables).chain(),
                    (abilities::refresh, abilities::show).chain(),
                    abilities::fill,
                    experience::refresh,
                    shortage::show,
                    times::show,
                    politics::show,
                    (show_grid, tips::show).chain(),
                    bars::slide,
                )
                    .in_set(super::Draw),
            )
                .run_if(in_state(DRAWN)),
        );
    }
}

/// The floaters the message log goes beside and under.
const LOG_LEFT_OF: &str = "FactionAbilitiesFloater";
const LOG_UNDER: &str = "GroupFloater";

/// Where on the layouts' screen the message log goes.
#[derive(Resource)]
struct LogAt(Vec2);

/// Puts the on-screen message log in the [`Frame`]'s top left corner, clear
/// of the floaters there, over the map's view rather than beside it.
fn place_log(
    mut commands: Commands,
    at: Option<Res<LogAt>>,
    frames: Query<Entity, With<Frame>>,
    mut logs: Query<(Entity, &mut Node), With<crate::messages::LogBox>>,
) {
    let (Ok(frame), Some(at)) = (frames.single(), at) else {
        return;
    };
    for (log, mut node) in &mut logs {
        node.left = Val::Percent(100.0 * at.0.x / SCREEN[0]);
        node.top = Val::Percent(100.0 * at.0.y / SCREEN[1]);
        commands.entity(frame).add_child(log);
    }
}

/// Takes down what [`spawn`] put up, leaving the message log for the
/// interface put up next to place, and gives the map's view back the whole
/// window.
fn leave(
    mut commands: Commands,
    frames: Query<Entity, With<Frame>>,
    logs: Query<Entity, With<crate::messages::LogBox>>,
    mut cameras: Query<&mut Camera, With<tgrender::camera::RtsCamera>>,
) {
    for log in &logs {
        // Taken out before the frame goes, as it would go with it.
        commands.entity(log).try_remove::<ChildOf>();
    }
    for frame in &frames {
        commands.entity(frame).try_despawn();
    }
    for mut camera in &mut cameras {
        camera.viewport = None;
    }
    commands.remove_resource::<LogAt>();
    commands.remove_resource::<lettering::Letters>();
    commands.remove_resource::<object::Layout>();
    commands.remove_resource::<LayoutScale>();
}

/// Spawns the camera the interface is drawn by over the whole window, as
/// that drawn by the map's camera would be cut to its view by [`fit_view`].
pub fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        // Its finishing pass runs over the map's picture too, which the
        // map's camera leaves as it is.
        Tonemapping::None,
        DebandDither::Disabled,
        Overlay,
        DespawnOnExit(DRAWN),
    ));
}

/// The camera [`spawn_overlay`] spawns.
#[derive(Component)]
pub struct Overlay;

/// Has the [`Overlay`] take the map camera's MSAA and HDR: drawing over what
/// that camera drew needs the same kind of texture, as with another the two
/// draw to different textures and the overlay's shows what's stale.
pub fn match_textures(
    mut commands: Commands,
    map: Query<(&Msaa, Has<Hdr>), With<tgrender::camera::RtsCamera>>,
    mut overlays: Query<(Entity, &mut Msaa, Has<Hdr>), OverlayOnly>,
) {
    let Ok((&msaa, hdr)) = map.single() else {
        return;
    };
    for (overlay, mut samples, has_hdr) in &mut overlays {
        samples.set_if_neq(msaa);
        match (hdr, has_hdr) {
            (true, false) => _ = commands.entity(overlay).insert(Hdr),
            (false, true) => _ = commands.entity(overlay).remove::<Hdr>(),
            _ => {}
        }
    }
}

type OverlayOnly = (With<Overlay>, Without<tgrender::camera::RtsCamera>);

/// The layouts' screen, which [`fit_view`] fits the map's view to.
#[derive(Component)]
pub struct Frame;

/// Fits the map's view, and the [`Frame`] over it, to the largest
/// rectangle of the layouts' shape centred in what's drawn to, black
/// beside it: the panels' pictures only line up at that shape, and the map
/// shows within them and nowhere else.
pub fn fit_view(
    mut cameras: Query<&mut Camera, With<tgrender::camera::RtsCamera>>,
    mut frames: Query<&mut Node, With<Frame>>,
    ui: Res<UiScale>,
    mut layout: ResMut<LayoutScale>,
) {
    for mut camera in &mut cameras {
        let (Some(size), Some(scale)) = (
            camera.physical_target_size(),
            camera.target_scaling_factor(),
        ) else {
            continue;
        };
        let (at, size) = fit(size);
        layout.set_if_neq(LayoutScale::of(size.y as f32 / scale / ui.0));
        let now = (camera.viewport.as_ref()).map(|v| (v.physical_position, v.physical_size));
        if now != Some((at, size)) {
            camera.viewport = Some(Viewport {
                physical_position: at,
                physical_size: size,
                ..default()
            });
        }
        for mut node in &mut frames {
            let mut placed = node.clone();
            crate::ui_scale::place_on_map(
                &mut placed,
                at.as_vec2() / scale,
                size.as_vec2() / scale,
                &ui,
            );
            node.set_if_neq(placed);
        }
    }
}

/// How many of the interface's pixels a pixel of the layouts' screen is,
/// drawn as [`fit_view`] fits it within a window `size` across, in logical
/// pixels.
pub fn layout_scale(size: Vec2) -> f32 {
    size.y.min(size.x * SCREEN[1] / SCREEN[0]) / SCREEN[1]
}

/// The corner and size of the largest rectangle of the layouts' shape
/// centred in `size`.
fn fit(size: UVec2) -> (UVec2, UVec2) {
    let shape = SCREEN[0] / SCREEN[1];
    let width = ((size.y as f32 * shape).round() as u32).min(size.x);
    let height = ((width as f32 / shape).round() as u32).min(size.y);
    let fitted = UVec2::new(width, height);
    ((size - fitted) / 2, fitted)
}

/// The panel the parts of the screen's lower edge are placed within.
const CONTROL_PANEL: &str = "ControlPanel";

/// The panels placed within the control panel, in the order they're drawn:
/// those before its own buttons, and those after.
const UNDER_BUTTONS: [&str; 2] = [OBJECT_DISPLAY, "ActionButtons"];

/// The panel showing what's selected: with nothing selected, only its own
/// pictures, as its parts show the selection.
const OBJECT_DISPLAY: &str = "ObjectDisplay";
/// The panel the minimap's drawn in, over the rest of the control panel.
const MINIMAP: &str = "GameMiniMap";

/// The match layer's own part that's on screen from the start, and the
/// panels placed against the whole screen that are, with the flag each
/// needs, if any.
const ON_GAME: &str = "ControlPanelTop";
const GAME: &str = "Game";
type Flag = fn(&InterfaceFlags) -> bool;
const ON_SCREEN: [(&str, Option<Flag>); 4] = [
    ("GroupFloater", None),
    ("FactionAbilitiesFloater", None),
    ("FactionExperience", Some(|f| f.faction_experience)),
    ("RecruitItemFloater", Some(|f| f.recruit_item_floater)),
];

/// The control panel's own parts that only some games have, with the flag
/// each needs.
const OPTIONAL: [(&str, Flag); 2] = [
    ("SidePanelTabs", |f| f.side_panel),
    ("ButtonPause", |f| f.pause_button),
];

/// One of the control panel's buttons that asks for what its key does.
struct Press {
    widget: &'static str,
    asks: &'static str,
    /// What it's [`called`](crate::ui::called): where ours has the same
    /// button, what ours is, so a script works with either.
    called: &'static str,
    click: crate::ui::Click,
    /// What it's switched on while, for one that stays on or off.
    follows: Option<Follows>,
}

impl Press {
    /// A button pushed to ask for `asks`.
    const fn push(widget: &'static str, asks: &'static str, called: &'static str) -> Press {
        Press {
            widget,
            asks,
            called,
            click: crate::ui::Click::PUSH,
            follows: None,
        }
    }

    /// The button that opens `panel`, which asks for it by its own name.
    const fn panel(panel: crate::displays::Panel) -> Press {
        Press::push(panel.keys().0, panel.keys().0, panel.called())
    }

    /// A button that stays on while what it `follows` is.
    const fn toggle(
        widget: &'static str,
        asks: &'static str,
        called: &'static str,
        follows: Follows,
    ) -> Press {
        Press {
            widget,
            asks,
            called,
            click: crate::ui::Click::TOGGLE,
            follows: Some(follows),
        }
    }
}

static PRESSABLE: [Press; 10] = [
    Press::push("ButtonMenu", crate::pause::MENU, "control.menu"),
    Press::toggle(
        "ButtonPause",
        crate::clock::PAUSE,
        crate::clock::PAUSE_BUTTON,
        Follows::Pause,
    ),
    Press::toggle(
        "ButtonSupplyBorder",
        crate::keys::SUPPLY_BORDER,
        "control.supply-border",
        Follows::Border,
    ),
    Press::toggle(
        "ButtonChat",
        crate::chat::ACTIVATE,
        crate::chat::BUTTON,
        Follows::Chat,
    ),
    Press::push(
        "ButtonSpeedDown",
        crate::clock::SPEED_DOWN,
        "control.slower",
    ),
    Press::push("ButtonSpeedUp", crate::clock::SPEED_UP, "control.faster"),
    Press::panel(crate::displays::Panel::Objectives),
    Press::panel(crate::displays::Panel::Technologies),
    Press::panel(crate::displays::Panel::MessageLog),
    Press::toggle(
        crate::politics::BUTTON,
        crate::politics::BUTTON,
        crate::politics::CALLED,
        Follows::Politics,
    ),
];

/// What a [`Pressable`] that stays on or off is switched on while.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Follows {
    /// The match is paused.
    Pause,
    /// The supply border is drawn.
    Border,
    /// The chat bar is open.
    Chat,
    /// The politics panel is open.
    Politics,
}

/// The widgets the interface repeats once for each entry a panel lists,
/// such as a group or a command: templates, not shown themselves, and
/// with nothing listed at rest.
const ENTRIES: [&str; 4] = [
    "Item",
    "GroupButton",
    crate::actions::WIDGET,
    abilities::ENTRY,
];

/// The command grid.
const GRID: &str = "ActionButtons";
/// The part of a grid button its command's icon is drawn in.
const GRID_ICON: &str = "ActionButtonsButtonDecal";
/// The fields giving how far in from each edge of a grid button its icon
/// is drawn, normally and while it's held down.
const ICON_INSET: &str = "action_button_unpressed_client_";
const ICON_INSET_PRESSED: &str = "action_button_pressed_client_";

/// One step from the screen to a picture: into a child's bounds, or moved
/// by an offset.
#[derive(Clone, Copy)]
enum Step {
    Into(Bounds),
    By([f32; 2]),
}

/// Draws the original's panels, in the skin of the player's nation.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    mut commands: Commands,
    loaded: Res<Loaded>,
    sim: Res<Sim>,
    local: Res<Local>,
    bindings: Res<crate::keys::Bindings>,
    mut images: ResMut<Assets<Image>>,
    mut faces: ResMut<Assets<bevy::text::Font>>,
    formats: Res<CompressedImageFormatSupport>,
    minimap: Res<crate::minimap::Minimap>,
    operations: Option<Res<crate::special_ops::Panel>>,
) {
    let game = &loaded.game;
    let skin = skin(game, &sim, &local);
    let widgets = game.game_widgets();
    let named = |name: &str| (widgets.iter()).find(|w| w.name.eq_ignore_ascii_case(name));
    let mut abilities = Vec::new();
    let mut experience = None;
    let mut drawer = Drawer {
        pictures: Pictures {
            game,
            skin: skin.as_ref(),
            images: &mut images,
            formats: formats.0,
            known: HashMap::new(),
        },
        flags: game.interface_flags(),
        drawn: Vec::new(),
        pressables: Vec::new(),
    };
    let mut grid = None;
    let mut letters = lettering::Letters::default();
    let mut object = None;
    // Where the minimap's drawn, and its panel's pictures over it.
    let mut minimap_at = None;
    if let Some(top) = named(GAME).and_then(|g| g.find(ON_GAME)) {
        drawer.widget(top, &[]);
    }
    if let Some(panel) = named(CONTROL_PANEL)
        && let Some(chain) = drawer.own(panel, &[])
    {
        for name in UNDER_BUTTONS {
            match named(name) {
                Some(w) if name == OBJECT_DISPLAY => {
                    if let Some(within) = drawer.own(w, &chain) {
                        let fonts = (&mut letters, &mut *faces);
                        let at = reach(&within);
                        object = Some(object::Layout::read(w, at, &mut drawer.pictures, fonts));
                    }
                }
                Some(w) if name == GRID => {
                    if let Some(within) = drawer.own(w, &chain) {
                        for child in &w.children {
                            drawer.widget(child, &within);
                        }
                        grid = w.find(crate::actions::WIDGET).map(|b| (reach(&within), b));
                    }
                }
                Some(w) => drawer.widget(w, &chain),
                None => {}
            }
        }
        for child in &panel.children {
            drawer.widget(child, &chain);
        }
        minimap_at = named(MINIMAP).and_then(|widget| {
            let within = drawer.own(widget, &chain)?;
            let under = drawer.drawn.len();
            for child in &widget.children {
                drawer.widget(child, &within);
            }
            Some((within, drawer.drawn.split_off(under)))
        });
    }
    for (name, flag) in ON_SCREEN {
        if flag.is_some_and(|f| !f(&drawer.flags)) {
            continue;
        }
        let Some(widget) = named(name) else {
            continue;
        };
        // A resource bar rests that far from its bounds until a preview
        // slides it there.
        let offset = (widget.value("hidden_offset"))
            .and_then(point2)
            .map(Step::By);
        let Some(within) = drawer.own(widget, &Vec::from_iter(offset)) else {
            continue;
        };
        for child in &widget.children {
            drawer.widget(child, &within);
        }
        let panel = operations.as_deref();
        if name == abilities::FLOATER {
            abilities = abilities::build(widget, &within, &mut drawer, panel, &bindings);
        } else if name == experience::WIDGET {
            let fonts = (&mut letters, &mut *faces);
            experience = experience::build(widget, &within, &mut drawer, panel, fonts);
        }
    }
    // The message log goes in the corner between the floater along the top
    // and the one down the left.
    let screen = Rect {
        left: 0.0,
        top: 0.0,
        right: SCREEN[0],
        bottom: SCREEN[1],
    };
    let edge = |name: &str, of: fn(Rect) -> f32| named(name).map_or(0.0, |w| of(w.bounds(screen)));
    commands.insert_resource(LogAt(Vec2::new(
        edge(LOG_LEFT_OF, |at| at.right),
        edge(LOG_UNDER, |at| at.bottom),
    )));
    let colour = |c: Option<[f32; 3]>, or: Color| c.map_or(or, |[r, g, b]| Color::srgb(r, g, b));
    let short = colour(game.invalid_colour(), Color::srgb(1.0, 0.0, 0.0));
    let key = colour(game.highlight_colour(), Color::srgb(1.0, 1.0, 0.0));
    let shortage = named(shortage::WIDGET).map(|w| shortage::build(w, &mut drawer));
    let politics = politics::build(&widgets, &mut drawer, (&mut letters, &mut *faces));
    let times: Vec<_> = (times::WIDGETS.iter())
        .filter_map(|&(name, which)| {
            let fonts = (&mut letters, &mut *faces);
            times::build(named(name)?, which, &mut drawer, fonts)
        })
        .collect();
    let cells = game.economy_bar();
    let impact = drawer.flags.economic_impact;
    let resource_bars: Vec<_> = (bars::BARS.iter())
        .filter(|name| **name != bars::BARS[0] || impact)
        .filter_map(|name| named(name))
        .map(|bar| drawer.bar(bar, &cells, (&mut letters, &mut faces)))
        .collect();
    let pane = tips::Built::new(
        &widgets,
        &mut drawer.pictures,
        (&mut letters, &mut faces),
        (key, short),
    );
    let buttons = grid.map(|(area, button)| grid_buttons(area, button, &mut drawer.pictures));
    let (drawn, pressables) = (drawer.drawn, drawer.pressables);
    let mut docked = None;
    // Where the original stretches it over a wider window instead.
    let frame = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            Frame,
            GlobalZIndex(crate::ui::layer::HUD),
            Pickable::IGNORE,
        ))
        .with_children(|screen| {
            for (chain, image) in drawn {
                screen.spawn(picture(&chain, image));
            }
            // Over the panels' pictures, as what's selected shows on them.
            if object.is_some() {
                screen.spawn(object::Holder::bundle());
            }
            for bar in resource_bars {
                bar.spawn(screen, game, &letters, short);
            }
            for button in buttons.into_iter().flatten() {
                button.spawn(screen);
            }
            for button in pressables {
                button.spawn(screen, &bindings);
            }
            for ability in abilities {
                ability.spawn(screen);
            }
            if let Some(experience) = experience {
                experience.spawn(screen, &letters);
            }
            if let Some(shortage) = shortage {
                shortage.spawn(screen);
            }
            for label in times {
                label.spawn(screen, &letters);
            }
            if let Some(politics) = politics {
                politics.spawn(screen);
            }
            if let Some((within, over)) = minimap_at {
                screen.spawn((placed(&within), minimap.view()));
                for (chain, image) in over {
                    screen.spawn(picture(&chain, image));
                }
            }
            docked = pane.map(|pane| pane.spawn(screen));
        })
        .id();
    // Every hint on the layouts' screen is said in the pane.
    if let Some(pane) = docked {
        commands.entity(frame).insert(crate::ui::Docks(pane));
    }
    // After the layouts' own, so as not to change which face stands in for
    // theirs the data doesn't hold.
    letters.read(game, &mut faces, lettering::PLAIN);
    commands.insert_resource(letters);
    if let Some(object) = object {
        commands.insert_resource(object);
    }
    commands.init_resource::<LayoutScale>();
}

/// One of the panels' pictures, given the steps from the screen to it.
fn picture(chain: &[Step], image: Handle<Image>) -> impl Bundle {
    (
        placed(chain),
        ImageNode::new(image).with_mode(NodeImageMode::Stretch),
        // Clicks on the panels aren't the map's.
        Blocks,
        RelativeCursorPosition::default(),
    )
}

/// Where on the layouts' screen the steps from it lead.
fn reach(chain: &[Step]) -> Rect {
    let screen = Rect {
        left: 0.0,
        top: 0.0,
        right: SCREEN[0],
        bottom: SCREEN[1],
    };
    chain.iter().fold(screen, |within, step| match *step {
        Step::Into(b) => b.place(within),
        Step::By([x, y]) => Rect {
            left: within.left + x,
            top: within.top + y,
            right: within.right + x,
            bottom: within.bottom + y,
        },
    })
}

/// A picture's node, given the steps from the screen to it.
fn placed(chain: &[Step]) -> Node {
    on_screen(reach(chain))
}

/// The node covering `at`, a rectangle of the layouts' screen.
fn on_screen(at: Rect) -> Node {
    let percent = |v: f32, of: f32| Val::Percent(100.0 * v / of);
    Node {
        position_type: PositionType::Absolute,
        left: percent(at.left, SCREEN[0]),
        top: percent(at.top, SCREEN[1]),
        width: percent(at.width().max(0.0), SCREEN[0]),
        height: percent(at.height().max(0.0), SCREEN[1]),
        ..default()
    }
}

/// The skin of the viewing player's nation, which the original's panels
/// are drawn in.
fn skin(game: &tgdata::GameData, sim: &Sim, local: &Local) -> Option<tgdata::Skin> {
    let nation = (local.0)
        .and_then(|p| sim.0.sides.player(p))
        .and_then(|p| p.nation.clone())?;
    let nations = game.nations();
    let nation = nations
        .into_iter()
        .find(|n| n.id == tgdata::Id::from(nation.as_str()))?;
    game.skin(&nation.skin)
}

/// Reads the group display's [`Cards`] from the original's layouts, for
/// an interface that embeds them without drawing the rest.
pub(crate) fn load_cards(
    mut commands: Commands,
    loaded: Res<Loaded>,
    sim: Res<Sim>,
    local: Res<Local>,
    mut images: ResMut<Assets<Image>>,
    mut faces: ResMut<Assets<bevy::text::Font>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &loaded.game;
    let skin = skin(game, &sim, &local);
    let widgets = game.game_widgets();
    let named = |name: &str| (widgets.iter()).find(|w| w.name.eq_ignore_ascii_case(name));
    let Some(display) = named(OBJECT_DISPLAY) else {
        return;
    };
    let mut pictures = Pictures {
        game,
        skin: skin.as_ref(),
        images: &mut images,
        formats: formats.0,
        known: HashMap::new(),
    };
    let mut letters = lettering::Letters::default();
    let at = display.bounds(Rect {
        left: 0.0,
        top: 0.0,
        right: SCREEN[0],
        bottom: SCREEN[1],
    });
    let layout = object::Layout::read(display, at, &mut pictures, (&mut letters, &mut faces));
    commands.insert_resource(Cards { layout, letters });
}

/// Takes down what [`load_cards`] read.
pub(crate) fn unload_cards(mut commands: Commands) {
    commands.remove_resource::<Cards>();
}

/// The pictures read so far, by archive path. They're decoded afresh for
/// each reading of the layouts rather than shared in the
/// [`ImageCache`](crate::ui::ImageCache), as their pixels are read to cut
/// and grey them, which a picture loses once it's drawn.
struct Pictures<'a> {
    game: &'a tgdata::GameData,
    skin: Option<&'a tgdata::Skin>,
    images: &'a mut Assets<Image>,
    formats: bevy::image::CompressedImageFormats,
    known: HashMap<String, Option<Handle<Image>>>,
}

impl Pictures<'_> {
    fn get(&mut self, path: &str) -> Option<Handle<Image>> {
        if let Some(known) = self.known.get(path) {
            return known.clone();
        }
        let skinned = self.skin.and_then(|s| s.read(path));
        let image = (skinned.or_else(|| self.game.read(path)))
            .and_then(|bytes| textures::decode(&bytes, path, self.formats, true))
            .map(|i| self.images.add(i));
        self.known.insert(path.to_string(), image.clone());
        image
    }

    /// The picture at `path`'s size and its pixels, four bytes each, for
    /// making pictures of its parts.
    fn rgba(&mut self, path: &str) -> Option<(UVec2, Vec<u8>)> {
        let handle = self.get(path)?;
        let image = crate::ui::rgba8(self.images.get(&handle)?)?;
        Some((image.size(), image.data?))
    }
}

/// A value written as two numbers, e.g. `0,25`.
fn point2(value: &str) -> Option<[f32; 2]> {
    let (x, y) = value.split_once(',')?;
    Some([x.trim().parse().ok()?, y.trim().parse().ok()?])
}

/// What's drawn so far, in order, with the steps to each picture.
struct Drawer<'a> {
    pictures: Pictures<'a>,
    flags: InterfaceFlags,
    drawn: Vec<(Vec<Step>, Handle<Image>)>,
    pressables: Vec<BuiltPressable>,
}

impl Drawer<'_> {
    /// Adds the pictures a widget shows at rest, `within` the steps to its
    /// parent.
    fn widget(&mut self, widget: &Widget, within: &[Step]) {
        let Some(chain) = self.own(widget, within) else {
            return;
        };
        for child in &widget.children {
            self.widget(child, &chain);
        }
    }

    /// Adds the pictures of a widget's own, not its children's, and gives
    /// the steps to its bounds; `None` for a widget not shown.
    fn own(&mut self, widget: &Widget, within: &[Step]) -> Option<Vec<Step>> {
        let optional = (OPTIONAL.iter()).find(|(name, _)| widget.name.eq_ignore_ascii_case(name));
        let entry = (ENTRIES.iter()).any(|name| widget.name.eq_ignore_ascii_case(name));
        if !widget.visible || entry || optional.is_some_and(|(_, flag)| !flag(&self.flags)) {
            return None;
        }
        let press = (PRESSABLE.iter()).find(|p| widget.name.eq_ignore_ascii_case(p.widget));
        if let Some(press) = press {
            self.pressable(press, widget, within);
            return None;
        }
        let mut chain = within.to_vec();
        if let Some(view) = &widget.view {
            chain.push(Step::Into(view.bounds));
            self.element(view, &chain);
        }
        for part in &widget.parts {
            let mut chain = chain.clone();
            chain.push(Step::Into(part.bounds));
            self.element(part, &chain);
        }
        Some(chain)
    }

    /// Keeps a [`PRESSABLE`] widget to spawn as a button, with all its
    /// pictures, as each is placed and the states each is drawn in.
    fn pressable(&mut self, press: &'static Press, widget: &Widget, within: &[Step]) {
        let mut chain = within.to_vec();
        let mut faces = Vec::new();
        if let Some(view) = &widget.view {
            chain.push(Step::Into(view.bounds));
            self.faces(view, &chain, &[], &mut faces);
        }
        for part in &widget.parts {
            let mut chain = chain.clone();
            chain.push(Step::Into(part.bounds));
            self.faces(part, &chain, &[], &mut faces);
        }
        let (name, help) = widget.tooltip(&self.pictures.game.strings());
        self.pressables.push(BuiltPressable {
            press,
            name: name.unwrap_or_default(),
            help: help.unwrap_or_default(),
            at: reach(&chain),
            faces,
        });
    }

    /// Adds `element`'s pictures and its children's to `faces`, each drawn
    /// in the states it and those it's in are, `chain` ending in its own
    /// bounds.
    fn faces(
        &mut self,
        element: &ViewElement,
        chain: &[Step],
        within: &[ViewElement],
        faces: &mut Vec<Face>,
    ) {
        let mut when = within.to_vec();
        when.push(ViewElement {
            children: Vec::new(),
            ..element.clone()
        });
        if let Some(image) = (element.texture.as_deref()).and_then(|t| self.pictures.get(t)) {
            let greyed = (self.pictures.images.get(&image))
                .and_then(grey)
                .map(|g| self.pictures.images.add(g));
            faces.push(Face {
                at: reach(chain),
                when: when.clone(),
                image,
                greyed,
            });
        }
        for child in &element.children {
            let mut chain = chain.to_vec();
            chain.push(Step::Into(child.bounds));
            self.faces(child, &chain, &when, faces);
        }
    }

    /// Adds a view element's pictures, `chain` ending in its own bounds.
    fn element(&mut self, element: &ViewElement, chain: &[Step]) {
        if !element.shown(&RESTING) {
            return;
        }
        if let Some(image) = (element.texture.as_deref()).and_then(|t| self.pictures.get(t)) {
            self.drawn.push((chain.to_vec(), image));
        }
        for child in &element.children {
            let mut chain = chain.to_vec();
            chain.push(Step::Into(child.bounds));
            self.element(child, &chain);
        }
    }
}

/// One of the command grid's buttons, and the mouse buttons that went
/// down on it and are still down, which hold it down.
#[derive(Component, Default)]
pub struct GridButton(Vec<MouseButton>);

/// A picture of a grid button's that's drawn only in the states it names.
#[derive(Component)]
pub struct Shows(ViewElement);

/// A grid button's command icon, and how far in from each edge of the
/// button it's drawn, as a fraction of the button's size, normally and
/// while held down.
#[derive(Component)]
pub struct GridIcon {
    inset: [f32; 4],
    pressed: [f32; 4],
}

/// A grid button ready to spawn.
struct Built {
    slot: usize,
    at: Rect,
    /// Its frame's pictures, each with when it's drawn.
    frame: Vec<(ViewElement, Handle<Image>)>,
    icon: GridIcon,
}

impl Built {
    fn spawn(self, screen: &mut bevy::ecs::hierarchy::ChildSpawnerCommands) {
        let slot = self.slot;
        let fill = Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        };
        screen
            .spawn((
                GridButton::default(),
                Slot(slot),
                Asks::new(crate::actions::widget(slot)),
                on_screen(self.at),
                Button,
                // As ours are, so a script works with either.
                crate::ui::called(format!("action.{}", slot + 1)),
                Hovered::default(),
                Visibility::Hidden,
                Blocks,
                RelativeCursorPosition::default(),
            ))
            .with_children(|button| {
                for (element, image) in self.frame {
                    button.spawn((
                        fill.clone(),
                        ImageNode::new(image).with_mode(NodeImageMode::Stretch),
                        Shows(element),
                        Pickable::IGNORE,
                    ));
                }
                button.spawn((
                    fill.clone(),
                    ImageNode::default().with_mode(NodeImageMode::Stretch),
                    self.icon,
                    Pickable::IGNORE,
                ));
            })
            .observe(hold);
    }
}

/// A button of [`PRESSABLE`]'s, drawn in the states it's held in.
#[derive(Component, Default)]
pub struct Pressable {
    /// Whether it's switched on, such as the pause button while the match
    /// is paused; set by whatever it asks for.
    pub selected: bool,
}

/// One of a [`Pressable`]'s pictures: where it's drawn on the screen, the
/// view elements it's in, from the outermost to its own, each of whose
/// states must hold for it to be drawn, and its picture and a greyed one
/// for while the button's disabled.
struct Face {
    at: Rect,
    when: Vec<ViewElement>,
    image: Handle<Image>,
    greyed: Option<Handle<Image>>,
}

/// A [`Face`] as spawned.
#[derive(Component)]
pub struct Drawn {
    when: Vec<ViewElement>,
    image: Handle<Image>,
    greyed: Option<Handle<Image>>,
}

/// A [`Pressable`] ready to spawn.
struct BuiltPressable {
    press: &'static Press,
    name: String,
    help: String,
    at: Rect,
    faces: Vec<Face>,
}

impl BuiltPressable {
    fn spawn(
        self,
        screen: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
        bindings: &crate::keys::Bindings,
    ) {
        let press = self.press;
        let at = self.at;
        let mut button = screen.spawn((
            Pressable::default(),
            Asks::new(press.asks),
            crate::keys::Named::new(bindings, press.asks, self.name),
            crate::ui::Hint(self.help),
            on_screen(at),
            Button,
            press.click,
            crate::ui::called(press.called),
            Hovered::default(),
            Blocks,
            RelativeCursorPosition::default(),
        ));
        if let Some(follows) = press.follows {
            button.insert(follows);
            // The pause button's greyed for those who may not pause.
            if follows == Follows::Pause {
                button.insert(crate::clock::Pauses);
            }
        }
        button.with_children(|button| {
            for face in self.faces {
                // Placed within the button, as a share of its size.
                let share = |v: f32, of: f32| Val::Percent(100.0 * v / of.max(1.0));
                button.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: share(face.at.left - at.left, at.width()),
                        top: share(face.at.top - at.top, at.height()),
                        width: share(face.at.width().max(0.0), at.width()),
                        height: share(face.at.height().max(0.0), at.height()),
                        ..default()
                    },
                    ImageNode::new(face.image.clone()).with_mode(NodeImageMode::Stretch),
                    Visibility::Hidden,
                    Drawn {
                        when: face.when,
                        image: face.image,
                        greyed: face.greyed,
                    },
                    Pickable::IGNORE,
                ));
            }
        });
    }
}

/// Switches each button that stays on or off on while what it
/// [`Follows`] is.
pub fn select_toggles(
    pause: Res<crate::clock::PauseState>,
    border: Res<crate::supply::Border>,
    chat: Res<crate::chat::Chat>,
    politics: Res<crate::politics::Politics>,
    mut buttons: Query<(&Follows, &mut Pressable)>,
) {
    for (follows, mut button) in &mut buttons {
        let on = match follows {
            Follows::Pause => pause.paused,
            Follows::Border => border.0,
            Follows::Chat => chat.is_open(),
            Follows::Politics => politics.open,
        };
        if button.selected != on {
            button.selected = on;
        }
    }
}

/// Shows each [`Pressable`]'s pictures for the states it's in: switched on
/// or not, hovered, held down, and disabled where it can't be pressed,
/// greyed as the original greys a command it can't give.
#[allow(clippy::type_complexity)]
pub fn show_pressables(
    buttons: Query<(
        &Pressable,
        &Hovered,
        Has<bevy::ui::Pressed>,
        Has<bevy::ui::InteractionDisabled>,
        &Children,
    )>,
    mut faces: Query<(&Drawn, &mut Visibility, &mut ImageNode)>,
) {
    for (button, hovered, pressed, disabled, children) in &buttons {
        let holding = holding(button.selected, hovered.get(), pressed, disabled);
        for &child in children {
            let Ok((drawn, mut shown, mut image)) = faces.get_mut(child) else {
                continue;
            };
            let all = drawn.when.iter().all(|e| e.shown(&holding));
            shown.set_if_neq(crate::ui::visible(all));
            let picture = match (disabled, &drawn.greyed) {
                (true, Some(greyed)) => greyed,
                _ => &drawn.image,
            };
            if image.image != *picture {
                image.image = picture.clone();
            }
        }
    }
}

/// The states a button's held in, as its layout names them.
fn holding(selected: bool, hovered: bool, pressed: bool, disabled: bool) -> [&'static str; 6] {
    let usable = !disabled;
    [
        if usable { "ENABLED" } else { "DISABLED" },
        if hovered && usable {
            "FOCUSED"
        } else {
            "UNFOCUSED"
        },
        if pressed && usable {
            "PRESSED"
        } else {
            "UNPRESSED"
        },
        if selected { "SELECTED" } else { "UNSELECTED" },
        "UNTARGETED",
        "CLOSED",
    ]
}

/// The grid's buttons: `button`'s layout repeated in each cell of
/// `area`, centred, as the interface spec's command grid lays them out.
fn grid_buttons(area: Rect, button: &Widget, pictures: &mut Pictures) -> Vec<Built> {
    let [columns, rows] = [COLUMNS, ROWS];
    let cell = [
        (area.width() / columns as f32).floor(),
        (area.height() / rows as f32).floor(),
    ];
    let origin = Rect {
        left: 0.0,
        top: 0.0,
        right: cell[0],
        bottom: cell[1],
    };
    let size = button.bounds(origin);
    let gap = [
        ((cell[0] - size.width()) / 2.0).floor(),
        ((cell[1] - size.height()) / 2.0).floor(),
    ];
    let frame: Vec<(ViewElement, Handle<Image>)> = (button.view.iter())
        .flat_map(|v| &v.children)
        .filter_map(|e| Some((e.clone(), pictures.get(e.texture.as_deref()?)?)))
        .collect();
    let inset = |prefix: &str, side: &str, of: f32| {
        let key = format!("{prefix}{side}");
        button
            .value(&key)
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0)
            / of
    };
    let insets = |prefix: &str| {
        [
            inset(prefix, "L", size.width()),
            inset(prefix, "T", size.height()),
            inset(prefix, "R", size.width()),
            inset(prefix, "B", size.height()),
        ]
    };
    // Only buttons that draw their command's icon have a place for it.
    let has_icon = button
        .parts
        .iter()
        .any(|p| p.name.eq_ignore_ascii_case(GRID_ICON));
    (0..columns * rows)
        .filter(|_| has_icon)
        .map(|slot| {
            let left = area.left + (slot % columns) as f32 * cell[0] + gap[0];
            let top = area.top + (slot / columns) as f32 * cell[1] + gap[1];
            Built {
                slot,
                at: Rect {
                    left,
                    top,
                    right: left + size.width(),
                    bottom: top + size.height(),
                },
                frame: frame.clone(),
                icon: GridIcon {
                    inset: insets(ICON_INSET),
                    pressed: insets(ICON_INSET_PRESSED),
                },
            }
        })
        .collect()
}

/// A mouse button going down on a grid button, which holds it down while
/// it stays down.
fn hold(press: On<PointerPress>, mut buttons: Query<&mut GridButton>) {
    let held = match press.button {
        PointerButton::Primary => MouseButton::Left,
        PointerButton::Secondary => MouseButton::Right,
        PointerButton::Middle => return,
    };
    if let Ok(mut button) = buttons.get_mut(press.entity)
        && !button.0.contains(&held)
    {
        button.0.push(held);
    }
}

/// Greyed copies of the grid's icons, by the icon's.
#[derive(Resource, Default)]
pub struct Greyed(HashMap<AssetId<Image>, Handle<Image>>);

/// Makes the [`Greyed`] icons the first time the interface is put up in a
/// match, from their files, as the icons themselves keep their pixels only
/// until they're first drawn.
fn grey_icons(
    mut commands: Commands,
    greyed: Option<Res<Greyed>>,
    loaded: Res<Loaded>,
    looks: Res<crate::actions::Looks>,
    formats: Res<CompressedImageFormatSupport>,
    mut images: ResMut<Assets<Image>>,
) {
    if greyed.is_some() {
        return;
    }
    let game = &loaded.game;
    let mut greyed = Greyed::default();
    for icon in looks.icons() {
        let Some(path) = looks.source(icon) else {
            continue;
        };
        let decoded = crate::ui::read_image(game, path, formats.0);
        if let Some(grey) = decoded.as_ref().and_then(grey) {
            greyed.0.insert(icon.id(), images.add(grey));
        }
    }
    commands.insert_resource(greyed);
}

/// Shows each grid button as its slot holds, in the states the mouse
/// puts it in.
#[allow(clippy::type_complexity)]
pub fn show_grid(
    shown: Res<Shown>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut buttons: Query<(&mut GridButton, &Slot, &Hovered, &Children, &mut Visibility)>,
    mut frames: Query<(&Shows, &mut Visibility), Without<GridButton>>,
    mut icons: Query<(&GridIcon, &mut ImageNode, &mut Node), Without<Shows>>,
    greyed: Res<Greyed>,
) {
    for (mut button, slot, hovered, children, mut visibility) in &mut buttons {
        if button.0.iter().any(|&b| !mouse.pressed(b)) {
            button.0.retain(|&b| mouse.pressed(b));
        }
        let pressed = !button.0.is_empty();
        let face = shown.faces[slot.0].as_ref();
        // An empty slot has no button, only the panel's picture.
        visibility.set_if_neq(crate::ui::visible(face.is_some()));
        let Some(face) = face else {
            continue;
        };
        // A grid button is never disabled, nor anything but toggled off.
        let holding = holding(false, hovered.get(), pressed, false);
        let icon = match (&face.icon, face.ready) {
            (Some(icon), true) => Some(icon.clone()),
            (Some(icon), false) => greyed.0.get(&icon.id()).cloned(),
            (None, _) => None,
        };
        for &child in children {
            if let Ok((shows, mut shown)) = frames.get_mut(child) {
                shown.set_if_neq(crate::ui::visible(shows.0.shown(&holding)));
            }
            if let Ok((inset, mut image, mut node)) = icons.get_mut(child) {
                let [l, t, r, b] = match pressed {
                    true => inset.pressed,
                    false => inset.inset,
                };
                let at = Node {
                    left: Val::Percent(100.0 * l),
                    top: Val::Percent(100.0 * t),
                    width: Val::Percent(100.0 * (1.0 - l - r)),
                    height: Val::Percent(100.0 * (1.0 - t - b)),
                    ..node.clone()
                };
                let at = Node {
                    display: match icon {
                        Some(_) => Display::Flex,
                        None => Display::None,
                    },
                    ..at
                };
                node.set_if_neq(at);
                if let Some(handle) = &icon
                    && image.image != *handle
                {
                    image.image = handle.clone();
                }
            }
        }
    }
}

/// An icon as the original greys it: each pixel's colour mixed down to one
/// grey, darkened to under half.
fn grey(icon: &Image) -> Option<Image> {
    let mut out = crate::ui::rgba8(icon)?;
    for pixel in out.data.as_mut()?.as_chunks_mut::<4>().0 {
        let [r, g, b] = [pixel[0], pixel[1], pixel[2]].map(f32::from);
        #[allow(clippy::approx_constant, reason = "the original's weight, not 1/π")]
        let grey = (0.114 * r + 0.318 * g + 0.043 * b).round() as u8;
        pixel[..3].fill(grey);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use bevy::ui::InteractionDisabled;
    use tgdata::widgets::Bounds;

    use super::*;
    use crate::clock::{PauseState, Pauses, disable_buttons};

    #[test]
    fn leaving_takes_the_frame_down_but_keeps_the_log_and_the_whole_view() {
        use bevy::state::app::StatesPlugin;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(crate::Screen::Match)
            .insert_resource(Hud::Original)
            .add_systems(OnExit(DRAWN), leave);
        crate::hud::add_states(&mut app);
        app.update();
        let world = app.world_mut();
        let log = world.spawn(crate::messages::LogBox).id();
        let frame = world.spawn(Frame).add_child(log).id();
        let viewport = Some(Viewport::default());
        let camera = Camera {
            viewport,
            ..default()
        };
        let map = world
            .spawn((camera, tgrender::camera::RtsCamera::new(Vec3::ZERO)))
            .id();
        *world.resource_mut::<Hud>() = Hud::Modern;
        app.update();
        let world = app.world();
        assert!(world.get_entity(frame).is_err());
        assert!(world.get_entity(log).is_ok());
        assert!(world.get::<ChildOf>(log).is_none());
        assert!(world.get::<Camera>(map).unwrap().viewport.is_none());
    }

    #[test]
    fn the_overlay_draws_to_the_same_kind_of_texture_as_the_map() {
        let mut app = App::new();
        app.add_systems(Update, match_textures);
        let world = app.world_mut();
        let map = (Msaa::Off, Hdr, tgrender::camera::RtsCamera::new(Vec3::ZERO));
        let map = world.spawn(map).id();
        let overlay = world.spawn((Overlay, Msaa::Sample4)).id();
        app.update();
        let world = app.world();
        assert_eq!(world.get::<Msaa>(overlay), Some(&Msaa::Off));
        assert!(world.get::<Hdr>(overlay).is_some());
        app.world_mut().entity_mut(map).remove::<Hdr>();
        app.update();
        assert!(app.world().get::<Hdr>(overlay).is_none());
    }

    #[test]
    fn the_pause_button_follows_the_pause_and_who_may_pause() {
        let mut app = App::new();
        app.init_resource::<PauseState>()
            .init_resource::<crate::supply::Border>()
            .init_resource::<crate::chat::Chat>()
            .init_resource::<crate::politics::Politics>()
            .add_systems(Update, (disable_buttons, select_toggles));
        let button = (Pauses, Follows::Pause, Pressable::default());
        let button = app.world_mut().spawn(button).id();
        let set = |app: &mut App, paused, usable| {
            *app.world_mut().resource_mut::<PauseState>() = PauseState { paused, usable };
            app.update();
            let world = app.world();
            let selected = world.get::<Pressable>(button).unwrap().selected;
            (selected, world.get::<InteractionDisabled>(button).is_some())
        };
        assert_eq!(set(&mut app, true, true), (true, false));
        assert_eq!(set(&mut app, false, false), (false, true));
        assert_eq!(set(&mut app, true, true), (true, false));
    }

    #[test]
    fn the_supply_border_button_is_on_while_the_border_is_drawn() {
        let mut app = App::new();
        app.init_resource::<PauseState>()
            .insert_resource(crate::supply::Border(true))
            .init_resource::<crate::chat::Chat>()
            .init_resource::<crate::politics::Politics>()
            .add_systems(Update, select_toggles);
        let border = (Follows::Border, Pressable::default());
        let border = app.world_mut().spawn(border).id();
        let chat = (Follows::Chat, Pressable::default());
        let chat = app.world_mut().spawn(chat).id();
        let selected = |app: &mut App| {
            app.update();
            let on = |e| app.world().get::<Pressable>(e).unwrap().selected;
            (on(border), on(chat))
        };
        assert_eq!(selected(&mut app), (true, false));
        app.world_mut().resource_mut::<crate::supply::Border>().0 = false;
        assert_eq!(selected(&mut app), (false, false));
    }

    #[test]
    fn each_control_panel_button_asks_for_what_its_key_does() {
        let asks = |widget: &str| {
            let press = PRESSABLE.iter().find(|p| p.widget == widget);
            press.map(|p| p.asks)
        };
        // As the hotkey files name what each key gives.
        assert_eq!(asks("ButtonMenu"), Some("ButtonMenu"));
        assert_eq!(asks("ButtonSupplyBorder"), Some("ButtonSupplyBorder"));
        assert_eq!(asks("ButtonChat"), Some("ChatActivate"));
        assert_eq!(asks("ButtonSpeedUp"), Some("SpeedUp"));
        assert_eq!(asks("ButtonSpeedDown"), Some("SpeedDown"));
    }

    #[test]
    fn what_is_drawn_over_the_view_scales_with_its_height() {
        assert_eq!(layout_scale(Vec2::new(1024.0, 768.0)), 1.0);
        assert_eq!(layout_scale(Vec2::new(1280.0, 720.0)), 720.0 / 768.0);
        // Too narrow for the window's height, the view's width decides.
        assert_eq!(layout_scale(Vec2::new(1280.0, 1024.0)), 1.25);
    }

    #[test]
    fn the_view_fits_the_layouts_shape_whichever_way_the_window_is_out() {
        let fit = |w, h| fit(UVec2::new(w, h));
        assert_eq!(fit(1280, 720), (UVec2::new(160, 0), UVec2::new(960, 720)));
        assert_eq!(fit(1280, 1024), (UVec2::new(0, 32), UVec2::new(1280, 960)));
        assert_eq!(fit(1024, 768), (UVec2::ZERO, UVec2::new(1024, 768)));
    }

    fn element(when: &[&str]) -> ViewElement {
        ViewElement {
            name: "Child".into(),
            kind: None,
            bounds: Bounds::default(),
            when: when.iter().map(|w| w.to_string()).collect(),
            texture: None,
            colour: [1.0; 3],
            text: None,
            font: None,
            justify: None,
            tiles: None,
            fields: Vec::new(),
            children: Vec::new(),
        }
    }

    #[test]
    fn a_face_shows_while_it_and_all_it_is_in_hold_greyed_while_disabled() {
        let mut app = App::new();
        app.add_systems(Update, show_pressables);
        let (image, greyed) = (
            Handle::<Image>::default(),
            Handle::Uuid(Default::default(), default()),
        );
        let face = app
            .world_mut()
            .spawn((
                Visibility::Hidden,
                ImageNode::new(image.clone()),
                Drawn {
                    when: vec![element(&["SELECTED"]), element(&["UNFOCUSED"])],
                    image: image.clone(),
                    greyed: Some(greyed.clone()),
                },
            ))
            .id();
        let button = app
            .world_mut()
            .spawn((Pressable::default(), Hovered(false)))
            .add_child(face)
            .id();
        let look = |app: &mut App| {
            app.update();
            let world = app.world();
            let shown = *world.get::<Visibility>(face).unwrap() == Visibility::Inherited;
            (shown, world.get::<ImageNode>(face).unwrap().image == greyed)
        };
        assert_eq!(look(&mut app), (false, false), "not selected");
        app.world_mut()
            .get_mut::<Pressable>(button)
            .unwrap()
            .selected = true;
        assert_eq!(look(&mut app), (true, false));
        app.world_mut().entity_mut(button).insert(Hovered(true));
        assert_eq!(look(&mut app), (false, false), "its own state fails");
        let mut button = app.world_mut().entity_mut(button);
        button.insert((Hovered(false), bevy::ui::InteractionDisabled));
        assert_eq!(look(&mut app), (true, true));
    }

    #[test]
    fn a_disabled_button_is_never_focused_nor_pressed_but_may_be_selected() {
        let held = holding(true, true, true, false);
        assert_eq!(held[..4], ["ENABLED", "FOCUSED", "PRESSED", "SELECTED"]);
        let held = holding(true, true, true, true);
        assert_eq!(
            held[..4],
            ["DISABLED", "UNFOCUSED", "UNPRESSED", "SELECTED"]
        );
        assert_eq!(holding(false, false, false, false), RESTING);
    }
}
