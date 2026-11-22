//! This project's own interface: panels in the window's corners and along
//! its lower edge, over a view as wide as the window, drawn in the
//! [theme](crate::ui::Theme)'s colours.

use bevy::prelude::*;

use super::{Hud, InMatch};

pub mod actions;
pub mod chat;
pub mod clock;
pub mod displays;
pub mod groups;
pub mod idle;
pub mod messages;
pub mod minimap;
pub mod object_display;
pub mod politics;
pub mod resource_bar;
pub mod special_ops;
pub mod times;

/// While a match is played with our interface.
pub(crate) const DRAWN: InMatch = InMatch(Hud::Modern);

pub struct ModernHudPlugin;

impl Plugin for ModernHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(DRAWN),
            (
                (
                    minimap::spawn,
                    displays::spawn_buttons,
                    clock::spawn,
                    chat::spawn,
                    politics::spawn,
                ),
                minimap::lay_out_rim,
            )
                .chain(),
        )
        .add_systems(
            OnEnter(DRAWN),
            (
                actions::spawn,
                object_display::spawn,
                resource_bar::spawn,
                idle::spawn,
                special_ops::spawn,
                groups::spawn,
                messages::place,
                crate::hud::original::load_cards,
                crate::object_display::redraw,
                politics::take_over,
            ),
        )
        .add_systems(
            OnExit(DRAWN),
            (
                crate::hud::original::unload_cards,
                // Switching interfaces, rather than leaving the match.
                politics::close.run_if(in_state(crate::Screen::Match)),
                |mut commands: Commands| {
                    commands.remove_resource::<crate::ui_scale::TopRow>();
                },
            ),
        )
        .add_systems(
            Update,
            (
                minimap::tint,
                object_display::emblem,
                crate::idle::show,
                (
                    object_display::lay_out.in_set(super::LayOut),
                    (object_display::economy, crate::hud::original::fill_bars),
                )
                    .chain(),
                (
                    clock::light_button,
                    chat::light_button,
                    displays::light_buttons,
                    actions::show,
                    // For the buttons on the cards it borrows from the original's.
                    crate::hud::original::show_pressables,
                    resource_bar::show,
                    times::show,
                    politics::show,
                    groups::show,
                )
                    .in_set(super::Draw),
                (special_ops::refresh, special_ops::tips)
                    .run_if(resource_exists::<crate::special_ops::Panel>),
            )
                .run_if(in_state(DRAWN)),
        );
    }
}
