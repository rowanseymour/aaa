//! The action buttons: a grid in the bottom-right corner of the orders the
//! selection can take, with the game's icons in the places its data gives
//! them. Orders given at a place, such as Move, wait for a left click on the
//! map, with a button to cancel; right click or Escape does too. A selected
//! building that commissions, such as a Corps HQ, has a button opening a
//! menu of what it commissions; [`crate::pane`] shows what it's been asked
//! for, and one to set the rally point what it commissions moves on to. A
//! building that packs, into a truck say, has Pack, dimmed while it can't.

use std::collections::HashMap;

use aaa_data::{GameData, Id};
use aaa_map::Regiment;
use aaa_render::textures;
use aaa_sim::Commission;
use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{Activate, Button};

use crate::{Loaded, Local, Orders, Raising, Screen, Selected, SelectedBuilding, Sim};

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
const ORDERS: [(&str, Option<Give>); 8] = [
    ("move", Some(Give::Aim(Aim::Move))),
    ("kill", Some(Give::Aim(Aim::Attack))),
    ("retreat", Some(Give::Aim(Aim::Run))),
    ("stop", Some(Give::Stop)),
    ("guard", None),
    ("hq_attach", None),
    ("hq_detach", None),
    ("unpack", Some(Give::Aim(Aim::Unpack))),
];

/// The command whose button opens a building's menu of what it
/// commissions.
const COMMISSION: &str = "commission";
/// The command that packs a building up, such as into a truck.
const PACK: &str = "pack";
/// The command that sets where what a building commissions moves on to.
pub const RALLY: &str = "set_rally_point";

/// Where the game puts the button that cancels an order being aimed.
const CANCEL: usize = 8;
/// Where the commission menu's way back goes.
const MENU_CANCEL: usize = 11;
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
    Unpack,
}

impl Aim {
    pub const ALL: [Aim; 4] = [Aim::Move, Aim::Attack, Aim::Run, Aim::Unpack];
}

/// What the grid is showing.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Page {
    #[default]
    Orders,
    /// Waiting for a click on the map to give an order there.
    Aiming(Aim),
    /// Waiting for a click on the map to set the selected building's
    /// rally point there.
    Rally,
    /// What the selected building commissions.
    Commission,
}

/// What a button is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// Index into [`ORDERS`].
    Order(usize),
    Cancel,
    /// Opens the commission menu.
    Commission,
    /// Index into the selected building's menu.
    Recruit(usize),
    /// Packs the selected building up.
    Pack,
    /// Waits for a place to set the selected building's rally point.
    Rally,
}

/// The buttons a selected building has: where its commission and rally
/// point buttons go and how much the menu behind the first holds, and
/// where its Pack button goes, if it packs.
#[derive(Clone, Copy, Debug)]
struct BuildingButtons {
    commission: Option<usize>,
    menu: usize,
    rally: Option<usize>,
    pack: Option<usize>,
}

/// Which of [`ORDERS`] the selection has: packed buildings move, stop and,
/// one at a time, unpack; organizations of them can also retreat and
/// guard; anything else has the rest.
fn offered(sim: &aaa_sim::World, selected: &[usize]) -> [bool; ORDERS.len()] {
    let packed = |&r: &usize| sim.regiments[r].unpacks.is_some();
    let all_packed = !selected.is_empty() && selected.iter().all(packed);
    let lone = selected.iter().all(|&r| {
        let r = &sim.regiments[r];
        matches!(&r.units[..], [u] if sim.units[*u].template == r.template)
    });
    ORDERS.map(|(ids, _)| match ids {
        "unpack" => all_packed && selected.len() == 1,
        "kill" | "hq_attach" | "hq_detach" => !all_packed,
        "retreat" | "guard" => !all_packed || !lone,
        _ => true,
    })
}

/// The buttons a page has, by slot, given each of [`ORDERS`]' slot and
/// whether the selection has it, or a selected building's buttons.
fn layout(
    page: Page,
    slots: &[Option<usize>],
    offered: &[bool],
    building: Option<BuildingButtons>,
) -> [Option<Entry>; SLOTS] {
    let mut out = [None; SLOTS];
    match (page, building) {
        (Page::Orders, Some(b)) => {
            if b.menu > 0 {
                if let Some(s) = b.commission.filter(|&s| s < SLOTS) {
                    out[s] = Some(Entry::Commission);
                }
                if let Some(s) = b.rally.filter(|&s| s < SLOTS && out[s].is_none()) {
                    out[s] = Some(Entry::Rally);
                }
            }
            if let Some(s) = b.pack.filter(|&s| s < SLOTS && out[s].is_none()) {
                out[s] = Some(Entry::Pack);
            }
        }
        (Page::Commission, Some(BuildingButtons { menu, .. })) => {
            for (i, slot) in out.iter_mut().take(menu.min(MENU_CANCEL)).enumerate() {
                *slot = Some(Entry::Recruit(i));
            }
            out[MENU_CANCEL] = Some(Entry::Cancel);
        }
        (Page::Rally, Some(b)) => {
            if let Some(s) = b.rally.filter(|&s| s < SLOTS && s != CANCEL) {
                out[s] = Some(Entry::Rally);
            }
            out[CANCEL] = Some(Entry::Cancel);
        }
        (Page::Commission | Page::Rally, None) => {}
        (Page::Orders, None) => {
            for (i, slot) in slots.iter().enumerate() {
                if !offered[i] {
                    continue;
                }
                if let Some(s) = slot.filter(|&s| s < SLOTS && out[s].is_none()) {
                    out[s] = Some(Entry::Order(i));
                }
            }
        }
        (Page::Aiming(aim), _) => {
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
#[derive(Clone)]
struct Look {
    name: String,
    icon: Option<Handle<Image>>,
}

/// One of the things a building commissions.
struct Recruit {
    look: Look,
    cost: f32,
    /// What it raises; `None` for what can't be raised yet, shown dimmed.
    regiment: Option<Regiment>,
    seconds: f32,
}

/// The icons and names of the buttons, loaded with the match.
#[derive(Resource)]
pub struct Looks {
    /// Per [`ORDERS`], its slot and look.
    orders: Vec<(Option<usize>, Look)>,
    commission: (Option<usize>, Look),
    rally: (Option<usize>, Look),
    pack: (Option<usize>, Look),
    cancel: Option<Handle<Image>>,
    /// What each of the map's building templates commissions, in its
    /// buttons' order.
    menus: HashMap<String, Vec<Recruit>>,
}

impl Looks {
    /// The menu of what the selected building commissions.
    fn menu(&self, sim: &aaa_sim::World, building: Option<usize>) -> &[Recruit] {
        building
            .and_then(|b| self.menus.get(&sim.buildings[b].template))
            .map_or(&[], Vec::as_slice)
    }
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

/// Part of the interface that takes clicks, so they don't go to the map
/// under it.
#[derive(Component)]
pub struct Blocks;

/// What each slot holds now.
#[derive(Resource, Default)]
pub struct Shown([Option<Entry>; SLOTS]);

pub fn spawn(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    raising: Res<Raising>,
    sim: Res<Sim>,
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
    let mut look_of = |game: &mut GameData, ids: &str| match commands_by_id.get(&Id::from(ids)) {
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
    };
    let orders = ORDERS.iter().map(|(ids, _)| look_of(game, ids)).collect();
    let commission = look_of(game, COMMISSION);
    let rally = look_of(game, RALLY);
    let pack = look_of(game, PACK);
    let cancel = image(game, CANCEL_ICON);
    let listings = game.listings();
    // Buildings unpacked during the match need menus too: those of the
    // map's trucks, and of what any menu commissions.
    let mut menus = HashMap::new();
    let mut templates: Vec<String> = sim.0.buildings.iter().map(|b| b.template.clone()).collect();
    templates.extend(
        sim.0
            .regiments
            .iter()
            .filter_map(|r| Some(r.unpacks.as_ref()?.building.template.clone())),
    );
    while let Some(template) = templates.pop() {
        if menus.contains_key(&template) {
            continue;
        }
        let list = raising.0.list(&template);
        let menu: Vec<Recruit> = list
            .iter()
            .map(|id| {
                let listing = listings.get(id);
                let regiment = raising
                    .0
                    .raise(id.as_str(), 0, 0, [0.0, 0.0], [0.0, -1.0])
                    .map_err(|e| debug!("can't commission {id}: {e}"))
                    .ok();
                Recruit {
                    look: Look {
                        name: listing.map_or_else(|| id.to_string(), |l| l.name.clone()),
                        icon: listing
                            .and_then(|l| l.icon.as_deref())
                            .and_then(|p| image(game, p)),
                    },
                    cost: listing.map_or(0.0, |l| l.cost),
                    regiment,
                    seconds: raising.0.recruit_time(id.as_str()),
                }
            })
            .collect();
        templates.extend(menu.iter().filter_map(|r| {
            Some(
                r.regiment
                    .as_ref()?
                    .unpacks
                    .as_ref()?
                    .building
                    .template
                    .clone(),
            )
        }));
        menus.insert(template, menu);
    }
    commands.insert_resource(Looks {
        orders,
        commission,
        rally,
        pack,
        cancel,
        menus,
    });
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
                Blocks,
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
    grids: Query<(&RelativeCursorPosition, &InheritedVisibility), With<Blocks>>,
) -> bool {
    grids
        .iter()
        .all(|(p, shown)| !shown.get() || !p.cursor_over)
}

pub fn aiming(page: Res<Page>) -> bool {
    matches!(*page, Page::Aiming(_) | Page::Rally)
}

/// Goes back to the orders on Escape, and when what the page is for is
/// no longer selected.
pub fn back_out(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    mut page: ResMut<Page>,
) {
    let gone = match *page {
        Page::Orders => false,
        Page::Aiming(_) => selected.0.is_empty(),
        Page::Commission | Page::Rally => chosen.0.is_none(),
    };
    if gone || keys.just_pressed(KeyCode::Escape) {
        page.set_if_neq(Page::Orders);
    }
}

/// The grid's panel, its root.
type PanelFilter = (With<Node>, Without<Slot>);

type SlotParts<'a> = (
    &'a Slot,
    &'a Hovered,
    &'a Children,
    &'a mut Visibility,
    &'a mut BorderColor,
);

/// Fills the grid's slots for the page and the selection, and says what's
/// under the cursor.
#[allow(clippy::too_many_arguments)]
pub fn show(
    page: Res<Page>,
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    sim: Res<Sim>,
    looks: Res<Looks>,
    mut shown: ResMut<Shown>,
    mut root: Query<&mut Visibility, PanelFilter>,
    grid: Single<&ChildOf, With<Grid>>,
    mut slots: Query<SlotParts, Without<SlotIcon>>,
    mut icons: Query<&mut ImageNode, With<SlotIcon>>,
    mut caption: Single<&mut Text, With<Caption>>,
) {
    let building = chosen.0.filter(|_| selected.0.is_empty());
    let nothing = selected.0.is_empty() && building.is_none();
    if let Ok(mut v) = root.get_mut(grid.parent()) {
        v.set_if_neq(match nothing {
            false => Visibility::Inherited,
            true => Visibility::Hidden,
        });
    }
    if nothing {
        return;
    }
    let menu = looks.menu(&sim.0, building);
    let order_slots: Vec<Option<usize>> = looks.orders.iter().map(|(s, _)| *s).collect();
    shown.0 = layout(
        *page,
        &order_slots,
        &offered(&sim.0, &selected.0),
        building.map(|b| BuildingButtons {
            commission: looks.commission.0,
            menu: menu.len(),
            rally: looks.rally.0,
            pack: looks
                .pack
                .0
                .filter(|_| sim.0.packing.contains_key(&sim.0.buildings[b].template)),
        }),
    );
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
                (look.icon.clone(), look.name.clone(), ready, lit)
            }
            Entry::Cancel => (looks.cancel.clone(), "Cancel".into(), true, false),
            Entry::Commission => {
                let look = &looks.commission.1;
                (look.icon.clone(), look.name.clone(), true, false)
            }
            Entry::Rally => {
                let look = &looks.rally.1;
                (
                    look.icon.clone(),
                    look.name.clone(),
                    true,
                    *page == Page::Rally,
                )
            }
            Entry::Pack => {
                let look = &looks.pack.1;
                let ready = building.is_some_and(|b| can_pack(&sim.0, b));
                (look.icon.clone(), look.name.clone(), ready, false)
            }
            Entry::Recruit(i) => {
                let r = &menu[i];
                let name = format!("{} ({} money, {:.0} s)", r.look.name, r.cost, r.seconds);
                (r.look.icon.clone(), name, r.regiment.is_some(), false)
            }
        };
        if hover.get() {
            hovered = Some(match ready {
                true => name,
                false => format!("{name} (not yet)"),
            });
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
        (None, Page::Aiming(Aim::Unpack)) => "Left click where to unpack".into(),
        (None, Page::Aiming(Aim::Attack)) => "Left click an enemy or where to attack".into(),
        (None, Page::Aiming(_)) => "Left click where to go".into(),
        (None, Page::Rally) => "Left click where new regiments go".into(),
        (None, _) => String::new(),
    };
    if caption.0 != text {
        caption.0 = text;
    }
}

/// A button pressed: gives its order, aims it, opens or works the
/// commission menu, or cancels.
#[allow(clippy::too_many_arguments)]
fn press(
    activate: On<Activate>,
    slots: Query<&Slot>,
    shown: Res<Shown>,
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    looks: Res<Looks>,
    local: Res<Local>,
    sim: Res<Sim>,
    mut orders: ResMut<Orders>,
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
                for &regiment in &selected.0 {
                    let _ = orders.give(&sim.0, player, aaa_sim::Command::Stop { regiment });
                }
                *page = Page::Orders;
            }
            None => {}
        },
        Entry::Cancel => *page = Page::Orders,
        Entry::Commission => *page = Page::Commission,
        Entry::Rally => *page = Page::Rally,
        Entry::Pack => {
            if let Some(b) = chosen.0
                && let Err(e) = orders.give(&sim.0, player, aaa_sim::Command::Pack { building: b })
            {
                info!("can't pack {}: {e:?}", sim.0.buildings[b].template);
            }
        }
        Entry::Recruit(i) => {
            let Some(b) = chosen.0 else {
                return;
            };
            let recruit = &looks.menu(&sim.0, Some(b))[i];
            if let Some(regiment) = &recruit.regiment {
                let command = aaa_sim::Command::Commission {
                    building: b,
                    commission: Box::new(Commission::new(regiment.clone(), recruit.seconds)),
                };
                if let Err(e) = orders.give(&sim.0, player, command) {
                    info!("can't commission {}: {e:?}", regiment.template);
                }
            }
        }
    }
}

/// Whether a building can pack now: not while it's packing or unpacking,
/// nor when it's too hurt.
fn can_pack(sim: &aaa_sim::World, building: usize) -> bool {
    let b = &sim.buildings[building];
    let floor = sim.packing.get(&b.template).map_or(1.0, |p| p.health_min);
    !b.busy() && b.health > floor * b.fighter.health
}

/// While an order is aimed, a left click on the map gives it there and a
/// right click calls it off. Attacking an enemy regiment sends the
/// selected after it instead. Unpacking somewhere the building wouldn't
/// fit waits for another click. A rally point is set for the selected
/// building.
#[allow(clippy::too_many_arguments)]
pub fn aim(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &aaa_render::camera::RtsCamera)>,
    loaded: Res<Loaded>,
    local: Res<Local>,
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    focus: Res<crate::Focus>,
    looks: Res<crate::waypoints::Looks>,
    sim: Res<Sim>,
    mut orders: ResMut<Orders>,
    mut page: ResMut<Page>,
    time: Res<Time>,
    mut sounds: ResMut<crate::sound::Sounds>,
    mut sent: MessageWriter<crate::waypoints::Sent>,
) {
    if !matches!(*page, Page::Aiming(_) | Page::Rally) {
        return;
    }
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
    let Page::Aiming(aim) = *page else {
        if let Some(b) = chosen.0
            && let Err(e) = orders.give(
                &sim.0,
                player,
                aaa_sim::Command::Rally {
                    building: b,
                    at: Some(point),
                },
            )
        {
            info!("can't rally {}: {e:?}", sim.0.buildings[b].template);
        }
        *page = Page::Orders;
        return;
    };
    if let Some(e) = crate::enemy_at(&sim.0, player, *focus).filter(|_| aim == Aim::Attack) {
        crate::acknowledge(&sim.0, &mut sounds, &time, player, &selected.0);
        let (regiments, _) = crate::pursue(&sim.0, &mut orders, player, &selected.0, e);
        sent.write(crate::waypoints::Sent {
            to: None,
            aim,
            regiments,
        });
        *page = Page::Orders;
        return;
    }
    let regiments = match aim {
        Aim::Unpack => {
            let &[r] = &selected.0[..] else {
                return;
            };
            let unpack = aaa_sim::Command::Unpack {
                regiment: r,
                at: point,
            };
            if orders.give(&sim.0, player, unpack).is_err() {
                return;
            }
            let at = aaa_sim::unpack_site(point);
            crate::acknowledge(&sim.0, &mut sounds, &time, player, &selected.0);
            vec![(r, at)]
        }
        Aim::Move | Aim::Attack | Aim::Run => {
            crate::acknowledge(&sim.0, &mut sounds, &time, player, &selected.0);
            crate::send(
                &sim.0,
                &mut orders,
                player,
                &selected.0,
                point,
                (aim, looks.together(aim)),
                loaded.map.ground.size(),
            )
        }
    };
    sent.write(crate::waypoints::Sent {
        to: Some(point),
        aim,
        regiments,
    });
    *page = Page::Orders;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The slots the game's data gives [`ORDERS`].
    const GAME: [Option<usize>; 8] = [
        Some(0),
        Some(1),
        Some(2),
        Some(3),
        Some(6),
        Some(10),
        Some(11),
        Some(9),
    ];

    #[test]
    fn orders_take_their_slots() {
        let all = [true; 8];
        let grid = layout(Page::Orders, &GAME, &all, None);
        assert_eq!(grid[0], Some(Entry::Order(0)));
        assert_eq!(grid[3], Some(Entry::Order(3)));
        assert_eq!(grid[6], Some(Entry::Order(4)));
        assert_eq!(grid[11], Some(Entry::Order(6)));
        assert_eq!(grid.iter().flatten().count(), 8);
        let truck = [true, false, false, true, false, false, false, true];
        let grid = layout(Page::Orders, &GAME, &truck, None);
        assert_eq!(grid[9], Some(Entry::Order(7)));
        assert_eq!(grid.iter().flatten().count(), 3);
    }

    #[test]
    fn aiming_shows_the_order_and_a_way_out() {
        let grid = layout(Page::Aiming(Aim::Run), &GAME, &[true; 8], None);
        assert_eq!(grid[2], Some(Entry::Order(2)));
        assert_eq!(grid[CANCEL], Some(Entry::Cancel));
        assert_eq!(grid.iter().flatten().count(), 2);
    }

    #[test]
    fn buildings_open_a_menu_of_what_they_commission() {
        let hq = BuildingButtons {
            commission: Some(0),
            menu: 11,
            rally: Some(5),
            pack: Some(9),
        };
        let grid = layout(Page::Orders, &GAME, &[true; 8], Some(hq));
        assert_eq!(grid[0], Some(Entry::Commission));
        assert_eq!(grid[5], Some(Entry::Rally));
        assert_eq!(grid[9], Some(Entry::Pack));
        assert_eq!(grid.iter().flatten().count(), 3);
        let depot = BuildingButtons {
            menu: 0,
            pack: None,
            ..hq
        };
        assert!(
            layout(Page::Orders, &GAME, &[true; 8], Some(depot))
                .iter()
                .all(Option::is_none)
        );
        let menu = layout(Page::Commission, &GAME, &[true; 8], Some(hq));
        assert_eq!(menu[10], Some(Entry::Recruit(10)));
        assert_eq!(menu[MENU_CANCEL], Some(Entry::Cancel));
        let rally = layout(Page::Rally, &GAME, &[true; 8], Some(hq));
        assert_eq!(rally[5], Some(Entry::Rally));
        assert_eq!(rally[CANCEL], Some(Entry::Cancel));
        assert_eq!(rally.iter().flatten().count(), 2);
    }
}
