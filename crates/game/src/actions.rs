//! The action buttons: a grid in the bottom-right corner of the orders the
//! selection can take, with the game's icons in the places its data gives
//! them. Orders given at a place, such as Move, wait for a left click on the
//! map or minimap, with a button to cancel; right click or Escape does too.
//! A selected building that recruits, such as a Corps HQ, has Deploy,
//! opening a menu of what comes out of it, and Deploy to Location, of what
//! a transport carries to where it's sent, each while that menu offers
//! anything; [`crate::object_display`] shows what it's been asked for, and a button
//! sets the rally point what it recruits moves on to. One that researches
//! has Research, opening a menu of the technologies it lists. A building that
//! packs, into a truck say, has Pack, dimmed while it can't. Each button
//! answers to the keys the game binds what it does to, and its tooltip
//! marks the letter.

use std::collections::HashMap;

use aaa_data::{GameData, Id};
use aaa_map::Regiment;
use aaa_render::textures;
use aaa_sim::{Lacking, Researchable};
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{Activate, Button};

use crate::keys::Bindings;
use crate::messages::{About, Failed, Refused};
use crate::resource_bar::{Preview, Previewed};
use crate::ui::{self, Click, Edge, Font, Ink, Marked, Shade, Surface, label, panel, tooltip};
use crate::{Loaded, Local, Orders, Raising, Screen, Selected, SelectedBuilding, Sim};

const COLUMNS: usize = 4;
const ROWS: usize = 3;
const SLOTS: usize = COLUMNS * ROWS;
/// A button's side, in logical pixels; the icons are drawn 42 across.
pub(crate) const BUTTON: f32 = 56.0;
pub(crate) const GAP: f32 = 4.0;
/// Black margin between the buttons and the panel's rounded edge.
pub(crate) const PAD: f32 = 8.0;
/// The grid's size, in logical pixels.
pub const SIZE: Vec2 = Vec2::new(span(COLUMNS), span(ROWS));

/// Across `n` buttons and the padding either side.
const fn span(n: usize) -> f32 {
    n as f32 * BUTTON + (n - 1) as f32 * GAP + 2.0 * PAD
}

/// The orders a regiment has, by command id, and what each does here;
/// `None` for those the sim can't carry out yet, which are shown dimmed.
/// The game leaves the rest of the commands off a regiment's grid.
const ORDERS: [(&str, Option<Give>); 9] = [
    ("move", Some(Give::Aim(Aim::Move))),
    ("kill", Some(Give::Aim(Aim::Attack))),
    ("retreat", Some(Give::Aim(Aim::Run))),
    ("stop", Some(Give::Stop)),
    ("guard", Some(Give::Aim(Aim::Guard))),
    ("hq_attach", Some(Give::Attach)),
    ("hq_detach", Some(Give::Detach)),
    ("unpack", Some(Give::Aim(Aim::Unpack))),
    ("disband", Some(Give::Disband)),
];

/// The commands whose buttons open a recruiter's two menus.
const DEPLOY: &str = "recruit";
const DEPLOY_TO_LOCATION: &str = "recruit_location";
/// The command whose button opens a building's research menu.
const RESEARCH: &str = "research";
/// The command that packs a building up, such as into a truck.
const PACK: &str = "pack";
/// The command that sells a building.
const SELL: &str = "sell";
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

/// Each resource's glyph, by the resource's field name in the data.
pub(crate) fn glyphs(
    game: &GameData,
    images: &mut Assets<Image>,
    formats: bevy::image::CompressedImageFormats,
) -> HashMap<&'static str, Handle<Image>> {
    let glyph = |&(kind, name): &(&'static str, &str)| {
        let path = format!("{GLYPH_DIR}/{name}.png");
        let image = textures::decode(&game.read(&path)?, &path, formats, true)?;
        Some((kind, images.add(image)))
    };
    GLYPHS.iter().filter_map(glyph).collect()
}

/// What an order does once its button is pressed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Give {
    /// Waits for a place on the map.
    Aim(Aim),
    Stop,
    Attach,
    Detach,
    /// Asks first.
    Disband,
}

impl Give {
    /// The order to attach, detach or disband `regiment`.
    fn command(self, regiment: usize) -> aaa_sim::Command {
        match self {
            Give::Attach => aaa_sim::Command::Attach { regiment },
            Give::Disband => aaa_sim::Command::Disband { regiment },
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
    Guard,
    Unpack,
}

impl Aim {
    pub const ALL: [Aim; 5] = [Aim::Move, Aim::Attack, Aim::Run, Aim::Guard, Aim::Unpack];
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
    /// One of the selected building's menus of what it recruits or
    /// researches.
    Menu(Menu),
}

/// Which of a building's menus something is in: Deploy for what comes out
/// of it, Deploy to Location for what names a transport, and Research.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    Deploy,
    Location,
    Research,
}

/// How a recruiter's menu shows one of its entries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Offer {
    Offered,
    /// Shown, but not pressable.
    Greyed,
    /// Left out, its slot kept empty.
    Hidden,
}

impl From<&Lacking> for Offer {
    fn from(lacking: &Lacking) -> Offer {
        if lacking.hidden {
            Offer::Hidden
        } else if lacking.is_empty() {
            Offer::Offered
        } else {
            Offer::Greyed
        }
    }
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
    /// Index into what the selected building researches.
    Research(usize),
    /// Packs the selected building up.
    Pack,
    /// Sells the selected building, once the player's sure.
    Sell,
    /// Waits for a place to set the selected building's rally point.
    Rally,
    /// Selects the regiments attached to the selected HQ.
    CommandAll,
}

/// Where a command's button goes: its slot and, that taken, which end of
/// the grid it takes a free one from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Place {
    slot: usize,
    /// Whether it takes the last free slot, rather than the first.
    high: bool,
}

/// Puts `entry` at `place` or, that slot taken, in the first free one, or
/// the last for one of high importance. A slot off the grid has no button.
fn put(out: &mut [Option<Entry>; SLOTS], place: Option<Place>, entry: Entry) {
    let Some(Place { slot, high }) = place.filter(|p| p.slot < SLOTS) else {
        return;
    };
    let free = |s: &usize| out[*s].is_none();
    let slot = match (free(&slot), high) {
        (true, _) => Some(slot),
        (false, false) => (0..SLOTS).find(free),
        (false, true) => (0..SLOTS).rev().find(free),
    };
    if let Some(s) = slot {
        out[s] = Some(entry);
    }
}

/// The buttons a selected building has: where its menus' and rally point
/// buttons go, which menu each thing it recruits is in and how it's
/// offered, and where its Pack button goes, if it packs.
#[derive(Clone, Copy, Debug)]
struct BuildingButtons<'a> {
    deploy: Option<Place>,
    location: Option<Place>,
    recruits: &'a [(Menu, Offer)],
    research: Option<Place>,
    /// How it offers each technology it lists, in its menu's order.
    researches: &'a [Researchable],
    rally: Option<Place>,
    pack: Option<Place>,
    /// Where its Sell button goes, while it can be sold.
    sell: Option<Place>,
}

impl BuildingButtons<'_> {
    /// Whether `menu` has anything to show.
    fn offers(&self, menu: Menu) -> bool {
        (self.recruits.iter()).any(|&(m, offer)| m == menu && offer != Offer::Hidden)
    }
}

/// Which of [`ORDERS`] the selection has: packed buildings move, stop and,
/// one at a time, unpack; organizations of them can also retreat and
/// guard; anything else has the rest. One thing selected alone can be
/// disbanded, unless it's attached to an HQ, which it can detach from
/// instead, or on its way to unpack; while it fights it's greyed.
fn offered(sim: &aaa_sim::World, selected: &[usize]) -> [bool; ORDERS.len()] {
    let packed = |&r: &usize| sim.regiments[r].unpacks.is_some();
    let all_packed = !selected.is_empty() && selected.iter().all(packed);
    let lone = selected.iter().all(|&r| sim.lone(r));
    let disband = match selected {
        &[r] => {
            let r = &sim.regiments[r];
            r.hq.is_none() && r.unpack_at.is_none()
        }
        _ => false,
    };
    ORDERS.map(|(ids, _)| match ids {
        "unpack" => all_packed && selected.len() == 1,
        "kill" | "hq_attach" => !all_packed,
        "hq_detach" => !all_packed && !disband,
        "retreat" | "guard" => !all_packed || !lone,
        "disband" => disband,
        _ => true,
    })
}

/// The buttons a page has, by slot, given each of [`ORDERS`]' slot and
/// whether the selection has it, or a selected building's buttons.
fn layout(
    page: Page,
    slots: &[Option<Place>],
    offered: &[bool],
    building: Option<BuildingButtons>,
    hq: bool,
) -> [Option<Entry>; SLOTS] {
    let mut out = [None; SLOTS];
    match (page, building) {
        (Page::Orders, Some(b)) => {
            for (menu, place) in [(Menu::Deploy, b.deploy), (Menu::Location, b.location)] {
                if b.offers(menu) {
                    put(&mut out, place, Entry::Open(menu));
                }
            }
            // Even once there's nothing left to research.
            if !b.researches.is_empty() {
                put(&mut out, b.research, Entry::Open(Menu::Research));
            }
            put(&mut out, b.rally, Entry::Rally);
            put(&mut out, b.pack, Entry::Pack);
            put(&mut out, b.sell, Entry::Sell);
        }
        (Page::Menu(Menu::Research), Some(b)) => {
            // What's done or under way keeps its slot empty; what's hidden
            // gives its slot up.
            let shown =
                (b.researches.iter().enumerate()).filter(|(_, r)| **r != Researchable::Hidden);
            for ((i, r), slot) in shown.zip(&mut out[..MENU_CANCEL]) {
                if *r != Researchable::Taken {
                    *slot = Some(Entry::Research(i));
                }
            }
            out[MENU_CANCEL] = Some(Entry::Cancel);
        }
        (Page::Menu(menu), Some(b)) => {
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
            if let Some(s) = b
                .rally
                .map(|p| p.slot)
                .filter(|&s| s < SLOTS && s != CANCEL)
            {
                out[s] = Some(Entry::Rally);
            }
            out[CANCEL] = Some(Entry::Cancel);
        }
        (Page::Menu(_) | Page::Rally, None) => {}
        (Page::Orders, None) => {
            for (i, &place) in slots.iter().enumerate() {
                if offered[i] {
                    put(&mut out, place, Entry::Order(i));
                }
            }
        }
        (Page::Aiming(aim), _) => {
            let aimed = ORDERS.iter().position(|(_, g)| *g == Some(Give::Aim(aim)));
            if let Some(i) = aimed
                && let Some(s) = slots[i].map(|p| p.slot).filter(|&s| s < SLOTS)
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
}

/// One of the technologies a building researches, as its button shows.
struct Topic {
    look: Look,
    /// Its cost, for the tooltip.
    economy: Vec<Row>,
    /// What the tooltip says under its cost: what the technology does,
    /// then its description.
    notes: Vec<String>,
}

/// The icons and names of the buttons, loaded with the match.
#[derive(Resource)]
pub struct Looks {
    /// Per [`ORDERS`], its place and look.
    orders: Vec<(Option<Place>, Look)>,
    deploy: (Option<Place>, Look),
    location: (Option<Place>, Look),
    research: (Option<Place>, Look),
    rally: (Option<Place>, Look),
    pack: (Option<Place>, Look),
    sell: (Option<Place>, Look),
    cancel: Look,
    command_all: Look,
    /// The tooltip's heading for what a recruit needs.
    requires: String,
    /// What a recruit the player can't afford says, given its name and
    /// cost.
    affording: String,
    /// The same for research.
    research_affording: String,
    /// What research refused for anything else says, given its name.
    research_refused: String,
    /// What a technology that wants buildings says, given its name and
    /// theirs.
    research_requires: String,
    /// What each of the map's building templates commissions, in its
    /// buttons' order.
    menus: HashMap<String, Vec<Recruit>>,
    /// What each building template researches, in its buttons' order.
    researches: HashMap<Id, Vec<Topic>>,
    /// What each template and technology is called, for saying what a
    /// commission needs.
    names: HashMap<Id, String>,
}

impl Looks {
    /// The menu of what the selected building commissions.
    fn menu(&self, sim: &aaa_sim::World, building: Option<usize>) -> &[Recruit] {
        building
            .and_then(|b| self.menus.get(sim.buildings[b].template.as_str()))
            .map_or(&[], Vec::as_slice)
    }

    /// The research menu of the selected building, as its research list
    /// has it.
    fn topics(&self, sim: &aaa_sim::World, building: Option<usize>) -> &[Topic] {
        building
            .and_then(|b| self.researches.get(&sim.buildings[b].template))
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
    /// Lines under the table.
    notes: Vec<String>,
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

impl Looks {
    /// The name of the order that aims `aim`.
    pub fn order(&self, aim: Aim) -> &str {
        let i = ORDERS.iter().position(|(_, g)| *g == Some(Give::Aim(aim)));
        i.map_or("", |i| self.orders[i].1.name.as_str())
    }
}

/// What each slot holds now.
#[derive(Resource, Default)]
pub struct Shown([Option<Entry>; SLOTS]);

/// The slot whose press was just refused and why, said while the cursor
/// stays on it.
#[derive(Resource, Default)]
pub struct Refusal(Option<(usize, aaa_sim::Refused)>);

pub fn spawn(
    mut commands: Commands,
    loaded: Res<Loaded>,
    raising: Res<Raising>,
    sim: Res<Sim>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &loaded.game;
    let glyphs = glyphs(game, &mut images, formats.0);
    let mut image = |game: &GameData, path: &str| {
        let image = textures::decode(&game.read(path)?, path, formats.0, true)?;
        Some(images.add(image))
    };
    let commands_by_id: HashMap<Id, aaa_data::Command> = game
        .commands()
        .into_iter()
        .map(|c| (c.ids.clone(), c))
        .collect();
    let mut look_of = |game: &GameData, ids: &str| match commands_by_id.get(&Id::from(ids)) {
        Some(c) => (
            c.slot.map(|slot| Place {
                slot,
                high: c.high_importance,
            }),
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
    let research = look_of(game, RESEARCH);
    let transports = game.transports();
    let rally = look_of(game, RALLY);
    let pack = look_of(game, PACK);
    let sell = look_of(game, SELL);
    let cancel = look_of(game, CANCEL_LOOK).1;
    let strings = game.strings();
    let title = |key: &str, or: &str| strings.get(key).map_or(or, String::as_str).to_string();
    let cost = title("ui_tooltip_item_title_cost", "Cost");
    let cost_title = cost.clone();
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
    let research_affording = title(
        "research_error_affording|ss",
        "Insufficient funds for %s. Requires %s.",
    );
    let research_requires = title("research_error_missing_requirements|ss", "%s required: %s");
    let research_refused = title("research_error_generic|s", "Could not research %s");
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
    let technologies = game.technologies();
    let researches = (sim.0.rules().research_lists.iter())
        .map(|(template, list)| {
            let topics = list.iter().map(|item| {
                let id = &item.technology;
                let technology = technologies.get(id);
                let cost: Vec<aaa_data::Amount> = (item.cost.nonzero())
                    .map(|(kind, amount)| aaa_data::Amount {
                        kind,
                        resource: aaa_data::resource_name(&strings, kind),
                        amount,
                    })
                    .collect();
                Topic {
                    look: Look {
                        name: technology.map_or_else(|| id.to_string(), |t| t.name.clone()),
                        help: None,
                        disabled: None,
                        icon: (technology.and_then(|t| t.icon.as_deref()))
                            .and_then(|p| image(game, p)),
                    },
                    economy: match cost.is_empty() {
                        true => Vec::new(),
                        false => vec![Row {
                            label: cost_title.clone(),
                            amounts: quantities(&cost, false),
                        }],
                    },
                    notes: technology.map_or_else(Vec::new, |t| {
                        (t.lines.iter().cloned())
                            .chain(t.description.clone())
                            .collect()
                    }),
                }
            });
            (template.clone(), topics.collect())
        })
        .collect();
    let listings = game.listings();
    // Buildings unpacked during the match need menus too: those of the
    // map's trucks, and of what any menu commissions.
    let mut menus = HashMap::new();
    let standing = sim.0.buildings.iter().map(|b| b.template.to_string());
    let mut templates: Vec<String> = standing.collect();
    templates.extend(
        sim.0
            .regiments
            .iter()
            .filter_map(|r| Some(r.unpacks.as_ref()?.building.template.to_string())),
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
                let regiment = sim.0.rules().raisable.get(id).cloned();
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
                    .to_string(),
            )
        }));
        menus.insert(template, menu);
    }
    commands.insert_resource(Looks {
        orders,
        deploy,
        location,
        research,
        rally,
        pack,
        sell,
        cancel,
        command_all,
        requires,
        affording,
        research_affording,
        research_refused,
        research_requires,
        menus,
        researches,
        names: (listings.iter().map(|(id, l)| (id.clone(), l.name.clone())))
            .chain((technologies.iter()).map(|(id, t)| (id.clone(), t.name.clone())))
            .collect(),
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
    mut was: bevy::ecs::system::Local<Option<usize>>,
    displays: Res<crate::displays::Displays>,
) {
    // A menu is the building's it was opened on.
    let switched = std::mem::replace(&mut *was, chosen.0) != chosen.0;
    let gone = match *page {
        Page::Orders => false,
        Page::Aiming(_) => selected.0.is_empty(),
        Page::Menu(_) => chosen.0.is_none() || switched,
        Page::Rally => chosen.0.is_none(),
    };
    // Escape closes a display panel that's up, not the page under it.
    let escaped = keys.just_pressed(KeyCode::Escape) && crate::displays::closed(Some(displays));
    if gone || escaped {
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
    let lacks: Vec<Lacking> = (menu.iter())
        .map(|r| match (building, &r.regiment, local.0) {
            (Some(b), Some(regiment), Some(player)) => sim.0.lacks(player, b, regiment),
            _ => Lacking::default(),
        })
        .collect();
    let recruits: Vec<(Menu, Offer)> = (menu.iter().zip(&lacks))
        .map(|(r, lacking)| (r.menu, Offer::from(lacking)))
        .collect();
    let topics = looks.topics(&sim.0, building);
    let list = building.map_or(&[][..], |b| sim.0.research_list(b));
    // Only what has a look, should the two ever differ.
    let list = &list[..list.len().min(topics.len())];
    let mut researches: Vec<Researchable> = match (building, local.0) {
        (Some(b), Some(player)) => sim.0.researchables(player, b),
        _ => vec![Researchable::Hidden; list.len()],
    };
    researches.truncate(list.len());
    let order_slots: Vec<Option<Place>> = looks.orders.iter().map(|(s, _)| *s).collect();
    shown.0 = layout(
        *page,
        &order_slots,
        &offered(&sim.0, &selected.0),
        building.map(|b| BuildingButtons {
            deploy: looks.deploy.0,
            location: looks.location.0,
            recruits: &recruits,
            research: looks.research.0,
            researches: &researches,
            // Where what it raises goes matters whether or not any of it
            // can be raised now.
            rally: looks.rally.0.filter(|_| !menu.is_empty()),
            pack: looks.pack.0.filter(|_| {
                sim.0
                    .rules()
                    .packing
                    .contains_key(&sim.0.buildings[b].template)
            }),
            sell: looks.sell.0.filter(|_| {
                local
                    .0
                    .is_some_and(|p| sim.0.check(p, &sell_command(b)).is_ok())
            }),
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
        let mut notes = Vec::new();
        // A greyed technology's tooltip says only what it wants.
        let mut only = None;
        let (look, ready, lit) = match entry {
            Entry::Order(i) => {
                let ready = match (ORDERS[i].1, local.0) {
                    (Some(give @ (Give::Attach | Give::Detach | Give::Disband)), Some(player)) => {
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
            Entry::Open(Menu::Research) => (&looks.research.1, true, false),
            Entry::Research(i) => {
                let topic = &topics[i];
                table.extend(topic.economy.iter().cloned());
                notes.extend(topic.notes.iter().cloned());
                if let Researchable::Lacking(missing) = &researches[i] {
                    let names: Vec<String> = (missing.iter())
                        .map(|id| looks.names.get(id).map_or(id.to_string(), Clone::clone))
                        .collect();
                    let with = [topic.look.name.as_str(), &names.join(", ")];
                    only = Some(fill(&looks.research_requires, &with));
                }
                let ready = researches[i] == Researchable::Ready;
                (&topic.look, ready, false)
            }
            Entry::Rally => (&looks.rally.1, true, *page == Page::Rally),
            Entry::CommandAll => (&looks.command_all, true, false),
            Entry::Pack => {
                let ready = building.is_some_and(|b| can_pack(&sim.0, b));
                (&looks.pack.1, ready, false)
            }
            Entry::Sell => (&looks.sell.1, true, false),
            Entry::Recruit(i) => {
                let r = &menu[i];
                table.extend(r.economy.iter().cloned());
                let lacking: Vec<String> = (lacks[i].properties.iter())
                    .chain(&lacks[i].structures)
                    .map(|id| looks.names.get(id).map_or(id.to_string(), Clone::clone))
                    .collect();
                if !lacking.is_empty() {
                    why = Some(format!("{}: {}", looks.requires, lacking.join(", ")));
                }
                let ready = r.regiment.is_some() && lacking.is_empty();
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
                Entry::Research(i) => {
                    previewed = list.get(i).cloned().map(Previewed::Research);
                }
                Entry::Order(i) if ORDERS[i].1 == Some(Give::Aim(Aim::Unpack)) => {
                    previewed = unpacking.map(Previewed::Unpack);
                }
                _ => {}
            }
            let refused = refusal.0.filter(|(s, _)| *s == slot.0).map(|(_, r)| r);
            let unaffordable = match (entry, refused) {
                (Entry::Recruit(i), Some(aaa_sim::Refused::Unaffordable)) => menu[i]
                    .regiment
                    .as_ref()
                    .map(|r| (&looks.affording, sim.0.cost(r))),
                _ => None,
            };
            let why = match (ready, why, refused) {
                (_, _, Some(_)) if unaffordable.is_some() => unaffordable.map(|(said, cost)| {
                    let cost = cost.ceil().to_string();
                    fill(said, &[&look.name, &cost])
                }),
                (_, _, Some(refused)) => Some(refused_why(refused).to_string()),
                (true, _, _) => None,
                (false, Some(why), _) => Some(why),
                (false, None, _) => Some(look.disabled.clone().unwrap_or("Not yet".into())),
            };
            let names = bound(entry, slot.0, *page, menu, list);
            let key = names.iter().find_map(|action| bindings.key(action));
            let help = match (entry, building) {
                (Entry::Sell, Some(b)) => (look.help.as_deref())
                    .map(|help| fill(help, &[&sim.0.sale_value(b).to_string()])),
                _ => look.help.clone(),
            };
            hovered = Some(match only.take() {
                Some(only) => Tip {
                    title: Marked::plain(only.as_str()),
                    ..default()
                },
                None => Tip {
                    title: match key {
                        Some(key) => key.mark(&look.name),
                        None => Marked::plain(look.name.as_str()),
                    },
                    help,
                    table,
                    notes,
                    why,
                },
            });
        }
        border.set_if_neq(Edge::lit(lit, hover.get() && ready, Edge::Clear));
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
        (None, Page::Aiming(Aim::Attack)) => {
            prompt(match local.0.map(|p| attack_at(&sim.0, p, *focus)) {
                Some(Struck::Friend(_)) => "Can't attack a friend",
                Some(Struck::Neutral(_)) => "Can't attack a neutral",
                _ => "Left click an enemy or where to attack",
            })
        }
        (None, Page::Aiming(Aim::Guard)) => prompt("Left click what or where to guard"),
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
        (Some(Previewed::Research(a)), Some(Previewed::Research(b))) => a != b,
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

/// What the game's hotkeys call what a button does, and the button
/// itself by its slot, for [`Bindings`].
fn bound(
    entry: Entry,
    slot: usize,
    page: Page,
    menu: &[Recruit],
    researches: &[aaa_map::Research],
) -> Vec<String> {
    let command = |ids: &str| format!("ActorCommand {ids}");
    let mut names = match entry {
        Entry::Order(i) => vec![command(ORDERS[i].0)],
        Entry::Cancel if matches!(page, Page::Menu(_)) => vec!["CancelRecruit".into()],
        Entry::Cancel => vec!["ActorCommandCancel".into()],
        Entry::Open(Menu::Deploy) => vec![command(DEPLOY)],
        Entry::Open(Menu::Location) => vec![command(DEPLOY_TO_LOCATION)],
        Entry::Open(Menu::Research) => vec![command(RESEARCH)],
        Entry::Recruit(i) => vec![format!("ActorDataSelectCommand {}", menu[i].id)],
        Entry::Research(i) => {
            let technology = researches.get(i).map(|r| r.technology.as_str());
            vec![format!(
                "PropertyDataSelectCommand {}",
                technology.unwrap_or("")
            )]
        }
        Entry::Pack => vec![command(PACK)],
        Entry::Sell => vec![command(SELL)],
        Entry::Rally => vec![command(RALLY)],
        Entry::CommandAll => vec!["CommandAll".into()],
    };
    names.push(format!("ActionButtonsButton_{slot}"));
    names
}

/// What pressing a button reads and changes.
#[derive(SystemParam)]
pub struct Presser<'w, 's> {
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
    failed: MessageWriter<'w, Failed>,
    words: Res<'w, crate::confirm::Words>,
    questions: Query<'w, 's, (), With<crate::confirm::Question>>,
    commands: Commands<'w, 's>,
}

impl Presser<'_, '_> {
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
                Some(Give::Disband) => {
                    if let &[regiment] = &self.selected.0[..] {
                        self.ask(slot, player, Give::Disband.command(regiment));
                    }
                }
                None => {}
            },
            Entry::Cancel => *self.page = Page::Orders,
            Entry::Open(menu) => *self.page = Page::Menu(menu),
            Entry::Rally => *self.page = Page::Rally,
            Entry::CommandAll => {
                let building = self.chosen.0.filter(|_| self.selected.0.is_empty());
                let attached: Vec<usize> = shown_hq(sim, &self.selected.0, building)
                    .map_or(Vec::new(), |hq| sim.attached(hq).collect());
                // As the game does, even where that leaves nothing selected.
                self.selected.0 = attached;
                self.chosen.0 = None;
            }
            Entry::Sell => {
                if let Some(b) = self.chosen.0 {
                    self.ask(slot, player, sell_command(b));
                }
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
                if recruit.regiment.is_some() {
                    let template = aaa_map::Id::from(recruit.id.as_str());
                    let command = aaa_sim::Command::Commission {
                        building: b,
                        template: template.clone(),
                    };
                    let given = self.orders.give(sim, player, command);
                    if let Err(e) = given {
                        debug!("can't commission {template}: {e:?}");
                        // As the game does, it says nothing for what's not
                        // offered.
                        if e != aaa_sim::Refused::Lacking {
                            self.refusal.0 = Some((slot, e));
                        }
                    }
                    *self.page = commissioned(*self.page, given);
                }
            }
            Entry::Research(i) => {
                let Some(b) = self.chosen.0 else {
                    return;
                };
                let Some(item) = sim.research_list(b).get(i) else {
                    return;
                };
                let command = aaa_sim::Command::Research {
                    building: b,
                    technology: item.technology.clone(),
                };
                let given = self.orders.give(sim, player, command);
                if let Err(e) = given {
                    debug!("can't research {}: {e:?}", item.technology);
                    self.refuse_research(b, i, e);
                }
                *self.page = commissioned(*self.page, given);
            }
        }
    }
}

impl Presser<'_, '_> {
    /// Asks the player whether they're sure, then gives `command` if it
    /// can still be given. What can't be given now is refused at once, and
    /// a press while a question's up is ignored.
    fn ask(&mut self, slot: usize, player: u32, command: aaa_sim::Command) {
        if !self.questions.is_empty() {
            return;
        }
        if let Err(e) = self.sim.0.check(player, &command) {
            self.refusal.0 = Some((slot, e));
            return;
        }
        let ok = move |_: On<Activate>, mut orders: ResMut<Orders>, sim: Res<Sim>| {
            if let Err(e) = orders.give(&sim.0, player, command.clone()) {
                debug!("can't {command:?}: {e:?}");
            }
        };
        let sure = self.words.sure.clone();
        crate::confirm::confirm(&mut self.commands, &self.words, Screen::Match, sure, ok);
    }

    /// Tells the player the selected building at `building` won't research
    /// the `i`th item it lists, and why.
    fn refuse_research(&mut self, building: usize, i: usize, refused: aaa_sim::Refused) {
        let sim = &self.sim.0;
        let Some(item) = sim.research_list(building).get(i) else {
            return;
        };
        let looks = &self.looks;
        let name = (looks.topics(sim, Some(building)).get(i))
            .map_or(item.technology.as_str(), |t| t.look.name.as_str());
        let text = match refused {
            aaa_sim::Refused::Unaffordable => {
                let cost = sim.research_cost(item).ceil().to_string();
                fill(&looks.research_affording, &[name, &cost])
            }
            _ => fill(&looks.research_refused, &[name]),
        };
        let about = About::Building(building);
        self.failed.write(Failed { text, about });
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
    let list = building.map_or(&[][..], |b| presser.sim.0.research_list(b));
    let page = *presser.page;
    let slot = (0..SLOTS).find(|&slot| {
        let names = presser.shown.0[slot]
            .filter(|e| !matches!(e, Entry::Recruit(i) if *i >= menu.len()))
            .filter(|e| !matches!(e, Entry::Research(i) if *i >= list.len()))
            .map_or(Vec::new(), |e| bound(e, slot, page, menu, list));
        names.iter().any(|action| bindings.pressed(&keys, action))
    });
    if let Some(slot) = slot {
        presser.press(slot);
        return;
    }
    // The key of what's done or under way, its slot left empty, is refused.
    let (Page::Menu(Menu::Research), Some(b), Some(player)) = (page, building, presser.local.0)
    else {
        return;
    };
    let sim = &presser.sim.0;
    let taken = (sim.research_list(b).iter().enumerate()).find(|(_, item)| {
        let action = format!("PropertyDataSelectCommand {}", item.technology);
        bindings.pressed(&keys, &action) && sim.researchable(player, b, item) == Researchable::Taken
    });
    if let Some((i, _)) = taken {
        presser.refuse_research(b, i, aaa_sim::Refused::Unable);
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
        aaa_sim::Refused::Lacking => "Lacks a building or technology it needs",
        aaa_sim::Refused::Unaffordable => "Not enough money",
        aaa_sim::Refused::Full => "HQ is full",
    }
}

/// The order to sell the building at `building`.
fn sell_command(building: usize) -> aaa_sim::Command {
    aaa_sim::Command::Sell { building }
}

/// Whether a building can pack now: not while it's packing or unpacking,
/// nor when it's too hurt.
fn can_pack(sim: &aaa_sim::World, building: usize) -> bool {
    let b = &sim.buildings[building];
    let floor = sim
        .rules()
        .packing
        .get(&b.template)
        .map_or(1.0, |p| p.health_min);
    !b.busy() && b.health > floor * b.fighter.health
}

/// What an attack aimed at `focus` would strike.
#[derive(Debug, PartialEq)]
enum Struck {
    /// The enemy regiment, to go after.
    Enemy(usize),
    /// The player's own regiment or building, or an ally's, which Attack
    /// refuses.
    Friend(About),
    /// A regiment or building that's nobody's enemy, so can't be attacked.
    Neutral(About),
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
        (crate::Focus::Regiment(r), Some(aaa_sim::Relation::Neutral)) => {
            Struck::Neutral(About::Regiment(r))
        }
        (crate::Focus::Building(b), Some(aaa_sim::Relation::Neutral)) => {
            Struck::Neutral(About::Building(b))
        }
        (crate::Focus::Regiment(r), _) => Struck::Friend(About::Regiment(r)),
        (crate::Focus::Building(b), _) => Struck::Friend(About::Building(b)),
    }
}

/// The enemy's regiment or building at `focus`, which Guard refuses.
fn enemy_at(sim: &aaa_sim::World, player: u32, focus: crate::Focus) -> Option<About> {
    let (about, owner) = match focus {
        crate::Focus::Nothing => return None,
        crate::Focus::Regiment(r) => (About::Regiment(r), sim.regiments[r].owner),
        crate::Focus::Building(b) => (About::Building(b), sim.buildings[b].owner),
    };
    (sim.sides.relation(player, owner) == aaa_sim::Relation::Enemy).then_some(about)
}

/// What a guard aimed at `focus` is posted beside: a regiment or building
/// that's no enemy's, and not one of the `selected` giving the guard, which
/// guard where they are instead.
fn guard_at(
    sim: &aaa_sim::World,
    player: u32,
    selected: &[usize],
    focus: crate::Focus,
) -> Option<aaa_sim::Piece> {
    let (thing, owner) = match focus {
        crate::Focus::Nothing => return None,
        crate::Focus::Regiment(r) if selected.contains(&r) => return None,
        crate::Focus::Regiment(r) => (aaa_sim::Piece::Regiment(r), sim.regiments[r].owner),
        crate::Focus::Building(b) => (aaa_sim::Piece::Building(b), sim.buildings[b].owner),
    };
    (sim.sides.relation(player, owner) != aaa_sim::Relation::Enemy).then_some(thing)
}

/// While an order is aimed, a left click on the map or minimap gives it
/// there and a right click calls it off. Attacking an enemy regiment, or
/// its mark on the minimap, sends the selected after it instead, and
/// guarding a friend posts them beside it. A click on something the order
/// refuses tells the player so and calls the order off. Unpacking somewhere
/// the building wouldn't fit waits for another click. A rally point is set
/// for the selected building.
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
    mut refused: MessageWriter<Refused>,
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
            Struck::Friend(about) => {
                refused.write(Refused {
                    about,
                    by: Some(aim),
                });
                *page = Page::Orders;
                return;
            }
            Struck::Neutral(about) => {
                refused.write(Refused { about, by: None });
                *page = Page::Orders;
                return;
            }
            Struck::Ground => {}
            Struck::Enemy(e) => {
                let (pursuing, rest) = crate::pursue(&sim.0, &mut orders, player, &selected.0, e);
                // An enemy none of them can attack is no target either.
                if pursuing.is_empty() {
                    let about = About::Regiment(e);
                    refused.write(Refused { about, by: None });
                    *page = Page::Orders;
                    return;
                }
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
    // Enemy marks are all a minimap click picks out, to attack; guarding
    // there guards the place.
    if aim == Aim::Guard
        && matches!(at, crate::Pointing::Ground(_))
        && let Some(about) = enemy_at(&sim.0, player, focus)
    {
        refused.write(Refused {
            about,
            by: Some(aim),
        });
        *page = Page::Orders;
        return;
    }
    if aim == Aim::Guard
        && let Some(piece) = guard_at(&sim.0, player, &selected.0, focus)
    {
        let guard = |regiment| aaa_sim::Command::GuardPiece { regiment, piece };
        let regiments: Vec<(usize, [f32; 2])> = (selected.0.iter())
            .filter(|&&r| orders.give(&sim.0, player, guard(r)).is_ok())
            .map(|&r| (r, point))
            .collect();
        if regiments.is_empty() {
            return;
        }
        crate::acknowledge(&sim.0, &mut sounds, &time, player, &selected.0);
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
                facing: UNPACK_FACING,
            };
            if orders.give(&sim.0, player, unpack).is_err() {
                return;
            }
            let at = aaa_sim::unpack_site(point);
            crate::acknowledge(&sim.0, &mut sounds, &time, player, &selected.0);
            vec![(r, at)]
        }
        Aim::Move | Aim::Attack | Aim::Run | Aim::Guard => {
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

    const fn at(slot: usize) -> Option<Place> {
        Some(Place { slot, high: false })
    }

    const fn high(slot: usize) -> Option<Place> {
        Some(Place { slot, high: true })
    }

    /// The slots the game's data gives [`ORDERS`].
    const GAME: [Option<Place>; 9] = [
        at(0),
        at(1),
        at(2),
        at(3),
        at(6),
        at(10),
        at(11),
        at(9),
        high(11),
    ];

    #[test]
    fn attacks_strike_only_enemies() {
        use crate::Focus::{Building, Nothing, Regiment};
        use aaa_sim::Relation::{Ally, Enemy, Itself, Neutral};
        assert_eq!(struck(Regiment(3), Some(Enemy)), Struck::Enemy(3));
        for friend in [Itself, Ally] {
            let refused = |about| Struck::Friend(about);
            assert_eq!(
                struck(Regiment(3), Some(friend)),
                refused(About::Regiment(3))
            );
            assert_eq!(
                struck(Building(3), Some(friend)),
                refused(About::Building(3))
            );
        }
        let neutral = Struck::Neutral(About::Regiment(3));
        assert_eq!(struck(Regiment(3), Some(Neutral)), neutral);
        assert_eq!(struck(Building(3), Some(Enemy)), Struck::Ground);
        assert_eq!(struck(Nothing, None), Struck::Ground);
    }

    #[test]
    fn orders_take_their_slots() {
        // A regiment attached to an HQ, which can't be disbanded.
        let mut all = [true; 9];
        all[8] = false;
        let grid = layout(Page::Orders, &GAME, &all, None, false);
        assert_eq!(grid[0], Some(Entry::Order(0)));
        assert_eq!(grid[3], Some(Entry::Order(3)));
        assert_eq!(grid[6], Some(Entry::Order(4)));
        assert_eq!(grid[11], Some(Entry::Order(6)));
        assert_eq!(grid.iter().flatten().count(), 8);
        let truck = [true, false, false, true, false, false, false, true, true];
        let grid = layout(Page::Orders, &GAME, &truck, None, false);
        assert_eq!(grid[9], Some(Entry::Order(7)));
        assert_eq!(grid[11], Some(Entry::Order(8)));
        assert_eq!(grid.iter().flatten().count(), 4);
        // Beside Detach from HQ, as never happens in the game, it gives way.
        let grid = layout(Page::Orders, &GAME, &[true; 9], None, false);
        assert_eq!(grid[11], Some(Entry::Order(6)));
        assert_eq!(grid[8], Some(Entry::Order(8)));
    }

    #[test]
    fn hqs_have_a_button_for_their_regiments_where_nothing_else_is() {
        let truck = [true, false, false, true, false, false, false, true, true];
        let grid = layout(Page::Orders, &GAME, &truck, None, true);
        assert_eq!(grid[COMMAND_ALL], Some(Entry::CommandAll));
        assert_eq!(grid.iter().flatten().count(), 5);
        // Not over an order, nor while one's aimed.
        let mut slots = GAME;
        slots[0] = at(COMMAND_ALL);
        let grid = layout(Page::Orders, &slots, &truck, None, true);
        assert_eq!(grid[COMMAND_ALL], Some(Entry::Order(0)));
        let grid = layout(Page::Aiming(Aim::Move), &GAME, &truck, None, true);
        assert!(!grid.contains(&Some(Entry::CommandAll)));
    }

    #[test]
    fn buttons_go_by_the_names_the_hotkeys_bind() {
        let names = |entry, page| bound(entry, 4, page, &[], &[]);
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
            names(Entry::Cancel, Page::Menu(Menu::Location))[0],
            "CancelRecruit"
        );
        assert_eq!(
            names(Entry::Open(Menu::Location), Page::Orders)[0],
            "ActorCommand recruit_location"
        );
        assert_eq!(names(Entry::Cancel, Page::Rally)[0], "ActorCommandCancel");
        assert_eq!(
            names(Entry::Open(Menu::Research), Page::Orders)[0],
            "ActorCommand research"
        );
    }

    #[test]
    fn aiming_shows_the_order_and_a_way_out() {
        let grid = layout(Page::Aiming(Aim::Run), &GAME, &[true; 9], None, false);
        assert_eq!(grid[2], Some(Entry::Order(2)));
        assert_eq!(grid[CANCEL], Some(Entry::Cancel));
        assert_eq!(grid.iter().flatten().count(), 2);
    }

    #[test]
    fn commissioning_goes_back_to_the_buildings_buttons() {
        let menu = Page::Menu(Menu::Deploy);
        assert_eq!(commissioned(menu, Ok(())), Page::Orders);
        assert_eq!(commissioned(menu, Err(aaa_sim::Refused::Unable)), menu);
    }

    #[test]
    fn buildings_open_a_menu_of_what_they_commission() {
        let corps = [(Menu::Deploy, Offer::Offered); 11];
        let hq = BuildingButtons {
            deploy: at(0),
            location: high(1),
            recruits: &corps,
            research: None,
            researches: &[],
            rally: at(5),
            pack: at(9),
            sell: at(11),
        };
        let grid = layout(Page::Orders, &GAME, &[true; 9], Some(hq), false);
        assert_eq!(grid[0], Some(Entry::Open(Menu::Deploy)));
        assert_eq!(grid[5], Some(Entry::Rally));
        assert_eq!(grid[9], Some(Entry::Pack));
        assert_eq!(grid[11], Some(Entry::Sell));
        assert_eq!(grid.iter().flatten().count(), 4);
        let depot = BuildingButtons {
            recruits: &[],
            rally: None,
            pack: None,
            sell: None,
            ..hq
        };
        assert!(
            layout(Page::Orders, &GAME, &[true; 9], Some(depot), false)
                .iter()
                .all(Option::is_none)
        );
        let menu = layout(Page::Menu(Menu::Deploy), &GAME, &[true; 9], Some(hq), false);
        assert_eq!(menu[10], Some(Entry::Recruit(10)));
        assert_eq!(menu[MENU_CANCEL], Some(Entry::Cancel));
        let rally = layout(Page::Rally, &GAME, &[true; 9], Some(hq), false);
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
            deploy: at(0),
            location: high(1),
            recruits: &infantry,
            research: None,
            researches: &[],
            rally: at(5),
            pack: None,
            sell: None,
        };
        let grid = layout(Page::Orders, &GAME, &[true; 9], Some(hq), false);
        assert_eq!(grid[0], Some(Entry::Open(Menu::Deploy)));
        assert_eq!(grid[1], None);
        // With everything hidden it still has its rally point.
        let hidden = [(Menu::Deploy, Hidden); 2];
        let none = BuildingButtons {
            recruits: &hidden,
            ..hq
        };
        let grid = layout(Page::Orders, &GAME, &[true; 9], Some(none), false);
        assert_eq!(grid[..2], [None, None]);
        assert_eq!(grid[5], Some(Entry::Rally));
        let menu = layout(Page::Menu(Menu::Deploy), &GAME, &[true; 9], Some(hq), false);
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
        let grid = layout(Page::Orders, &GAME, &[true; 9], Some(ship), false);
        assert_eq!(grid[0], Some(Entry::Open(Menu::Deploy)));
        assert_eq!(grid[1], Some(Entry::Open(Menu::Location)));
        // Its slot taken, a button takes the first free one, or the last for
        // one of high importance.
        let crowded = BuildingButtons {
            location: high(0),
            rally: at(0),
            ..ship
        };
        let grid = layout(Page::Orders, &GAME, &[true; 9], Some(crowded), false);
        assert_eq!(grid[0], Some(Entry::Open(Menu::Deploy)));
        assert_eq!(grid[11], Some(Entry::Open(Menu::Location)));
        assert_eq!(grid[1], Some(Entry::Rally));
        let deploy = layout(
            Page::Menu(Menu::Deploy),
            &GAME,
            &[true; 9],
            Some(ship),
            false,
        );
        assert_eq!(deploy[0], Some(Entry::Recruit(1)));
        assert_eq!(deploy.iter().flatten().count(), 2);
        let carried = layout(
            Page::Menu(Menu::Location),
            &GAME,
            &[true; 9],
            Some(ship),
            false,
        );
        assert_eq!(
            carried[..3],
            [Some(Entry::Recruit(0)), None, Some(Entry::Recruit(3))]
        );
    }

    #[test]
    fn research_menus_keep_slots_for_whats_done_and_close_up_whats_hidden() {
        use Researchable::{Hidden, Lacking, Ready, Taken};
        let researches = [
            Taken,
            Hidden,
            Ready,
            Lacking(vec![Id::from("depot")]),
            Ready,
        ];
        let pool = BuildingButtons {
            deploy: None,
            location: None,
            recruits: &[],
            research: at(6),
            researches: &researches,
            rally: None,
            pack: None,
            sell: None,
        };
        let grid = layout(Page::Orders, &GAME, &[true; 9], Some(pool), false);
        assert_eq!(grid[6], Some(Entry::Open(Menu::Research)));
        assert_eq!(grid.iter().flatten().count(), 1);
        let menu = layout(
            Page::Menu(Menu::Research),
            &GAME,
            &[true; 9],
            Some(pool),
            false,
        );
        assert_eq!(
            menu[..4],
            [
                None,
                Some(Entry::Research(2)),
                Some(Entry::Research(3)),
                Some(Entry::Research(4))
            ]
        );
        assert_eq!(menu[MENU_CANCEL], Some(Entry::Cancel));
        // The button stays once everything's researched.
        let done = BuildingButtons {
            researches: &[Taken, Taken],
            ..pool
        };
        let grid = layout(Page::Orders, &GAME, &[true; 9], Some(done), false);
        assert_eq!(grid[6], Some(Entry::Open(Menu::Research)));
    }
}
