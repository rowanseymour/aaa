//! Our buttons on the minimap's rim that open the display panels.

use bevy::ecs::query::QueryItem;
use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::Button;

use super::minimap;
use crate::Loaded;
use crate::displays::{Displays, Icons, PANELS, Panel, icon};
use crate::intent::Asks;
use crate::keys::{Bindings, Named};
use crate::ui::Blocks;
use crate::ui::{self, Click, Edge, Hint, Ink, Surface, Theme};

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

/// The button that opens a panel.
#[derive(Component)]
pub struct PanelButton(Panel);

/// The glyph on a [`rim_button`].
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
        let (rim, path, [name, name_or], [help, help_or]) = match panel {
            Panel::Objectives => (
                Rim::BottomLeft,
                icon("objectives"),
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
                icon("technologies"),
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
                Rim::TopLeft,
                icon("message-log"),
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
        let glyph = icons.get(&loaded.game, &path, &mut images, formats.0);
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
        rim_button(rim, glyph, Click::PUSH),
        Named::new(bindings, panel.keys().0, name),
        (Asks::new(panel.keys().0), Hint(help)),
        ui::called(panel.called()),
        PanelButton(panel),
    ));
}

/// A round button at `rim` drawn with `glyph`, sounding `click` when
/// pressed.
pub fn rim_button(rim: Rim, glyph: Handle<Image>, click: Click) -> impl Bundle {
    round_button(rim.node(), glyph, GLYPH_SIZE, click)
}

/// A round button laid out as `node`, drawn with `glyph` `size` across,
/// sounding `click` when pressed.
pub fn round_button(node: Node, glyph: Handle<Image>, size: f32, click: Click) -> impl Bundle {
    (
        Blocks,
        Button,
        click,
        RelativeCursorPosition::default(),
        node,
        Surface::Clear,
        Edge::Border,
        GlobalZIndex(crate::ui::layer::HUD_TOP),
        DespawnOnExit(super::DRAWN),
        children![(
            Glyph,
            ImageNode::new(glyph),
            Node {
                width: px(size),
                height: px(size),
                ..default()
            },
            Pickable::IGNORE,
        )],
    )
}

/// What [`light`] reads and sets on a [`rim_button`].
pub type RimLook = (
    Ref<'static, Hovered>,
    Has<bevy::ui::Pressed>,
    &'static mut Edge,
    &'static mut Surface,
    &'static Children,
);

/// How [`light`] shows a [`rim_button`].
pub struct Lighting {
    /// Its edge and glyph are lit, as while what it's for is on.
    pub lit: bool,
    /// It can be pressed; it's greyed if not.
    pub usable: bool,
    /// What lights it, or the theme, may have changed, so its glyph's
    /// recoloured.
    pub restyle: bool,
}

/// Lights a [`rim_button`] as `lighting` says, its edge while it's
/// hovered and its face while it's held down.
pub fn light(
    (hovered, pressed, mut edge, mut surface, children): QueryItem<RimLook>,
    Lighting {
        lit,
        usable,
        restyle,
    }: Lighting,
    theme: &Theme,
    glyphs: &mut Query<&mut ImageNode, With<Glyph>>,
) {
    surface.set_if_neq(match pressed && usable {
        true => Surface::Hovered,
        false => Surface::Clear,
    });
    if restyle || hovered.is_changed() {
        edge.set_if_neq(Edge::lit(lit, hovered.get() && usable, Edge::Border));
    }
    if !restyle {
        return;
    }
    let colour = match (lit, usable) {
        (true, _) => theme.edge(Edge::Lit),
        (false, true) => theme.ink(Ink::Help),
        (false, false) => theme.ink(Ink::Muted),
    };
    for &child in children {
        if let Ok(mut glyph) = glyphs.get_mut(child)
            && glyph.color != colour
        {
            glyph.color = colour;
        }
    }
}

/// Lights a button while its panel's up.
pub fn light_buttons(
    displays: Res<Displays>,
    theme: Res<Theme>,
    mut buttons: Query<(Ref<PanelButton>, RimLook)>,
    mut glyphs: Query<&mut ImageNode, With<Glyph>>,
) {
    let restyle = displays.is_changed() || theme.is_changed();
    for (button, look) in &mut buttons {
        let open = displays.is_open(button.0);
        let restyle = restyle || button.is_added();
        let lighting = Lighting {
            lit: open,
            usable: true,
            restyle,
        };
        light(look, lighting, &theme, &mut glyphs);
    }
}
