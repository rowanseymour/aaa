//! Our idle buttons, stacked over the minimap, each drawn with the
//! pictures its panel in the original's layouts rests on, in the order
//! those panels stand down the screen.

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::Button;
use tgdata::widgets::{Rect, SCREEN};

use super::minimap;
use crate::Loaded;
use crate::idle::Idle;
use crate::intent::Asks;
use crate::ui::{Blocks, Hint};

const SIZE: f32 = 42.0;

/// Spawns the idle buttons whose panels the game draws, above the minimap.
pub fn spawn(
    mut commands: Commands,
    loaded: Res<Loaded>,
    bindings: Res<crate::keys::Bindings>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<crate::ui::ImageCache>,
) {
    let game = &loaded.game;
    let widgets = game.game_widgets();
    let strings = game.strings();
    let screen = Rect {
        left: 0.0,
        top: 0.0,
        right: SCREEN[0],
        bottom: SCREEN[1],
    };
    let mut panels: Vec<_> = (Idle::ALL.iter())
        .filter_map(|&idle| {
            let panel = (widgets.iter()).find(|w| w.name.eq_ignore_ascii_case(idle.widget()))?;
            let pictures: Vec<_> = (panel.resting().into_iter())
                .filter_map(|path| cache.get(game, &mut images, path))
                .collect();
            // A panel the game leaves empty has no button.
            (!pictures.is_empty()).then_some((idle, panel, pictures))
        })
        .collect();
    panels.sort_by(|a, b| a.1.bounds(screen).top.total_cmp(&b.1.bounds(screen).top));
    let column = Node {
        position_type: PositionType::Absolute,
        flex_direction: FlexDirection::Column,
        row_gap: px(crate::ui_scale::MARGIN),
        left: px(crate::ui_scale::MARGIN),
        bottom: px(crate::ui_scale::MARGIN + minimap::DISC + crate::ui_scale::MARGIN),
        ..default()
    };
    let fill = Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    };
    commands
        .spawn((
            column,
            Pickable::IGNORE,
            GlobalZIndex(crate::ui::layer::HUD),
            DespawnOnExit(super::DRAWN),
        ))
        .with_children(|column| {
            for (idle, panel, pictures) in panels {
                let button = panel.children.first().unwrap_or(panel);
                let (name, help) = button.tooltip(&strings);
                let name = name.unwrap_or_default();
                let mut spawned = column.spawn((
                    idle,
                    crate::keys::Named::new(&bindings, idle.asks(), name),
                    Blocks,
                    Button,
                    crate::ui::called(idle.called()),
                    Asks::new(idle.asks()),
                    RelativeCursorPosition::default(),
                    Hint(help.unwrap_or_default()),
                    Node {
                        display: Display::None,
                        width: px(SIZE),
                        height: px(SIZE),
                        ..default()
                    },
                ));
                spawned.with_children(|button| {
                    for picture in pictures {
                        button.spawn((fill.clone(), ImageNode::new(picture), Pickable::IGNORE));
                    }
                });
            }
        });
}
