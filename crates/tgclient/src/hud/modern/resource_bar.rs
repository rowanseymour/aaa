//! Our resource bar in the window's upper right corner: the shared
//! [cells](crate::resource_bar::Cell) in a bar, and a second bar under it
//! previewing what the order under the cursor would make of them, with
//! the [times](super::times) beside it and the tooltip under both.

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::prelude::*;

use crate::displays::Icon;
use crate::resource_bar::{Cell, Previewing, Wording, stockpile};
use crate::ui::{self, Font, Hint, Ink, Surface, backed, label};
use crate::{Loaded, Local, Sim};

const CELL: f32 = 80.0;

/// The stockpile's cell's width, which has room for the stockpile beside
/// its rate.
const STOCKPILE_CELL: f32 = 112.0;
const HEIGHT: f32 = 22.0;
/// A cell's icon's size.
const ICON: f32 = 16.0;
const GAP: f32 = 4.0;

/// The widest the tooltip under the bar is before its words wrap.
const TIP_WIDTH: f32 = 300.0;

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
    mut cache: ResMut<crate::ui::ImageCache>,
) {
    let game = &loaded.game;
    let strings = game.strings();
    let said = crate::ui::said(&strings);
    let shortage = cache.get(game, &mut images, SHORTAGE_ICON);
    let shortage_tip = said(
        "game_interface_UpkeepShortageIcon_ToolTip",
        "Insufficient money to pay upkeep.  Units will take damage when out of supply.",
    );
    // Our own icons where the data has them, else the game's glyphs.
    let icons = crate::ui::resource_icons(game);
    let glyphs = crate::ui::glyphs(game, &mut images, &mut cache);
    let stockpile = stockpile(game);
    let cells = game.economy_bar();
    // The row's pills, shortage icon and bar at their widest, then the
    // group strip beside the special operations.
    let widths = cells
        .iter()
        .map(|kind| match stockpile.as_ref() == Some(kind) {
            true => STOCKPILE_CELL,
            false => CELL,
        });
    let bar_width = widths.sum::<f32>() + GAP * cells.len().saturating_sub(1) as f32 + GAP * 4.0;
    let right = 2.0 * super::times::PILL + HEIGHT + bar_width + 3.0 * GAP;
    let left = super::special_ops::CLEAR + super::groups::WIDTH;
    commands.insert_resource(crate::ui_scale::TopRow(
        left + 2.0 * GAP + right + super::actions::PAD,
    ));
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
                    let node = Node {
                        width: px(ICON),
                        height: px(ICON),
                        ..default()
                    };
                    if let Some(path) = icons.get(kind) {
                        cell.spawn((Icon::new(path.as_str()), node));
                    } else if let Some(glyph) = glyphs.get(kind) {
                        cell.spawn((ImageNode::new(glyph.clone()), node));
                    }
                    let (text, hint) = Cell::new(game, preview, kind, Wording::Ours);
                    let mut text = cell.spawn((text, label("", Font::Hud, Ink::Body)));
                    // Unlike a label's, its text is hovered.
                    if let Some(hint) = hint {
                        text.insert((hint, Pickable::default()));
                    }
                });
            }
        });
    };
    // Under the bars, as far from them as the other docked tooltips are
    // from their panes.
    let said = commands
        .spawn((
            ui::hint_box(
                Node {
                    margin: UiRect::top(px(super::actions::PAD - GAP)),
                    ..default()
                },
                px(TIP_WIDTH),
            ),
            GlobalZIndex(ui::layer::HUD_TOP),
        ))
        .id();
    commands
        .spawn((
            ui::Docks(said),
            Node {
                position_type: PositionType::Absolute,
                // As far in as the action palette under it, and the
                // tooltips from their panes.
                top: px(super::actions::PAD),
                right: px(super::actions::PAD),
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
                super::times::spawn(row, HEIGHT);
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
        })
        .add_child(said);
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
