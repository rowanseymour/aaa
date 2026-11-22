//! Our group strip, along the top beside the special operations panel: a
//! slot for each control group showing its picture, or the key that
//! selects it while there's no such group, with the tooltip under the
//! strip. What each shows and does is
//! [`control_groups`](crate::control_groups)', shared with the original's.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use super::actions::GAP;
use crate::Loaded;
use crate::control_groups::{GROUPS, GroupButton, Shown, Strip, select_group};
use crate::keys::Bindings;
use crate::ui::{self, Blocks, Click, Edge, Font, ImageCache, Ink, Surface, label, panel};

/// A slot's side, and how far in its picture is drawn.
const SLOT: f32 = 36.0;
const INSET: f32 = 2.0;
/// A slot's corners, rounder than that would leave too little square
/// for its picture.
const CORNER: f32 = 6.0;

/// How tall and wide the strip is, for what goes under and beside it.
pub const HEIGHT: f32 = SLOT + 2.0 * GAP;
pub const WIDTH: f32 = GROUPS as f32 * SLOT + (GROUPS + 1) as f32 * GAP;

/// The widest the tooltip under the strip is before its words wrap.
const TIP_WIDTH: f32 = 300.0;

/// A slot's picture of its group.
#[derive(Component)]
pub struct Picture;

/// The key shown in a slot while there's no group.
#[derive(Component)]
pub struct KeyLabel;

/// Spawns the strip at the top of the window, `left` of its left edge.
pub fn spawn(mut commands: Commands, local: Res<crate::Local>, leaders: Res<crate::Leaders>) {
    let left = super::special_ops::beside(&local, &leaders);
    let said = commands
        .spawn((
            ui::hint_box(Node::default(), px(TIP_WIDTH)),
            // Over the message log, which it opens onto.
            GlobalZIndex(ui::layer::HUD_TOP),
        ))
        .id();
    let strip = commands
        .spawn((
            Blocks,
            ui::Docks(said),
            panel(
                Surface::Hud,
                Node {
                    padding: UiRect::all(px(GAP)),
                    column_gap: px(GAP),
                    ..default()
                },
            ),
            Edge::Border,
        ))
        .with_children(|strip| {
            for n in 0..GROUPS {
                strip
                    .spawn((
                        GroupButton(n),
                        GroupButton::called(n),
                        Button,
                        Click::COMMAND,
                        Hovered::default(),
                        Pickable::default(),
                        ui::backed(
                            Surface::Slot,
                            Node {
                                width: px(SLOT),
                                height: px(SLOT),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(CORNER)),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                        ),
                        Edge::Border,
                    ))
                    .with_children(|slot| {
                        slot.spawn((
                            Picture,
                            ImageNode::default(),
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(INSET),
                                top: px(INSET),
                                right: px(INSET),
                                bottom: px(INSET),
                                ..default()
                            },
                            Visibility::Hidden,
                            Pickable::IGNORE,
                        ));
                        slot.spawn((KeyLabel, label("", Font::Hud, Ink::Muted)));
                    });
            }
        })
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(crate::ui_scale::MARGIN),
                left: px(left),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Start,
                row_gap: px(GAP),
                ..default()
            },
            Pickable::IGNORE,
            // Under the special operations' tooltips, which open onto it.
            GlobalZIndex(ui::layer::HUD),
            DespawnOnExit(super::DRAWN),
        ))
        .add_children(&[strip, said]);
}

/// Shows each slot's group's picture, or its key while there's no group,
/// and lights the slot under the pointer.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn show(
    strip: Res<Strip>,
    bindings: Res<Bindings>,
    loaded: Res<Loaded>,
    mut cache: ResMut<ImageCache>,
    mut images: ResMut<Assets<Image>>,
    mut slots: Query<(&GroupButton, &Hovered, &mut Edge, &Children)>,
    mut pictures: Query<(&mut ImageNode, &mut Visibility), With<Picture>>,
    mut keys: Query<(&mut Text, &mut Visibility), (With<KeyLabel>, Without<Picture>)>,
) {
    for (button, hovered, mut edge, children) in &mut slots {
        edge.set_if_neq(Edge::lit(false, hovered.get(), Edge::Border));
        let shown = &strip.0[button.0];
        let picture = match shown {
            Shown::Group(Some(path)) => cache.get(&loaded.game, &mut images, path),
            _ => None,
        };
        for &child in children {
            if let Ok((mut image, mut visibility)) = pictures.get_mut(child) {
                visibility.set_if_neq(ui::visible(picture.is_some()));
                if let Some(picture) = &picture
                    && image.image != *picture
                {
                    image.image = picture.clone();
                }
            }
            if let Ok((mut text, mut visibility)) = keys.get_mut(child) {
                visibility.set_if_neq(ui::visible(*shown == Shown::Unassigned));
                let key = (bindings.key(&select_group(button.0))).map_or("", |k| k.name());
                if text.0 != key {
                    text.0 = key.to_string();
                }
            }
        }
    }
}
