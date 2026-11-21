//! Our button over the minimap that steps through the player's detached
//! organizations.

use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::Button;

use super::minimap;
use crate::intent::Asks;
use crate::keys;
use crate::ui::Blocks;
use crate::ui::Hint;
use crate::{Loaded, Local, Sim};

/// The button over the minimap that steps through the player's detached
/// organizations, as F5 does; there only while there are some.
#[derive(Component)]
pub struct CycleDetached;

const CYCLE_ICON: &str = "UI/Game/Floaters/ButtonDetachedRegimentUnpressed.tga";
const CYCLE_HELP: &str = "idle_buttons_IdleButton_tooltip_2";
const CYCLE_SIZE: f32 = 42.0;

/// Spawns our interface's button that cycles through detached
/// organizations, above the minimap.
pub fn spawn(
    mut commands: Commands,
    loaded: Res<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &loaded.game;
    let icon = crate::ui::read_image(game, CYCLE_ICON, formats.0).map(|i| images.add(i));
    let help = game.strings().get(CYCLE_HELP).cloned();
    commands.spawn((
        CycleDetached,
        Blocks,
        Button,
        crate::ui::called("supply.detached"),
        Asks::new(keys::CYCLE_DETACHED),
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
        GlobalZIndex(crate::ui::layer::HUD),
        DespawnOnExit(super::DRAWN),
    ));
}

/// Shows the button that cycles through detached organizations while there's
/// one that an HQ of the player's could take, places aside, though the
/// cycle takes in those none could.
pub fn show_cycle_button(
    sim: Res<Sim>,
    local: Res<Local>,
    mut button: Single<(&mut Node, Ref<CycleDetached>)>,
) {
    let (button, added) = &mut *button;
    if !sim.is_changed() && !local.is_changed() && !added.is_added() {
        return;
    }
    let any = local.0.is_some_and(|p| sim.0.attachable(p));
    crate::ui::show(button, any);
}
