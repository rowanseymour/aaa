//! Supply as the map shows it: a border along the edge of the ground the
//! player's side is supplied over, and a tint on buildings going without.
//! A button at the minimap's corner, or its key, shows and hides the
//! border, as the options do. Another over the minimap, or its key, steps
//! through the player's regiments that have no HQ.

use aaa_render::camera::RtsCamera;
use aaa_render::terrain::world;
use aaa_render::{Tint, overlay, textures};
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::light::NotShadowCaster;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{Activate, Button};

use crate::actions::Blocks;
use crate::effects::Effects;
use crate::keys::{self, Bindings};
use crate::sound::Sounds;
use crate::ui::{Click, Edge, Hint, Marked, Surface, Theme};
use crate::{BuildingView, Loaded, Local, Screen, Selected, SelectedBuilding, Sim, minimap};

/// The tint a building out of supply is drawn in.
const OUT_OF_SUPPLY: &str = "out_of_supply_vfx";

/// Whether the supply border is drawn.
#[derive(Resource)]
pub struct Border(pub bool);

impl Default for Border {
    fn default() -> Self {
        Border(true)
    }
}

/// The paths the border follows, for the minimap to draw too; empty while
/// the border's hidden.
#[derive(Resource, Default)]
pub struct Outline(pub Vec<Vec<[f32; 2]>>);

/// How the border looks, from the game's settings.
#[derive(Resource)]
pub struct BorderLook {
    pub colour: Color,
    /// What the button that shows and hides it is called.
    name: String,
    width: f32,
    /// The side of the squares the map is cut into, in world units.
    block: f32,
    material: Handle<StandardMaterial>,
}

/// The border as drawn.
#[derive(Component)]
pub struct BorderLine;

/// The outline the border follows, of the zones of these buildings, by
/// index into the sim's, each with where its zone is laid and how far it
/// reaches; kept while the border's hidden, as where its loops start
/// follows from how the zones came to be.
#[derive(Default)]
pub struct Traced {
    giving: Vec<(usize, (i32, i32), f32)>,
    outline: aaa_map::Outline,
}

/// Draws the border round the zones of the player's side and its allies,
/// again whenever one comes, goes, moves or changes its reach.
#[allow(clippy::too_many_arguments)]
pub fn show_border(
    mut commands: Commands,
    border: Res<Border>,
    sim: Res<Sim>,
    local: Res<Local>,
    loaded: Res<Loaded>,
    lines: Query<Entity, With<BorderLine>>,
    look: Res<BorderLook>,
    mut traced: bevy::prelude::Local<Traced>,
    mut outline: ResMut<Outline>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if sim.is_added() {
        *traced = Traced::default();
    }
    let sim = &sim.0;
    let zone = |b: usize| sim.buildings[b].supply.zone.as_ref();
    let laid = |b: usize| {
        let (around, reach) = sim.buildings[b].supply.laid()?;
        Some((b, around, reach))
    };
    let giving: Vec<(usize, (i32, i32), f32)> = match local.0 {
        Some(player) => (0..sim.buildings.len())
            .filter(|&b| sim.shares_supply(player, sim.buildings[b].owner))
            .filter_map(laid)
            .collect(),
        None => Vec::new(),
    };
    let moved = traced.giving != giving;
    if moved {
        let zones = giving.iter().filter_map(|&(b, ..)| zone(b));
        let block = (look.block / tgck::map::CELL_SIZE).round() as i32;
        traced.outline.retrace(zones, block);
        traced.giving = giving;
    }
    let shown = border.0 && !traced.giving.is_empty();
    if !moved && shown != lines.is_empty() {
        return;
    }
    lines.iter().for_each(|e| commands.entity(e).despawn());
    if !shown {
        outline.0.clear();
        return;
    }
    let edges = traced.outline.loops();
    let mesh = overlay::border(&loaded.map.ground, &edges, look.width);
    outline.0 = overlay::border_paths(&edges);
    commands.spawn((
        BorderLine,
        NotShadowCaster,
        Pickable::IGNORE,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(look.material.clone()),
        DespawnOnExit(Screen::Match),
    ));
}

/// On a building out of supply: when it went out, in seconds of the wall
/// clock, which its tint's pulse runs on.
#[derive(Component)]
pub struct OutOfSupply(f64);

/// The steps a tint's strength moves in, so that a pulsing one recolours
/// its materials a few times a second rather than every frame.
const TINT_STEPS: f32 = 16.0;

/// Tints the buildings that are out of supply, pulsing for as long as they
/// are and plain again at once when they're not.
pub fn tint_buildings(
    mut commands: Commands,
    sim: Res<Sim>,
    effects: Option<Res<Effects>>,
    time: Res<Time<Real>>,
    views: Query<(Entity, &BuildingView, Option<&OutOfSupply>, Option<&Tint>)>,
) {
    let Some(pulse) = effects.and_then(|e| e.tint(OUT_OF_SUPPLY)) else {
        return;
    };
    let now = time.elapsed_secs_f64();
    for (entity, view, since, tint) in &views {
        let b = view.building;
        let out = sim.0.out_of_supply(b) && !sim.0.buildings[b].gone();
        let since = match (out, since) {
            (true, Some(&OutOfSupply(since))) => since,
            (true, None) => {
                commands.entity(entity).insert(OutOfSupply(now));
                now
            }
            (false, Some(_)) => {
                commands.entity(entity).remove::<(OutOfSupply, Tint)>();
                continue;
            }
            (false, None) => continue,
        };
        let level = (pulse.level((now - since) as f32) * TINT_STEPS).round() / TINT_STEPS;
        let colour = Tint(pulse.colour(level));
        if tint != Some(&colour) {
            commands.entity(entity).insert(colour);
        }
    }
}

/// The button over the minimap that steps through the player's detached
/// regiments, as F5 does; there only while there are some.
#[derive(Component)]
pub struct CycleDetached;

const CYCLE_ICON: &str = "UI/Game/Floaters/ButtonDetachedRegimentUnpressed.tga";
const CYCLE_HELP: &str = "idle_buttons_IdleButton_tooltip_2";
const CYCLE_SIZE: f32 = 42.0;

/// The button at the minimap's corner that shows and hides the border:
/// a ring in the border's colour, lit while it's shown.
#[derive(Component)]
pub struct BorderButton {
    /// Whether it last showed the border as shown.
    shown: bool,
    /// Whether it was pressed since, which has sounded already.
    pressed: bool,
}

/// The ring in the [`BorderButton`].
#[derive(Component)]
pub struct BorderRing;

pub const BORDER_SIZE: f32 = 34.0;
const RING_SIZE: f32 = 18.0;
const BORDER_NAME: &str = "control_panel_ButtonSupplyBorder_ToolTip";
const BORDER_HELP: &str = "control_panel_ButtonSupplyBorder_tooltip_2";

/// Loads how the border looks, and spawns the buttons that show and hide
/// it and that cycle through detached regiments.
pub fn spawn(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    formats: Res<CompressedImageFormatSupport>,
    bindings: Res<Bindings>,
    border: Res<Border>,
) {
    let shown = border.0;
    let game = &mut loaded.game;
    let border = game.supply_border();
    let [r, g, b] = border.colour;
    let colour = Color::srgb(r, g, b);
    let image = (border.texture.as_deref())
        .and_then(|path| overlay::border_image(game, path, formats.0))
        .map(|i| images.add(i));
    let material = materials.add(overlay::mark_material(image, colour));
    let icon = (game.read(CYCLE_ICON))
        .and_then(|bytes| textures::decode(&bytes, CYCLE_ICON, formats.0, true))
        .map(|i| images.add(i));
    let strings = game.strings();
    let said = |key: &str, or: &str| strings.get(key).map_or(or, String::as_str).to_string();
    let name = said(BORDER_NAME, "Toggle Supply Border");
    let what = said(BORDER_HELP, "Toggles the supply border off/on.");
    commands
        .spawn((
            BorderButton {
                shown,
                pressed: false,
            },
            Blocks,
            Button,
            Click::TOGGLE,
            RelativeCursorPosition::default(),
            bindings.marked(keys::SUPPLY_BORDER, &name),
            Hint(what),
            // In the corner the minimap's disc leaves of its square, where
            // the game's control panel has it.
            Node {
                position_type: PositionType::Absolute,
                left: px(crate::MARGIN),
                bottom: px(crate::MARGIN + minimap::DISC - BORDER_SIZE),
                width: px(BORDER_SIZE),
                height: px(BORDER_SIZE),
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
                BorderRing,
                Node {
                    width: px(RING_SIZE),
                    height: px(RING_SIZE),
                    border: UiRect::all(px(3)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor::all(colour),
                Pickable::IGNORE,
            )],
        ))
        .observe(
            |activate: On<Activate>,
             mut border: ResMut<Border>,
             mut buttons: Query<&mut BorderButton>| {
                border.0 = !border.0;
                if let Ok(mut button) = buttons.get_mut(activate.entity) {
                    button.pressed = true;
                }
            },
        );
    commands.insert_resource(BorderLook {
        colour,
        name,
        width: border.width,
        block: border.block,
        material,
    });
    let help = strings.get(CYCLE_HELP).cloned();
    commands
        .spawn((
            CycleDetached,
            Blocks,
            Button,
            RelativeCursorPosition::default(),
            Hint(help.unwrap_or_else(|| "Cycles through all detached regiments.".into())),
            ImageNode::new(icon.unwrap_or_default()),
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                left: px(crate::MARGIN),
                bottom: px(crate::MARGIN + minimap::DISC + crate::MARGIN),
                width: px(CYCLE_SIZE),
                height: px(CYCLE_SIZE),
                ..default()
            },
            GlobalZIndex(1),
            DespawnOnExit(Screen::Match),
        ))
        .observe(|_: On<Activate>, mut cycle: CycleParts| {
            cycle.step();
        });
}

/// The border button's name with its key marked.
/// Marks the border button's key afresh once the hotkeys change.
pub fn mark_border(
    bindings: Res<Bindings>,
    look: Res<BorderLook>,
    mut buttons: Query<&mut Marked, With<BorderButton>>,
) {
    for mut button in &mut buttons {
        button.set_if_neq(bindings.marked(keys::SUPPLY_BORDER, &look.name));
    }
}

/// What stepping through the detached regiments reads and changes.
#[derive(SystemParam)]
pub struct CycleParts<'w, 's> {
    sim: Res<'w, Sim>,
    local: Res<'w, Local>,
    loaded: Res<'w, Loaded>,
    selected: ResMut<'w, Selected>,
    chosen: ResMut<'w, SelectedBuilding>,
    scenery: Res<'w, crate::banners::Scenery>,
    cameras: Query<'w, 's, &'static mut RtsCamera>,
}

impl CycleParts<'_, '_> {
    /// Selects the player's next detached regiment after the one selected,
    /// round to the first again, and looks at it.
    fn step(&mut self) {
        let Some(player) = self.local.0 else {
            return;
        };
        // Those that can't be selected are passed over.
        let mut detached = self.sim.0.detached(player);
        detached.retain(|&r| !self.scenery.has(&self.sim.0, r));
        let after = self.selected.0.first().map_or(0, |&now| now + 1);
        let next = detached.iter().find(|&&r| r >= after).or(detached.first());
        let Some(&r) = next.filter(|&&r| self.selected.0 != [r]) else {
            return;
        };
        self.selected.0 = vec![r];
        self.chosen.0 = None;
        let [x, y] = self.sim.0.leader_position(r);
        for mut camera in &mut self.cameras {
            camera.focus = world(x, y, self.loaded.map.ground.height_at(x, y));
        }
    }
}

/// Steps through the detached regiments on the key for it.
pub fn cycle_detached(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    mut cycle: CycleParts,
) {
    if bindings.pressed(&keys, keys::CYCLE_DETACHED) {
        cycle.step();
    }
}

/// Shows or hides the border on the key for it.
pub fn toggle_border(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    mut border: ResMut<Border>,
) {
    if bindings.pressed(&keys, keys::SUPPLY_BORDER) {
        border.0 = !border.0;
    }
}

/// Lights the border's button while the border's shown, and greys its ring
/// while it's hidden. Shown by anything but a press of the button, such as
/// its key, the button sounds as the game's toggle buttons do when set.
pub fn show_border_button(
    border: Res<Border>,
    look: Res<BorderLook>,
    theme: Res<Theme>,
    time: Res<Time>,
    mut sounds: ResMut<Sounds>,
    mut buttons: Query<(&mut BorderButton, Ref<Hovered>, &mut Edge)>,
    mut rings: Query<&mut BorderColor, With<BorderRing>>,
) {
    // The look is inserted afresh with each match's button.
    let restyle = border.is_changed() || theme.is_changed() || look.is_changed();
    for (mut button, hovered, mut edge) in &mut buttons {
        if button.shown != border.0 {
            if border.0 && !button.pressed {
                Click::TOGGLE.play(&mut sounds, &time);
            }
            button.shown = border.0;
        }
        if button.pressed {
            button.pressed = false;
        }
        if restyle || hovered.is_changed() {
            edge.set_if_neq(match (border.0, hovered.get()) {
                (true, _) => Edge::Lit,
                (false, true) => Edge::Hovered,
                (false, false) => Edge::Border,
            });
        }
    }
    if !restyle {
        return;
    }
    let ring = match border.0 {
        true => look.colour,
        false => theme.edge(Edge::Border),
    };
    for mut colour in &mut rings {
        colour.set_if_neq(BorderColor::all(ring));
    }
}

/// Shows the button that cycles through detached regiments while there's
/// one that an HQ of the player's could take, places aside, though the
/// cycle takes in those none could.
pub fn show_cycle_button(
    sim: Res<Sim>,
    local: Res<Local>,
    mut button: Single<&mut Node, With<CycleDetached>>,
) {
    if !sim.is_changed() && !local.is_changed() {
        return;
    }
    let any = local.0.is_some_and(|p| sim.0.attachable(p));
    let display = match any {
        true => Display::Flex,
        false => Display::None,
    };
    if button.display != display {
        button.display = display;
    }
}
