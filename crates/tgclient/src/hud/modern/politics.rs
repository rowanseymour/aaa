//! Our politics panel: a round button at the end of the resource bar's
//! row, and under the bars while it's open, the shared
//! [rows](crate::politics::Rows), each with its side's gem and name and
//! the font's marker for how the player stands to it.

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use super::displays::{Glyph, Lighting, RimLook, light, round_button};
use crate::intent::Asks;
use crate::keys::{Bindings, Named};
use crate::politics::{BUTTON, CALLED, Politics, Rows};
use crate::ui::{self, Click, Font, Gem, Hint, Ink, Marked, Surface, Theme, backed, label};

/// The button that opens and closes the panel.
#[derive(Component)]
pub struct PoliticsButton;

/// The panel, holding its rows.
#[derive(Component)]
pub struct Panel {
    /// The ally, neutral and enemy markers.
    markers: [Option<Handle<Image>>; 3],
}

/// The marker's size, and how wide a row's name may run before it.
const MARKER: f32 = 16.0;
const NAME: f32 = 180.0;

/// Spawns the button in `row`, the size of the bars, with `glyph` on it.
pub fn spawn_button(
    row: &mut ChildSpawnerCommands,
    size: f32,
    glyph: Handle<Image>,
    bindings: &Bindings,
    (name, help): (String, String),
) {
    let node = Node {
        width: px(size),
        height: px(size),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::MAX,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    row.spawn((
        round_button(node, glyph, size * 0.7, Click::TOGGLE),
        Named::new(bindings, BUTTON, name),
        (Asks::new(BUTTON), Hint(help)),
        ui::called(CALLED),
        PoliticsButton,
    ));
}

/// Spawns the panel in `column`, hidden, its markers as the font draws
/// them.
pub fn spawn_panel(column: &mut ChildSpawnerCommands, markers: Vec<Option<Handle<Image>>>) {
    let mut markers = markers.into_iter();
    let markers = std::array::from_fn(|_| markers.next().flatten());
    column.spawn((
        Panel { markers },
        backed(
            Surface::Hud,
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(10), px(6)),
                row_gap: px(3),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
        ),
        crate::ui::Blocks,
        RelativeCursorPosition::default(),
    ));
}

/// Shows the panel while it's open, its rows written afresh whenever they
/// change.
pub fn show(
    mut commands: Commands,
    politics: Res<Politics>,
    rows: Res<Rows>,
    gem: Res<Gem>,
    mut panels: Query<(Entity, Ref<Panel>, &mut Node)>,
) {
    for (entity, panel, mut node) in &mut panels {
        if (node.display == Display::None) == politics.open {
            ui::show(&mut node, politics.open);
        }
        if !rows.is_changed() && !panel.is_added() {
            continue;
        }
        commands.entity(entity).despawn_children();
        commands.entity(entity).with_children(|lines| {
            for row in &rows.0 {
                write(lines, &panel, &gem, row);
            }
        });
    }
}

/// Writes `row`: its gem, its name, greyed once it's out of the match, and
/// the marker for how the player stands to it.
fn write(lines: &mut ChildSpawnerCommands, panel: &Panel, gem: &Gem, row: &crate::politics::Row) {
    let mut line = lines.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: px(6),
        ..default()
    });
    if let Some(tip) = &row.tip {
        line.insert((
            Marked::plain(row.name.clone()),
            Hint(tip.clone()),
            RelativeCursorPosition::default(),
        ));
    }
    let ink = match row.out {
        true => Ink::Muted,
        false => Ink::Body,
    };
    let square = |side: f32| Node {
        width: px(side),
        height: px(side),
        flex_shrink: 0.0,
        ..default()
    };
    line.with_children(|line| {
        line.spawn(ui::side_mark(gem, row.colour, square(ui::GEM)));
        line.spawn((
            label(row.name.clone(), Font::Hud, ink),
            Node {
                width: px(NAME),
                overflow: Overflow::clip(),
                ..default()
            },
        ));
        let marker = row
            .relation
            .and_then(crate::politics::marker)
            .and_then(|at| panel.markers[at].as_ref());
        match marker {
            Some(image) => line.spawn((
                ImageNode::new(image.clone()),
                square(MARKER),
                Pickable::IGNORE,
            )),
            None => line.spawn(square(MARKER)),
        };
    });
}

/// Lights the button while the panel's open.
pub fn light_button(
    politics: Res<Politics>,
    theme: Res<Theme>,
    mut buttons: Query<(Ref<PoliticsButton>, RimLook)>,
    mut glyphs: Query<&mut ImageNode, With<Glyph>>,
) {
    let restyle = politics.is_changed() || theme.is_changed();
    for (button, look) in &mut buttons {
        let lighting = Lighting {
            lit: politics.open,
            usable: true,
            restyle: restyle || button.is_added(),
        };
        light(look, lighting, &theme, &mut glyphs);
    }
}
