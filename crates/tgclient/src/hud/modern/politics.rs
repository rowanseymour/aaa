//! Our politics panel: one of the [display panels](crate::displays),
//! opened by its button on the minimap's rim, whose window lists the
//! shared [rows](crate::politics::Rows), each with its side's gem and name
//! and the font's marker for how the player stands to it.

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use crate::Loaded;
use crate::displays::{self, Window};
use crate::politics::{Look, Rows};
use crate::ui::{self, Font, Gem, Hint, Ink, Marked, label};

/// The box in the panel's window holding its rows.
#[derive(Component)]
pub struct Panel {
    /// The ally, neutral and enemy markers.
    markers: [Option<Handle<Image>>; 3],
}

/// The marker's size, and how wide a row's name may run before it.
const MARKER: f32 = 16.0;
const NAME: f32 = 180.0;

/// Spawns the panel's window, hidden, in the middle of the screen, its
/// markers as the font draws them.
pub fn spawn(
    mut commands: Commands,
    loaded: Res<Loaded>,
    mut cache: ResMut<ui::ImageCache>,
    mut images: ResMut<Assets<Image>>,
) {
    let game = &loaded.game;
    let strings = game.strings();
    let said = ui::said(&strings);
    let mut marker = |name: &str| {
        let path = format!("{}/{name}.png", ui::GLYPH_DIR);
        cache.get(game, &mut images, &path)
    };
    let mut markers =
        (Look::new(game).glyphs.into_iter()).map(|g| g.as_deref().and_then(&mut marker));
    let markers = std::array::from_fn(|_| markers.next().flatten());
    let (window, contents) = displays::spawn_window(
        &mut commands,
        Window {
            panel: displays::Panel::Politics,
            title: said("politics_display_Child_text", "Politics"),
            at: Vec2::new(382.0, 264.0),
            size: Vec2::new(260.0, 240.0),
            pane: true,
        },
    );
    commands.entity(contents).insert(Panel { markers });
    // Gone with our interface, so the original's draws politics its own way.
    commands.entity(window).insert(DespawnOnExit(super::DRAWN));
}

/// Closes the original's panel as our interface takes over, since ours
/// can't.
pub fn take_over(mut politics: ResMut<crate::politics::Politics>) {
    if politics.open {
        politics.open = false;
    }
}

/// Closes the panel as our interface goes, letting the match go on if
/// opening it held it.
pub fn close(mut toggling: displays::Toggling) {
    toggling.close_panel(displays::Panel::Politics);
}

/// Writes the rows afresh whenever they change.
pub fn show(
    mut commands: Commands,
    rows: Res<Rows>,
    gem: Res<Gem>,
    panels: Query<(Entity, Ref<Panel>)>,
) {
    for (entity, panel) in &panels {
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
