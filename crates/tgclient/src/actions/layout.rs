//! Which orders and menu entries the grid offers, and the slots they
//! take.

use bevy::prelude::*;
use tgdata::Id;
use tgsim::{Lacking, Researchable};

use crate::control_groups;

use super::looks::{Looks, look_of};
use super::press::{can_pack, sell_command, shown_hq};
use super::{
    CANCEL, COMMAND_ALL, Entry, Face, Give, MENU_CANCEL, Menu, ORDERS, Offer, Page, SLOTS, Shown,
};

/// Where a command's button goes: its slot and, that taken, which end of
/// the grid it takes a free one from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Place {
    pub(crate) slot: usize,
    /// Whether it takes the last free slot, rather than the first.
    pub(crate) high: bool,
}

/// Puts `entry` at `place` or, that slot taken, in the first free one, or
/// the last for one of high importance. A slot off the grid has no button.
pub(crate) fn put(out: &mut [Option<Entry>; SLOTS], place: Option<Place>, entry: Entry) {
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

/// Puts each of `shown`, given in the game's order of commands, in its
/// slot unless one later in the order wants it; only then do those left
/// out move on, so none of them pushes another out of its own slot.
pub(crate) fn place(
    out: &mut [Option<Entry>; SLOTS],
    shown: impl DoubleEndedIterator<Item = (Option<Place>, Entry)>,
) {
    let wanted = shown.filter_map(|(p, entry)| Some((p.filter(|p| p.slot < SLOTS)?, entry)));
    let mut lost = Vec::new();
    for (place, entry) in wanted.rev() {
        match out[place.slot] {
            None => out[place.slot] = Some(entry),
            Some(_) => lost.push((place, entry)),
        }
    }
    for (place, entry) in lost.into_iter().rev() {
        put(out, Some(place), entry);
    }
}

/// The buttons a selected building has: where its menus' and rally point
/// buttons go, which menu each thing it recruits is in and how it's
/// offered, and where its Pack button goes, if it packs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BuildingButtons<'a> {
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
    /// Where its Cancel button goes, while it's a site going up, which
    /// has no other.
    site: Option<Place>,
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
/// instead, or on its way to unpack; while it fights it's greyed. Only
/// where the game has HQs can organizations attach to them, and only where
/// it has heroes can one company selected alone take one on or let it go.
/// Rout shows while any of them could rout now. Two or more have the
/// Group Behavior switch of their current group, `behavior`, to flip.
pub(crate) fn offered(
    sim: &tgsim::World,
    player: Option<u32>,
    selected: &[usize],
    has: GameHas,
    behavior: Option<bool>,
) -> [bool; ORDERS.len()] {
    let packed = |&r: &usize| sim.organizations[r].unpacks.is_some();
    let all_packed = !selected.is_empty() && selected.iter().all(packed);
    let lone = selected.iter().all(|&r| sim.lone(r));
    let disband = match selected {
        &[r] => {
            let r = &sim.organizations[r];
            r.hq.is_none() && r.unpack_at.is_none()
        }
        _ => false,
    };
    // A company leads with its captain, whose place a hero takes.
    let led = match selected {
        &[r] => has.heroes && sim.organizations[r].leader.is_some() && !sim.lone(r) && !all_packed,
        _ => false,
    };
    let rout = selected.iter().any(|&r| can(sim, player, Give::Rout, r));
    let builds = builder(sim, selected).is_some();
    let repairs = selected.iter().any(|&r| repairs(sim, r));
    ORDERS.map(|(ids, _)| match ids {
        "construct" => builds,
        "repair" => repairs,
        "unpack" => all_packed && selected.len() == 1,
        "rout" => rout,
        "kill" => !all_packed,
        "hq_attach" => has.hqs && !all_packed,
        "hq_detach" => has.hqs && !all_packed && !disband,
        "retreat" | "guard" => !all_packed || !lone,
        "disband" => disband,
        "attach" | "detach" => led,
        control_groups::ENABLE => behavior == Some(false),
        control_groups::DISABLE => behavior == Some(true),
        _ => true,
    })
}

/// The first of the selected organizations that can build anything, which
/// the Build menu is for.
pub(crate) fn builder(sim: &tgsim::World, selected: &[usize]) -> Option<usize> {
    (selected.iter().copied())
        .find(|&r| sim.organizations[r].unpacks.is_none() && !sim.buildable(r).is_empty())
}

/// What the Build menu offers the selection.
pub(crate) fn builds(sim: &tgsim::World, selected: &[usize]) -> Vec<Id> {
    let mut out = builder(sim, selected).map_or(Vec::new(), |r| sim.buildable(r));
    out.retain(|t| sim.rules().buildable.contains_key(t));
    out
}

/// Whether the organization at `r` has a member that repairs.
pub(crate) fn repairs(sim: &tgsim::World, r: usize) -> bool {
    let fighter = |u: usize| sim.units[u].fighter.as_ref();
    sim.organizations[r].unpacks.is_none()
        && (sim.survivors(r)).any(|u| fighter(u).is_some_and(|f| !f.repairs.is_empty()))
}

/// Whether `player` could give `give` to the organization at `r` now.
pub(crate) fn can(sim: &tgsim::World, player: Option<u32>, give: Give, r: usize) -> bool {
    let command = || give.command(r);
    player.is_some_and(|p| command().is_some_and(|c| sim.check(p, &c).is_ok()))
}

/// What the game's data has that some orders need, which a match can't
/// change.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GameHas {
    /// Templates that take attachments, as HQs.
    hqs: bool,
    heroes: bool,
}

impl GameHas {
    pub(crate) fn of(rules: &tgsim::Rules) -> GameHas {
        let attaching = |l: &tgmap::Logistics| l.attachments.is_some_and(|n| n > 0);
        GameHas {
            hqs: rules.logistics.values().any(attaching),
            heroes: !rules.heroes.is_empty(),
        }
    }
}

/// The buttons a page has, by slot, given each of [`ORDERS`]' slot and
/// whether the selection has it, or a selected building's buttons.
pub(crate) fn layout(
    page: Page,
    slots: &[Option<Place>],
    offered: &[bool],
    building: Option<BuildingButtons>,
    hq: bool,
    builds: usize,
) -> [Option<Entry>; SLOTS] {
    let mut out = [None; SLOTS];
    match (page, building) {
        (Page::Orders, Some(BuildingButtons { site: Some(p), .. })) => {
            put(&mut out, Some(p), Entry::CancelSite);
        }
        // Aiming a delivery, the grid is back to the building's own.
        (Page::Orders | Page::Delivering(_), Some(b)) => {
            // In the game's order of commands.
            let buttons = [
                (b.offers(Menu::Deploy), b.deploy, Entry::Open(Menu::Deploy)),
                (true, b.rally, Entry::Rally),
                (
                    b.offers(Menu::Location),
                    b.location,
                    Entry::Open(Menu::Location),
                ),
                (true, b.sell, Entry::Sell),
                (true, b.pack, Entry::Pack),
                // Even once there's nothing left to research.
                (
                    !b.researches.is_empty(),
                    b.research,
                    Entry::Open(Menu::Research),
                ),
            ];
            let shown = buttons.into_iter().filter(|(shows, _, _)| *shows);
            place(&mut out, shown.map(|(_, place, entry)| (place, entry)));
        }
        (Page::Menu(Menu::Build), None) => {
            for (i, slot) in (0..builds).zip(&mut out[..MENU_CANCEL]) {
                *slot = Some(Entry::Build(i));
            }
            out[MENU_CANCEL] = Some(Entry::Cancel);
        }
        (Page::Placing(_), None) => out[CANCEL] = Some(Entry::Cancel),
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
        (Page::Menu(_) | Page::Rally | Page::Delivering(_), None) => {}
        (Page::Placing(_), Some(_)) => {}
        (Page::Orders, None) => {
            let shown = (slots.iter().enumerate()).filter(|&(i, _)| offered[i]);
            place(&mut out, shown.map(|(i, &p)| (p, Entry::Order(i))));
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

/// Whether pressing the button for `entry` can do anything now.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ready(
    entry: Entry,
    sim: &tgsim::World,
    looks: &Looks,
    selected: &[usize],
    building: Option<usize>,
    local: Option<u32>,
    lacks: &[Lacking],
    researches: &[Researchable],
) -> bool {
    match entry {
        Entry::Order(i) => match (ORDERS[i].1, local) {
            (
                Some(give @ (Give::Attach | Give::Detach | Give::Disband | Give::DetachHero)),
                Some(player),
            ) => selected.iter().any(|&r| can(sim, Some(player), give, r)),
            (give, _) => give.is_some(),
        },
        Entry::Research(i) => researches[i] == Researchable::Ready,
        Entry::Pack => building.is_some_and(|b| can_pack(sim, b)),
        Entry::Recruit(i) => {
            let lacking = &lacks[i];
            looks.menu(sim, building)[i].organization.is_some()
                && lacking.properties.is_empty()
                && lacking.structures.is_empty()
        }
        Entry::Cancel
        | Entry::Open(_)
        | Entry::Rally
        | Entry::CommandAll
        | Entry::Sell
        | Entry::Build(_)
        | Entry::CancelSite => true,
    }
}

/// Whether the button for `entry` is lit for the page it's opened.
pub(crate) fn lit(entry: Entry, page: Page) -> bool {
    match entry {
        Entry::Order(i) => {
            matches!((page, ORDERS[i].1), (Page::Aiming(a), Some(Give::Aim(b))) if a == b)
        }
        Entry::Rally => page == Page::Rally,
        _ => false,
    }
}

/// What the grid holds for the page and the selection, whichever
/// interface draws it.
pub(crate) fn contents(
    page: Page,
    sim: &tgsim::World,
    looks: &Looks,
    selected: &[usize],
    building: Option<usize>,
    local: Option<u32>,
    behavior: Option<bool>,
) -> Shown {
    let hq = shown_hq(sim, selected, building);
    let menu = looks.menu(sim, building);
    let lacks: Vec<Lacking> = (menu.iter())
        .map(|r| match (building, &r.organization, local) {
            (Some(b), Some(organization), Some(player)) => sim.lacks(player, b, organization),
            _ => Lacking::default(),
        })
        .collect();
    let recruits: Vec<(Menu, Offer)> = (menu.iter().zip(&lacks))
        .map(|(r, lacking)| (r.menu, Offer::from(lacking)))
        .collect();
    let topics = looks.topics(sim, building);
    let list = building.map_or(&[][..], |b| sim.research_list(b));
    // Only what has a look, should the two ever differ.
    let list = &list[..list.len().min(topics.len())];
    let mut researches: Vec<Researchable> = match (building, local) {
        (Some(b), Some(player)) => sim.researchables(player, b),
        _ => vec![Researchable::Hidden; list.len()],
    };
    researches.truncate(list.len());
    let order_slots: Vec<Option<Place>> = looks.orders.iter().map(|(s, _)| *s).collect();
    let builds = builds(sim, selected);
    let entries = layout(
        page,
        &order_slots,
        &offered(sim, local, selected, looks.has, behavior),
        building.map(|b| BuildingButtons {
            deploy: looks.deploy.0,
            location: looks.location.0,
            recruits: &recruits,
            research: looks.research.0,
            researches: &researches,
            // Where what it raises goes matters whether or not any of it
            // can be raised now.
            rally: looks.rally.0.filter(|_| !menu.is_empty()),
            pack: looks
                .pack
                .0
                .filter(|_| sim.rules().packing.contains_key(&sim.buildings[b].template)),
            sell: looks
                .sell
                .0
                .filter(|_| local.is_some_and(|p| sim.check(p, &sell_command(b)).is_ok())),
            site: looks.site.filter(|_| sim.buildings[b].constructing),
        }),
        hq.is_some(),
        builds.len(),
    );
    let faces = entries.map(|entry| {
        entry.map(|e| Face {
            icon: look_of(e, sim, looks, building, &builds).icon.clone(),
            ready: ready(
                e,
                sim,
                looks,
                selected,
                building,
                local,
                &lacks,
                &researches,
            ),
        })
    });
    Shown {
        commanded: true,
        entries,
        faces,
        lacks,
        researches,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Aim;

    const fn at(slot: usize) -> Option<Place> {
        Some(Place { slot, high: false })
    }

    const fn high(slot: usize) -> Option<Place> {
        Some(Place { slot, high: true })
    }

    /// The slots the game's data gives [`ORDERS`].
    const GAME: [Option<Place>; ORDERS.len()] = [
        at(1),
        at(6),
        at(5),
        at(4),
        at(0),
        at(3),
        at(2),
        at(10),
        high(11),
        at(9),
        at(9),
        at(10),
        at(10),
        at(11),
        at(4),
        at(4),
    ];

    /// Which of [`ORDERS`] the selection has, by command id.
    fn having(ids: &[&str]) -> [bool; ORDERS.len()] {
        ORDERS.map(|(id, _)| ids.contains(&id))
    }

    /// Where [`ORDERS`]' `id` is.
    fn order(id: &str) -> Option<Entry> {
        ORDERS.iter().position(|(i, _)| *i == id).map(Entry::Order)
    }

    #[test]
    fn hq_and_hero_orders_are_offered_only_where_the_game_has_them() {
        let mut w = crate::orders::testing::fresh();
        let ids = |offered: [bool; ORDERS.len()]| -> Vec<&str> {
            (ORDERS.iter().zip(offered))
                .filter(|&(_, o)| o)
                .map(|((ids, _), _)| *ids)
                .filter(|ids| ids.contains("attach") || ids.contains("detach"))
                .collect()
        };
        let both = GameHas {
            hqs: true,
            heroes: true,
        };
        assert!(ids(offered(&w, None, &[0], GameHas::default(), None)).is_empty());
        // A hero takes a captain's place, which this has none of.
        assert_eq!(ids(offered(&w, None, &[0], both, None)), ["hq_attach"]);
        w.organizations[0].leader = Some(0);
        let led = ["attach", "detach", "hq_attach"];
        assert_eq!(ids(offered(&w, None, &[0], both, None)), led);
        // One company at a time.
        assert_eq!(
            ids(offered(&w, None, &[0, 1], both, None)),
            ["hq_attach", "hq_detach"]
        );
    }

    #[test]
    fn rout_is_offered_only_in_a_fight() {
        let mut w = crate::orders::testing::fresh();
        let i = ORDERS.iter().position(|(id, _)| *id == "rout").unwrap();
        let routs = |w: &tgsim::World, player, selected: &[usize]| {
            offered(w, Some(player), selected, GameHas::default(), None)[i]
        };
        w.organizations[0].morale_max = 50.0;
        w.organizations[0].morale = 50.0;
        assert!(!routs(&w, 1, &[0]));
        w.organizations[0]
            .engaged
            .push(tgsim::Piece::Organization(1));
        assert!(routs(&w, 1, &[0]));
        // Any one of several will do; another side's never does.
        assert!(routs(&w, 1, &[1, 0]));
        assert!(!routs(&w, 2, &[0]));
    }

    #[test]
    fn orders_take_their_slots() {
        let attached = having(&[
            "move",
            "kill",
            "retreat",
            "stop",
            "guard",
            "hq_attach",
            "hq_detach",
        ]);
        let grid = layout(Page::Orders, &GAME, &attached, None, false, 0);
        for (slot, id) in [
            (0, "move"),
            (1, "kill"),
            (3, "stop"),
            (6, "guard"),
            (11, "hq_detach"),
        ] {
            assert_eq!(grid[slot], order(id));
        }
        assert_eq!(grid.iter().flatten().count(), 7);
        let truck = having(&["move", "stop", "unpack", "disband"]);
        let grid = layout(Page::Orders, &GAME, &truck, None, false, 0);
        assert_eq!(grid[9], order("unpack"));
        assert_eq!(grid[11], order("disband"));
        assert_eq!(grid.iter().flatten().count(), 4);
    }

    #[test]
    fn of_two_wanting_a_slot_the_later_keeps_it_and_the_other_moves_on() {
        // As never happens in the game: Detach from HQ beside Disband, and
        // Attach to HQ beside Rout.
        let both = having(&[
            "move",
            "kill",
            "retreat",
            "stop",
            "guard",
            "hq_attach",
            "hq_detach",
            "disband",
        ]);
        let grid = layout(Page::Orders, &GAME, &both, None, false, 0);
        assert_eq!(grid[11], order("hq_detach"));
        assert_eq!(grid[10], order("hq_attach"));
        assert_eq!(grid[9], order("disband"));
        let rout = having(&["move", "guard", "rout", "hq_attach"]);
        let grid = layout(Page::Orders, &GAME, &rout, None, false, 0);
        assert_eq!(grid[10], order("hq_attach"));
        // The first slot no shown order wants, not Move's.
        assert_eq!(grid[0], order("move"));
        assert_eq!(grid[1], order("rout"));
    }

    #[test]
    fn the_build_menu_lists_what_can_be_built_and_sites_offer_only_cancel() {
        // Not with Group Behavior's, which would take Construct's slot.
        let all = ORDERS.map(|(id, _)| !id.starts_with("group_behavior"));
        let menu = layout(Page::Menu(Menu::Build), &GAME, &all, None, false, 3);
        let built: Vec<Option<Entry>> = (0..3).map(|i| Some(Entry::Build(i))).collect();
        assert_eq!(menu[..3], built[..]);
        assert_eq!(menu.iter().flatten().count(), 4);
        assert_eq!(menu[MENU_CANCEL], Some(Entry::Cancel));
        let grid = layout(Page::Orders, &GAME, &all, None, false, 3);
        assert_eq!(grid[4], order("construct"));
        assert_eq!(grid[5], order("repair"));
        let site = BuildingButtons {
            deploy: at(0),
            location: None,
            recruits: &[],
            research: None,
            researches: &[],
            rally: at(5),
            pack: at(9),
            sell: None,
            site: high(11),
        };
        let grid = layout(Page::Orders, &GAME, &all, Some(site), false, 0);
        assert_eq!(grid[11], Some(Entry::CancelSite));
        assert_eq!(grid.iter().flatten().count(), 1);
    }

    #[test]
    fn hqs_have_a_button_for_their_organizations_where_nothing_else_is() {
        let truck = having(&["move", "stop", "unpack", "disband"]);
        let grid = layout(Page::Orders, &GAME, &truck, None, true, 0);
        assert_eq!(grid[COMMAND_ALL], Some(Entry::CommandAll));
        assert_eq!(grid.iter().flatten().count(), 5);
        // Not over an order, nor while one's aimed.
        let mut slots = GAME;
        let moving = ORDERS.iter().position(|(id, _)| *id == "move").unwrap();
        slots[moving] = at(COMMAND_ALL);
        let grid = layout(Page::Orders, &slots, &truck, None, true, 0);
        assert_eq!(grid[COMMAND_ALL], order("move"));
        let grid = layout(Page::Aiming(Aim::Move), &GAME, &truck, None, true, 0);
        assert!(!grid.contains(&Some(Entry::CommandAll)));
    }

    #[test]
    fn aiming_shows_the_order_and_a_way_out() {
        let grid = layout(
            Page::Aiming(Aim::Run),
            &GAME,
            &[true; ORDERS.len()],
            None,
            false,
            0,
        );
        assert_eq!(grid[2], order("retreat"));
        assert_eq!(grid[CANCEL], Some(Entry::Cancel));
        assert_eq!(grid.iter().flatten().count(), 2);
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
            site: None,
        };
        let grid = layout(
            Page::Orders,
            &GAME,
            &[true; ORDERS.len()],
            Some(hq),
            false,
            0,
        );
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
            layout(
                Page::Orders,
                &GAME,
                &[true; ORDERS.len()],
                Some(depot),
                false,
                0
            )
            .iter()
            .all(Option::is_none)
        );
        let menu = layout(
            Page::Menu(Menu::Deploy),
            &GAME,
            &[true; ORDERS.len()],
            Some(hq),
            false,
            0,
        );
        assert_eq!(menu[10], Some(Entry::Recruit(10)));
        assert_eq!(menu[MENU_CANCEL], Some(Entry::Cancel));
        let rally = layout(
            Page::Rally,
            &GAME,
            &[true; ORDERS.len()],
            Some(hq),
            false,
            0,
        );
        assert_eq!(rally[5], Some(Entry::Rally));
        assert_eq!(rally[CANCEL], Some(Entry::Cancel));
        assert_eq!(rally.iter().flatten().count(), 2);
    }

    #[test]
    fn what_a_transport_carries_has_a_menu_of_its_own() {
        use Offer::{Greyed, Hidden, Offered};
        // An Infantry HQ: its amphibious organization, last, hidden.
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
            site: None,
        };
        let grid = layout(
            Page::Orders,
            &GAME,
            &[true; ORDERS.len()],
            Some(hq),
            false,
            0,
        );
        assert_eq!(grid[0], Some(Entry::Open(Menu::Deploy)));
        assert_eq!(grid[1], None);
        // With everything hidden it still has its rally point.
        let hidden = [(Menu::Deploy, Hidden); 2];
        let none = BuildingButtons {
            recruits: &hidden,
            ..hq
        };
        let grid = layout(
            Page::Orders,
            &GAME,
            &[true; ORDERS.len()],
            Some(none),
            false,
            0,
        );
        assert_eq!(grid[..2], [None, None]);
        assert_eq!(grid[5], Some(Entry::Rally));
        let menu = layout(
            Page::Menu(Menu::Deploy),
            &GAME,
            &[true; ORDERS.len()],
            Some(hq),
            false,
            0,
        );
        assert_eq!(
            menu[..3],
            [Some(Entry::Recruit(0)), Some(Entry::Recruit(1)), None]
        );
        // A transported organization and a truck: one in each menu, each at the
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
        let grid = layout(
            Page::Orders,
            &GAME,
            &[true; ORDERS.len()],
            Some(ship),
            false,
            0,
        );
        assert_eq!(grid[0], Some(Entry::Open(Menu::Deploy)));
        assert_eq!(grid[1], Some(Entry::Open(Menu::Location)));
        // Of three wanting one slot, the latest in the game's order keeps it
        // and the others take the first free ones in turn.
        let crowded = BuildingButtons {
            location: high(0),
            rally: at(0),
            ..ship
        };
        let grid = layout(
            Page::Orders,
            &GAME,
            &[true; ORDERS.len()],
            Some(crowded),
            false,
            0,
        );
        assert_eq!(grid[0], Some(Entry::Open(Menu::Location)));
        assert_eq!(grid[1], Some(Entry::Open(Menu::Deploy)));
        assert_eq!(grid[2], Some(Entry::Rally));
        // One of high importance counts down from the last.
        let crowded = BuildingButtons {
            sell: at(1),
            ..ship
        };
        let grid = layout(
            Page::Orders,
            &GAME,
            &[true; ORDERS.len()],
            Some(crowded),
            false,
            0,
        );
        assert_eq!(grid[1], Some(Entry::Sell));
        assert_eq!(grid[11], Some(Entry::Open(Menu::Location)));
        let deploy = layout(
            Page::Menu(Menu::Deploy),
            &GAME,
            &[true; ORDERS.len()],
            Some(ship),
            false,
            0,
        );
        assert_eq!(deploy[0], Some(Entry::Recruit(1)));
        assert_eq!(deploy.iter().flatten().count(), 2);
        let carried = layout(
            Page::Menu(Menu::Location),
            &GAME,
            &[true; ORDERS.len()],
            Some(ship),
            false,
            0,
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
            site: None,
        };
        let grid = layout(
            Page::Orders,
            &GAME,
            &[true; ORDERS.len()],
            Some(pool),
            false,
            0,
        );
        assert_eq!(grid[6], Some(Entry::Open(Menu::Research)));
        assert_eq!(grid.iter().flatten().count(), 1);
        let menu = layout(
            Page::Menu(Menu::Research),
            &GAME,
            &[true; ORDERS.len()],
            Some(pool),
            false,
            0,
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
        let grid = layout(
            Page::Orders,
            &GAME,
            &[true; ORDERS.len()],
            Some(done),
            false,
            0,
        );
        assert_eq!(grid[6], Some(Entry::Open(Menu::Research)));
    }
}
