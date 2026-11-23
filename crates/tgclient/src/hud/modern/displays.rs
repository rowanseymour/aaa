//! Our buttons on the minimap's rim that open the display panels.

use bevy::ecs::query::QueryItem;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::Button;

use super::minimap;
use crate::Loaded;
use crate::displays::{Displays, Icon, PANELS, Panel, icon};
use crate::intent::Asks;
use crate::keys::{Bindings, Named};
use crate::ui::Blocks;
use crate::ui::{self, Click, Edge, Hint, Ink, Surface, Theme};

/// A glyph's size on its button.
pub const GLYPH_SIZE: f32 = 22.0;

/// The quarters of the minimap's rim, between the diamond's corners, that
/// hold buttons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quadrant {
    TopLeft,
    BottomLeft,
    BottomRight,
}

impl Quadrant {
    /// Its first spot for a round button, the clockwise one of its pair.
    pub const fn first(self) -> Rim {
        Rim {
            quadrant: self,
            second: false,
        }
    }

    /// Its second spot for a round button, anticlockwise of the first.
    pub const fn second(self) -> Rim {
        Rim {
            quadrant: self,
            second: true,
        }
    }

    /// The angle of its middle, anticlockwise from the disc's right.
    fn middle(self) -> f32 {
        let degrees: f32 = match self {
            Quadrant::TopLeft => 135.0,
            Quadrant::BottomLeft => 225.0,
            Quadrant::BottomRight => 315.0,
        };
        degrees.to_radians()
    }
}

/// Where on the minimap's rim a round button sits: one of the pair of
/// spots either side of a quadrant's middle.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rim {
    quadrant: Quadrant,
    second: bool,
}

/// The angle between the two buttons in a quadrant, a little more than
/// their width at the rim.
const RIM_STEP: f32 = 20.0;

impl Rim {
    /// Its centre from the disc's, y up.
    pub fn centre(self) -> Vec2 {
        let side = if self.second { 0.5 } else { -0.5 };
        let angle = self.quadrant.middle() + side * RIM_STEP.to_radians();
        Vec2::from_angle(angle) * minimap::DISC / 2.0
    }

    /// A round button's node, placed here.
    fn node(self) -> Node {
        let corner = crate::ui_scale::MARGIN + minimap::DISC / 2.0 - minimap::RIM_BUTTON / 2.0;
        let at = corner + self.centre();
        Node {
            position_type: PositionType::Absolute,
            left: px(at.x),
            bottom: px(at.y),
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
pub fn spawn_buttons(mut commands: Commands, loaded: Res<Loaded>, bindings: Res<Bindings>) {
    let strings = loaded.game.strings();
    let said = ui::said(&strings);
    for panel in PANELS {
        let (rim, path, [name, name_or], [help, help_or]) = match panel {
            Panel::Objectives => (
                Quadrant::BottomLeft.first(),
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
                Quadrant::BottomRight.second(),
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
                Quadrant::TopLeft.second(),
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
            Panel::Politics => (
                Quadrant::BottomLeft.second(),
                icon("politics"),
                ["control_panel_ButtonPolitics_ToolTip", "Display Politics"],
                [
                    "control_panel_ButtonPolitics_tooltip_2",
                    "Shows all sides and their relation to your side.",
                ],
            ),
        };
        spawn_button(
            &mut commands,
            &bindings,
            panel,
            rim,
            path,
            said(name, name_or),
            said(help, help_or),
        );
    }
}

/// Spawns the button at `rim` that opens `panel`, drawn with the icon at
/// `glyph`.
fn spawn_button(
    commands: &mut Commands,
    bindings: &Bindings,
    panel: Panel,
    rim: Rim,
    glyph: String,
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

/// A round button at `rim` drawn with the icon at `glyph`, sounding
/// `click` when pressed.
pub fn rim_button(rim: Rim, glyph: String, click: Click) -> impl Bundle {
    (rim, round_button(rim.node(), glyph, GLYPH_SIZE, click))
}

/// A round button laid out as `node`, drawn with the icon at `glyph`
/// `size` across, sounding `click` when pressed.
pub fn round_button(node: Node, glyph: String, size: f32, click: Click) -> impl Bundle {
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
            Icon::new(glyph),
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
