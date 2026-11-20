//! This project's own interface: panels in the window's corners and along
//! its lower edge, over a view as wide as the window, drawn in the
//! [theme](crate::ui::Theme)'s colours.

use bevy::prelude::*;

use super::{Hud, Setup, playing};
use crate::Screen;

pub mod actions;
pub mod clock;
pub mod displays;
pub mod messages;
pub mod minimap;
pub mod object_display;
pub mod resource_bar;
pub mod special_ops;
pub mod supply;

pub struct ModernHudPlugin;

impl Plugin for ModernHudPlugin {
    fn build(&self, app: &mut App) {
        let drawn = || playing(Hud::Modern);
        app.add_systems(
            OnEnter(Screen::Match),
            (
                minimap::spawn,
                actions::spawn,
                object_display::spawn,
                resource_bar::spawn,
                supply::spawn,
                clock::spawn,
                displays::spawn_buttons,
                special_ops::spawn,
                messages::place.after(crate::messages::spawn),
            )
                .after(Setup)
                .before(crate::console::start_scripts)
                .run_if(drawn()),
        )
        .add_systems(
            Update,
            (
                minimap::tint,
                supply::show_cycle_button,
                clock::light_button.after(crate::clock::disable_buttons),
                displays::light_buttons.after(crate::displays::Keys),
                displays::mark_buttons.run_if(resource_exists_and_changed::<crate::keys::Bindings>),
                actions::show.after(crate::actions::tell),
                (
                    object_display::lay_out
                        .after(crate::object_display::choose)
                        .before(crate::object_display::refresh),
                    object_display::economy,
                )
                    .chain(),
                object_display::portray.after(crate::object_display::portray),
                resource_bar::show.after(crate::resource_bar::show),
                (special_ops::refresh, special_ops::tips)
                    .run_if(resource_exists::<crate::special_ops::Panel>),
            )
                .run_if(in_state(Screen::Match))
                .run_if(drawn()),
        );
    }
}
