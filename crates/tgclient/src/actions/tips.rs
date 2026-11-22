//! What a button's tooltip says: what it does, costs and needs, or why
//! it can't be pressed.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use tgsim::Researchable;

use crate::keys::Bindings;
use crate::resource_bar::{Preview, Previewed};
use crate::ui::Marked;
use crate::{Local, Selected, SelectedBuilding, Sim};

use super::aim::{Struck, attack_at};
use super::layout::builds;
use super::looks::{Looks, Quantity, look_of};
use super::press::bound;
use super::{Aim, Entry, Give, ORDERS, Page, Refusal, Shown, Slot};

/// What the tooltip says: the button's name, what it does, a table of
/// such as what it costs, and why it can't be pressed.
#[derive(Clone, PartialEq, Default, Debug)]
pub struct Tip {
    /// The button's name, with the key that presses it marked.
    pub title: Marked,
    pub help: Option<String>,
    pub table: Vec<Row>,
    /// Lines under the table.
    pub notes: Vec<String>,
    pub why: Option<String>,
}

/// A row of the tooltip's table, such as Cost and what it is.
#[derive(Clone, PartialEq, Debug)]
pub struct Row {
    pub label: String,
    pub amounts: Vec<Quantity>,
}

/// Previews on the resource bar what the order being aimed, or the
/// button under the cursor, would do to the economy, and forgets a
/// refusal once the cursor's left its button, whichever interface draws
/// the grid.
#[allow(clippy::too_many_arguments)]
pub fn hover(
    page: Res<Page>,
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    sim: Res<Sim>,
    looks: Res<Looks>,
    shown: Res<Shown>,
    local: Res<Local>,
    slots: Query<(&Slot, &Hovered)>,
    mut refusal: ResMut<Refusal>,
    mut preview: ResMut<Preview>,
) {
    for (slot, hover) in &slots {
        if !hover.get() && refusal.0.is_some_and(|(s, _)| s == slot.0) {
            refusal.0 = None;
        }
    }
    let building = chosen.0.filter(|_| selected.0.is_empty());
    // What was under the cursor goes with the selection.
    let previewed = match crate::selection::commanded(&sim.0, local.0, &selected.0, building) {
        true => previewed(*page, &sim.0, &looks, &shown, &selected.0, building, &slots),
        false => None,
    };
    if preview.0 != previewed {
        preview.0 = previewed;
    }
}

/// What the button under the cursor would do to the economy, or else the
/// order being aimed.
pub(crate) fn previewed(
    page: Page,
    sim: &tgsim::World,
    looks: &Looks,
    shown: &Shown,
    selected: &[usize],
    building: Option<usize>,
    slots: &Query<(&Slot, &Hovered)>,
) -> Option<Previewed> {
    let unpacking = match selected {
        &[r] => Some(r).filter(|&r| {
            sim.organizations
                .get(r)
                .is_some_and(|r| r.unpacks.is_some())
        }),
        _ => None,
    };
    let hovered = (slots.iter())
        .filter(|(_, hover)| hover.get())
        .find_map(|(slot, _)| shown.entries[slot.0]);
    match hovered {
        Some(Entry::Recruit(i)) => (looks.menu(sim, building).get(i)).and_then(|r| {
            Some(Previewed::Commission(
                building?,
                Box::new(r.organization.clone()?),
            ))
        }),
        Some(Entry::Research(i)) => {
            let list = building.map_or(&[][..], |b| sim.research_list(b));
            list.get(i).cloned().map(Previewed::Research)
        }
        Some(Entry::Order(i)) if ORDERS[i].1 == Some(Give::Aim(Aim::Unpack)) => {
            unpacking.map(Previewed::Unpack)
        }
        Some(Entry::Build(i)) => builds(sim, selected).get(i).cloned().map(Previewed::Build),
        _ => match page {
            Page::Placing(i) => builds(sim, selected).get(i).cloned().map(Previewed::Build),
            Page::Aiming(Aim::Unpack) => unpacking.map(Previewed::Unpack),
            Page::Delivering(_) => (looks.delivered(sim, building, page))
                .and_then(|r| Some(Previewed::Commission(building?, Box::new(r.clone())))),
            _ => None,
        },
    }
}

/// What the tooltip says, whichever interface shows it.
#[derive(Resource, Default)]
pub struct Said(pub Option<Tip>);

/// Has the tooltip say what the grid's button under the cursor is, or else
/// what the order being aimed wants.
#[allow(clippy::too_many_arguments)]
pub fn tell(
    page: Res<Page>,
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    sim: Res<Sim>,
    looks: Res<Looks>,
    shown: Res<Shown>,
    slots: Query<(&Slot, &Hovered)>,
    local: Res<Local>,
    focus: Res<crate::Focus>,
    refusal: Res<Refusal>,
    bindings: Res<Bindings>,
    mut said: ResMut<Said>,
) {
    let building = chosen.0.filter(|_| selected.0.is_empty());
    let tip = match crate::selection::commanded(&sim.0, local.0, &selected.0, building) {
        true => {
            let under = (slots.iter()).find_map(|(slot, hover)| {
                let entry = shown.entries[slot.0].filter(|_| hover.get())?;
                Some((slot.0, entry))
            });
            let hovered = under.map(|(slot, entry)| {
                let tip = Grounds {
                    page: *page,
                    sim: &sim.0,
                    looks: &looks,
                    shown: &shown,
                    selected: &selected.0,
                    building,
                    refusal: &refusal,
                    bindings: &bindings,
                };
                tip.of(slot, entry)
            });
            prompted(hovered, *page, &sim.0, &selected.0, local.0, *focus)
        }
        false => None,
    };
    if said.0 != tip {
        said.0 = tip;
    }
}

/// What a grid button's tooltip is worked out from.
struct Grounds<'a> {
    page: Page,
    sim: &'a tgsim::World,
    looks: &'a Looks,
    shown: &'a Shown,
    selected: &'a [usize],
    building: Option<usize>,
    refusal: &'a Refusal,
    bindings: &'a Bindings,
}

impl Grounds<'_> {
    /// The tooltip of the button in `slot`, holding `entry`.
    fn of(&self, slot: usize, entry: Entry) -> Tip {
        let Grounds {
            page,
            sim,
            looks,
            shown,
            building,
            ..
        } = *self;
        let Shown {
            lacks, researches, ..
        } = shown;
        let menu = looks.menu(sim, building);
        let topics = looks.topics(sim, building);
        let list = building.map_or(&[][..], |b| sim.research_list(b));
        let list = &list[..list.len().min(topics.len())];
        let builds = builds(sim, self.selected);
        // Why it can't be pressed, when that's down to the player.
        let mut why = None;
        let mut table = Vec::new();
        let mut notes = Vec::new();
        // A greyed technology's tooltip says only what it wants.
        let mut only = None;
        let look = look_of(entry, sim, looks, building, &builds);
        let ready = shown.faces[slot].as_ref().is_some_and(|f| f.ready);
        match entry {
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
            }
            Entry::Build(i) => {
                if let Some(b) = builds.get(i).and_then(|t| looks.buildable(t)) {
                    table.extend(b.economy.iter().cloned());
                    notes.extend(b.notes.iter().cloned());
                }
            }
            Entry::Recruit(i) => {
                let mut economy = menu[i].economy.clone();
                // What it costs here, shown rounded up as it's charged.
                if let (true, Some(b), Some(cost)) = (menu[i].costed, building, economy.first_mut())
                {
                    let times = sim.recruit_multiplier(b);
                    for q in &mut cost.amounts {
                        q.amount = (q.amount * times).ceil();
                    }
                }
                table.extend(economy);
                let lacking: Vec<String> = (lacks[i].properties.iter())
                    .chain(&lacks[i].structures)
                    .map(|id| looks.names.get(id).map_or(id.to_string(), Clone::clone))
                    .collect();
                if !lacking.is_empty() {
                    why = Some(format!("{}: {}", looks.requires, lacking.join(", ")));
                }
            }
            _ => {}
        }
        let refused = (self.refusal.0).filter(|(s, _)| *s == slot).map(|(_, r)| r);
        let unaffordable = match (entry, refused) {
            (Entry::Recruit(i), Some(tgsim::Refused::Unaffordable)) => {
                (menu[i].organization.as_ref())
                    .zip(building)
                    .map(|(r, b)| (&looks.affording, sim.cost(b, r)))
            }
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
        let names = bound(entry, slot, page, (menu, &builds), list);
        let key = names.iter().find_map(|action| self.bindings.key(action));
        let help = match (entry, building) {
            (Entry::Sell, Some(b)) => {
                (look.help.as_deref()).map(|help| fill(help, &[&sim.sale_value(b).to_string()]))
            }
            _ => look.help.clone(),
        };
        match only {
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
        }
    }
}

/// The tooltip of the button under the cursor, or with none, what the
/// order being aimed wants.
fn prompted(
    hovered: Option<Tip>,
    page: Page,
    sim: &tgsim::World,
    selected: &[usize],
    local: Option<u32>,
    focus: crate::Focus,
) -> Option<Tip> {
    let prompt = |title: &str| {
        Some(Tip {
            title: Marked::plain(title),
            ..default()
        })
    };
    match (hovered, page) {
        (Some(tip), _) => Some(tip),
        (None, Page::Aiming(Aim::Unpack)) => prompt("Left click where to unpack"),
        (None, Page::Aiming(Aim::Attack)) => {
            prompt(match local.map(|p| attack_at(sim, p, focus)) {
                Some(Struck::Friend(_)) => "Can't attack a friend",
                Some(Struck::Neutral(_)) => "Can't attack a neutral",
                _ => "Left click an enemy or where to attack",
            })
        }
        (None, Page::Aiming(Aim::Guard)) => prompt("Left click what or where to guard"),
        (None, Page::Aiming(Aim::Repair)) => prompt("Left click what to repair"),
        (None, Page::Aiming(_)) => prompt("Left click where to go"),
        (None, Page::Rally) => prompt("Left click where new regiments go"),
        (None, Page::AirAttack) => prompt("Left click on a target to air attack"),
        (None, Page::Delivering(_)) => prompt("Left click where to deploy"),
        (None, Page::Placing(i)) => {
            let built = builds(sim, selected).into_iter().nth(i);
            prompt(
                match built.is_some_and(|t| sim.rules().axis_locked.contains(&t)) {
                    true => "Left click where to build",
                    false => "Left click where to build, or drag to turn it",
                },
            )
        }
        (None, _) => None,
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
fn refused_why(refused: tgsim::Refused) -> &'static str {
    match refused {
        tgsim::Refused::NotYours => "Not yours",
        tgsim::Refused::Unable => "Can't now",
        tgsim::Refused::NoRoom => "No room",
        tgsim::Refused::Lacking => "Lacks a building or technology it needs",
        tgsim::Refused::Unaffordable => "Not enough money",
        tgsim::Refused::Full => "HQ is full",
        tgsim::Refused::Recharging => "Recharging",
        tgsim::Refused::Summoned => "Waiting on what it summoned",
        tgsim::Refused::Misplaced => "Can't be aimed there",
        tgsim::Refused::Besieged => "Under siege",
        tgsim::Refused::OverLimit => "Over a limit",
        tgsim::Refused::Unsettled => "Can't change heroes now",
        tgsim::Refused::OutOfSupply => "Out of supply",
        tgsim::Refused::Understrength => "Needs at least two units",
        tgsim::Refused::Dead => "Not alive",
        tgsim::Refused::Routing => "Routing",
        tgsim::Refused::Exhausted => "Exhausted",
    }
}
