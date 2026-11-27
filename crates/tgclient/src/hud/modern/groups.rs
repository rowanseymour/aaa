//! Our group strip, hung from the top edge beside the special operations
//! panel: a slot for each control group showing its picture, or the key
//! that selects it while there's no such group, with the tooltip under
//! the strip where the panel's tooltips open. What each shows and does is
//! [`control_groups`](crate::control_groups)', shared with the original's.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use super::actions::PAD;
use crate::Loaded;
use crate::control_groups::{GROUPS, GroupButton, Shown, Strip, select_group};
use crate::keys::Bindings;
use crate::ui::{self, Blocks, Click, Edge, Font, ImageCache, Ink, Surface, label};

/// A slot's side, and how far in its picture is drawn from the sides
/// away from the window's edge.
const SLOT: f32 = 36.0;
const INSET: f32 = 2.0;

/// How tall and wide the strip is, padded as the action palette is but
/// for its top, where the slots run off the window's edge, for what goes
/// under and beside it.
pub const HEIGHT: f32 = SLOT + PAD;
pub const WIDTH: f32 = GROUPS as f32 * SLOT + 2.0 * PAD;

/// The widest the tooltip under the strip is before its words wrap.
const TIP_WIDTH: f32 = 300.0;

/// The box the strip's tooltips are said in, which the special operations
/// panel's share.
#[derive(Resource)]
pub struct SharedTip(pub Entity);

/// A slot's picture of its group.
#[derive(Component)]
pub struct Picture;

/// The key shown in a slot while there's no group.
#[derive(Component)]
pub struct KeyLabel;

/// Spawns the strip on the window's top edge, clear of the special
/// operations panel, and its tooltip's box where the panel's open.
pub fn spawn(mut commands: Commands, local: Res<crate::Local>, leaders: Res<crate::Leaders>) {
    let at = super::special_ops::tips_at(&local, &leaders);
    let said = commands
        .spawn((
            ui::hint_box(
                Node {
                    position_type: PositionType::Absolute,
                    left: px(at.x),
                    top: px(at.y),
                    ..default()
                },
                px(TIP_WIDTH),
            ),
            // Over the message log, which it opens onto.
            GlobalZIndex(ui::layer::HUD_TOP),
            DespawnOnExit(super::DRAWN),
        ))
        .id();
    commands.insert_resource(SharedTip(said));
    commands
        .spawn((
            Blocks,
            ui::Docks(said),
            ui::hanging(
                Surface::Hud,
                Node {
                    position_type: PositionType::Absolute,
                    top: px(0),
                    left: px(at.x),
                    padding: UiRect {
                        top: px(0),
                        ..UiRect::all(px(PAD))
                    },
                    ..default()
                },
            ),
            // Under the special operations' tooltips, which open onto it.
            GlobalZIndex(ui::layer::HUD),
            DespawnOnExit(super::DRAWN),
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
                                // Square and touching, as one row of tiles,
                                // open to the window's edge.
                                border: UiRect {
                                    top: px(0),
                                    ..UiRect::all(px(1))
                                },
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                        ),
                        Edge::Clear,
                    ))
                    .with_children(|slot| {
                        slot.spawn((
                            Picture,
                            ImageNode::default(),
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(INSET),
                                top: px(0),
                                right: px(INSET),
                                // Square, as the slot no longer is inside.
                                aspect_ratio: Some(1.0),
                                ..default()
                            },
                            Visibility::Hidden,
                            Pickable::IGNORE,
                        ));
                        slot.spawn((KeyLabel, label("", Font::Hud, Ink::Muted)));
                    });
            }
        });
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
        edge.set_if_neq(Edge::lit(false, hovered.get(), Edge::Clear));
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
