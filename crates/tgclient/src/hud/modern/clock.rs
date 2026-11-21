//! Our pause button, a glyph on the minimap's rim.

use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;

use super::displays::{Glyph, Lighting, Rim, RimLook, light, rim_button};
use crate::Loaded;
use crate::clock::{PAUSE, PAUSE_BUTTON, PauseState, Pauses};
use crate::displays;
use crate::intent::Asks;
use crate::keys::{Bindings, Named};
use crate::ui::{Click, Hint, Theme};

/// Our interface's pause button, a glyph on the minimap's rim.
#[derive(Component)]
pub struct PauseButton;

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
        rim_button(Rim::TopLeft, glyph, Click::TOGGLE),
        Named::new(&bindings, PAUSE, name),
        (Asks::new(PAUSE), Hint(help)),
        crate::ui::called(PAUSE_BUTTON),
        (Pauses, PauseButton),
    ));
}

/// Lights our pause button while the match is paused, and greys it for
/// those who may not pause.
pub fn light_button(
    state: Res<PauseState>,
    theme: Res<Theme>,
    mut buttons: Query<(Ref<PauseButton>, RimLook)>,
    mut glyphs: Query<&mut ImageNode, With<Glyph>>,
) {
    let PauseState { paused, usable } = *state;
    for (button, look) in &mut buttons {
        let restyle = state.is_changed() || theme.is_changed() || button.is_added();
        let lighting = Lighting {
            lit: paused,
            usable,
            restyle,
        };
        light(look, lighting, &theme, &mut glyphs);
    }
}
