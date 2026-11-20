//! Our pause button, a glyph on the minimap's rim.

use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::Button;

use super::displays::Rim;
use crate::Loaded;
use crate::actions::Blocks;
use crate::clock::{PAUSE, PAUSE_BUTTON, PauseState, Pauses};
use crate::displays;
use crate::intent::Asks;
use crate::keys::{Bindings, Named};
use crate::ui::{Click, Edge, Hint, Ink, Surface, Theme};

/// Our interface's pause button, a glyph on the minimap's rim.
#[derive(Component)]
pub struct PauseButton;

/// The glyph on the [`PauseButton`].
#[derive(Component)]
pub struct PauseGlyph;

/// Spawns the pause button.
pub fn spawn(
    mut commands: Commands,
    loaded: Res<Loaded>,
    bindings: Res<Bindings>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    mut icons: ResMut<displays::Icons>,
) {
    let strings = loaded.game.strings();
    let said = crate::ui::said(&strings);
    let name = said("control_panel_ButtonPause_ToolTip", "Pause Game");
    let help = said("control_panel_ButtonPause_tooltip_2", "Pauses the game.");
    let glyph = icons.get(displays::icon!("pause"), &mut images, formats.0);
    commands.spawn((
        Blocks,
        Button,
        Click::TOGGLE,
        RelativeCursorPosition::default(),
        Named::new(&bindings, PAUSE, name),
        (Asks::new(PAUSE), Hint(help)),
        crate::ui::called(PAUSE_BUTTON),
        (Pauses, PauseButton),
        Rim::TopLeft.node(),
        Surface::Clear,
        Edge::Border,
        GlobalZIndex(crate::ui::layer::HUD_TOP),
        DespawnOnExit(super::DRAWN),
        children![(
            PauseGlyph,
            ImageNode::new(glyph),
            Node {
                width: px(super::displays::GLYPH_SIZE),
                height: px(super::displays::GLYPH_SIZE),
                ..default()
            },
            Pickable::IGNORE,
        )],
    ));
}

/// What [`light_button`] reads and sets on the button.
type ButtonLook = (
    Ref<'static, PauseButton>,
    Ref<'static, Hovered>,
    Has<bevy::ui::Pressed>,
    &'static mut Edge,
    &'static mut Surface,
    &'static Children,
);

/// Lights our pause button while the match is paused, its edge while it's
/// hovered and its face while it's held down, and greys it for those who
/// may not pause.
pub fn light_button(
    state: Res<PauseState>,
    theme: Res<Theme>,
    mut buttons: Query<ButtonLook>,
    mut glyphs: Query<&mut ImageNode, With<PauseGlyph>>,
) {
    let PauseState { paused, usable } = *state;
    for (button, hovered, pressed, mut edge, mut surface, children) in &mut buttons {
        surface.set_if_neq(match pressed && usable {
            true => Surface::Hovered,
            false => Surface::Clear,
        });
        let restyle = state.is_changed() || theme.is_changed() || button.is_added();
        if restyle || hovered.is_changed() {
            edge.set_if_neq(Edge::lit(paused, hovered.get() && usable, Edge::Border));
        }
        if !restyle {
            continue;
        }
        let colour = match (paused, usable) {
            (true, _) => theme.edge(Edge::Lit),
            (false, true) => theme.ink(Ink::Help),
            (false, false) => theme.ink(Ink::Muted),
        };
        for &child in children {
            if let Ok(mut glyph) = glyphs.get_mut(child) {
                glyph.color = colour;
            }
        }
    }
}
