//! The original interface's in-match panels, drawn from its layouts (see
//! [`tgdata::widgets`]) in place of ours: their pictures as they look at
//! rest.

use std::collections::HashMap;

use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use tgdata::widgets::{Bounds, InterfaceFlags, RESTING, Rect, SCREEN, ViewElement, Widget};
use tgrender::textures;

use crate::actions::Blocks;
use crate::{Loaded, Local, Screen, Sim};

/// Which interface a match is played with.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Hud {
    /// This project's own.
    #[default]
    Ours,
    /// The original's, from its layouts.
    Original,
}

/// The parts of our interface the original's takes the place of.
#[derive(Component)]
pub struct Ours;

/// Whether the match is played with the original's interface.
pub fn original(hud: Res<Hud>) -> bool {
    *hud == Hud::Original
}

/// The panel the parts of the screen's lower edge are placed within.
const CONTROL_PANEL: &str = "ControlPanel";

/// The panels placed within the control panel, in the order they're drawn:
/// those before its own buttons, and those after.
const UNDER_BUTTONS: [&str; 2] = [OBJECT_DISPLAY, "ActionButtons"];

/// The panel showing what's selected: with nothing selected, only its own
/// pictures, as its parts show the selection.
const OBJECT_DISPLAY: &str = "ObjectDisplay";
const OVER_BUTTONS: [&str; 1] = ["GameMiniMap"];

/// The match layer's own part that's on screen from the start, and the
/// panels placed against the whole screen that are, with the flag each
/// needs, if any.
const ON_GAME: &str = "ControlPanelTop";
const GAME: &str = "Game";
type Flag = fn(&InterfaceFlags) -> bool;
const ON_SCREEN: [(&str, Option<Flag>); 5] = [
    ("EconomyBar", None),
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

/// The widgets the interface repeats once for each entry a panel lists,
/// such as a group or a command: templates, not shown themselves, and
/// with nothing listed at rest.
const ENTRIES: [&str; 3] = ["Item", "GroupButton", "ActionButtonsButton"];

/// One step from the screen to a picture: into a child's bounds, or moved
/// by an offset.
#[derive(Clone, Copy)]
enum Step {
    Into(Bounds),
    By([f32; 2]),
}

/// Takes away the parts of our interface the original's replaces. They're
/// spawned all the same, as what they set up besides themselves is needed
/// either way.
pub fn remove_ours(mut commands: Commands, ours: Query<Entity, Added<Ours>>) {
    for part in &ours {
        commands.entity(part).despawn();
    }
}

/// Draws the original's panels, in the skin of the player's nation.
pub fn spawn(
    mut commands: Commands,
    loaded: Res<Loaded>,
    sim: Res<Sim>,
    local: Res<Local>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &loaded.game;
    let nation = (local.0)
        .and_then(|p| sim.0.sides.player(p))
        .and_then(|p| p.nation.clone());
    let skin = nation.and_then(|id| {
        let nations = game.nations();
        let nation = nations
            .into_iter()
            .find(|n| n.id == tgdata::Id::from(id.as_str()))?;
        game.skin(&nation.skin)
    });
    let widgets = game.game_widgets();
    let named = |name: &str| (widgets.iter()).find(|w| w.name.eq_ignore_ascii_case(name));
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
    };
    if let Some(top) = named(GAME).and_then(|g| g.find(ON_GAME)) {
        drawer.widget(top, &[]);
    }
    if let Some(panel) = named(CONTROL_PANEL)
        && let Some(chain) = drawer.own(panel, &[])
    {
        for name in UNDER_BUTTONS {
            match named(name) {
                Some(w) if name == OBJECT_DISPLAY => _ = drawer.own(w, &chain),
                Some(w) => drawer.widget(w, &chain),
                None => {}
            }
        }
        for child in &panel.children {
            drawer.widget(child, &chain);
        }
        for name in OVER_BUTTONS {
            named(name).inspect(|w| drawer.widget(w, &chain));
        }
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
        drawer.widget(widget, &Vec::from_iter(offset));
    }
    let drawn = drawer.drawn;
    // The layouts' screen at its own shape, as tall as the window and
    // centred in it: its pictures only line up at that shape, and the
    // original stretches it over a wider window instead.
    let screen = commands
        .spawn((
            Node {
                height: Val::Percent(100.0),
                aspect_ratio: Some(SCREEN[0] / SCREEN[1]),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|screen| {
            for (chain, image) in drawn {
                screen.spawn((
                    placed(&chain),
                    ImageNode::new(image).with_mode(NodeImageMode::Stretch),
                    // Clicks on the panels aren't the map's.
                    Blocks,
                    RelativeCursorPosition::default(),
                ));
            }
        })
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            DespawnOnExit(Screen::Match),
        ))
        .add_child(screen);
}

/// A picture's node, given the steps from the screen to it.
fn placed(chain: &[Step]) -> Node {
    let screen = Rect {
        left: 0.0,
        top: 0.0,
        right: SCREEN[0],
        bottom: SCREEN[1],
    };
    let at = chain.iter().fold(screen, |within, step| match *step {
        Step::Into(b) => b.place(within),
        Step::By([x, y]) => Rect {
            left: within.left + x,
            top: within.top + y,
            right: within.right + x,
            bottom: within.bottom + y,
        },
    });
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

/// The pictures read so far, by archive path.
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
