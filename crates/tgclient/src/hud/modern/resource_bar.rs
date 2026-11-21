//! Our resource bar in the window's upper right corner: the shared
//! [cells](crate::resource_bar::Cell) in a bar, and a second bar under it
//! previewing what the order under the cursor would make of them.

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;

use crate::resource_bar::{Cell, Previewing, stockpile};
use crate::ui::{Font, Hint, Ink, Surface, backed, label};
use crate::{Loaded, Local, Sim};

const CELL: f32 = 80.0;

/// The stockpile's cell's width, which has room for the stockpile beside
/// its rate.
const STOCKPILE_CELL: f32 = 112.0;
const HEIGHT: f32 = 22.0;
const GAP: f32 = 4.0;

/// One of the two bars: the player's economy now, or previewed.
#[derive(Component)]
pub struct Bar(bool);

/// The mark beside the bar while the player can't pay their upkeep.
#[derive(Component)]
pub struct ShortageIcon;

const SHORTAGE_ICON: &str = "UI/Game/ControlPanel/BleedIcon.png";

/// Spawns our interface's bars, in the window's upper right corner.
pub fn spawn(
    mut commands: Commands,
    loaded: Res<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &loaded.game;
    let strings = game.strings();
    let said = crate::ui::said(&strings);
    let shortage = crate::ui::read_image(game, SHORTAGE_ICON, formats.0).map(|i| images.add(i));
    let shortage_tip = said(
        "game_interface_UpkeepShortageIcon_ToolTip",
        "Insufficient money to pay upkeep.  Units will take damage when out of supply.",
    );
    let glyphs = crate::ui::glyphs(game, &mut images, formats.0);
    let stockpile = stockpile(game);
    let cells = game.economy_bar();
    let bar = |root: &mut ChildSpawnerCommands, preview: bool| {
        root.spawn((
            Bar(preview),
            backed(
                Surface::Hud,
                Node {
                    display: match preview {
                        true => Display::None,
                        false => Display::Flex,
                    },
                    height: px(HEIGHT),
                    column_gap: px(GAP),
                    padding: UiRect::horizontal(px(GAP * 2.0)),
                    border_radius: BorderRadius::all(px(6)),
                    ..default()
                },
            ),
        ))
        .with_children(|bar| {
            for kind in &cells {
                bar.spawn(Node {
                    width: px(match stockpile.as_ref() == Some(kind) {
                        true => STOCKPILE_CELL,
                        false => CELL,
                    }),
                    column_gap: px(3),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|cell| {
                    if let Some(glyph) = glyphs.get(kind) {
                        cell.spawn((
                            ImageNode::new(glyph.clone()),
                            Node {
                                width: px(16),
                                height: px(16),
                                ..default()
                            },
                        ));
                    }
                    let (text, hint) = Cell::new(game, preview, kind);
                    let mut text = cell.spawn((text, label("", Font::Hud, Ink::Body)));
                    // Unlike a label's, its text is hovered.
                    if let Some(hint) = hint {
                        text.insert((hint, Pickable::default()));
                    }
                });
            }
        });
    };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(crate::ui_scale::MARGIN),
                right: px(crate::ui_scale::MARGIN),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: px(GAP),
                ..default()
            },
            Pickable::IGNORE,
            GlobalZIndex(crate::ui::layer::HUD),
            DespawnOnExit(super::DRAWN),
        ))
        .with_children(|root| {
            root.spawn(Node {
                column_gap: px(GAP),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                if let Some(icon) = shortage {
                    row.spawn((
                        ShortageIcon,
                        Hint(shortage_tip),
                        ImageNode::new(icon),
                        Node {
                            display: Display::None,
                            width: px(HEIGHT),
                            height: px(HEIGHT),
                            ..default()
                        },
                    ));
                }
                bar(row, false);
            });
            bar(root, true);
        });
}

/// Shows the preview bar only while there's something to preview, and
/// marks a shortage beside the bars.
pub fn show(
    sim: Res<Sim>,
    local: Res<Local>,
    previewing: Res<Previewing>,
    mut bars: Query<(&Bar, &mut Node), Without<ShortageIcon>>,
    mut icons: Query<&mut Node, With<ShortageIcon>>,
) {
    let short = local.0.is_some_and(|p| sim.0.short(p).is_some());
    for mut node in &mut icons {
        crate::ui::show(&mut node, short);
    }
    for (bar, mut node) in &mut bars {
        crate::ui::show(&mut node, local.0.is_some() && (previewing.0 || !bar.0));
    }
}
