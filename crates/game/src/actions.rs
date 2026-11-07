//! The action buttons: a grid in the bottom-right corner of the orders the
//! selected regiments can take, with the game's icons in the places its
//! data gives them. Orders given at a place, such as Move, wait for a left
//! click on the map, with a button to cancel; right click or Escape does too.

use std::collections::HashMap;

use aaa_data::{GameData, Id};
use aaa_render::textures;
use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{Activate, Button};

use crate::{Loaded, Local, Screen, Selected, Sim};

const COLUMNS: usize = 4;
const ROWS: usize = 3;
const SLOTS: usize = COLUMNS * ROWS;
/// A button's side, in logical pixels; the icons are drawn 42 across.
const BUTTON: f32 = 56.0;
const GAP: f32 = 4.0;
/// Black margin between the buttons and the panel's rounded edge.
const PAD: f32 = 8.0;

/// The orders a regiment has, by command id, and what each does here;
/// `None` for those the sim can't carry out yet, which are shown dimmed.
/// The game leaves the rest of the commands off a regiment's grid.
const ORDERS: [(&str, Option<Give>); 7] = [
    ("move", Some(Give::Aim(Aim::Move))),
    ("kill", Some(Give::Aim(Aim::Attack))),
    ("retreat", Some(Give::Aim(Aim::Run))),
    ("stop", Some(Give::Stop)),
    ("guard", None),
    ("hq_attach", None),
    ("hq_detach", None),
];

/// Where the game puts the button that cancels an order being aimed.
const CANCEL: usize = 8;
const CANCEL_ICON: &str = "UI/Game/ActionButtons/IconCancel.png";

/// What an order does once its button is pressed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Give {
    /// Waits for a place on the map.
    Aim(Aim),
    Stop,
}

/// An order given at a place.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Aim {
    Move,
    Attack,
    Run,
}

impl Aim {
    pub const ALL: [Aim; 3] = [Aim::Move, Aim::Attack, Aim::Run];
}

/// What the grid is showing.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Page {
    #[default]
    Orders,
    /// Waiting for a click on the map to give an order there.
    Aiming(Aim),
}

/// What a button is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// Index into [`ORDERS`].
    Order(usize),
    Cancel,
}

/// The buttons a page has, by slot, given each of [`ORDERS`]' slot.
fn layout(page: Page, slots: &[Option<usize>]) -> [Option<Entry>; SLOTS] {
    let mut out = [None; SLOTS];
    match page {
        Page::Orders => {
            for (i, slot) in slots.iter().enumerate() {
                if let Some(s) = slot.filter(|&s| s < SLOTS && out[s].is_none()) {
                    out[s] = Some(Entry::Order(i));
                }
            }
        }
        Page::Aiming(aim) => {
            let aimed = ORDERS.iter().position(|(_, g)| *g == Some(Give::Aim(aim)));
            if let Some(i) = aimed
                && let Some(s) = slots[i].filter(|&s| s < SLOTS)
            {
                out[s] = Some(Entry::Order(i));
            }
            out[CANCEL] = Some(Entry::Cancel);
        }
    }
    out
}

/// How a button shows.
struct Look {
    name: String,
    icon: Option<Handle<Image>>,
}

/// The icons and names of the buttons, loaded with the match.
#[derive(Resource)]
pub struct Looks {
    /// Per [`ORDERS`], its slot and look.
    orders: Vec<(Option<usize>, Look)>,
    cancel: Option<Handle<Image>>,
}

/// The grid's buttons, at this slot.
#[derive(Component)]
pub struct Slot(usize);

#[derive(Component)]
pub struct SlotIcon;

/// The grid, whose cursor position says whether clicks are for it.
#[derive(Component)]
pub struct Grid;

/// Says what the button under the cursor is.
#[derive(Component)]
pub struct Caption;

/// What each slot holds now.
#[derive(Resource, Default)]
pub struct Shown([Option<Entry>; SLOTS]);

pub fn spawn(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &mut loaded.game;
    let mut image = |game: &mut GameData, path: &str| {
        let image = textures::decode(&game.read(path)?, path, formats.0, true)?;
        Some(images.add(image))
    };
    let commands_by_id: HashMap<Id, aaa_data::Command> = game
        .commands()
        .into_iter()
        .map(|c| (c.ids.clone(), c))
        .collect();
    let orders = ORDERS
        .iter()
        .map(|(ids, _)| match commands_by_id.get(&Id::from(*ids)) {
            Some(c) => (
                c.slot,
                Look {
                    name: c.name.clone(),
                    icon: c.icon.as_deref().and_then(|p| image(game, p)),
                },
            ),
            None => {
                warn!("no {ids} command in the game's data");
                (
                    None,
                    Look {
                        name: ids.to_string(),
                        icon: None,
                    },
                )
            }
        })
        .collect();
    let cancel = image(game, CANCEL_ICON);
    commands.insert_resource(Looks { orders, cancel });
    commands.init_resource::<Shown>();
    commands.insert_resource(Page::Orders);

    let size = |n: usize| px(n as f32 * BUTTON + (n - 1) as f32 * GAP + 2.0 * PAD);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(12),
                bottom: px(12),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::End,
                row_gap: px(4),
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(1),
            DespawnOnExit(Screen::Match),
        ))
        .with_children(|root| {
            root.spawn((
                Caption,
                Text::default(),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextShadow::default(),
            ));
            root.spawn((
                Grid,
                RelativeCursorPosition::default(),
                Node {
                    display: Display::Grid,
                    width: size(COLUMNS),
                    height: size(ROWS),
                    grid_template_columns: RepeatedGridTrack::px(COLUMNS as u16, BUTTON),
                    grid_template_rows: RepeatedGridTrack::px(ROWS as u16, BUTTON),
                    column_gap: px(GAP),
                    row_gap: px(GAP),
                    padding: UiRect::all(px(PAD)),
                    border_radius: BorderRadius::all(px(PAD * 1.5)),
                    ..default()
                },
                // Black like the minimap's disc, so the icons stand out.
                BackgroundColor(Color::BLACK),
            ))
            .with_children(|grid| {
                for s in 0..SLOTS {
                    grid.spawn((
                        Slot(s),
                        Button,
                        Hovered::default(),
                        Node {
                            border: UiRect::all(px(2)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                        BorderColor::all(Color::NONE),
                        children![(
                            SlotIcon,
                            ImageNode::default(),
                            Node {
                                width: percent(100),
                                height: percent(100),
                                ..default()
                            },
                            Pickable::IGNORE,
                        )],
                    ))
                    .observe(press);
                }
            });
        });
}

/// Whether the cursor is off the grid, so clicks go to the map.
pub fn pointer_away(
    grids: Query<(&RelativeCursorPosition, &InheritedVisibility), With<Grid>>,
) -> bool {
    grids
        .iter()
        .all(|(p, shown)| !shown.get() || !p.cursor_over)
}

pub fn aiming(page: Res<Page>) -> bool {
    matches!(*page, Page::Aiming(_))
}

/// Goes back to the orders when the selection is gone, and on Escape.
pub fn back_out(keys: Res<ButtonInput<KeyCode>>, selected: Res<Selected>, mut page: ResMut<Page>) {
    if selected.0.is_empty() || keys.just_pressed(KeyCode::Escape) {
        page.set_if_neq(Page::Orders);
    }
}

/// Fills the grid's slots for the page and the selection, and says what's
/// under the cursor.
#[allow(clippy::too_many_arguments)]
pub fn show(
    page: Res<Page>,
    selected: Res<Selected>,
    looks: Res<Looks>,
    mut shown: ResMut<Shown>,
    mut root: Query<&mut Visibility, (With<Node>, Without<Slot>)>,
    grid: Single<&ChildOf, With<Grid>>,
    mut slots: Query<
        (
            &Slot,
            &Hovered,
            &Children,
            &mut Visibility,
            &mut BorderColor,
        ),
        Without<SlotIcon>,
    >,
    mut icons: Query<&mut ImageNode, With<SlotIcon>>,
    mut caption: Single<&mut Text, With<Caption>>,
) {
    if let Ok(mut v) = root.get_mut(grid.parent()) {
        v.set_if_neq(match selected.0.is_empty() {
            false => Visibility::Inherited,
            true => Visibility::Hidden,
        });
    }
    if selected.0.is_empty() {
        return;
    }
    let order_slots: Vec<Option<usize>> = looks.orders.iter().map(|(s, _)| *s).collect();
    shown.0 = layout(*page, &order_slots);
    let mut hovered = None;
    for (slot, hover, children, mut visibility, mut border) in &mut slots {
        let entry = shown.0[slot.0];
        visibility.set_if_neq(match entry {
            Some(_) => Visibility::Inherited,
            None => Visibility::Hidden,
        });
        let Some(entry) = entry else {
            continue;
        };
        let (icon, name, ready, lit) = match entry {
            Entry::Order(i) => {
                let look = &looks.orders[i].1;
                let ready = ORDERS[i].1.is_some();
                let lit =
                    matches!((*page, ORDERS[i].1), (Page::Aiming(a), Some(Give::Aim(b))) if a == b);
                let name = match ready {
                    true => look.name.clone(),
                    false => format!("{} (not yet)", look.name),
                };
                (look.icon.clone(), name, ready, lit)
            }
            Entry::Cancel => (looks.cancel.clone(), "Cancel".into(), true, false),
        };
        if hover.get() {
            hovered = Some(name);
        }
        let colour = match (lit, hover.get() && ready) {
            (true, _) => Color::srgb(0.1, 0.9, 0.1),
            (false, true) => Color::WHITE,
            (false, false) => Color::NONE,
        };
        border.set_if_neq(BorderColor::all(colour));
        for &child in children {
            if let Ok(mut image) = icons.get_mut(child) {
                let tint = match ready {
                    true => Color::WHITE,
                    false => Color::srgb(0.35, 0.35, 0.35),
                };
                if image.image != icon.clone().unwrap_or_default() || image.color != tint {
                    image.image = icon.clone().unwrap_or_default();
                    image.color = tint;
                }
            }
        }
    }
    let text = match (hovered, *page) {
        (Some(name), _) => name,
        (None, Page::Aiming(Aim::Attack)) => "Left click where to attack".into(),
        (None, Page::Aiming(_)) => "Left click where to go".into(),
        (None, _) => String::new(),
    };
    if caption.0 != text {
        caption.0 = text;
    }
}

/// A button pressed: gives its order, aims it or cancels aiming.
#[allow(clippy::too_many_arguments)]
fn press(
    activate: On<Activate>,
    slots: Query<&Slot>,
    shown: Res<Shown>,
    selected: Res<Selected>,
    local: Res<Local>,
    mut sim: ResMut<Sim>,
    mut page: ResMut<Page>,
    time: Res<Time>,
    mut sounds: ResMut<crate::sound::Sounds>,
) {
    let Ok(slot) = slots.get(activate.entity) else {
        return;
    };
    let (Some(entry), Some(player)) = (shown.0[slot.0], local.0) else {
        return;
    };
    match entry {
        Entry::Order(i) => match ORDERS[i].1 {
            Some(Give::Aim(aim)) => *page = Page::Aiming(aim),
            Some(Give::Stop) => {
                crate::acknowledge(&sim.0, &mut sounds, &time, player, &selected.0);
                for &r in &selected.0 {
                    let _ = sim.0.order_stop(player, r);
                }
                *page = Page::Orders;
            }
            None => {}
        },
        Entry::Cancel => *page = Page::Orders,
    }
}

/// While an order is aimed, a left click on the map gives it there and a
/// right click calls it off.
#[allow(clippy::too_many_arguments)]
pub fn aim(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &aaa_render::camera::RtsCamera)>,
    loaded: Res<Loaded>,
    local: Res<Local>,
    selected: Res<Selected>,
    mut sim: ResMut<Sim>,
    mut page: ResMut<Page>,
    time: Res<Time>,
    mut sounds: ResMut<crate::sound::Sounds>,
    mut sent: MessageWriter<crate::waypoints::Sent>,
) {
    let Page::Aiming(aim) = *page else {
        return;
    };
    if buttons.just_pressed(MouseButton::Right) {
        *page = Page::Orders;
        return;
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let (Some(player), Some(point)) = (
        local.0,
        crate::pointed(&windows, &cameras, &loaded.map.ground),
    ) else {
        return;
    };
    crate::acknowledge(&sim.0, &mut sounds, &time, player, &selected.0);
    let regiments = crate::send(
        &mut sim.0,
        player,
        &selected.0,
        point,
        aim,
        loaded.map.ground.size(),
    );
    sent.write(crate::waypoints::Sent {
        to: point,
        aim,
        regiments,
    });
    *page = Page::Orders;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The slots the game's data gives [`ORDERS`].
    const GAME: [Option<usize>; 7] = [
        Some(0),
        Some(1),
        Some(2),
        Some(3),
        Some(6),
        Some(10),
        Some(11),
    ];

    #[test]
    fn orders_take_their_slots() {
        let grid = layout(Page::Orders, &GAME);
        assert_eq!(grid[0], Some(Entry::Order(0)));
        assert_eq!(grid[3], Some(Entry::Order(3)));
        assert_eq!(grid[6], Some(Entry::Order(4)));
        assert_eq!(grid[11], Some(Entry::Order(6)));
        assert_eq!(grid.iter().flatten().count(), 7);
    }

    #[test]
    fn aiming_shows_the_order_and_a_way_out() {
        let grid = layout(Page::Aiming(Aim::Run), &GAME);
        assert_eq!(grid[2], Some(Entry::Order(2)));
        assert_eq!(grid[CANCEL], Some(Entry::Cancel));
        assert_eq!(grid.iter().flatten().count(), 2);
    }
}
