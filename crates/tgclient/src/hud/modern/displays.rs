//! Our buttons on the minimap's rim that open the display panels.

use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::Button;

use super::minimap;
use crate::Loaded;
use crate::displays::{Displays, Icons, PANELS, Panel, icon};
use crate::intent::Asks;
use crate::keys::Bindings;
use crate::ui::Blocks;
use crate::ui::{self, Click, Edge, Hint, Ink, Marked, Surface, Theme};

/// A glyph's size on its button.
pub const GLYPH_SIZE: f32 = 22.0;

/// The places on the minimap's rim its round buttons sit, between the
/// diamond's corners.
#[derive(Clone, Copy)]
pub enum Rim {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Rim {
    /// Where a round button sits there.
    pub fn node(self) -> Node {
        let centre = crate::ui_scale::MARGIN + minimap::DISC / 2.0 - minimap::RIM_BUTTON / 2.0;
        let near = px(centre - minimap::BOLT);
        let far = px(centre + minimap::BOLT);
        let (left, bottom) = match self {
            Rim::TopLeft => (near, far),
            Rim::TopRight => (far, far),
            Rim::BottomLeft => (near, near),
            Rim::BottomRight => (far, near),
        };
        Node {
            position_type: PositionType::Absolute,
            left,
            bottom,
            width: px(minimap::RIM_BUTTON),
            height: px(minimap::RIM_BUTTON),
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
pub struct PanelButton {
    panel: Panel,
    name: String,
}

/// The glyph on a [`PanelButton`].
#[derive(Component)]
pub struct Glyph;

/// Spawns our interface's buttons on the minimap's rim that open the
/// panels.
pub fn spawn_buttons(
    mut commands: Commands,
    loaded: Res<Loaded>,
    bindings: Res<Bindings>,
    mut icons: ResMut<Icons>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let strings = loaded.game.strings();
    let said = ui::said(&strings);
    for panel in PANELS {
        let (rim, icon, [name, name_or], [help, help_or]) = match panel {
            Panel::Objectives => (
                Rim::BottomLeft,
                icon!("objectives"),
                [
                    "control_panel_ButtonObjectives_ToolTip",
                    "Display Objectives",
                ],
                [
                    "control_panel_ButtonObjectives_tooltip_2",
                    "Displays a list of mission objectives.",
                ],
            ),
            Panel::Technologies => (
                Rim::TopRight,
                icon!("technologies"),
                [
                    "control_panel_ButtonTechnology_ToolTip",
                    "Display Technologies",
                ],
                [
                    "control_panel_ButtonTechnology_tooltip_2",
                    "Displays a list of all your technologies.",
                ],
            ),
            Panel::MessageLog => (
                Rim::BottomRight,
                icon!("message-log"),
                [
                    "control_panel_ButtonMessageLog_ToolTip",
                    "Display Message Log",
                ],
                [
                    "control_panel_ButtonMessageLog_tooltip_2",
                    "Displays a log of all recent messages.",
                ],
            ),
        };
        let glyph = icons.get(icon, &mut images, formats.0);
        spawn_button(
            &mut commands,
            &bindings,
            panel,
            rim,
            glyph,
            said(name, name_or),
            said(help, help_or),
        );
    }
}

/// Spawns the button at `rim` that opens `panel`, drawn with `glyph`.
fn spawn_button(
    commands: &mut Commands,
    bindings: &Bindings,
    panel: Panel,
    rim: Rim,
    glyph: Handle<Image>,
    name: String,
    help: String,
) {
    commands.spawn((
        Blocks,
        Button,
        Click::PUSH,
        RelativeCursorPosition::default(),
        bindings.marked(panel.keys().0, &name),
        (Asks::new(panel.keys().0), Hint(help)),
        ui::called(match panel {
            Panel::Objectives => "panel.objectives",
            Panel::Technologies => "panel.technologies",
            Panel::MessageLog => "panel.messages",
        }),
        PanelButton { panel, name },
        rim.node(),
        Surface::Clear,
        Edge::Border,
        GlobalZIndex(crate::ui::layer::HUD_TOP),
        DespawnOnExit(super::DRAWN),
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
    ));
}

/// Marks the buttons' keys afresh once the hotkeys change.
pub fn mark_buttons(bindings: Res<Bindings>, mut buttons: Query<(&PanelButton, &mut Marked)>) {
    for (button, mut marked) in &mut buttons {
        marked.set_if_neq(bindings.marked(button.panel.keys().0, &button.name));
    }
}

/// What [`light_buttons`] reads and sets on a button.
type ButtonLook = (
    Ref<'static, PanelButton>,
    Ref<'static, Hovered>,
    Has<bevy::ui::Pressed>,
    &'static mut Edge,
    &'static mut Surface,
    &'static Children,
);

/// Lights a button while its panel's up, its edge while it's hovered, and
/// its face while it's held down.
pub fn light_buttons(
    displays: Res<Displays>,
    theme: Res<Theme>,
    mut buttons: Query<ButtonLook>,
    mut glyphs: Query<&mut ImageNode, With<Glyph>>,
) {
    let restyle = displays.is_changed() || theme.is_changed();
    for (button, hovered, pressed, mut edge, mut surface, children) in &mut buttons {
        let open = displays.is_open(button.panel);
        let restyle = restyle || button.is_added();
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
