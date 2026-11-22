//! Our chat button, a glyph on the minimap's rim.

use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;

use super::displays::{Glyph, Lighting, Quadrant, RimLook, light, rim_button};
use crate::Loaded;
use crate::chat::{ACTIVATE, BUTTON, Chat};
use crate::displays;
use crate::intent::Asks;
use crate::keys::{Bindings, Named};
use crate::ui::{Click, Hint, Theme};

/// Our interface's chat button.
#[derive(Component)]
pub struct ChatButton;

/// Spawns the chat button.
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
    let name = said("control_panel_ButtonChat_ToolTip", "Chat");
    let help = said(
        "control_panel_ButtonChat_tooltip_2",
        "Allows you to send messages to other players.",
    );
    let glyph = icons.get(
        &loaded.game,
        &displays::icon("chat"),
        &mut images,
        formats.0,
    );
    commands.spawn((
        rim_button(Quadrant::TopLeft.at(0), glyph, Click::TOGGLE),
        Named::new(&bindings, ACTIVATE, name),
        (Asks::new(ACTIVATE), Hint(help)),
        crate::ui::called(BUTTON),
        ChatButton,
    ));
}

/// Lights our chat button while the chat bar is open.
pub fn light_button(
    chat: Res<Chat>,
    theme: Res<Theme>,
    mut buttons: Query<(Ref<ChatButton>, RimLook)>,
    mut glyphs: Query<&mut ImageNode, With<Glyph>>,
) {
    for (button, look) in &mut buttons {
        let restyle = chat.is_changed() || theme.is_changed() || button.is_added();
        let lighting = Lighting {
            lit: chat.is_open(),
            usable: true,
            restyle,
        };
        light(look, lighting, &theme, &mut glyphs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{Edge, Surface};
    use bevy::picking::hover::Hovered;

    #[test]
    fn the_button_is_lit_while_the_bar_is_open() {
        let mut app = App::new();
        app.init_resource::<Chat>()
            .init_resource::<Theme>()
            .add_systems(Update, light_button);
        let world = app.world_mut();
        let glyph = world.spawn((Glyph, ImageNode::default())).id();
        let button = (ChatButton, Hovered::default(), Edge::Border, Surface::Clear);
        let button = world.spawn(button).add_child(glyph).id();
        let edge = |app: &App| *app.world().get::<Edge>(button).unwrap();
        app.update();
        assert_eq!(edge(&app), Edge::Border);
        *app.world_mut().resource_mut::<Chat>() = Chat::opened();
        app.update();
        assert_eq!(edge(&app), Edge::Lit);
        *app.world_mut().resource_mut::<Chat>() = Chat::default();
        app.update();
        assert_eq!(edge(&app), Edge::Border);
    }
}
