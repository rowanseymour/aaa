//! Our command grid in the window's lower right corner: the shared
//! [`Shown`] slots as buttons, with the tooltip [`Said`] in a box above.

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::ecs::system::SystemParam;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::Button;

use crate::actions::{
    COLUMNS, Looks, Page, Quantity, ROWS, SLOTS, Said, Shown, Slot, Tip, builds, lit, look_of,
    widget,
};
use crate::intent::Asks;
use crate::ui::{self, Blocks, Click, Edge, Font, Ink, Shade, Surface, label, panel, tooltip};
use crate::{Local, Selected, SelectedBuilding, Sim};

/// A button's side, in logical pixels; the icons are drawn 42 across.
pub const BUTTON: f32 = 56.0;
pub const GAP: f32 = 4.0;

/// Black margin between the buttons and the panel's rounded edge.
pub const PAD: f32 = 8.0;

/// The grid's size, in logical pixels.
pub const SIZE: Vec2 = Vec2::new(span(COLUMNS), span(ROWS));

/// Across `n` buttons and the padding either side.
const fn span(n: usize) -> f32 {
    n as f32 * BUTTON + (n - 1) as f32 * GAP + 2.0 * PAD
}

#[derive(Component)]
pub struct SlotIcon;

/// The panel holding the caption and the grid.
#[derive(Component)]
pub struct Panel;

/// The grid, whose cursor position says whether clicks are for it.
#[derive(Component)]
pub struct Grid;

/// The box above the grid saying what the button under the cursor is.
#[derive(Component)]
pub struct Tooltip;

/// Spawns our interface's command grid, in the window's lower right
/// corner, with the tooltip above it.
pub fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Panel,
            Node {
                position_type: PositionType::Absolute,
                right: px(crate::MARGIN),
                bottom: px(crate::MARGIN),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::End,
                row_gap: px(PAD),
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(crate::ui::layer::HUD),
            DespawnOnExit(super::DRAWN),
        ))
        .with_children(|root| {
            // As wide as the grid, which the object display is kept clear of, and as
            // tall as its text wraps to, growing upwards.
            root.spawn((
                Tooltip,
                tooltip(Node {
                    display: Display::None,
                    width: px(SIZE.x),
                    ..default()
                }),
            ));
            root.spawn((
                Grid,
                Blocks,
                RelativeCursorPosition::default(),
                panel(
                    Surface::Hud,
                    Node {
                        display: Display::Grid,
                        width: px(SIZE.x),
                        height: px(SIZE.y),
                        grid_template_columns: RepeatedGridTrack::px(COLUMNS as u16, BUTTON),
                        grid_template_rows: RepeatedGridTrack::px(ROWS as u16, BUTTON),
                        column_gap: px(GAP),
                        row_gap: px(GAP),
                        padding: UiRect::all(px(PAD)),
                        ..default()
                    },
                ),
            ))
            .with_children(|grid| {
                for s in 0..SLOTS {
                    grid.spawn((
                        Slot(s),
                        Button,
                        ui::called(format!("action.{}", s + 1)),
                        Asks::new(widget(s)),
                        Hovered::default(),
                        Pickable::default(),
                        Click::COMMAND,
                        Node {
                            border: UiRect::all(px(2)),
                            ..default()
                        },
                        Surface::Slot,
                        Edge::Clear,
                        children![(
                            SlotIcon,
                            Shade::Full,
                            Node {
                                width: percent(100),
                                height: percent(100),
                                ..default()
                            },
                            Pickable::IGNORE,
                        )],
                    ));
                }
            });
        });
}

type SlotParts<'a> = (
    Entity,
    Has<Click>,
    &'a Slot,
    &'a Hovered,
    &'a Children,
    &'a mut Edge,
    &'a mut Pickable,
);

/// Draws the grid's slots as [`fill_slots`](crate::actions::fill_slots) filled them, and in
/// its box what [`tell`](crate::actions::tell) has the tooltip say.
#[allow(clippy::too_many_arguments)]
pub fn show(
    page: Res<Page>,
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    sim: Res<Sim>,
    looks: Res<Looks>,
    shown: Res<Shown>,
    said: Res<Said>,
    mut panel: Single<&mut Visibility, (With<Panel>, Without<SlotIcon>)>,
    mut slots: Query<SlotParts, Without<SlotIcon>>,
    mut icons: Query<(&mut ImageNode, &mut Shade, &mut Visibility), With<SlotIcon>>,
    mut tooltip: TipParts,
    local: Res<Local>,
    mut commands: Commands,
) {
    let building = chosen.0.filter(|_| selected.0.is_empty());
    // Others' things have no buttons.
    let nothing = !crate::selection::commanded(&sim.0, local.0, &selected.0, building);
    panel.set_if_neq(ui::visible(!nothing));
    if nothing {
        return;
    }
    let builds = builds(&sim.0, &selected.0);
    for (id, clicks, slot, hover, children, mut border, mut pickable) in &mut slots {
        let entry = shown.entries[slot.0];
        // Empty slots are just boxes, which nothing should hear clicks on.
        let want = match entry {
            Some(_) => Pickable::default(),
            None => Pickable::IGNORE,
        };
        if *pickable != want {
            *pickable = want;
        }
        let Some(entry) = entry else {
            border.set_if_neq(Edge::Clear);
            set_icon(children, &mut icons, None);
            continue;
        };
        let look = look_of(entry, &sim.0, &looks, building, &builds);
        let ready = shown.faces[slot.0].as_ref().is_some_and(|f| f.ready);
        let lit = lit(entry, *page);
        let icon = look.icon.clone();
        // Only a press that can do something sounds like it.
        match (ready, clicks) {
            (true, false) => {
                commands.entity(id).insert(Click::COMMAND);
            }
            (false, true) => {
                commands.entity(id).remove::<Click>();
            }
            _ => {}
        }
        border.set_if_neq(Edge::lit(lit, hover.get() && ready, Edge::Clear));
        let shade = match ready {
            true => Shade::Full,
            false => Shade::Dimmed,
        };
        set_icon(children, &mut icons, icon.map(|i| (i, shade)));
    }
    tooltip.say(said.0.clone());
}

/// The tooltip's node, not the slots'.
type TooltipOnly = (With<Tooltip>, Without<Slot>);

/// The tooltip's box and what it last said.
#[derive(SystemParam)]
pub struct TipParts<'w, 's> {
    tooltip: Single<'w, 's, (Entity, &'static mut Node, Ref<'static, Tooltip>), TooltipOnly>,
    said: bevy::ecs::system::Local<'s, Tip>,
    commands: Commands<'w, 's>,
}

impl TipParts<'_, '_> {
    /// Shows the tip, or hides the box for none.
    fn say(&mut self, tip: Option<Tip>) {
        let (entity, node, fresh) = &mut *self.tooltip;
        ui::show(node, tip.is_some());
        let tip = tip.unwrap_or_default();
        // What it last said was in the box before this one.
        if *self.said == tip && !fresh.is_added() {
            return;
        }
        let mut tooltip = self.commands.entity(*entity);
        tooltip.despawn_children();
        tooltip.with_children(|parent| {
            ui::marked(parent, &tip.title, Font::HudTitle, Ink::Body);
            if let Some(help) = &tip.help {
                parent.spawn(label(help.clone(), Font::Small, Ink::Help));
            }
            if !tip.table.is_empty() {
                parent
                    .spawn(Node {
                        display: Display::Grid,
                        grid_template_columns: vec![GridTrack::auto(), GridTrack::flex(1.0)],
                        column_gap: px(PAD * 2.0),
                        row_gap: px(2),
                        ..default()
                    })
                    .with_children(|table| {
                        for row in &tip.table {
                            table.spawn(label(row.label.clone(), Font::Small, Ink::Help));
                            table
                                .spawn(Node {
                                    column_gap: px(PAD),
                                    align_items: AlignItems::Center,
                                    ..default()
                                })
                                .with_children(|cell| {
                                    for q in &row.amounts {
                                        quantity(cell, q);
                                    }
                                });
                        }
                    });
            }
            for note in &tip.notes {
                parent.spawn(label(note.clone(), Font::Small, Ink::Help));
            }
            if let Some(why) = &tip.why {
                parent.spawn(label(why.clone(), Font::Small, Ink::Warn));
            }
        });
        *self.said = tip;
    }
}

/// An amount after its resource's glyph, as the game's tooltips show it,
/// or before its name where there's no glyph.
fn quantity(cell: &mut ChildSpawnerCommands, q: &Quantity) {
    cell.spawn(Node {
        column_gap: px(2),
        align_items: AlignItems::Center,
        ..default()
    })
    .with_children(|pair| match &q.glyph {
        Some(glyph) => {
            pair.spawn((
                ImageNode::new(glyph.clone()),
                Node {
                    width: px(16),
                    height: px(16),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            pair.spawn(label(q.number(), Font::Small, Ink::Body));
        }
        None => {
            pair.spawn(label(
                format!("{} {}", q.number(), q.resource),
                Font::Small,
                Ink::Body,
            ));
        }
    });
}

/// Shows a slot's icon in this shade, or hides it, say for an empty slot
/// or a command the data gives no icon, which would otherwise draw white.
fn set_icon(
    children: &Children,
    icons: &mut Query<(&mut ImageNode, &mut Shade, &mut Visibility), With<SlotIcon>>,
    icon: Option<(Handle<Image>, Shade)>,
) {
    for &child in children {
        let Ok((mut image, mut shade, mut visibility)) = icons.get_mut(child) else {
            continue;
        };
        visibility.set_if_neq(match icon {
            Some(_) => Visibility::Inherited,
            None => Visibility::Hidden,
        });
        if let Some((handle, want)) = icon.clone() {
            if image.image != handle {
                image.image = handle;
            }
            shade.set_if_neq(want);
        }
    }
}
