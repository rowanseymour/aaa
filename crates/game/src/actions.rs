//! The action buttons: a grid in the bottom-right corner of the orders the
//! selection can take, with the game's icons in the places its data gives
//! them. Orders given at a place, such as Move, wait for a left click on the
//! map or minimap, with a button to cancel; right click or Escape does too.
//! A selected building that recruits, such as a Corps HQ, has Deploy, opening
//! a menu of what comes out of it, and Deploy to Location, of what a
//! transport carries to where it's sent, each while that menu offers
//! anything; [`crate::pane`] shows what it's been asked for, and a button
//! sets the rally point what it recruits moves on to. A building that packs, into a truck say, has Pack, dimmed while
//! it can't. Each button answers to the keys the game binds what it does
//! to, and its tooltip marks the letter.

use std::collections::HashMap;

use aaa_data::{GameData, Id};
use aaa_map::Regiment;
use aaa_render::textures;
use aaa_sim::{Commission, Offer};
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{Activate, Button};

use crate::keys::Bindings;
use crate::resource_bar::{Preview, Previewed};
use crate::ui::{self, Click, Edge, Font, Ink, Marked, Shade, Surface, label, panel, tooltip};
use crate::{Loaded, Local, Orders, Raising, Screen, Selected, SelectedBuilding, Sim};

const COLUMNS: usize = 4;
const ROWS: usize = 3;
const SLOTS: usize = COLUMNS * ROWS;
/// A button's side, in logical pixels; the icons are drawn 42 across.
const BUTTON: f32 = 56.0;
const GAP: f32 = 4.0;
/// Black margin between the buttons and the panel's rounded edge.
const PAD: f32 = 8.0;
/// The grid's size, in logical pixels.
pub const SIZE: Vec2 = Vec2::new(span(COLUMNS), span(ROWS));

/// Across `n` buttons and the padding either side.
const fn span(n: usize) -> f32 {
    n as f32 * BUTTON + (n - 1) as f32 * GAP + 2.0 * PAD
}

/// The orders a regiment has, by command id, and what each does here;
/// `None` for those the sim can't carry out yet, which are shown dimmed.
/// The game leaves the rest of the commands off a regiment's grid.
const ORDERS: [(&str, Option<Give>); 8] = [
    ("move", Some(Give::Aim(Aim::Move))),
    ("kill", Some(Give::Aim(Aim::Attack))),
    ("retreat", Some(Give::Aim(Aim::Run))),
    ("stop", Some(Give::Stop)),
    ("guard", None),
    ("hq_attach", Some(Give::Attach)),
    ("hq_detach", Some(Give::Detach)),
    ("unpack", Some(Give::Aim(Aim::Unpack))),
];

/// The commands whose buttons open a recruiter's two menus.
const DEPLOY: &str = "recruit";
const DEPLOY_TO_LOCATION: &str = "recruit_location";
/// The command that packs a building up, such as into a truck.
const PACK: &str = "pack";
/// The command that sets where what a building commissions moves on to.
pub const RALLY: &str = "set_rally_point";

/// Where the game puts an HQ's button that selects the regiments attached
/// to it, the image it shows, and its strings' keys.
const COMMAND_ALL: usize = 4;
const COMMAND_ALL_ICON: &str = "UI/Game/ActionButtons/IconCommandAll.png";
const COMMAND_ALL_NAME: &str = "action_buttons_CommandAll_name";
const COMMAND_ALL_HELP: &str = "action_buttons_CommandAll_tooltip";

/// Where the game puts the button that cancels an order being aimed.
const CANCEL: usize = 8;
/// Where the commission menu's way back goes.
const MENU_CANCEL: usize = 11;
/// The command whose look the cancel button borrows, as the game gives
/// none of its own for backing out of an order or a menu.
const CANCEL_LOOK: &str = "construction_cancel";
/// Where the game's font keeps the glyphs it draws resources with, and
/// each resource's, by its field name in the data.
pub(crate) const GLYPH_DIR: &str = "Fonts/medium/1280";
pub(crate) const GLYPHS: [(&str, &str); 7] = [
    ("money", "money"),
    ("ammo", "ammo"),
    ("oil", "fuel"),
    ("unit_limit_consumed", "unitlimit"),
    ("building_limit_consumed", "building"),
    ("unit_limit_provided", "unitlimit"),
    ("building_limit_provided", "building"),
];

/// What an order does once its button is pressed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Give {
    /// Waits for a place on the map.
    Aim(Aim),
    Stop,
    Attach,
    Detach,
}

impl Give {
    /// The order to attach or detach `regiment`.
    fn command(self, regiment: usize) -> aaa_sim::Command {
        match self {
            Give::Attach => aaa_sim::Command::Attach { regiment },
            _ => aaa_sim::Command::Detach { regiment },
        }
    }
}

/// The way buildings the player unpacks face, for the order and its
/// preview alike.
pub const UNPACK_FACING: [f32; 2] = aaa_map::UNPACKED_FACING;

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
    /// One of the selected building's menus of what it recruits.
    Deploy(Menu),
}

/// Which of a recruiter's menus something it recruits is in: Deploy for
/// what comes out of it, Deploy to Location for what names a transport.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    Deploy,
    Location,
}

/// What a button is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// Index into [`ORDERS`].
    Order(usize),
    Cancel,
    Open(Menu),
    /// Index into everything the selected building recruits.
    Recruit(usize),
    /// Packs the selected building up.
    Pack,
    /// Waits for a place to set the selected building's rally point.
    Rally,
    /// Selects the regiments attached to the selected HQ.
    CommandAll,
}

/// The buttons a selected building has: where its menus' and rally point
/// buttons go, which menu each thing it recruits is in and how it's
/// offered, and where its Pack button goes, if it packs.
#[derive(Clone, Copy, Debug)]
struct BuildingButtons<'a> {
    deploy: Option<usize>,
    location: Option<usize>,
    recruits: &'a [(Menu, Offer)],
    rally: Option<usize>,
    pack: Option<usize>,
}

impl BuildingButtons<'_> {
    /// Whether `menu` has anything to show.
    fn offers(&self, menu: Menu) -> bool {
        (self.recruits.iter()).any(|&(m, offer)| m == menu && offer != Offer::Hidden)
    }
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
    hq: bool,
) -> [Option<Entry>; SLOTS] {
    let mut out = [None; SLOTS];
    match (page, building) {
        (Page::Orders, Some(b)) => {
            for (menu, slot) in [(Menu::Deploy, b.deploy), (Menu::Location, b.location)] {
                if b.offers(menu)
                    && let Some(s) = slot.filter(|&s| s < SLOTS && out[s].is_none())
                {
                    out[s] = Some(Entry::Open(menu));
                }
            }
            if (b.offers(Menu::Deploy) || b.offers(Menu::Location))
                && let Some(s) = b.rally.filter(|&s| s < SLOTS && out[s].is_none())
            {
                out[s] = Some(Entry::Rally);
            }
            if let Some(s) = b.pack.filter(|&s| s < SLOTS && out[s].is_none()) {
                out[s] = Some(Entry::Pack);
            }
        }
        (Page::Deploy(menu), Some(b)) => {
            // The other menu's take no place; hidden ones keep theirs empty.
            let ours = (b.recruits.iter().enumerate()).filter(|(_, (m, _))| *m == menu);
            for ((i, &(_, offer)), slot) in ours.zip(&mut out[..MENU_CANCEL]) {
                if offer != Offer::Hidden {
                    *slot = Some(Entry::Recruit(i));
                }
            }
            out[MENU_CANCEL] = Some(Entry::Cancel);
        }
        (Page::Rally, Some(b)) => {
            if let Some(s) = b.rally.filter(|&s| s < SLOTS && s != CANCEL) {
                out[s] = Some(Entry::Rally);
            }
            out[CANCEL] = Some(Entry::Cancel);
        }
        (Page::Deploy(_) | Page::Rally, None) => {}
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
    if hq && page == Page::Orders && out[COMMAND_ALL].is_none() {
        out[COMMAND_ALL] = Some(Entry::CommandAll);
    }
    out
}

/// How a button shows.
#[derive(Clone)]
struct Look {
    name: String,
    /// What it does, and why it can't be pressed when it can't.
    help: Option<String>,
    disabled: Option<String>,
    icon: Option<Handle<Image>>,
}

/// One of the things a building commissions.
struct Recruit {
    /// Its template, as the hotkeys name it.
    id: String,
    menu: Menu,
    look: Look,
    /// Its cost, production and upkeep, for the tooltip.
    economy: Vec<Row>,
    /// What it raises; `None` for what can't be raised yet, shown dimmed.
    regiment: Option<Regiment>,
    seconds: f32,
}

/// The icons and names of the buttons, loaded with the match.
#[derive(Resource)]
pub struct Looks {
    /// Per [`ORDERS`], its slot and look.
    orders: Vec<(Option<usize>, Look)>,
    deploy: (Option<usize>, Look),
    location: (Option<usize>, Look),
    rally: (Option<usize>, Look),
    pack: (Option<usize>, Look),
    cancel: Look,
    command_all: Look,
    /// The tooltip's heading for what a recruit needs.
    requires: String,
    /// What a recruit the player can't afford says, given its name and
    /// cost.
    affording: String,
    /// What each of the map's building templates commissions, in its
    /// buttons' order.
    menus: HashMap<String, Vec<Recruit>>,
    /// What each template is called, for saying which buildings a
    /// commission needs.
    names: HashMap<Id, String>,
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

/// The panel holding the caption and the grid.
#[derive(Component)]
pub struct Panel;

/// The grid, whose cursor position says whether clicks are for it.
#[derive(Component)]
pub struct Grid;

/// The box above the grid saying what the button under the cursor is.
#[derive(Component)]
pub struct Tooltip;

/// What the tooltip says: the button's name, what it does, a table of
/// such as what it costs, and why it can't be pressed.
#[derive(Clone, PartialEq, Default, Debug)]
struct Tip {
    /// The button's name, with the key that presses it marked.
    title: Marked,
    help: Option<String>,
    table: Vec<Row>,
    why: Option<String>,
}

/// A row of the tooltip's table, such as Cost and what it is.
#[derive(Clone, PartialEq, Debug)]
struct Row {
    label: String,
    amounts: Vec<Quantity>,
}

/// So much of a resource, shown by its glyph, or its name without one.
#[derive(Clone, PartialEq, Debug)]
struct Quantity {
    glyph: Option<Handle<Image>>,
    amount: f32,
    /// Shown signed, as what's earned is.
    earned: bool,
    resource: String,
}

impl Quantity {
    fn number(&self) -> String {
        if self.earned && self.amount > 0.0 {
            format!("+{}", self.amount)
        } else {
            self.amount.to_string()
        }
    }
}

/// Part of the interface that takes clicks, so they don't go to the map
/// under it.
#[derive(Component)]
pub struct Blocks;

/// What each slot holds now.
#[derive(Resource, Default)]
pub struct Shown([Option<Entry>; SLOTS]);

/// The slot whose press was just refused and why, said while the cursor
/// stays on it.
#[derive(Resource, Default)]
pub struct Refusal(Option<(usize, aaa_sim::Refused)>);

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
                help: c.tooltip.clone(),
                disabled: c.disabled.clone(),
                icon: c.icon.as_deref().and_then(|p| image(game, p)),
            },
        ),
        None => {
            warn!("no {ids} command in the game's data");
            (
                None,
                Look {
                    name: ids.to_string(),
                    help: None,
                    disabled: None,
                    icon: None,
                },
            )
        }
    };
    let orders = ORDERS.iter().map(|(ids, _)| look_of(game, ids)).collect();
    let deploy = look_of(game, DEPLOY);
    let location = look_of(game, DEPLOY_TO_LOCATION);
    let transports = game.transports();
    let rally = look_of(game, RALLY);
    let pack = look_of(game, PACK);
    let cancel = look_of(game, CANCEL_LOOK).1;
    let strings = game.strings();
    let title = |key: &str, or: &str| strings.get(key).map_or(or, String::as_str).to_string();
    let cost = title("ui_tooltip_item_title_cost", "Cost");
    let production = title("ui_tooltip_item_title_production", "Production");
    let upkeep = title("ui_tooltip_item_title_upkeep", "Upkeep");
    let requires = title("ui_tooltip_item_title_requires", "Requires");
    let command_all = Look {
        name: title(COMMAND_ALL_NAME, "Command All"),
        help: Some(title(COMMAND_ALL_HELP, "Select all attached regiments.")),
        disabled: None,
        icon: image(game, COMMAND_ALL_ICON),
    };
    let affording = title(
        "recruit_error_affording|ss",
        "Insufficient funds for %s. Requires %s.",
    );
    let glyphs: HashMap<&str, Handle<Image>> = GLYPHS
        .iter()
        .filter_map(|&(kind, name)| Some((kind, image(game, &format!("{GLYPH_DIR}/{name}.png"))?)))
        .collect();
    let quantities = |amounts: &[aaa_data::Amount], earned: bool| -> Vec<Quantity> {
        amounts
            .iter()
            .map(|a| Quantity {
                glyph: glyphs.get(a.kind).cloned(),
                amount: a.amount,
                earned,
                resource: a.resource.clone(),
            })
            .collect()
    };
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
                let economy = listing.map_or_else(Vec::new, |l| {
                    [
                        (&cost, &l.cost, false),
                        (&production, &l.production, true),
                        (&upkeep, &l.upkeep, false),
                    ]
                    .into_iter()
                    .filter(|(_, amounts, _)| !amounts.is_empty())
                    .map(|(label, amounts, earned)| Row {
                        label: label.clone(),
                        amounts: quantities(amounts, earned),
                    })
                    .collect()
                });
                Recruit {
                    id: id.to_string(),
                    menu: match transports.contains_key(id) {
                        true => Menu::Location,
                        false => Menu::Deploy,
                    },
                    look: Look {
                        name: listing.map_or_else(|| id.to_string(), |l| l.name.clone()),
                        help: None,
                        disabled: None,
                        icon: listing
                            .and_then(|l| l.icon.as_deref())
                            .and_then(|p| image(game, p)),
                    },
                    economy,
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
        deploy,
        location,
        rally,
        pack,
        cancel,
        command_all,
        requires,
        affording,
        menus,
        names: listings.into_iter().map(|(id, l)| (id, l.name)).collect(),
    });
    commands.init_resource::<Shown>();
    commands.init_resource::<Refusal>();
    commands.insert_resource(Page::Orders);

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
            GlobalZIndex(1),
            DespawnOnExit(Screen::Match),
        ))
        .with_children(|root| {
            // As wide as the grid, which the pane is kept clear of, and as
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
    !over_grid(&grids)
}

pub fn over_grid(
    grids: &Query<(&RelativeCursorPosition, &InheritedVisibility), With<Blocks>>,
) -> bool {
    grids.iter().any(|(p, shown)| shown.get() && p.cursor_over)
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
        Page::Deploy(_) | Page::Rally => chosen.0.is_none(),
    };
    if gone || keys.just_pressed(KeyCode::Escape) {
        page.set_if_neq(Page::Orders);
    }
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
    mut panel: Single<&mut Visibility, (With<Panel>, Without<SlotIcon>)>,
    mut slots: Query<SlotParts, Without<SlotIcon>>,
    mut icons: Query<(&mut ImageNode, &mut Shade, &mut Visibility), With<SlotIcon>>,
    mut tooltip: TipParts,
    local: Res<Local>,
    focus: Res<crate::Focus>,
    mut refusal: ResMut<Refusal>,
    mut preview: ResMut<Preview>,
    bindings: Res<Bindings>,
    mut commands: Commands,
) {
    let building = chosen.0.filter(|_| selected.0.is_empty());
    let nothing = selected.0.is_empty() && building.is_none();
    // What hovering, or aiming, the order would do to the economy.
    let unpacking = selected.0.first().copied().filter(|&r| {
        selected.0.len() == 1 && sim.0.regiments.get(r).is_some_and(|r| r.unpacks.is_some())
    });
    let mut previewed = match *page {
        Page::Aiming(Aim::Unpack) => unpacking.map(Previewed::Unpack),
        _ => None,
    };
    panel.set_if_neq(match nothing {
        false => Visibility::Inherited,
        true => Visibility::Hidden,
    });
    if nothing {
        // Nor is anything left for a key to press.
        shown.0 = [None; SLOTS];
        // What was under the cursor went with the selection.
        if preview.0.is_some() {
            preview.0 = None;
        }
        return;
    }
    let hq = shown_hq(&sim.0, &selected.0, building);
    let menu = looks.menu(&sim.0, building);
    let recruits: Vec<(Menu, Offer)> = (menu.iter())
        .map(|r| {
            let offer = match (building, &r.regiment) {
                (Some(b), Some(regiment)) => sim.0.offer(b, regiment),
                _ => Offer::Offered,
            };
            (r.menu, offer)
        })
        .collect();
    let order_slots: Vec<Option<usize>> = looks.orders.iter().map(|(s, _)| *s).collect();
    shown.0 = layout(
        *page,
        &order_slots,
        &offered(&sim.0, &selected.0),
        building.map(|b| BuildingButtons {
            deploy: looks.deploy.0,
            location: looks.location.0,
            recruits: &recruits,
            rally: looks.rally.0,
            pack: looks
                .pack
                .0
                .filter(|_| sim.0.packing.contains_key(&sim.0.buildings[b].template)),
        }),
        hq.is_some(),
    );
    let mut hovered = None;
    for (id, clicks, slot, hover, children, mut border, mut pickable) in &mut slots {
        let entry = shown.0[slot.0];
        // Empty slots are just boxes, which nothing should hear clicks on.
        let want = match entry {
            Some(_) => Pickable::default(),
            None => Pickable::IGNORE,
        };
        if *pickable != want {
            *pickable = want;
        }
        if !hover.get() && refusal.0.is_some_and(|(s, _)| s == slot.0) {
            refusal.0 = None;
        }
        let Some(entry) = entry else {
            border.set_if_neq(Edge::Clear);
            set_icon(children, &mut icons, None);
            continue;
        };
        // Why it can't be pressed, when that's down to the player.
        let mut why = None;
        let mut table = Vec::new();
        let (look, ready, lit) = match entry {
            Entry::Order(i) => {
                let ready = match (ORDERS[i].1, local.0) {
                    (Some(give @ (Give::Attach | Give::Detach)), Some(player)) => {
                        let can = |&r: &usize| sim.0.check(player, &give.command(r)).is_ok();
                        selected.0.iter().any(can)
                    }
                    (give, _) => give.is_some(),
                };
                let lit =
                    matches!((*page, ORDERS[i].1), (Page::Aiming(a), Some(Give::Aim(b))) if a == b);
                (&looks.orders[i].1, ready, lit)
            }
            Entry::Cancel => (&looks.cancel, true, false),
            Entry::Open(Menu::Deploy) => (&looks.deploy.1, true, false),
            Entry::Open(Menu::Location) => (&looks.location.1, true, false),
            Entry::Rally => (&looks.rally.1, true, *page == Page::Rally),
            Entry::CommandAll => (&looks.command_all, true, false),
            Entry::Pack => {
                let ready = building.is_some_and(|b| can_pack(&sim.0, b));
                (&looks.pack.1, ready, false)
            }
            Entry::Recruit(i) => {
                let r = &menu[i];
                table.extend(r.economy.iter().cloned());
                let lacking: Vec<&str> = match (&r.regiment, local.0) {
                    (Some(regiment), Some(player)) => sim
                        .0
                        .lacks(player, regiment)
                        .into_iter()
                        .map(|id| looks.names.get(id).map_or(id.as_str(), String::as_str))
                        .collect(),
                    _ => Vec::new(),
                };
                if !lacking.is_empty() {
                    why = Some(format!("{}: {}", looks.requires, lacking.join(", ")));
                }
                let offered = recruits[i].1 == Offer::Offered;
                let ready = r.regiment.is_some() && lacking.is_empty() && offered;
                (&r.look, ready, false)
            }
        };
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
        if hover.get() {
            match entry {
                Entry::Recruit(i) => {
                    previewed = menu[i]
                        .regiment
                        .clone()
                        .map(|r| Previewed::Commission(Box::new(r)));
                }
                Entry::Order(i) if ORDERS[i].1 == Some(Give::Aim(Aim::Unpack)) => {
                    previewed = unpacking.map(Previewed::Unpack);
                }
                _ => {}
            }
            let refused = refusal.0.filter(|(s, _)| *s == slot.0).map(|(_, r)| r);
            let unaffordable = match (entry, refused) {
                (Entry::Recruit(i), Some(aaa_sim::Refused::Unaffordable)) => {
                    menu[i].regiment.as_ref()
                }
                _ => None,
            };
            let why = match (ready, why, refused) {
                (_, _, Some(_)) if unaffordable.is_some() => unaffordable.map(|r| {
                    let cost = sim.0.cost(r).ceil().to_string();
                    fill(&looks.affording, &[&look.name, &cost])
                }),
                (_, _, Some(refused)) => Some(refused_why(refused).to_string()),
                (true, _, _) => None,
                (false, Some(why), _) => Some(why),
                (false, None, _) => Some(look.disabled.clone().unwrap_or("Not yet".into())),
            };
            let names = bound(entry, slot.0, *page, menu);
            let key = names.iter().find_map(|action| bindings.key(action));
            hovered = Some(Tip {
                title: match key {
                    Some(key) => key.mark(&look.name),
                    None => Marked::plain(look.name.as_str()),
                },
                help: look.help.clone(),
                table,
                why,
            });
        }
        border.set_if_neq(match (lit, hover.get() && ready) {
            (true, _) => Edge::Lit,
            (false, true) => Edge::Hovered,
            (false, false) => Edge::Clear,
        });
        let shade = match ready {
            true => Shade::Full,
            false => Shade::Dimmed,
        };
        set_icon(children, &mut icons, icon.map(|i| (i, shade)));
    }
    let prompt = |title: &str| {
        Some(Tip {
            title: Marked::plain(title),
            ..default()
        })
    };
    let tip = match (hovered, *page) {
        (Some(tip), _) => Some(tip),
        (None, Page::Aiming(Aim::Unpack)) => prompt("Left click where to unpack"),
        (None, Page::Aiming(Aim::Attack))
            if local
                .0
                .is_some_and(|p| attack_at(&sim.0, p, *focus) == Struck::Friend) =>
        {
            prompt("Can't attack a friend")
        }
        (None, Page::Aiming(Aim::Attack)) => prompt("Left click an enemy or where to attack"),
        (None, Page::Aiming(_)) => prompt("Left click where to go"),
        (None, Page::Rally) => prompt("Left click where new regiments go"),
        (None, _) => None,
    };
    tooltip.say(tip);
    let changed = match (&preview.0, &previewed) {
        (None, None) => false,
        (Some(Previewed::Unpack(a)), Some(Previewed::Unpack(b))) => a != b,
        (Some(Previewed::Commission(a)), Some(Previewed::Commission(b))) => {
            a.template != b.template
        }
        _ => true,
    };
    if changed {
        preview.0 = previewed;
    }
}

/// The tooltip's node, not the slots'.
type TooltipOnly = (With<Tooltip>, Without<Slot>);

/// The tooltip's box and what it last said.
#[derive(SystemParam)]
pub struct TipParts<'w, 's> {
    tooltip: Single<'w, 's, (Entity, &'static mut Node), TooltipOnly>,
    said: bevy::ecs::system::Local<'s, Tip>,
    commands: Commands<'w, 's>,
}

impl TipParts<'_, '_> {
    /// Shows the tip, or hides the box for none.
    fn say(&mut self, tip: Option<Tip>) {
        let (entity, node) = &mut *self.tooltip;
        let display = match tip {
            Some(_) => Display::Flex,
            None => Display::None,
        };
        if node.display != display {
            node.display = display;
        }
        let tip = tip.unwrap_or_default();
        if *self.said == tip {
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

/// What the game's hotkeys call what a button does, and the button
/// itself by its slot, for [`Bindings`].
fn bound(entry: Entry, slot: usize, page: Page, menu: &[Recruit]) -> Vec<String> {
    let command = |ids: &str| format!("ActorCommand {ids}");
    let mut names = match entry {
        Entry::Order(i) => vec![command(ORDERS[i].0)],
        Entry::Cancel if matches!(page, Page::Deploy(_)) => vec!["CancelRecruit".into()],
        Entry::Cancel => vec!["ActorCommandCancel".into()],
        Entry::Open(Menu::Deploy) => vec![command(DEPLOY)],
        Entry::Open(Menu::Location) => vec![command(DEPLOY_TO_LOCATION)],
        Entry::Recruit(i) => vec![format!("ActorDataSelectCommand {}", menu[i].id)],
        Entry::Pack => vec![command(PACK)],
        Entry::Rally => vec![command(RALLY)],
        Entry::CommandAll => vec!["CommandAll".into()],
    };
    names.push(format!("ActionButtonsButton_{slot}"));
    names
}

/// What pressing a button reads and changes.
#[derive(SystemParam)]
pub struct Presser<'w> {
    shown: Res<'w, Shown>,
    selected: ResMut<'w, Selected>,
    chosen: ResMut<'w, SelectedBuilding>,
    looks: Res<'w, Looks>,
    local: Res<'w, Local>,
    sim: Res<'w, Sim>,
    orders: ResMut<'w, Orders>,
    page: ResMut<'w, Page>,
    time: Res<'w, Time>,
    sounds: ResMut<'w, crate::sound::Sounds>,
    refusal: ResMut<'w, Refusal>,
}

impl Presser<'_> {
    /// Presses the button in `slot`: gives its order, aims it, opens or
    /// works the commission menu, or cancels.
    fn press(&mut self, slot: usize) {
        let (Some(entry), Some(player)) = (self.shown.0[slot], self.local.0) else {
            return;
        };
        let sim = &self.sim.0;
        match entry {
            Entry::Order(i) => match ORDERS[i].1 {
                Some(Give::Aim(aim)) => *self.page = Page::Aiming(aim),
                Some(Give::Stop) => {
                    let selected = &self.selected.0;
                    crate::acknowledge(sim, &mut self.sounds, &self.time, player, selected);
                    for &regiment in selected {
                        let stop = aaa_sim::Command::Stop { regiment };
                        let _ = self.orders.give(sim, player, stop);
                    }
                    *self.page = Page::Orders;
                }
                Some(give @ (Give::Attach | Give::Detach)) => {
                    for &regiment in &self.selected.0 {
                        let _ = self.orders.give(sim, player, give.command(regiment));
                    }
                }
                None => {}
            },
            Entry::Cancel => *self.page = Page::Orders,
            Entry::Open(menu) => *self.page = Page::Deploy(menu),
            Entry::Rally => *self.page = Page::Rally,
            Entry::CommandAll => {
                let building = self.chosen.0.filter(|_| self.selected.0.is_empty());
                let attached: Vec<usize> = shown_hq(sim, &self.selected.0, building)
                    .map_or(Vec::new(), |hq| sim.attached(hq).collect());
                // As the game does, even where that leaves nothing selected.
                self.selected.0 = attached;
                self.chosen.0 = None;
            }
            Entry::Pack => {
                if let Some(b) = self.chosen.0
                    && let Err(e) =
                        (self.orders).give(sim, player, aaa_sim::Command::Pack { building: b })
                {
                    debug!("can't pack {}: {e:?}", sim.buildings[b].template);
                }
            }
            Entry::Recruit(i) => {
                let Some(b) = self.chosen.0 else {
                    return;
                };
                let Some(recruit) = self.looks.menu(sim, Some(b)).get(i) else {
                    return;
                };
                if let Some(regiment) = &recruit.regiment {
                    let command = aaa_sim::Command::Commission {
                        building: b,
                        commission: Box::new(Commission::new(regiment.clone(), recruit.seconds)),
                    };
                    let given = self.orders.give(sim, player, command);
                    if let Err(e) = given {
                        debug!("can't commission {}: {e:?}", regiment.template);
                        // As the game does, it says nothing for what's not
                        // offered.
                        if e != aaa_sim::Refused::NotOffered {
                            self.refusal.0 = Some((slot, e));
                        }
                    }
                    *self.page = commissioned(*self.page, given);
                }
            }
        }
    }
}

/// A button clicked.
fn press(activate: On<Activate>, slots: Query<&Slot>, mut presser: Presser) {
    if let Ok(slot) = slots.get(activate.entity) {
        presser.press(slot.0);
    }
}

/// Presses the button a key just pressed is bound to, if it's showing.
pub fn hotkeys(keys: Res<ButtonInput<KeyCode>>, bindings: Res<Bindings>, mut presser: Presser) {
    if keys.get_just_pressed().next().is_none() {
        return;
    }
    let building = presser.chosen.0.filter(|_| presser.selected.0.is_empty());
    let menu = presser.looks.menu(&presser.sim.0, building);
    let page = *presser.page;
    let slot = (0..SLOTS).find(|&slot| {
        let names = presser.shown.0[slot]
            .filter(|e| !matches!(e, Entry::Recruit(i) if *i >= menu.len()))
            .map_or(Vec::new(), |e| bound(e, slot, page, menu));
        names.iter().any(|action| bindings.pressed(&keys, action))
    });
    if let Some(slot) = slot {
        presser.press(slot);
    }
}

/// The HQ the selection is, by its id: the selected building, or the one
/// selected regiment, packed.
fn shown_hq(sim: &aaa_sim::World, selected: &[usize], building: Option<usize>) -> Option<u32> {
    match (selected, building) {
        ([], Some(b)) => sim.hq_of(b),
        ([r], _) => sim.packed_hq(*r),
        _ => None,
    }
}

/// The page after a press of a recruit in the commission menu: as the
/// game does, back to the building's own buttons once it's been given.
fn commissioned(page: Page, given: Result<(), aaa_sim::Refused>) -> Page {
    match given {
        Ok(()) => Page::Orders,
        Err(_) => page,
    }
}

/// `text` with each `%s` filled in turn from `with`.
pub(crate) fn fill(text: &str, with: &[&str]) -> String {
    let mut out = String::new();
    let mut rest = text;
    for w in with {
        let Some((before, after)) = rest.split_once("%s") else {
            break;
        };
        out.push_str(before);
        out.push_str(w);
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Why an order was refused, for the tooltip.
fn refused_why(refused: aaa_sim::Refused) -> &'static str {
    match refused {
        aaa_sim::Refused::NotYours => "Not yours",
        aaa_sim::Refused::Unable => "Can't now",
        aaa_sim::Refused::NoRoom => "No room",
        aaa_sim::Refused::Lacking => "Lacks a building it needs",
        aaa_sim::Refused::Unaffordable => "Not enough money",
        aaa_sim::Refused::Full => "HQ is full",
        aaa_sim::Refused::NotOffered => "Not offered",
    }
}

/// Whether a building can pack now: not while it's packing or unpacking,
/// nor when it's too hurt.
fn can_pack(sim: &aaa_sim::World, building: usize) -> bool {
    let b = &sim.buildings[building];
    let floor = sim.packing.get(&b.template).map_or(1.0, |p| p.health_min);
    !b.busy() && b.health > floor * b.fighter.health
}

/// What an attack aimed at `focus` would strike.
#[derive(Debug, PartialEq, Eq)]
enum Struck {
    /// The enemy regiment, to go after.
    Enemy(usize),
    /// A regiment or building that's no enemy's, so nothing to attack.
    Friend,
    /// The ground, or an enemy building on it, to attack-move to.
    Ground,
}

fn attack_at(sim: &aaa_sim::World, player: u32, focus: crate::Focus) -> Struck {
    let owner = match focus {
        crate::Focus::Nothing => None,
        crate::Focus::Regiment(r) => Some(sim.regiments[r].owner),
        crate::Focus::Building(b) => Some(sim.buildings[b].owner),
    };
    struck(focus, owner.map(|o| sim.sides.relation(player, o)))
}

/// What an attack strikes at `focus`, toward whose owner `player` stands
/// as `toward`.
fn struck(focus: crate::Focus, toward: Option<aaa_sim::Relation>) -> Struck {
    match (focus, toward) {
        (crate::Focus::Nothing, _) => Struck::Ground,
        (crate::Focus::Regiment(e), Some(aaa_sim::Relation::Enemy)) => Struck::Enemy(e),
        (crate::Focus::Building(_), Some(aaa_sim::Relation::Enemy)) => Struck::Ground,
        _ => Struck::Friend,
    }
}

/// While an order is aimed, a left click on the map or minimap gives it
/// there and a right click calls it off. Attacking an enemy regiment, or
/// its mark on the minimap, sends the selected after it instead. Attacking
/// a friend, or unpacking somewhere the building wouldn't fit, waits for
/// another click. A rally point is set for the selected building.
#[allow(clippy::too_many_arguments)]
pub fn aim(
    buttons: Res<ButtonInput<MouseButton>>,
    pointer: crate::Pointer,
    scenery: Res<crate::banners::Scenery>,
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
    let (Some(player), Some(at)) = (local.0, pointer.at(&loaded.map.ground)) else {
        return;
    };
    let point = at.point();
    let focus = match at {
        crate::Pointing::Ground(_) => *focus,
        // The minimap's marks are too close together to pick out friends,
        // so a click there is only ever on an enemy or the ground.
        crate::Pointing::Minimap(p) => {
            crate::nearest_regiment(&sim.0, p, pointer.minimap.reach(), |r| {
                !scenery.has(&sim.0, r)
                    && sim.0.sides.relation(player, sim.0.regiments[r].owner)
                        == aaa_sim::Relation::Enemy
            })
            .map_or(crate::Focus::Nothing, crate::Focus::Regiment)
        }
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
            debug!("can't rally {}: {e:?}", sim.0.buildings[b].template);
        }
        *page = Page::Orders;
        return;
    };
    if aim == Aim::Attack {
        match attack_at(&sim.0, player, focus) {
            Struck::Friend => return,
            Struck::Ground => {}
            Struck::Enemy(e) => {
                let (pursuing, rest) = crate::pursue(&sim.0, &mut orders, player, &selected.0, e);
                // Those with nothing to fire go there as they would anywhere.
                let regiments = crate::send(
                    &sim.0,
                    &mut orders,
                    player,
                    &rest,
                    point,
                    (aim, looks.together(aim)),
                    loaded.map.ground.size(),
                );
                if pursuing.is_empty() && regiments.is_empty() {
                    return;
                }
                crate::acknowledge(&sim.0, &mut sounds, &time, player, &selected.0);
                sent.write(crate::waypoints::Sent {
                    to: None,
                    aim,
                    regiments: pursuing,
                });
                sent.write(crate::waypoints::Sent {
                    to: Some(point),
                    aim,
                    regiments,
                });
                *page = Page::Orders;
                return;
            }
        }
    }
    let regiments = match aim {
        Aim::Unpack => {
            let &[r] = &selected.0[..] else {
                return;
            };
            let unpack = aaa_sim::Command::Unpack {
                regiment: r,
                at: point,
                facing: UNPACK_FACING,
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
    fn attacks_strike_only_enemies() {
        use crate::Focus::{Building, Nothing, Regiment};
        use aaa_sim::Relation::{Ally, Enemy, Itself, Neutral};
        assert_eq!(struck(Regiment(3), Some(Enemy)), Struck::Enemy(3));
        for friend in [Itself, Ally, Neutral] {
            assert_eq!(struck(Regiment(3), Some(friend)), Struck::Friend);
            assert_eq!(struck(Building(3), Some(friend)), Struck::Friend);
        }
        assert_eq!(struck(Building(3), Some(Enemy)), Struck::Ground);
        assert_eq!(struck(Nothing, None), Struck::Ground);
    }

    #[test]
    fn orders_take_their_slots() {
        let all = [true; 8];
        let grid = layout(Page::Orders, &GAME, &all, None, false);
        assert_eq!(grid[0], Some(Entry::Order(0)));
        assert_eq!(grid[3], Some(Entry::Order(3)));
        assert_eq!(grid[6], Some(Entry::Order(4)));
        assert_eq!(grid[11], Some(Entry::Order(6)));
        assert_eq!(grid.iter().flatten().count(), 8);
        let truck = [true, false, false, true, false, false, false, true];
        let grid = layout(Page::Orders, &GAME, &truck, None, false);
        assert_eq!(grid[9], Some(Entry::Order(7)));
        assert_eq!(grid.iter().flatten().count(), 3);
    }

    #[test]
    fn hqs_have_a_button_for_their_regiments_where_nothing_else_is() {
        let truck = [true, false, false, true, false, false, false, true];
        let grid = layout(Page::Orders, &GAME, &truck, None, true);
        assert_eq!(grid[COMMAND_ALL], Some(Entry::CommandAll));
        assert_eq!(grid.iter().flatten().count(), 4);
        // Not over an order, nor while one's aimed.
        let mut slots = GAME;
        slots[0] = Some(COMMAND_ALL);
        let grid = layout(Page::Orders, &slots, &truck, None, true);
        assert_eq!(grid[COMMAND_ALL], Some(Entry::Order(0)));
        let grid = layout(Page::Aiming(Aim::Move), &GAME, &truck, None, true);
        assert!(!grid.contains(&Some(Entry::CommandAll)));
    }

    #[test]
    fn buttons_go_by_the_names_the_hotkeys_bind() {
        let names = |entry, page| bound(entry, 4, page, &[]);
        assert_eq!(
            names(Entry::Order(5), Page::Orders),
            ["ActorCommand hq_attach", "ActionButtonsButton_4"]
        );
        assert_eq!(names(Entry::CommandAll, Page::Orders)[0], "CommandAll");
        assert_eq!(
            names(Entry::Rally, Page::Orders)[0],
            "ActorCommand set_rally_point"
        );
        assert_eq!(
            names(Entry::Cancel, Page::Deploy(Menu::Location))[0],
            "CancelRecruit"
        );
        assert_eq!(
            names(Entry::Open(Menu::Location), Page::Orders)[0],
            "ActorCommand recruit_location"
        );
        assert_eq!(names(Entry::Cancel, Page::Rally)[0], "ActorCommandCancel");
    }

    #[test]
    fn aiming_shows_the_order_and_a_way_out() {
        let grid = layout(Page::Aiming(Aim::Run), &GAME, &[true; 8], None, false);
        assert_eq!(grid[2], Some(Entry::Order(2)));
        assert_eq!(grid[CANCEL], Some(Entry::Cancel));
        assert_eq!(grid.iter().flatten().count(), 2);
    }

    #[test]
    fn commissioning_goes_back_to_the_buildings_buttons() {
        let menu = Page::Deploy(Menu::Deploy);
        assert_eq!(commissioned(menu, Ok(())), Page::Orders);
        assert_eq!(commissioned(menu, Err(aaa_sim::Refused::Unable)), menu);
    }

    #[test]
    fn buildings_open_a_menu_of_what_they_commission() {
        let corps = [(Menu::Deploy, Offer::Offered); 11];
        let hq = BuildingButtons {
            deploy: Some(0),
            location: Some(1),
            recruits: &corps,
            rally: Some(5),
            pack: Some(9),
        };
        let grid = layout(Page::Orders, &GAME, &[true; 8], Some(hq), false);
        assert_eq!(grid[0], Some(Entry::Open(Menu::Deploy)));
        assert_eq!(grid[5], Some(Entry::Rally));
        assert_eq!(grid[9], Some(Entry::Pack));
        assert_eq!(grid.iter().flatten().count(), 3);
        let depot = BuildingButtons {
            recruits: &[],
            pack: None,
            ..hq
        };
        assert!(
            layout(Page::Orders, &GAME, &[true; 8], Some(depot), false)
                .iter()
                .all(Option::is_none)
        );
        let menu = layout(
            Page::Deploy(Menu::Deploy),
            &GAME,
            &[true; 8],
            Some(hq),
            false,
        );
        assert_eq!(menu[10], Some(Entry::Recruit(10)));
        assert_eq!(menu[MENU_CANCEL], Some(Entry::Cancel));
        let rally = layout(Page::Rally, &GAME, &[true; 8], Some(hq), false);
        assert_eq!(rally[5], Some(Entry::Rally));
        assert_eq!(rally[CANCEL], Some(Entry::Cancel));
        assert_eq!(rally.iter().flatten().count(), 2);
    }

    #[test]
    fn what_a_transport_carries_has_a_menu_of_its_own() {
        use Offer::{Greyed, Hidden, Offered};
        // An Infantry HQ: its amphibious regiment, last, hidden.
        let infantry = [
            (Menu::Deploy, Offered),
            (Menu::Deploy, Greyed),
            (Menu::Location, Hidden),
        ];
        let hq = BuildingButtons {
            deploy: Some(0),
            location: Some(1),
            recruits: &infantry,
            rally: Some(5),
            pack: None,
        };
        let grid = layout(Page::Orders, &GAME, &[true; 8], Some(hq), false);
        assert_eq!(grid[0], Some(Entry::Open(Menu::Deploy)));
        assert_eq!(grid[1], None);
        let menu = layout(
            Page::Deploy(Menu::Deploy),
            &GAME,
            &[true; 8],
            Some(hq),
            false,
        );
        assert_eq!(
            menu[..3],
            [Some(Entry::Recruit(0)), Some(Entry::Recruit(1)), None]
        );
        // A transported regiment and a truck: one in each menu, each at the
        // front; a hidden one keeps its place.
        let mixed = [
            (Menu::Location, Offered),
            (Menu::Deploy, Offered),
            (Menu::Location, Hidden),
            (Menu::Location, Greyed),
        ];
        let ship = BuildingButtons {
            recruits: &mixed,
            ..hq
        };
        let grid = layout(Page::Orders, &GAME, &[true; 8], Some(ship), false);
        assert_eq!(grid[0], Some(Entry::Open(Menu::Deploy)));
        assert_eq!(grid[1], Some(Entry::Open(Menu::Location)));
        let deploy = layout(
            Page::Deploy(Menu::Deploy),
            &GAME,
            &[true; 8],
            Some(ship),
            false,
        );
        assert_eq!(deploy[0], Some(Entry::Recruit(1)));
        assert_eq!(deploy.iter().flatten().count(), 2);
        let carried = layout(
            Page::Deploy(Menu::Location),
            &GAME,
            &[true; 8],
            Some(ship),
            false,
        );
        assert_eq!(
            carried[..3],
            [Some(Entry::Recruit(0)), None, Some(Entry::Recruit(3))]
        );
    }
}
